use std::sync::Arc;
use std::time::Instant;

use jsonwebtoken::{decode, decode_header, Algorithm, Validation};
use serde_json::{json, Value};
use tokio::sync::RwLock;

use super::config::{rewrite_internal_oidc_url, OidcConfig};
use super::discovery::{fetch_openid_configuration, OpenIdConfiguration};
use super::jwks::{fetch_jwks, jwks_to_decoding_keys, JwksCache};
use super::principal::{AuthClaims, Principal};
use crate::{RiverbaseError, RiverbaseResult};

/// Raw JWT claims after signature validation.
#[derive(Debug, Clone)]
pub struct RawTokenClaims {
    /// Claims.
    pub claims: Value,
}

/// Validates bearer JWTs against an OIDC provider JWKS (Keycloak-compatible).
#[derive(Clone)]
pub struct JwtValidator {
    config: OidcConfig,
    client: reqwest::Client,
    oidc: Arc<RwLock<Option<OpenIdConfiguration>>>,
    jwks: Arc<RwLock<JwksCache>>,
    last_unknown_kid_refresh: Arc<RwLock<Option<Instant>>>,
}

impl JwtValidator {
    /// Construct a new value.
    pub fn new(config: OidcConfig) -> RiverbaseResult<Self> {
        if config.issuer.trim().is_empty() {
            return Err(crate::errors::AUT_030.with_data(json!({
                "field": "issuer",
            })));
        }
        let timeout = std::time::Duration::from_secs(config.http_timeout_secs);
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| {
                crate::errors::AUT_031.with_data(json!({
                    "operation": "build_http_client",
                    "cause": e.to_string(),
                }))
            })?;
        Ok(Self {
            config,
            client,
            oidc: Arc::new(RwLock::new(None)),
            jwks: Arc::new(RwLock::new(JwksCache::empty())),
            last_unknown_kid_refresh: Arc::new(RwLock::new(None)),
        })
    }

    /// Bootstrap discovery + JWKS (call once at service startup).
    pub async fn warmup(&self) -> RiverbaseResult<()> {
        let mut last = self.refresh_jwks().await;
        for _ in 0..40 {
            if last.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            last = self.refresh_jwks().await;
        }
        last
    }

    /// Validate a bearer token string (without `Bearer ` prefix).
    pub async fn validate_token(&self, token: &str) -> RiverbaseResult<Principal> {
        let header = decode_header(token).map_err(|e| {
            crate::errors::AUT_032.with_data(json!({
                "operation": "decode_header",
                "cause": e.to_string(),
            }))
        })?;
        let kid = header.kid.ok_or_else(|| {
            crate::errors::AUT_033.with_data(json!({
                "operation": "decode_header",
            }))
        })?;
        let alg = header.alg;
        if alg != Algorithm::RS256 {
            return Err(crate::errors::AUT_034.with_data(json!({
                "expected_algorithm": "RS256",
                "actual_algorithm": format!("{alg:?}"),
            })));
        }

        let key = self.decoding_key_for_kid(&kid).await.map_err(|_| {
            crate::errors::AUT_035.with_data(json!({
                "kid": kid,
            }))
        })?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.leeway = self.config.leeway_secs;
        validation.set_issuer(&[self.config.issuer.trim_end_matches('/')]);
        if let Some(aud) = &self.config.audience {
            validation.set_audience(&[aud.as_str()]);
        } else {
            validation.validate_aud = false;
        }

        let token_data = decode::<Value>(token, &key, &validation).map_err(|e| {
            crate::errors::AUT_036.with_data(json!({
                "operation": "jwt_decode_validate",
                "cause": e.to_string(),
            }))
        })?;

        let auth_claims: AuthClaims =
            serde_json::from_value(token_data.claims.clone()).map_err(|e| {
                crate::errors::AUT_036.with_data(json!({
                    "operation": "jwt_decode_validate",
                    "cause": e.to_string(),
                }))
            })?;

        if let Some(expected_aud) = &self.config.audience {
            if !auth_claims.aud.contains(expected_aud) {
                return Err(crate::errors::AUT_037.with_data(json!({
                    "expected_audience": expected_aud,
                    "actual_audience": format!("{:?}", auth_claims.aud),
                })));
            }
        }

        Ok(Principal::from_token_claims(auth_claims, token_data.claims))
    }

    /// Decode access token and return raw claims (OAuth callback / session path).
    ///
    /// Keycloak access tokens often use `aud: "account"`; signature and expiry are validated,
    /// but audience is not (matches Python `decode_ac_token`).
    pub async fn decode_access_token_claims(&self, token: &str) -> RiverbaseResult<RawTokenClaims> {
        self.decode_token_raw(token, false).await
    }

    /// Decode ID token and return raw claims (validates iss + aud when configured).
    pub async fn decode_id_token_claims(&self, token: &str) -> RiverbaseResult<RawTokenClaims> {
        self.decode_token_raw(token, true).await
    }

    async fn decode_token_raw(
        &self,
        token: &str,
        validate_audience: bool,
    ) -> RiverbaseResult<RawTokenClaims> {
        let header =
            decode_header(token).map_err(|e| crate::errors::AUT_140.with_data(e.to_string()))?;
        let kid = header
            .kid
            .ok_or_else(|| crate::errors::AUT_141.with_data("JWT header is missing kid"))?;
        if header.alg != Algorithm::RS256 {
            return Err(crate::errors::AUT_142
                .with_data(format!("Unsupported algorithm: {:?}", header.alg)));
        }

        let key = self.decoding_key_for_kid(&kid).await.map_err(|_| {
            crate::errors::AUT_143.with_data(format!("Public key not found for kid: {kid}"))
        })?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.leeway = self.config.leeway_secs;
        validation.set_issuer(&[self.config.issuer.trim_end_matches('/')]);
        if validate_audience {
            if let Some(aud) = &self.config.audience {
                validation.set_audience(&[aud.as_str()]);
            } else {
                validation.validate_aud = false;
            }
        } else {
            validation.validate_aud = false;
        }

        let token_data = decode::<Value>(token, &key, &validation)
            .map_err(|e| decode_jwt_error(&e.to_string()))?;

        // Access tokens skip audience (Keycloak `aud: account`); skip `azp` too.
        // Gitea ID tokens omit `azp` and put the client id in `aud`.
        if validate_audience && !self.config.accepted_azp.is_empty() {
            ensure_accepted_azp(&token_data.claims, &self.config.accepted_azp)?;
        }

        if validate_audience {
            if let Some(expected_aud) = &self.config.audience {
                let aud = token_data.claims.get("aud");
                let ok = match aud {
                    Some(Value::String(s)) => s == expected_aud,
                    Some(Value::Array(arr)) => arr
                        .iter()
                        .any(|v| v.as_str() == Some(expected_aud.as_str())),
                    _ => false,
                };
                if !ok {
                    return Err(crate::errors::AUT_145.with_data(format!(
                        "JWT audience mismatch (expected {expected_aud}, got {aud:?})"
                    )));
                }
            }
        }

        Ok(RawTokenClaims {
            claims: token_data.claims,
        })
    }

    /// Parse `Authorization: Bearer <token>`.
    pub async fn validate_authorization_header(
        &self,
        header_value: &str,
    ) -> RiverbaseResult<Principal> {
        let token = header_value
            .strip_prefix("Bearer ")
            .or_else(|| header_value.strip_prefix("bearer "))
            .ok_or_else(|| {
                crate::errors::AUT_038.with_data(json!({
                    "authorization_header": header_value,
                }))
            })?;
        self.validate_token(token.trim()).await
    }

    async fn decoding_key_for_kid(&self, kid: &str) -> RiverbaseResult<jsonwebtoken::DecodingKey> {
        self.ensure_jwks().await?;
        if let Some(key) = self.jwks.read().await.get(kid).cloned() {
            return Ok(key);
        }
        let should_refresh = {
            let last = self.last_unknown_kid_refresh.read().await;
            last.map(|at| at.elapsed().as_secs() >= 5).unwrap_or(true)
        };
        if should_refresh {
            *self.last_unknown_kid_refresh.write().await = Some(Instant::now());
            self.refresh_jwks().await?;
        }
        self.jwks
            .read()
            .await
            .get(kid)
            .cloned()
            .ok_or_else(|| crate::errors::AUT_035.with_data(json!({ "kid": kid })))
    }

    async fn ensure_jwks(&self) -> RiverbaseResult<()> {
        let stale = {
            let cache = self.jwks.read().await;
            cache.is_stale(self.config.jwks_ttl_secs)
        };
        if stale {
            self.refresh_jwks().await?;
        }
        Ok(())
    }

    async fn refresh_jwks(&self) -> RiverbaseResult<()> {
        let oidc = self.oidc_config().await?;
        let doc = fetch_jwks(&oidc.jwks_uri, &self.client).await?;
        let keys = jwks_to_decoding_keys(&doc)?;
        let mut cache = self.jwks.write().await;
        cache.replace(keys);
        Ok(())
    }

    async fn oidc_config(&self) -> RiverbaseResult<OpenIdConfiguration> {
        {
            let guard = self.oidc.read().await;
            if let Some(doc) = guard.as_ref() {
                return Ok(doc.clone());
            }
        }
        let mut doc =
            fetch_openid_configuration(&self.config.discovery_url(), &self.client).await?;
        let expected_issuer = self.config.issuer.trim_end_matches('/');
        let actual_issuer = doc.issuer.trim_end_matches('/');
        let internal = std::env::var("GFS_OIDC_INTERNAL_ORIGIN")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty());
        let issuer_ok =
            actual_issuer == expected_issuer || internal.as_deref() == Some(actual_issuer);
        if !issuer_ok {
            return Err(crate::errors::AUT_039.with_data(json!({
                "expected_issuer": expected_issuer,
                "actual_issuer": doc.issuer,
            })));
        }
        doc.jwks_uri = rewrite_internal_oidc_url(&doc.jwks_uri);
        let mut guard = self.oidc.write().await;
        *guard = Some(doc.clone());
        Ok(doc)
    }
}

