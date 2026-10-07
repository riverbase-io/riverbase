use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::HeaderMap;
use rand::RngCore;
use url::Url;

/// Join path segments under a base path (Python `uri()` helper).
pub fn uri(base: &str, paths: &[&str], queries: Option<&HashMap<String, String>>) -> String {
    let mut path = base.trim_end_matches('/').to_string();
    if path.is_empty() {
        path = String::new();
    }
    for segment in paths {
        let seg = segment.trim_matches('/');
        if seg.is_empty() {
            continue;
        }
        path.push('/');
        path.push_str(seg);
    }
    if let Some(q) = queries {
        let pairs: Vec<_> = q
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k}={}", urlencoding_encode(v)))
            .collect();
        if !pairs.is_empty() {
            path.push('?');
            path.push_str(&pairs.join("&"));
        }
    }
    path
}

fn urlencoding_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Return existing client token from session map or generate and store one.
#[allow(dead_code)]
pub fn generate_client_token(
    session: &mut HashMap<String, serde_json::Value>,
    field: &str,
) -> String {
    if let Some(v) = session.get(field).and_then(|v| v.as_str()) {
        return v.to_string();
    }
    let token = generate_urlsafe_token();
    session.insert(field.into(), serde_json::Value::String(token.clone()));
    token
}

/// Return existing session id from session map or generate and store one.
#[allow(dead_code)]
pub fn generate_session_id(
    session: &mut HashMap<String, serde_json::Value>,
    field: &str,
) -> String {
    if let Some(v) = session.get(field).and_then(|v| v.as_str()) {
        return v.to_string();
    }
    let id = generate_urlsafe_token();
    session.insert(field.into(), serde_json::Value::String(id.clone()));
    id
}

/// Cryptographically random URL-safe token (client/session ids).
pub fn random_token() -> String {
    generate_urlsafe_token()
}

fn generate_urlsafe_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Whether `url` is a safe redirect (relative or whitelisted domain).
pub fn is_safe_redirect_url(url: &str, whitelist: &[String]) -> bool {
    if url.is_empty() {
        return false;
    }
    if let Ok(parsed) = Url::parse(url) {
        if parsed.scheme().is_empty() && parsed.host().is_none() {
            return true;
        }
        if whitelist.iter().any(|d| d == "*") {
            return true;
        }
        if let Some(host) = parsed.host_str() {
            return whitelist.iter().any(|d| d.eq_ignore_ascii_case(host));
        }
    } else if url.starts_with('/') && !url.starts_with("//") {
        return true;
    }
    tracing::warn!(url = %url, "invalid redirect URL");
    false
}

/// Pick safe redirect URL, optionally appending cache-bust `_t` query param.
pub fn validate_redirect_url(
    url: &str,
    default: &str,
    whitelist: &[String],
    cache_invalidate: bool,
) -> String {
    let mut redir = if is_safe_redirect_url(url, whitelist) {
        url.to_string()
    } else {
        default.to_string()
    };
    if cache_invalidate {
        let sep = if redir.contains('?') { '&' } else { '?' };
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        redir = format!("{redir}{sep}_t={ts}");
    }
    redir
}

/// Public API mount prefix from `X-Forwarded-Prefix` (e.g. `/v1/reader`).
pub fn forwarded_api_prefix(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-prefix")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .unwrap_or_default()
}

/// Infer `/v1/{zone}` from `default_callback_uri` (`…/v1/reader/auth/callback`).
pub fn prefix_from_callback_uri(callback: &str) -> String {
    let path = Url::parse(callback)
        .ok()
        .map(|parsed| parsed.path().to_string())
        .unwrap_or_else(|| {
            callback
                .split(['?', '#'])
                .next()
                .unwrap_or(callback)
                .to_string()
        });
    let path = path.trim_end_matches('/');
    let Some(auth) = path.strip_suffix("/callback") else {
        return String::new();
    };
    let Some(prefix) = auth.strip_suffix("/auth") else {
        return String::new();
    };
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() || prefix == "/api" {
        return String::new();
    }
    prefix.to_string()
}

