//! In-process OAuth2 authorization-code IdP for [`AuthProvider::MockAuth`].

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine as _;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use oauth2::{CsrfToken, PkceCodeChallenge};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::session_helper::uri;
use crate::base::RiverbaseError;
use crate::config::{AuthConfig, MockUser};

const CODE_TTL: Duration = Duration::from_secs(300);
const ID_TOKEN_TTL_SECS: i64 = 3600;

/// One-time authorization code bound to PKCE and the selected user.
#[derive(Debug, Clone)]
pub struct PendingCode {
    /// Selected mock user `sub`.
    pub sub: String,
    /// S256 `code_challenge` from authorize.
    pub code_challenge: String,
    /// OAuth `client_id`.
    pub client_id: String,
    /// Registered redirect URI.
    pub redirect_uri: String,
    /// Keycloak-style IdP session id.
    pub session_state: String,
    expires_at: Instant,
}

/// In-memory authorization-code store.
#[derive(Default)]
pub struct MockIdp {
    codes: Mutex<HashMap<String, PendingCode>>,
}

impl MockIdp {
    /// Construct an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Issue a one-time code (`{uuid}.{session_state}.{uuid}`).
    pub fn issue_code(
        &self,
        sub: &str,
        code_challenge: &str,
        client_id: &str,
        redirect_uri: &str,
    ) -> (String, String) {
        self.gc();
        let session_state = Uuid::new_v4().to_string();
        let code = format!("{}.{}.{}", Uuid::new_v4(), session_state, Uuid::new_v4());
        self.codes.lock().expect("mock idp codes").insert(
            code.clone(),
            PendingCode {
                sub: sub.to_string(),
                code_challenge: code_challenge.to_string(),
                client_id: client_id.to_string(),
                redirect_uri: redirect_uri.to_string(),
                session_state: session_state.clone(),
                expires_at: Instant::now() + CODE_TTL,
            },
        );
        (code, session_state)
    }

    /// Consume a code (one-time). `None` if missing or expired.
    pub fn consume_code(&self, code: &str) -> Option<PendingCode> {
        self.gc();
        let mut codes = self.codes.lock().expect("mock idp codes");
        let pending = codes.remove(code)?;
        if Instant::now() >= pending.expires_at {
            return None;
        }
        Some(pending)
    }

    fn gc(&self) {
        let now = Instant::now();
        self.codes
            .lock()
            .expect("mock idp codes")
            .retain(|_, pending| now < pending.expires_at);
    }
}

/// Build the mock authorize URL (`{base}/mock-auth?...`).
pub fn mock_authorize_url(
    base: &str,
    client_id: &str,
    redirect_uri: &str,
) -> (String, String, String, String) {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let csrf_state = CsrfToken::new_random();
    let mut q: HashMap<String, String> = HashMap::new();
    q.insert("response_type".into(), "code".into());
    q.insert("client_id".into(), client_id.to_string());
    q.insert("redirect_uri".into(), redirect_uri.to_string());
    q.insert("scope".into(), "openid profile email".into());
    q.insert("state".into(), csrf_state.secret().to_string());
    q.insert("code_challenge".into(), pkce_challenge.as_str().to_string());
    q.insert("code_challenge_method".into(), "S256".into());
    let url = uri(base, &["mock-auth"], Some(&q));
    (
        url,
        pkce_verifier.secret().to_string(),
        csrf_state.secret().to_string(),
        pkce_challenge.as_str().to_string(),
    )
}

/// S256 `code_challenge` for a PKCE verifier.
pub fn pkce_challenge_s256(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
}

/// Whether `verifier` satisfies the stored S256 challenge.
pub fn pkce_matches(verifier: &str, challenge: &str) -> bool {
    pkce_challenge_s256(verifier) == challenge
}

