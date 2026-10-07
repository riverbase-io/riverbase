use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::logstore::parse_bool_env;

/// HTTP header carrying the active profile UUID for stateless API calls.
pub const X_PROFILE_HEADER: &str = "x-profile";

/// Authentication backend for HTTP routes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AuthProvider {
    /// No authentication (no [`Principal`] injected).
    #[default]
    #[serde(alias = "none", alias = "None", alias = "")]
    None,
    /// Local OAuth2 IdP plus `Authorization: MockAuth-…` (no Keycloak/JWKS).
    #[serde(alias = "mock", alias = "mock_auth")]
    MockAuth,
    /// JWT bearer validation + optional Keycloak OAuth2 browser login.
    #[serde(
        alias = "keycloak",
        alias = "Keycloak",
        alias = "oidc",
        alias = "Oidc",
        alias = "jwt",
        alias = "Jwt"
    )]
    Keycloak,
}

impl FromStr for AuthProvider {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "none" => Ok(Self::None),
            "mockauth" | "mock" | "mock_auth" => Ok(Self::MockAuth),
            "keycloak" | "oidc" | "jwt" => Ok(Self::Keycloak),
            _ => Err(()),
        }
    }
}

/// Extra MockAuth identity shown on the local OAuth user picker.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MockUser {
    /// Subject id (`sub`); use a UUID for audit `profile_id`.
    #[serde(default)]
    pub sub: String,
    /// Display / preferred username.
    #[serde(default, alias = "preferred_username")]
    pub username: Option<String>,
    /// Email.
    #[serde(default)]
    pub email: Option<String>,
    /// Given name.
    #[serde(default)]
    pub given_name: Option<String>,
    /// Family name.
    #[serde(default)]
    pub family_name: Option<String>,
    /// Roles stamped on the principal.
    #[serde(default)]
    pub roles: Vec<String>,
    /// Organization id (`org_id` claim). Empty inherits `[riverbase.auth] mock_org_id`.
    #[serde(default)]
    pub org_id: String,
    /// Organization display name. Empty inherits `mock_org_name`.
    #[serde(default)]
    pub org_name: String,
    /// Optional `_tenant`. Empty inherits `mock_tenant` / `org_id`.
    #[serde(default)]
    pub tenant: String,
    /// Organization codes (`org_codes` claim).
    #[serde(default)]
    pub org_codes: Vec<String>,
    /// Argon2id PHC password hash. Empty inherits `[riverbase.auth] mock_password_hash`.
    /// When the effective hash is non-empty, the mock-auth picker requires a password.
    #[serde(default)]
    pub password_hash: String,
}

impl MockUser {
    /// Whether this account must submit a password on the mock-auth picker.
    pub fn requires_password(&self) -> bool {
        !self.password_hash.trim().is_empty()
    }
}