/// Bearer-path `azp` check ([SEC-02]). `accepted` must be non-empty.
pub fn azp_is_accepted(claims: &Value, accepted: &[String]) -> bool {
    if let Some(azp) = claims.get("azp").and_then(Value::as_str) {
        return accepted.iter().any(|allowed| allowed == azp);
    }
    // Gitea ID tokens omit `azp` and put the client id in `aud`.
    match claims.get("aud") {
        Some(Value::String(aud)) => accepted.iter().any(|allowed| allowed == aud),
        Some(Value::Array(auds)) => auds
            .iter()
            .filter_map(Value::as_str)
            .any(|aud| accepted.iter().any(|allowed| allowed == aud)),
        _ => false,
    }
}

fn ensure_accepted_azp(claims: &Value, accepted: &[String]) -> RiverbaseResult<()> {
    if azp_is_accepted(claims, accepted) {
        return Ok(());
    }
    Err(crate::errors::AUT_147.with_data(format!(
        "JWT azp is not in the accepted list (got azp={:?} aud={:?})",
        claims.get("azp"),
        claims.get("aud")
    )))
}

/// Map JWT decode failures to AUT-146 (expired) or AUT-144 (malformed/invalid).
pub(crate) fn decode_jwt_error_code(cause: &str) -> &'static str {
    if cause.contains("ExpiredSignature") || cause.to_ascii_lowercase().contains("expired") {
        "AUT-146"
    } else {
        "AUT-144"
    }
}

