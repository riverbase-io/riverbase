//! Signed command hook tokens (`:hook`), carrying aggroot + scope + payload binding.
//!
//! Tokens are HMAC-SHA256 (`v2.` prefix). SHA-1 tokens are rejected ([SUR-06]).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::base::RiverbaseResult;
use crate::config::HookTokenConfig;

use super::link_token::{decode_url_safe_payload, hmac_sha256, url_safe_payload};

const TOKEN_VERSION_PREFIX: &str = "v2.";

/// Payload embedded in a `:hook` token (aggroot + scope + nonce + payload hash).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookTokenClaims {
    /// Cmdkey.
    pub cmdkey: String,
    /// Resource.
    pub resource: String,
    /// Identifier.
    pub identifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Scope.
    pub scope: Option<String>,
    /// Expires at.
    pub expires_at: DateTime<Utc>,
    /// OIDC nonce.
    pub nonce: String,
    /// Payload hash.
    pub payload_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Tenant data scope copied onto the hook principal (`_tenant`).
    pub tenant: Option<Uuid>,
}

/// Canonical SHA-256 hex of sorted `key=value` query parameters.
pub fn payload_hash_for_params(params: &HashMap<String, String>) -> String {
    let mut keys: Vec<&String> = params.keys().collect();
    keys.sort();
    let canonical = keys
        .into_iter()
        .map(|key| format!("{key}={}", params[key]))
        .collect::<Vec<_>>()
        .join("&");
    format!("{:x}", Sha256::digest(canonical.as_bytes()))
}

/// Build claims with a default expiry, nonce, and payload binding.
pub fn hook_token_claims(
    cmdkey: impl Into<String>,
    resource: impl Into<String>,
    identifier: impl Into<String>,
    scope: Option<String>,
    ttl: Duration,
    params: &HashMap<String, String>,
) -> HookTokenClaims {
    HookTokenClaims {
        cmdkey: cmdkey.into(),
        resource: resource.into(),
        identifier: identifier.into(),
        scope,
        expires_at: Utc::now() + ttl,
        nonce: Uuid::new_v4().to_string(),
        payload_hash: payload_hash_for_params(params),
        tenant: None,
    }
}

impl HookTokenClaims {
    /// Stamp `_tenant` so hook execution can satisfy fail-closed tenant scoping.
    pub fn with_tenant(mut self, tenant: Uuid) -> Self {
        self.tenant = Some(tenant);
        self
    }
}

fn derive_signing_key(config: &HookTokenConfig) -> RiverbaseResult<Vec<u8>> {
    let mut hasher = Sha256::new();
    hasher.update(config.require_salt()?.as_bytes());
    hasher.update(b"signer");
    hasher.update(config.require_secret()?.as_bytes());
    Ok(hasher.finalize().to_vec())
}

fn sign_payload(payload: &[u8], config: &HookTokenConfig) -> RiverbaseResult<String> {
    use super::link_token::base64_url_encode;
    let key = derive_signing_key(config)?;
    let sig = hmac_sha256(&key, payload);
    let sig_b64 = base64_url_encode(&sig);
    Ok(format!(
        "{TOKEN_VERSION_PREFIX}{}.{}",
        String::from_utf8_lossy(payload),
        String::from_utf8_lossy(&sig_b64)
    ))
}

fn unsign_payload(token: &str, config: &HookTokenConfig) -> RiverbaseResult<Vec<u8>> {
    use super::link_token::{base64_url_decode, constant_time_eq};
    let token = token
        .strip_prefix(TOKEN_VERSION_PREFIX)
        .ok_or_else(|| crate::errors::HOK_009.with_data("legacy token"))?;
    let (value, sig) = token
        .rsplit_once('.')
        .ok_or_else(|| crate::errors::HOK_004.with_data("invalid token"))?;
    let value = value.as_bytes();
    let sig = base64_url_decode(sig.as_bytes())
        .map_err(|_| crate::errors::HOK_005.with_data("invalid token"))?;
    let key = derive_signing_key(config)?;
    let expected = hmac_sha256(&key, value);
    if !constant_time_eq(&expected, &sig) {
        return Err(crate::errors::HOK_006.with_data("invalid token"));
    }
    Ok(value.to_vec())
}

/// Encode a signed hook token.
pub fn encode_hook_token(
    claims: &HookTokenClaims,
    config: &HookTokenConfig,
) -> RiverbaseResult<String> {
    let json =
        serde_json::to_vec(claims).map_err(|e| crate::errors::HOK_001.with_data(e.to_string()))?;
    let payload = url_safe_payload(&json)?;
    sign_payload(&payload, config)
}