/// OIDC / JWT authentication (Keycloak-compatible OAuth2 relying party).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// Authentication backend (`None` = disabled).
    #[serde(default)]
    pub auth_provider: AuthProvider,
    /// Issuer URL, e.g. `https://keycloak.example/realms/myrealm`.
    pub issuer: String,
    /// Expected JWT `aud` claim (client id). Empty skips audience check on the ID-token path.
    #[serde(default)]
    pub audience: String,
    /// Accepted JWT `azp` (authorized party / minting client) values for bearer access tokens.
    #[serde(default)]
    pub accepted_azp: Vec<String>,
    #[serde(default = "default_jwks_ttl_secs")]
    /// Jwks ttl secs.
    pub jwks_ttl_secs: u64,
    /// Clock-skew leeway in seconds for JWT `nbf` / `exp` ([SEC-07]).
    #[serde(default = "default_leeway_secs")]
    pub leeway_secs: u64,
    #[serde(default = "default_http_timeout_secs")]
    /// Http timeout secs.
    pub http_timeout_secs: u64,

    // Keycloak host (issuer is `{base}/realms/{realm}` when `issuer` is empty).
    #[serde(default = "default_keycloak_base_url")]
    /// Keycloak base url.
    pub keycloak_base_url: String,
    #[serde(default = "default_keycloak_realm")]
    /// Keycloak realm.
    pub keycloak_realm: String,
    /// OAuth2 client id shared by Keycloak and MockAuth (`oauth2_client_id`).
    #[serde(default = "default_oauth2_client_id", alias = "keycloak_client_id")]
    pub oauth2_client_id: String,
    /// OAuth2 client secret shared by Keycloak and MockAuth (`oauth2_client_secret`).
    #[serde(default, alias = "keycloak_client_secret")]
    pub oauth2_client_secret: String,

    /// Optional override for auth route prefix; empty → `{api_base}/auth`.
    #[serde(default)]
    pub base_path: String,

    /// Secret for signing session cookies (tower-sessions). Unset is representable.
    #[serde(default)]
    pub application_secret_key: Option<String>,
    #[serde(default = "default_session_cookie")]
    /// Session cookie.
    pub session_cookie: String,
    #[serde(default = "default_cookie_https_only")]
    /// Cookie https only.
    pub cookie_https_only: bool,
    #[serde(default = "default_cookie_same_site")]
    /// Cookie same site.
    pub cookie_same_site: String,

    #[serde(default)]
    /// Default callback uri.
    pub default_callback_uri: String,
    #[serde(default)]
    /// Default signin redirect uri.
    pub default_signin_redirect_uri: String,
    #[serde(default)]
    /// Default logout redirect uri.
    pub default_logout_redirect_uri: String,

    #[serde(default = "default_safe_redirect_domains")]
    /// Safe redirect domains.
    pub safe_redirect_domains: Vec<String>,

    #[serde(default = "default_validate_csrf_token")]
    /// Validate csrf token.
    pub validate_csrf_token: bool,

    /// Session / cookie field names (mirror Python defaults).
    #[serde(default = "default_ses_client_token_field")]
    pub ses_client_token_field: String,
    #[serde(default = "default_ses_id_token_field")]
    /// Ses id token field.
    pub ses_id_token_field: String,
    #[serde(default = "default_ses_ac_token_field")]
    /// Ses ac token field.
    pub ses_ac_token_field: String,
    #[serde(default = "default_ses_user_field")]
    /// Ses user field.
    pub ses_user_field: String,
    #[serde(default = "default_ses_session_id_field")]
    /// Ses session id field.
    pub ses_session_id_field: String,
    /// Session key for the active profile UUID (`active_profile_id`).
    #[serde(default = "default_ses_active_profile_field")]
    pub ses_active_profile_field: String,

    #[serde(default = "default_resp_header_idempotency")]
    /// Resp header idempotency.
    pub resp_header_idempotency: String,

    /// Pluggable auth profile provider type name (empty = default).
    #[serde(default)]
    pub auth_profile_provider: Option<String>,

    /// Subject id for mock principal (`sub`); use a UUID for audit `profile_id`.
    #[serde(default)]
    pub mock_sub: String,
    /// Organization id exposed on mock `/auth/info` profile (defaults to `mock_sub` when empty).
    #[serde(default)]
    pub mock_org_id: String,
    /// Optional `_tenant` for MockAuth. When empty, `_tenant` is `mock_org_id`.
    #[serde(default)]
    pub mock_tenant: String,
    /// Organization display name on mock `/auth/info` (omitted when empty).
    #[serde(default)]
    pub mock_org_name: String,
    #[serde(default)]
    /// Mock username.
    pub mock_username: Option<String>,
    #[serde(default)]
    /// Mock email.
    pub mock_email: Option<String>,
    #[serde(default)]
    /// Mock given name.
    pub mock_given_name: Option<String>,
    #[serde(default)]
    /// Mock family name.
    pub mock_family_name: Option<String>,
    /// Keycloak realm role required to enter this process. Empty disables the gate.
    #[serde(default)]
    pub require_realm_role: String,
    /// Roles copied into MockAuth `realm_access.roles` and the profile-role claim.
    #[serde(default, alias = "mock_roles")]
    pub mock_realm_access: Vec<String>,
    /// Organization codes stamped on the mock principal (`org_codes` claim).
    #[serde(default)]
    pub mock_org_codes: Vec<String>,
    /// Extra picker identities (`[[riverbase.auth.mock_users]]`). The default
    /// `[riverbase.auth] mock_*` user is always the first row.
    #[serde(default)]
    pub mock_users: Vec<MockUser>,
    /// Argon2id PHC hash for the default mock user, and the fallback for any
    /// `[[riverbase.auth.mock_users]]` row whose `password_hash` is empty.
    /// Empty means those accounts sign in without a password.
    #[serde(default)]
    pub mock_password_hash: String,

    /// When true, create an initial organization and profile on first Keycloak login
    /// if no active IDM profile exists (see `IdmAuthProfileProvider`).
    #[serde(default)]
    pub create_initial_profile: bool,
}