/// Check the picker password against the user's Argon2id hash.
///
/// Accounts with an empty hash sign in without a password.
pub fn mock_password_ok(user: &MockUser, password: &str) -> Result<(), &'static str> {
    let hash = user.password_hash.trim();
    if hash.is_empty() {
        return Ok(());
    }
    if password.is_empty() {
        return Err("Enter the password for this account.");
    }
    let parsed = argon2::PasswordHash::new(hash)
        .map_err(|_| "The configured password hash is not a valid Argon2 hash.")?;
    argon2::PasswordVerifier::verify_password(
        &argon2::Argon2::default(),
        password.as_bytes(),
        &parsed,
    )
    .map_err(|_| "Password does not match.")
}

/// Build `{redirect_uri}?code=&state=&session_state=`.
pub fn mock_callback_redirect(
    redirect_uri: &str,
    code: &str,
    state: &str,
    session_state: &str,
) -> String {
    let mut q: HashMap<String, String> = HashMap::new();
    q.insert("code".into(), code.to_string());
    q.insert("state".into(), state.to_string());
    q.insert("session_state".into(), session_state.to_string());
    uri(redirect_uri, &[], Some(&q))
}

/// HMAC secret used to mint mock ID tokens.
pub fn mock_signing_secret(config: &AuthConfig) -> Result<Vec<u8>, RiverbaseError> {
    let secret = config.oauth2_hmac_secret();
    if secret.is_empty() {
        return Err(crate::errors::AUT_130.with_data(json!({})));
    }
    Ok(secret.as_bytes().to_vec())
}

/// Claims JSON for a picker user (Keycloak-shaped + flat `roles`).
pub fn mock_user_claims(config: &AuthConfig, user: &MockUser, session_state: &str) -> Value {
    let now = chrono::Utc::now().timestamp();
    let client_id = config.oauth2_client_id.trim();
    let issuer = {
        let configured = config.issuer.trim();
        if configured.is_empty() {
            format!("{}/mock", config.base_path.trim_end_matches('/'))
        } else {
            configured.trim_end_matches('/').to_string()
        }
    };
    let org = if user.org_id.trim().is_empty() {
        config.mock_org_id.trim()
    } else {
        user.org_id.trim()
    };
    let tenant = if !user.tenant.trim().is_empty() {
        user.tenant.trim()
    } else if !config.mock_tenant.trim().is_empty() {
        config.mock_tenant.trim()
    } else {
        org
    };
    let org_name = if user.org_name.trim().is_empty() {
        config.mock_org_name.trim()
    } else {
        user.org_name.trim()
    };
    let org_codes = if !user.org_codes.is_empty() {
        user.org_codes.clone()
    } else if !config.mock_org_codes.is_empty() {
        config.mock_org_codes.clone()
    } else if !org_name.is_empty() {
        vec![org_name.to_string()]
    } else {
        Vec::new()
    };
    let name = match (&user.given_name, &user.family_name) {
        (Some(first), Some(last)) if !first.is_empty() && !last.is_empty() => {
            Some(format!("{first} {last}"))
        }
        (Some(first), _) if first.as_str() != "" => Some(first.clone()),
        (_, Some(last)) if last.as_str() != "" => Some(last.clone()),
        _ => user.username.clone(),
    };
    let sid = Uuid::parse_str(session_state).unwrap_or_else(|_| Uuid::new_v4());
    let mut claims = json!({
        "exp": now + ID_TOKEN_TTL_SECS,
        "iat": now,
        "auth_time": now,
        "jti": Uuid::new_v4().to_string(),
        "iss": issuer,
        "aud": client_id,
        "sub": user.sub,
        "typ": "ID",
        "azp": client_id,
        "sid": sid.to_string(),
        "session_state": sid.to_string(),
        "email_verified": true,
        "preferred_username": user.username,
        "email": user.email,
        "given_name": user.given_name,
        "family_name": user.family_name,
        "name": name,
        "roles": user.roles,
        "realm_access": { "roles": user.roles },
    });
    if let Some(obj) = claims.as_object_mut() {
        if !org.is_empty() {
            obj.insert("org_id".into(), json!(org));
            obj.insert("organization_id".into(), json!(org));
        }
        if !tenant.is_empty() {
            obj.insert("_tenant".into(), json!(tenant));
        }
        if !org_codes.is_empty() {
            obj.insert("org_codes".into(), json!(org_codes));
        }
        if !org_name.is_empty() {
            obj.insert("org_name".into(), json!(org_name));
        }
    }
    claims
}