/// Decode and verify a `:hook` token.
pub fn decode_hook_token(token: &str, config: &HookTokenConfig) -> RiverbaseResult<HookTokenClaims> {
    let payload = unsign_payload(token, config)?;
    let json = decode_url_safe_payload(&payload)?;
    let data: HookTokenClaims = serde_json::from_slice(&json)
        .map_err(|_| crate::errors::HOK_002.with_data("invalid token"))?;
    if data.nonce.trim().is_empty() || data.payload_hash.trim().is_empty() {
        return Err(crate::errors::HOK_002.with_data("invalid token"));
    }
    if data.expires_at < Utc::now() {
        return Err(crate::errors::HOK_003.with_data("expired"));
    }
    Ok(data)
}

struct HookNonceStore {
    seen: Mutex<HashMap<String, DateTime<Utc>>>,
}

impl HookNonceStore {
    fn consume(&self, nonce: &str, expires_at: DateTime<Utc>) -> RiverbaseResult<()> {
        let now = Utc::now();
        let mut seen = self.seen.lock().expect("hook nonce lock");
        seen.retain(|_, expiry| *expiry > now);
        if seen.contains_key(nonce) {
            return Err(crate::errors::HOK_008.with_data("replay"));
        }
        seen.insert(nonce.to_string(), expires_at);
        Ok(())
    }
}

fn nonce_store() -> &'static HookNonceStore {
    static STORE: OnceLock<HookNonceStore> = OnceLock::new();
    STORE.get_or_init(|| HookNonceStore {
        seen: Mutex::new(HashMap::new()),
    })
}

/// Mark a hook nonce used. Subsequent uses of the same nonce fail with `HOK-008`.
pub fn consume_hook_nonce(nonce: &str, expires_at: DateTime<Utc>) -> RiverbaseResult<()> {
    nonce_store().consume(nonce, expires_at)
}

/// Reject when the inbound query string does not match the token's payload hash.
pub fn verify_hook_payload_hash(
    claims: &HookTokenClaims,
    params: &HashMap<String, String>,
) -> RiverbaseResult<()> {
    let actual = payload_hash_for_params(params);
    if actual != claims.payload_hash {
        return Err(crate::errors::HOK_010.with_data("payload hash mismatch"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> HookTokenConfig {
        HookTokenConfig {
            secret: Some("test-hook-secret".into()),
            salt: Some("test-hook-salt".into()),
        }
    }

    #[test]
    fn hook_token_roundtrip() {
        let params = HashMap::from([("amount".into(), "10".into())]);
        let claims = hook_token_claims(
            "confirm-order",
            "todo",
            "01234567-89ab-cdef-0123-456789abcdef",
            Some("~".to_string()),
            Duration::hours(1),
            &params,
        );
        let token = encode_hook_token(&claims, &test_config()).unwrap();
        assert!(token.starts_with("v2."));
        let decoded = decode_hook_token(&token, &test_config()).unwrap();
        assert_eq!(decoded, claims);
        verify_hook_payload_hash(&decoded, &params).unwrap();
    }

    #[test]
    fn altered_query_parameter_invalidates_binding() {
        let params = HashMap::from([("amount".into(), "10".into())]);
        let claims = hook_token_claims("pay", "payment", "p-1", None, Duration::hours(1), &params);
        let mut other = params.clone();
        other.insert("amount".into(), "999".into());
        let err = verify_hook_payload_hash(&claims, &other).expect_err("hash mismatch");
        assert_eq!(err.errcode.as_str(), "HOK-010");
    }

    #[test]
    fn replayed_nonce_is_rejected() {
        let claims = hook_token_claims(
            "pay",
            "payment",
            "p-1",
            None,
            Duration::hours(1),
            &HashMap::new(),
        );
        consume_hook_nonce(&claims.nonce, claims.expires_at).unwrap();
        let err = consume_hook_nonce(&claims.nonce, claims.expires_at).expect_err("replay");
        assert_eq!(err.errcode.as_str(), "HOK-008");
    }

    #[test]
    fn rejects_sha1_token() {
        let err = decode_hook_token("not-a-v2-token.sig", &test_config()).expect_err("sha1");
        assert_eq!(err.errcode.as_str(), "HOK-009");
    }
}