fn default_jwks_ttl_secs() -> u64 {
    300
}

fn default_leeway_secs() -> u64 {
    60
}

fn default_http_timeout_secs() -> u64 {
    10
}

fn default_keycloak_base_url() -> String {
    "https://id.adaptive-bits.com/auth".into()
}

fn default_keycloak_realm() -> String {
    "dev-1.riverbase.io".into()
}

fn default_oauth2_client_id() -> String {
    "sample_app".into()
}

fn default_session_cookie() -> String {
    "session".into()
}

fn default_cookie_https_only() -> bool {
    true
}

fn default_cookie_same_site() -> String {
    // Lax is the documented OAuth-callback opt-out; Strict is safer for cookie-only APIs.
    "lax".into()
}

fn default_validate_csrf_token() -> bool {
    true
}

fn default_safe_redirect_domains() -> Vec<String> {
    vec!["localhost".into()]
}

fn default_ses_client_token_field() -> String {
    "client_token".into()
}

fn default_ses_id_token_field() -> String {
    "id_token".into()
}

fn default_ses_ac_token_field() -> String {
    "access_token".into()
}

fn default_ses_user_field() -> String {
    "user".into()
}

fn default_ses_session_id_field() -> String {
    "session_id".into()
}

fn default_ses_active_profile_field() -> String {
    "active_profile_id".into()
}

fn default_resp_header_idempotency() -> String {
    "Idempotency-Key".into()
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            auth_provider: AuthProvider::None,
            issuer: String::new(),
            audience: String::new(),
            accepted_azp: Vec::new(),
            jwks_ttl_secs: default_jwks_ttl_secs(),
            leeway_secs: default_leeway_secs(),
            http_timeout_secs: default_http_timeout_secs(),
            keycloak_base_url: default_keycloak_base_url(),
            keycloak_realm: default_keycloak_realm(),
            oauth2_client_id: default_oauth2_client_id(),
            oauth2_client_secret: String::new(),
            base_path: String::new(),
            application_secret_key: None,
            session_cookie: default_session_cookie(),
            cookie_https_only: true,
            cookie_same_site: default_cookie_same_site(),
            default_callback_uri: String::new(),
            default_signin_redirect_uri: String::new(),
            default_logout_redirect_uri: String::new(),
            safe_redirect_domains: default_safe_redirect_domains(),
            validate_csrf_token: true,
            ses_client_token_field: default_ses_client_token_field(),
            ses_id_token_field: default_ses_id_token_field(),
            ses_ac_token_field: default_ses_ac_token_field(),
            ses_user_field: default_ses_user_field(),
            ses_session_id_field: default_ses_session_id_field(),
            ses_active_profile_field: default_ses_active_profile_field(),
            resp_header_idempotency: default_resp_header_idempotency(),
            auth_profile_provider: None,
            mock_sub: String::new(),
            mock_org_id: String::new(),
            mock_tenant: String::new(),
            mock_org_name: String::new(),
            mock_username: None,
            mock_email: None,
            mock_given_name: None,
            mock_family_name: None,
            require_realm_role: String::new(),
            mock_realm_access: Vec::new(),
            mock_org_codes: Vec::new(),
            mock_users: Vec::new(),
            mock_password_hash: String::new(),
            create_initial_profile: false,
        }
    }
}

fn auth_env(field: &str) -> Option<String> {
    std::env::var(format!(
        "{}AUTH_{}",
        super::ENV_PREFIX,
        field.to_ascii_uppercase()
    ))
    .ok()
}

fn apply_auth_env_str(target: &mut String, field: &str) {
    if let Some(value) = auth_env(field) {
        *target = value;
    }
}

fn apply_auth_env_opt_str(target: &mut Option<String>, field: &str) {
    if let Some(value) = auth_env(field) {
        *target = if value.is_empty() { None } else { Some(value) };
    }
}

fn apply_auth_env_bool(target: &mut bool, field: &str) {
    if let Some(value) = auth_env(field) {
        *target = parse_bool_env(&value);
    }
}

fn apply_auth_env_u64(target: &mut u64, field: &str) {
    if let Some(value) = auth_env(field) {
        if let Ok(parsed) = value.parse() {
            *target = parsed;
        }
    }
}