/// Mint a short-lived HS256 id_token for the selected user.
pub fn mint_id_token(
    config: &AuthConfig,
    user: &MockUser,
    session_state: &str,
) -> Result<(String, Value), RiverbaseError> {
    let claims = mock_user_claims(config, user, session_state);
    let secret = mock_signing_secret(config)?;
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret),
    )
    .map_err(|e| crate::errors::AUT_185.with_data(json!({ "detail": e.to_string() })))?;
    Ok((token, claims))
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Preferred username, falling back to `sub`.
fn picker_label(user: &MockUser) -> &str {
    user.username
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(user.sub.as_str())
}

/// `Given Family` when available, else the picker label.
fn picker_full_name(user: &MockUser) -> String {
    let given = user.given_name.as_deref().unwrap_or("").trim();
    let family = user.family_name.as_deref().unwrap_or("").trim();
    match (given.is_empty(), family.is_empty()) {
        (false, false) => format!("{given} {family}"),
        (false, true) => given.to_string(),
        (true, false) => family.to_string(),
        (true, true) => picker_label(user).to_string(),
    }
}

/// Up to two uppercase initials for the avatar bubble.
fn picker_initials(user: &MockUser) -> String {
    let name = picker_full_name(user);
    let initials: String = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.')
        .filter_map(|part| part.chars().next())
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .collect();
    if initials.is_empty() {
        "?".into()
    } else {
        initials.to_uppercase()
    }
}

/// Tailwind-styled user card rendered as the form submit button.
fn render_picker_user(user: &MockUser) -> String {
    let mut roles = String::new();
    for role in &user.roles {
        roles.push_str(&format!(
            r#"<span class="inline-flex items-center rounded-md bg-indigo-50 px-2 py-0.5 font-mono text-[11px] font-medium text-indigo-700 ring-1 ring-inset ring-indigo-200 dark:bg-indigo-400/10 dark:text-indigo-300 dark:ring-indigo-400/25">{role}</span>"#,
            role = html_escape(role),
        ));
    }
    if roles.is_empty() {
        roles.push_str(
            r#"<span class="text-[11px] italic text-slate-400 dark:text-slate-500">no roles</span>"#,
        );
    }

    let org = user.org_name.trim();
    let org_line = if org.is_empty() {
        String::new()
    } else {
        format!(
            r#"<span class="mt-1 block truncate text-xs text-slate-500 dark:text-slate-400">{org}</span>"#,
            org = html_escape(org),
        )
    };

    let name = picker_full_name(user);
    let username = picker_label(user);
    let handle = if username == name {
        String::new()
    } else {
        format!(
            r#"<span class="truncate font-mono text-xs text-slate-500 dark:text-slate-400">@{username}</span>"#,
            username = html_escape(username),
        )
    };

    let email = user.email.as_deref().unwrap_or("").trim();
    let email_line = if email.is_empty() {
        String::new()
    } else {
        format!(
            r#"<span class="mt-1 block truncate text-xs text-slate-500 dark:text-slate-400">{email}</span>"#,
            email = html_escape(email),
        )
    };

    format!(
        r#"<button type="submit" name="sub" value="{sub}" data-riverbase-mock-user="{sub}"
      class="group flex w-full cursor-pointer items-center gap-4 rounded-xl border border-slate-200 bg-white p-4 text-left shadow-sm transition hover:border-indigo-400 hover:bg-indigo-50/60 hover:shadow-md focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500 dark:border-slate-700 dark:bg-slate-900 dark:hover:border-indigo-400 dark:hover:bg-slate-800">
      <span class="flex size-11 shrink-0 items-center justify-center rounded-full bg-indigo-100 text-sm font-semibold text-indigo-700 dark:bg-indigo-400/15 dark:text-indigo-300">{initials}</span>
      <span class="min-w-0 flex-1">
        <span class="flex min-w-0 items-baseline gap-2">
          <span class="truncate font-medium text-slate-900 dark:text-slate-100">{name}</span>
          {handle}
        </span>
        {email_line}
        {org_line}
        <span class="roles mt-2 flex flex-wrap items-center gap-1.5">{roles}</span>
      </span>
      <svg class="size-4 shrink-0 text-slate-300 transition group-hover:translate-x-0.5 group-hover:text-indigo-500 dark:text-slate-600" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
        <path fill-rule="evenodd" d="M7.21 14.77a.75.75 0 0 1 .02-1.06L11.168 10 7.23 6.29a.75.75 0 1 1 1.04-1.08l4.5 4.25a.75.75 0 0 1 0 1.08l-4.5 4.25a.75.75 0 0 1-1.06-.02Z" clip-rule="evenodd"/>
      </svg>
    </button>"#,
        sub = html_escape(&user.sub),
        initials = html_escape(&picker_initials(user)),
        name = html_escape(&name),
        handle = handle,
        email_line = email_line,
        org_line = org_line,
        roles = roles,
    )
}