fn decode_jwt_error(cause: &str) -> RiverbaseError {
    if decode_jwt_error_code(cause) == "AUT-146" {
        crate::errors::AUT_146.with_data(json!({ "detail": cause }))
    } else {
        crate::errors::AUT_144.with_data(json!({ "detail": cause }))
    }
}

#[cfg(test)]
mod tests {
    use super::{azp_is_accepted, decode_jwt_error_code};
    use serde_json::json;

    #[test]
    fn azp_rejects_other_client() {
        let claims = json!({ "azp": "other-client" });
        assert!(!azp_is_accepted(&claims, &["sample_app".to_string()]));
    }

    #[test]
    fn azp_accepts_configured_client() {
        let claims = json!({ "azp": "sample_app" });
        assert!(azp_is_accepted(&claims, &["sample_app".to_string()]));
    }

    #[test]
    fn azp_accepts_gitea_aud_when_azp_absent() {
        let claims = json!({ "aud": "gitea-client" });
        assert!(azp_is_accepted(&claims, &["gitea-client".to_string()]));
        let claims = json!({ "aud": ["gitea-client", "other"] });
        assert!(azp_is_accepted(&claims, &["gitea-client".to_string()]));
        assert!(!azp_is_accepted(&claims, &["gfs".to_string()]));
    }

    #[test]
    fn expired_signature_maps_to_aut_146() {
        assert_eq!(decode_jwt_error_code("ExpiredSignature"), "AUT-146");
        assert_eq!(decode_jwt_error_code("Error: token has expired"), "AUT-146");
        assert_eq!(decode_jwt_error_code("InvalidSignature"), "AUT-144");
        assert_eq!(decode_jwt_error_code("ImmatureSignature"), "AUT-144");
    }
}
