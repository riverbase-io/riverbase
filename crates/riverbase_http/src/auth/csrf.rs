use rand::RngCore;
use subtle::ConstantTimeEq;

const CSRF_TOKEN_LENGTH: usize = 32;

/// Generate a cryptographically secure CSRF token (URL-safe base64).
pub fn generate_csrf_token() -> String {
    let mut bytes = [0u8; CSRF_TOKEN_LENGTH];
    rand::rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Validate CSRF token from request against the session value (constant-time).
pub fn validate_csrf_token(session_token: Option<&str>, token: &str) -> bool {
    let Some(session_token) = session_token.filter(|s| !s.is_empty()) else {
        return false;
    };
    if token.is_empty() {
        return false;
    }
    session_token.as_bytes().ct_eq(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_csrf_rejects_mismatch() {
        assert!(!validate_csrf_token(Some("aaa"), "bbb"));
        assert!(!validate_csrf_token(None, "bbb"));
        assert!(!validate_csrf_token(Some("aaa"), ""));
    }

    #[test]
    fn validate_csrf_accepts_match() {
        let t = "same-token-value";
        assert!(validate_csrf_token(Some(t), t));
    }
}