/// HTML user picker (no SPA). Styled with the Tailwind Play CDN; the form still
/// submits (unstyled) when the CDN is unreachable.
pub fn render_picker_html(
    action: &str,
    users: &[MockUser],
    state: &str,
    code_challenge: &str,
    redirect_uri: &str,
    client_id: &str,
    error: Option<&str>,
) -> String {
    let cards = users
        .iter()
        .map(render_picker_user)
        .collect::<Vec<_>>()
        .join("\n    ");
    let asks_password = users.iter().any(MockUser::requires_password);
    let password_field = if asks_password {
        let alert = match error {
            Some(message) => format!(
                r#"<p role="alert" class="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-700 ring-1 ring-inset ring-red-200 dark:bg-red-400/10 dark:text-red-300 dark:ring-red-400/30">{message}</p>"#,
                message = html_escape(message),
            ),
            None => String::new(),
        };
        format!(
            r#"{alert}<label class="flex flex-col gap-1 text-sm font-medium text-slate-700 dark:text-slate-200">Password<input type="password" name="password" autocomplete="current-password" class="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm font-normal text-slate-900 shadow-sm outline-none focus:border-indigo-500 focus:ring-2 focus:ring-indigo-200 dark:border-slate-600 dark:bg-slate-950 dark:text-slate-100"/></label>"#
        )
    } else {
        String::new()
    };
    let note = if asks_password {
        "A password is required for accounts that have one configured."
    } else {
        "Development only — no password is required and no real credentials are checked."
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <meta name="robots" content="noindex"/>
  <meta name="color-scheme" content="light dark"/>
  <title>Sign in</title>
  <script src="https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4"></script>
</head>
<body class="min-h-screen bg-slate-100 font-sans text-slate-900 antialiased dark:bg-slate-950 dark:text-slate-100">
  <main class="mx-auto flex min-h-screen w-full max-w-lg flex-col justify-center px-4 py-10">
    <div class="overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-xl dark:border-slate-800 dark:bg-slate-900">
      <header class="border-b border-slate-200 bg-slate-50 px-6 py-5 dark:border-slate-800 dark:bg-slate-900/60">
        <div class="flex items-center justify-between gap-3">
          <span class="text-sm font-semibold tracking-tight text-slate-900 dark:text-slate-100">Riverbase</span>
          <span class="inline-flex items-center gap-1.5 rounded-full bg-amber-100 px-2.5 py-0.5 text-[11px] font-medium text-amber-800 ring-1 ring-inset ring-amber-300 dark:bg-amber-400/10 dark:text-amber-300 dark:ring-amber-400/25">
            <span class="size-1.5 rounded-full bg-amber-500"></span>Mock identity provider
          </span>
        </div>
        <h1 class="mt-4 text-xl font-semibold tracking-tight text-slate-900 dark:text-slate-50">Sign in</h1>
        <p class="mt-1 text-sm leading-6 text-slate-600 dark:text-slate-400">
          Choose a mock user. This IdP issues an authorization code to <code class="rounded bg-slate-200 px-1 py-0.5 font-mono text-[12px] text-slate-800 dark:bg-slate-800 dark:text-slate-200">/auth/callback</code>.
        </p>
      </header>
      <form id="riverbase-mock-auth" method="post" action="{action}" data-riverbase-mock-auth="1" class="flex flex-col gap-3 px-6 py-6">
        <input type="hidden" name="state" value="{state}"/>
        <input type="hidden" name="code_challenge" value="{challenge}"/>
        <input type="hidden" name="redirect_uri" value="{redirect}"/>
        <input type="hidden" name="client_id" value="{client}"/>
        {password_field}
        {cards}
      </form>
      <footer class="border-t border-slate-200 bg-slate-50 px-6 py-4 dark:border-slate-800 dark:bg-slate-900/60">
        <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-[11px] leading-5">
          <dt class="font-medium text-slate-500 dark:text-slate-400">client_id</dt>
          <dd class="truncate font-mono text-slate-700 dark:text-slate-300">{client}</dd>
          <dt class="font-medium text-slate-500 dark:text-slate-400">redirect_uri</dt>
          <dd class="truncate font-mono text-slate-700 dark:text-slate-300">{redirect}</dd>
        </dl>
      </footer>
    </div>
    <p class="mt-4 text-center text-xs text-slate-500 dark:text-slate-500">{note}</p>
  </main>
</body>
</html>
"#,
        action = html_escape(action),
        state = html_escape(state),
        challenge = html_escape(code_challenge),
        redirect = html_escape(redirect_uri),
        client = html_escape(client_id),
        password_field = password_field,
        cards = cards,
        note = note,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthProvider;

    fn test_config() -> AuthConfig {
        let mut config = AuthConfig::default();
        config.auth_provider = AuthProvider::MockAuth;
        config.oauth2_client_id = "gfs".into();
        config.application_secret_key = Some("test-hmac-secret".into());
        config.base_path = "/api/auth".into();
        config.mock_sub = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into();
        config.mock_username = Some("gfs-reader".into());
        config.mock_realm_access = vec!["gfs_reader".into()];
        config.mock_users = vec![MockUser {
            sub: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
            username: Some("gfs-manager".into()),
            roles: vec!["gfs_manager".into()],
            ..MockUser::default()
        }];
        config
    }

    #[test]
    fn authorize_url_has_pkce_and_state() {
        let (url, verifier, state, challenge) =
            mock_authorize_url("/api/auth", "gfs", "https://app.example/api/auth/callback");
        assert!(url.starts_with("/api/auth/mock-auth?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=gfs"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("code_challenge="));
        assert!(url.contains("state="));
        assert!(!verifier.is_empty());
        assert!(!state.is_empty());
        assert_eq!(pkce_challenge_s256(&verifier), challenge);
    }

    #[test]
    fn code_is_one_time() {
        let idp = MockIdp::new();
        let (code, session_state) = idp.issue_code(
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "challenge",
            "gfs",
            "https://app.example/callback",
        );
        assert!(code.contains(&session_state));
        assert!(code.split('.').count() == 3);
        assert!(idp.consume_code(&code).is_some());
        assert!(idp.consume_code(&code).is_none());
    }

    #[test]
    fn pkce_mismatch_is_detected() {
        let (_, verifier, _, challenge) =
            mock_authorize_url("/api/auth", "gfs", "https://app.example/callback");
        assert!(pkce_matches(&verifier, &challenge));
        assert!(!pkce_matches("wrong-verifier", &challenge));
    }

    #[test]
    fn picker_redirect_targets_callback() {
        let dest = mock_callback_redirect(
            "https://gitsfusion.localhost/v1/reader/auth/callback",
            "code-1",
            "state-1",
            "sess-1",
        );
        assert!(dest.starts_with("https://gitsfusion.localhost/v1/reader/auth/callback?"));
        assert!(dest.contains("code=code-1"));
        assert!(dest.contains("state=state-1"));
        assert!(dest.contains("session_state=sess-1"));
    }

    #[test]
    fn picker_html_lists_users() {
        let config = test_config();
        let html = render_picker_html(
            "/api/auth/mock-auth",
            &config.all_mock_users(),
            "st",
            "ch",
            "https://app.example/callback",
            "gfs",
            None,
        );
        assert!(!html.contains("name=\"password\""));
        assert!(html.contains("no password is required"));
        assert!(html.contains("id=\"riverbase-mock-auth\""));
        assert!(html.contains("gfs-reader"));
        assert!(html.contains("gfs-manager"));
        assert!(html.contains("name=\"sub\""));
        assert!(html.contains("data-riverbase-mock-user="));
        assert!(html.contains("@tailwindcss/browser"));
    }

    #[test]
    fn password_hash_is_checked_when_set() {
        use argon2::password_hash::{PasswordHasher, SaltString};
        let salt = SaltString::encode_b64(b"riverbase-mock-salt").expect("salt");
        let hash = argon2::Argon2::default()
            .hash_password(b"open-sesame", &salt)
            .expect("hash")
            .to_string();
        let user = MockUser {
            sub: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
            password_hash: hash,
            ..MockUser::default()
        };
        assert!(mock_password_ok(&user, "open-sesame").is_ok());
        assert_eq!(
            mock_password_ok(&user, "wrong").unwrap_err(),
            "Password does not match."
        );
        assert_eq!(
            mock_password_ok(&user, "").unwrap_err(),
            "Enter the password for this account."
        );
        let open = MockUser {
            sub: user.sub.clone(),
            ..MockUser::default()
        };
        assert!(mock_password_ok(&open, "").is_ok());

        let html = render_picker_html("/api/auth/mock-auth", &[user], "st", "ch", "https://app.example/callback", "gfs", Some("Password does not match."));
        assert!(html.contains("name=\"password\""));
        assert!(html.contains("Password does not match."));
    }

    #[test]
    fn picker_card_shows_name_handle_and_roles() {
        let user = MockUser {
            sub: "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(),
            username: Some("gfs-manager".into()),
            email: Some("manager@gfs.example".into()),
            given_name: Some("Gfs".into()),
            family_name: Some("Manager".into()),
            roles: vec!["gfs_manager".into()],
            org_name: "GitsFusion Dev".into(),
            ..MockUser::default()
        };
        let card = render_picker_user(&user);
        assert!(card.contains(">GM<"));
        assert!(card.contains(">Gfs Manager<"));
        assert!(card.contains(">@gfs-manager<"));
        assert!(card.contains(">manager@gfs.example<"));
        assert!(card.contains(">GitsFusion Dev<"));
        assert!(card.contains(">gfs_manager<"));
    }

    #[test]
    fn picker_card_falls_back_to_username_and_marks_missing_roles() {
        let user = MockUser {
            sub: "dddddddd-dddd-4ddd-8ddd-dddddddddddd".into(),
            username: Some("gfs-reader".into()),
            ..MockUser::default()
        };
        let card = render_picker_user(&user);
        assert!(card.contains(">GR<"));
        assert!(card.contains(">gfs-reader<"));
        assert!(!card.contains("@gfs-reader"));
        assert!(card.contains("no roles"));
    }

    #[test]
    fn mints_hs256_id_token() {
        let config = test_config();
        let user = config.default_mock_user();
        let (token, claims) = mint_id_token(&config, &user, &Uuid::new_v4().to_string()).unwrap();
        assert_eq!(token.split('.').count(), 3);
        assert_eq!(claims.get("aud").and_then(|v| v.as_str()), Some("gfs"));
        assert_eq!(
            claims.get("preferred_username").and_then(|v| v.as_str()),
            Some("gfs-reader")
        );
    }
}