impl AuthConfig {
    /// Apply `RIVERBASE_AUTH_*` environment overrides (wins over TOML).
    ///
    /// Field `oauth2_client_secret` maps to `RIVERBASE_AUTH_OAUTH2_CLIENT_SECRET`.
    /// Legacy aliases: `RIVERBASE_OIDC_ISSUER`, `RIVERBASE_OIDC_AUDIENCE`.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(value) = std::env::var("RIVERBASE_AUTH_ENABLED") {
            self.auth_provider = if parse_bool_env(&value) {
                AuthProvider::Keycloak
            } else {
                AuthProvider::None
            };
        }
        if let Ok(value) = std::env::var("RIVERBASE_AUTH_PROVIDER") {
            if let Ok(provider) = value.parse() {
                self.auth_provider = provider;
            }
        }

        if let Some(value) =
            auth_env("issuer").or_else(|| std::env::var("RIVERBASE_OIDC_ISSUER").ok())
        {
            self.issuer = value;
        }
        if let Some(value) =
            auth_env("audience").or_else(|| std::env::var("RIVERBASE_OIDC_AUDIENCE").ok())
        {
            self.audience = value;
        }

        apply_auth_env_u64(&mut self.jwks_ttl_secs, "jwks_ttl_secs");
        apply_auth_env_u64(&mut self.leeway_secs, "leeway_secs");
        apply_auth_env_u64(&mut self.http_timeout_secs, "http_timeout_secs");
        apply_auth_env_str(&mut self.keycloak_base_url, "keycloak_base_url");
        apply_auth_env_str(&mut self.keycloak_realm, "keycloak_realm");
        apply_auth_env_str(&mut self.oauth2_client_id, "oauth2_client_id");
        if auth_env("oauth2_client_id").is_none() {
            apply_auth_env_str(&mut self.oauth2_client_id, "keycloak_client_id");
        }
        apply_auth_env_str(&mut self.oauth2_client_secret, "oauth2_client_secret");
        if auth_env("oauth2_client_secret").is_none() {
            apply_auth_env_str(&mut self.oauth2_client_secret, "keycloak_client_secret");
        }
        apply_auth_env_str(&mut self.base_path, "base_path");
        apply_auth_env_opt_str(&mut self.application_secret_key, "application_secret_key");
        if let Some(value) = auth_env("accepted_azp") {
            self.accepted_azp = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        apply_auth_env_str(&mut self.session_cookie, "session_cookie");
        apply_auth_env_bool(&mut self.cookie_https_only, "cookie_https_only");
        apply_auth_env_str(&mut self.cookie_same_site, "cookie_same_site");
        apply_auth_env_str(&mut self.default_callback_uri, "default_callback_uri");
        apply_auth_env_str(
            &mut self.default_signin_redirect_uri,
            "default_signin_redirect_uri",
        );
        apply_auth_env_str(
            &mut self.default_logout_redirect_uri,
            "default_logout_redirect_uri",
        );
        if let Some(value) = auth_env("safe_redirect_domains") {
            self.safe_redirect_domains = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        apply_auth_env_bool(&mut self.validate_csrf_token, "validate_csrf_token");
        apply_auth_env_str(&mut self.ses_client_token_field, "ses_client_token_field");
        apply_auth_env_str(&mut self.ses_id_token_field, "ses_id_token_field");
        apply_auth_env_str(&mut self.ses_ac_token_field, "ses_ac_token_field");
        apply_auth_env_str(&mut self.ses_user_field, "ses_user_field");
        apply_auth_env_str(&mut self.ses_session_id_field, "ses_session_id_field");
        apply_auth_env_str(
            &mut self.ses_active_profile_field,
            "ses_active_profile_field",
        );
        apply_auth_env_str(&mut self.resp_header_idempotency, "resp_header_idempotency");
        apply_auth_env_opt_str(&mut self.auth_profile_provider, "auth_profile_provider");
        apply_auth_env_str(&mut self.mock_sub, "mock_sub");
        apply_auth_env_str(&mut self.mock_org_id, "mock_org_id");
        apply_auth_env_str(&mut self.mock_tenant, "mock_tenant");
        apply_auth_env_str(&mut self.mock_org_name, "mock_org_name");
        apply_auth_env_opt_str(&mut self.mock_username, "mock_username");
        apply_auth_env_opt_str(&mut self.mock_email, "mock_email");
        apply_auth_env_opt_str(&mut self.mock_given_name, "mock_given_name");
        apply_auth_env_opt_str(&mut self.mock_family_name, "mock_family_name");
        apply_auth_env_str(&mut self.require_realm_role, "require_realm_role");
        if let Some(value) = auth_env("mock_realm_access") {
            self.mock_realm_access = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        if let Some(value) = auth_env("mock_org_codes") {
            self.mock_org_codes = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        apply_auth_env_bool(&mut self.create_initial_profile, "create_initial_profile");
        apply_auth_env_str(&mut self.mock_password_hash, "mock_password_hash");
    }

    /// Whether authentication is disabled ([`AuthProvider::None`]).
    pub fn auth_disabled(&self) -> bool {
        self.auth_provider == AuthProvider::None
    }

    /// Whether MockAuth (local IdP + `Authorization: MockAuth-…`) is configured.
    pub fn uses_mock_auth(&self) -> bool {
        self.auth_provider == AuthProvider::MockAuth
    }

    /// Whether JWT / Keycloak OIDC is configured.
    pub fn uses_keycloak_auth(&self) -> bool {
        self.auth_provider == AuthProvider::Keycloak
    }

    /// Realm issuer URL: `{base}/realms/{realm}`.
    pub fn keycloak_issuer(&self) -> String {
        let base = self.keycloak_base_url.trim_end_matches('/');
        format!("{base}/realms/{}", self.keycloak_realm)
    }

    /// Effective OIDC issuer (explicit `issuer` or derived Keycloak realm URL).
    pub fn effective_issuer(&self) -> String {
        if self.issuer.trim().is_empty() {
            self.keycloak_issuer()
        } else {
            self.issuer.trim_end_matches('/').to_string()
        }
    }

    /// Effective audience (explicit `audience` or Keycloak client id).
    pub fn effective_audience(&self) -> Option<String> {
        if !self.audience.trim().is_empty() {
            Some(self.audience.clone())
        } else if !self.oauth2_client_id.trim().is_empty() {
            Some(self.oauth2_client_id.clone())
        } else {
            None
        }
    }

    /// HMAC / token secret: `oauth2_client_secret` if set, else `application_secret_key`.
    pub fn oauth2_hmac_secret(&self) -> &str {
        let secret = self.oauth2_client_secret.trim();
        if !secret.is_empty() {
            return secret;
        }
        self.application_secret_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("")
    }

    /// Default picker identity from `[riverbase.auth] mock_*` fields.
    pub fn default_mock_user(&self) -> MockUser {
        MockUser {
            sub: if self.mock_sub.trim().is_empty() {
                "dev-user".into()
            } else {
                self.mock_sub.clone()
            },
            username: self.mock_username.clone(),
            email: self.mock_email.clone(),
            given_name: self.mock_given_name.clone(),
            family_name: self.mock_family_name.clone(),
            roles: self.mock_realm_access.clone(),
            org_id: self.mock_org_id.clone(),
            org_name: self.mock_org_name.clone(),
            tenant: self.mock_tenant.clone(),
            org_codes: self.mock_org_codes.clone(),
            password_hash: self.mock_password_hash.clone(),
        }
    }

    /// Default mock user first, then extras that do not duplicate its `sub`.
    ///
    /// An extra user with an empty `password_hash` inherits `mock_password_hash`.
    pub fn all_mock_users(&self) -> Vec<MockUser> {
        let default = self.default_mock_user();
        let mut users = vec![default.clone()];
        for extra in &self.mock_users {
            if extra.sub.trim().is_empty() || extra.sub == default.sub {
                continue;
            }
            let mut user = extra.clone();
            if user.password_hash.trim().is_empty() {
                user.password_hash.clone_from(&self.mock_password_hash);
            }
            users.push(user);
        }
        users
    }

    /// Look up a picker user by `sub`.
    pub fn mock_user_by_sub(&self, sub: &str) -> Option<MockUser> {
        self.all_mock_users()
            .into_iter()
            .find(|user| user.sub == sub)
    }

    /// Derive auth route prefix and default redirect URIs from [`api_base`].
    pub fn normalize_paths(&mut self, api_base: &str) {
        if self.base_path.trim().is_empty() {
            self.base_path = crate::api_path::auth_base_path(api_base);
        }
        let base = self.base_path.trim_end_matches('/');
        if self.default_callback_uri.trim().is_empty() {
            self.default_callback_uri = format!("{base}/callback");
        }
        if self.default_signin_redirect_uri.trim().is_empty() {
            self.default_signin_redirect_uri = format!("{base}/info");
        }
        if self.default_logout_redirect_uri.trim().is_empty() {
            self.default_logout_redirect_uri = "/".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_env<F: FnOnce()>(key: &str, value: &str, f: F) {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        f();
        if let Some(prev) = previous {
            std::env::set_var(key, prev);
        } else {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn auth_env_overrides_oauth2_fields() {
        with_env("RIVERBASE_AUTH_OAUTH2_CLIENT_SECRET", "s3cr3t", || {
            with_env("RIVERBASE_AUTH_KEYCLOAK_REALM", "id.entry.express", || {
                let mut cfg = AuthConfig::default();
                cfg.apply_env_overrides();
                assert_eq!(cfg.oauth2_client_secret, "s3cr3t");
                assert_eq!(cfg.keycloak_realm, "id.entry.express");
            });
        });
    }

    #[test]
    fn auth_env_overrides_toml_values_on_load() {
        with_env("RIVERBASE_AUTH_OAUTH2_CLIENT_ID", "from-env", || {
            let mut cfg = AuthConfig {
                oauth2_client_id: "from-toml".into(),
                ..Default::default()
            };
            cfg.apply_env_overrides();
            assert_eq!(cfg.oauth2_client_id, "from-env");
        });
    }

    #[test]
    fn auth_env_issuer_prefers_flrs_auth_over_oidc_alias() {
        with_env(
            "RIVERBASE_OIDC_ISSUER",
            "https://legacy.example/realms/x",
            || {
                with_env(
                    "RIVERBASE_AUTH_ISSUER",
                    "https://new.example/realms/y",
                    || {
                        let mut cfg = AuthConfig::default();
                        cfg.apply_env_overrides();
                        assert_eq!(cfg.issuer, "https://new.example/realms/y");
                    },
                );
            },
        );
    }

    #[test]
    fn default_leeway_secs_is_sixty() {
        assert_eq!(AuthConfig::default().leeway_secs, 60);
    }

    #[test]
    fn auth_env_create_initial_profile_bool() {
        with_env("RIVERBASE_AUTH_CREATE_INITIAL_PROFILE", "true", || {
            let mut cfg = AuthConfig::default();
            cfg.apply_env_overrides();
            assert!(cfg.create_initial_profile);
        });
    }

    #[test]
    fn oauth2_client_id_deserializes() {
        let cfg: AuthConfig = toml::from_str(
            r#"
            oauth2_client_id = "gfs"
            oauth2_client_secret = "s3cr3t"
            "#,
        )
        .expect("parse");
        assert_eq!(cfg.oauth2_client_id, "gfs");
        assert_eq!(cfg.oauth2_client_secret, "s3cr3t");
        assert_eq!(cfg.oauth2_hmac_secret(), "s3cr3t");
    }

    #[test]
    fn mock_users_follow_default_then_extras() {
        let cfg: AuthConfig = toml::from_str(
            r#"
            auth_provider = "MockAuth"
            mock_sub = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
            mock_username = "gfs-reader"
            mock_realm_access = ["gfs_reader"]

            [[mock_users]]
            sub = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
            username = "gfs-manager"
            roles = ["gfs_manager"]
            "#,
        )
        .expect("parse");
        let users = cfg.all_mock_users();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].username.as_deref(), Some("gfs-reader"));
        assert_eq!(users[1].username.as_deref(), Some("gfs-manager"));
        assert!(cfg
            .mock_user_by_sub("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")
            .is_some());
    }

    #[test]
    fn mock_password_hash_applies_to_the_default_user_and_is_inherited() {
        let cfg: AuthConfig = toml::from_str(
            r#"
            mock_password_hash = "$argon2id$default"

            [[mock_users]]
            sub = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
            username = "inherits"

            [[mock_users]]
            sub = "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
            username = "own"
            password_hash = "$argon2id$own"
            "#,
        )
        .expect("parse");
        let users = cfg.all_mock_users();
        assert_eq!(users[0].password_hash, "$argon2id$default");
        assert_eq!(users[1].password_hash, "$argon2id$default");
        assert_eq!(users[2].password_hash, "$argon2id$own");
        assert!(users[0].requires_password());
    }
}