/// SPA `Location` for an authenticated `GET /auth/home`.
///
/// `?next=` when it passes [`validate_redirect_url`], else
/// `default_signin_redirect_uri`. Anonymous `/auth/home` starts the IdP the
/// same way as `/auth/sign-in` and does not use this helper.
pub fn auth_home_location(config: &crate::config::AuthConfig, next: Option<&str>) -> String {
    validate_redirect_url(
        next.unwrap_or(""),
        &config.default_signin_redirect_uri,
        &config.safe_redirect_domains,
        false,
    )
}

/// Rewrite an internal `/api/…` path onto the public mount (`/v1/reader/…`).
pub fn public_api_path(path: &str, headers: &HeaderMap, fallback_prefix: &str) -> String {
    let prefix = {
        let forwarded = forwarded_api_prefix(headers);
        if forwarded.is_empty() {
            fallback_prefix.trim_end_matches('/').to_string()
        } else {
            forwarded
        }
    };
    if prefix.is_empty() {
        return path.to_string();
    }
    // Only the internal API mount is rewritten. UI paths and paths that are
    // already public stay as they are, so a prefix cannot turn `/ui/…` into
    // `/v1/{zone}/ui/…`.
    if path != "/api" && !path.starts_with("/api/") {
        return path.to_string();
    }
    let rest = path.strip_prefix("/api").unwrap_or("");
    format!("{prefix}{rest}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_joins_paths() {
        assert_eq!(uri("/auth", &["callback"], None), "/auth/callback");
        let mut q: HashMap<String, String> = HashMap::new();
        q.insert("a".into(), "1".into());
        assert_eq!(uri("/auth", &["sign-out"], Some(&q)), "/auth/sign-out?a=1");
    }

    #[test]
    fn safe_redirect_relative() {
        assert!(is_safe_redirect_url("/login", &["localhost".into()]));
        assert!(!is_safe_redirect_url("", &[]));
    }

    #[test]
    fn safe_redirect_whitelist() {
        assert!(is_safe_redirect_url(
            "http://localhost/home",
            &["localhost".into()]
        ));
        assert!(!is_safe_redirect_url(
            "http://evil.example/home",
            &["localhost".into()]
        ));
    }

    #[test]
    fn prefix_from_public_callback_uri() {
        assert_eq!(
            prefix_from_callback_uri("https://gitsfusion.localhost/v1/reader/auth/callback"),
            "/v1/reader"
        );
        assert_eq!(prefix_from_callback_uri("/api/auth/callback"), "");
    }

    #[test]
    fn public_api_path_rewrites_internal_api_base() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-prefix", "/v1/reader".parse().unwrap());
        assert_eq!(
            public_api_path("/api/auth/sign-in", &headers, ""),
            "/v1/reader/auth/sign-in"
        );
        assert_eq!(
            public_api_path("/api/gfs.publication/pkg/x/index.html", &headers, ""),
            "/v1/reader/gfs.publication/pkg/x/index.html"
        );
        assert_eq!(
            public_api_path("/api/auth", &HeaderMap::new(), "/v1/reader"),
            "/v1/reader/auth"
        );
        assert_eq!(
            public_api_path("/ui/sysadmin/", &headers, ""),
            "/ui/sysadmin/"
        );
        assert_eq!(
            public_api_path("/v1/sysadmin/auth/sign-in", &headers, ""),
            "/v1/sysadmin/auth/sign-in"
        );
    }

    fn home_config() -> crate::config::AuthConfig {
        let mut config = crate::config::AuthConfig::default();
        config.base_path = "/api/auth".into();
        config.default_signin_redirect_uri = "/ui/manager/".into();
        config.default_callback_uri = "https://stratify.localhost/v1/manager/auth/callback".into();
        config.safe_redirect_domains = vec!["stratify.localhost".into()];
        config
    }

    #[test]
    fn auth_home_logged_in_uses_spa_default() {
        let loc = auth_home_location(&home_config(), None);
        assert_eq!(loc, "/ui/manager/");
    }

    #[test]
    fn auth_home_logged_in_honors_safe_next() {
        let loc = auth_home_location(&home_config(), Some("/ui/manager/goals"));
        assert_eq!(loc, "/ui/manager/goals");
    }

    #[test]
    fn auth_home_rejects_unsafe_next() {
        let loc = auth_home_location(&home_config(), Some("https://evil.example/phish"));
        assert_eq!(loc, "/ui/manager/");
    }
}
