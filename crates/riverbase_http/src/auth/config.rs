use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::Principal;
use crate::base::RiverbaseError;
pub use crate::config::AuthConfig;

/// Scheme prefix for Python-compatible mock identity headers (`Authorization: MockAuth-…`).
pub const MOCK_AUTH_SCHEME: &str = "MockAuth-";

/// OIDC provider settings (Keycloak realm, Auth0 tenant, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcConfig {
    /// Issuer URL (realm issuer), e.g. `https://keycloak.example/realms/myrealm`.
    pub issuer: String,
    /// Expected JWT audience (`aud`). Omit to skip audience validation.
    #[serde(default)]
    pub audience: Option<String>,
    /// Accepted JWT `azp` values for bearer access tokens ([SEC-02]).
    #[serde(default)]
    pub accepted_azp: Vec<String>,
    /// JWKS cache TTL in seconds (default 300).
    #[serde(default = "default_jwks_ttl_secs")]
    pub jwks_ttl_secs: u64,
    #[serde(default = "default_leeway_secs")]
    /// Leeway secs.
    pub leeway_secs: u64,
    /// HTTP timeout for discovery/JWKS fetch in seconds (default 10).
    #[serde(default = "default_http_timeout_secs")]
    pub http_timeout_secs: u64,
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

impl OidcConfig {
    /// Keycloak.
    pub fn keycloak(realm_url: impl Into<String>, audience: Option<String>) -> Self {
        Self {
            issuer: realm_url.into(),
            audience,
            accepted_azp: Vec::new(),
            jwks_ttl_secs: default_jwks_ttl_secs(),
            leeway_secs: default_leeway_secs(),
            http_timeout_secs: default_http_timeout_secs(),
        }
    }

    /// Discovery url.
    pub fn discovery_url(&self) -> String {
        if let Ok(url) = std::env::var("RIVERBASE_OIDC_DISCOVERY_URL") {
            let url = url.trim();
            if !url.is_empty() {
                return url.to_string();
            }
        }
        let issuer = self.issuer.trim_end_matches('/');
        format!("{issuer}/.well-known/openid-configuration")
    }
}

/// Rewrite a public OIDC URL onto `GFS_OIDC_INTERNAL_ORIGIN` for server-side fetches.
pub fn rewrite_internal_oidc_url(url: &str) -> String {
    let Ok(internal) = std::env::var("GFS_OIDC_INTERNAL_ORIGIN") else {
        return url.to_string();
    };
    let internal = internal.trim().trim_end_matches('/');
    if internal.is_empty() {
        return url.to_string();
    }
    for public in [
        "https://gitsfusion.localhost",
        "http://gitsfusion.localhost",
        "http://localhost:8080",
        "http://gitsfusion.com:8080",
        "http://gitfusions.com:8080",
    ] {
        if let Some(rest) = url.strip_prefix(public) {
            return format!("{internal}{rest}");
        }
    }
    url.to_string()
}

impl Default for OidcConfig {
    fn default() -> Self {
        Self {
            issuer: String::new(),
            audience: None,
            accepted_azp: Vec::new(),
            jwks_ttl_secs: default_jwks_ttl_secs(),
            leeway_secs: default_leeway_secs(),
            http_timeout_secs: default_http_timeout_secs(),
        }
    }
}

/// Build [`OidcConfig`] for JWT validation from auth settings.
pub fn to_oidc_config(config: &AuthConfig) -> OidcConfig {
    let mut accepted_azp = config.accepted_azp.clone();
    for extra in [
        config.effective_audience(),
        Some(config.oauth2_client_id.trim())
            .filter(|id| !id.is_empty())
            .map(str::to_string),
    ]
    .into_iter()
    .flatten()
    {
        if !accepted_azp.iter().any(|allowed| allowed == &extra) {
            accepted_azp.push(extra);
        }
    }
    OidcConfig {
        issuer: config.effective_issuer(),
        audience: config.effective_audience(),
        accepted_azp,
        jwks_ttl_secs: config.jwks_ttl_secs,
        leeway_secs: config.leeway_secs,
        http_timeout_secs: config.http_timeout_secs,
    }
}

/// Synthetic principal for local/dev when [`AuthProvider::MockAuth`] is selected.
pub fn mock_principal(config: &AuthConfig) -> Option<Principal> {
    if !config.uses_mock_auth() {
        return None;
    }
    let sub = if config.mock_sub.trim().is_empty() {
        "dev-user".to_string()
    } else {
        config.mock_sub.clone()
    };
    let roles = config.mock_realm_access.clone();
    let mut claims = json!({
        "sub": sub,
        "preferred_username": config.mock_username,
        "email": config.mock_email,
        "given_name": config.mock_given_name,
        "family_name": config.mock_family_name,
        "roles": roles,
        "realm_access": { "roles": roles },
    });
    // Query `scope_policy` (org_scope) reads these JWT claims. `/auth/info` already
    // exposes mock_org_id on the profile; omitting it here yields 403 QRY-124.
    if let Some(obj) = claims.as_object_mut() {
        let org = config.mock_org_id.trim();
        if !org.is_empty() {
            obj.insert("org_id".into(), json!(org));
            obj.insert("organization_id".into(), json!(org));
        }
        let tenant = config.mock_tenant.trim();
        let tenant = if tenant.is_empty() { org } else { tenant };
        if !tenant.is_empty() {
            obj.insert("_tenant".into(), json!(tenant));
        }
        let org_codes = if !config.mock_org_codes.is_empty() {
            config.mock_org_codes.clone()
        } else if !config.mock_org_name.trim().is_empty() {
            vec![config.mock_org_name.clone()]
        } else {
            Vec::new()
        };
        if !org_codes.is_empty() {
            obj.insert("org_codes".into(), json!(org_codes));
        }
    }
    Some(Principal {
        sub,
        preferred_username: config.mock_username.clone(),
        email: config.mock_email.clone(),
        roles: roles.clone(),
        iam_roles: roles,
        claims,
    })
}

/// Encode a principal as `Authorization: MockAuth-<base64url(JSON claims)>`.
pub fn encode_mock_auth_header(principal: &Principal) -> String {
    let mut claims = if principal.claims.is_object() {
        principal.claims.clone()
    } else {
        json!({})
    };
    if let Some(obj) = claims.as_object_mut() {
        obj.insert("sub".into(), json!(principal.sub.clone()));
        if let Some(username) = &principal.preferred_username {
            obj.entry("preferred_username")
                .or_insert_with(|| json!(username));
        }
        if let Some(email) = &principal.email {
            obj.entry("email").or_insert_with(|| json!(email));
        }
        if !principal.roles.is_empty() {
            obj.entry("roles").or_insert_with(|| json!(principal.roles));
        }
    }
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&claims).unwrap_or_else(|_| b"{}".to_vec()));
    format!("{MOCK_AUTH_SCHEME}{encoded}")
}

/// Resolve the mock principal for a request.
///
/// - no `Authorization` header → `Ok(None)` (anonymous; session may still apply)
/// - `Authorization: MockAuth-<base64url(JSON claims)>` → principal from claims
/// - any other scheme → error (`AUT-172`)
pub fn resolve_mock_principal(
    authorization: Option<&str>,
) -> Result<Option<Principal>, RiverbaseError> {
    let Some(header) = authorization.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    if !header.starts_with(MOCK_AUTH_SCHEME) {
        return Err(crate::errors::AUT_172.with_data(json!({
            "detail": "Authorization header must use MockAuth scheme (or be omitted)."
        })));
    }

    let encoded = header[MOCK_AUTH_SCHEME.len()..].trim();
    if encoded.is_empty() {
        return Err(crate::errors::AUT_172
            .with_data(json!({ "detail": "MockAuth header is missing claims payload." })));
    }

    let bytes = decode_mock_auth_payload(encoded).map_err(|detail| {
        crate::errors::AUT_172
            .with_data(json!({ "detail": format!("Invalid MockAuth base64 payload: {detail}") }))
    })?;
    let claims: Value = serde_json::from_slice(&bytes).map_err(|e| {
        crate::errors::AUT_172
            .with_data(json!({ "detail": format!("Invalid MockAuth JSON claims: {e}") }))
    })?;
    principal_from_mock_claims(&claims).map(Some)
}

/// Build a [`Principal`] from MockAuth JSON claims (Keycloak-shaped or flat).
pub fn principal_from_mock_claims(claims: &Value) -> Result<Principal, RiverbaseError> {
    let sub = claims
        .get("sub")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            crate::errors::AUT_172
                .with_data(json!({ "detail": "MockAuth claims must include a non-empty `sub`." }))
        })?
        .to_string();

    let preferred_username = claims
        .get("preferred_username")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let email = claims
        .get("email")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let roles = roles_from_mock_claims(claims);
    let iam_roles = realm_roles_from_claims(claims);

    Ok(Principal {
        sub,
        preferred_username,
        email,
        roles,
        iam_roles,
        claims: claims.clone(),
    })
}

fn realm_roles_from_claims(claims: &Value) -> Vec<String> {
    claims
        .pointer("/realm_access/roles")
        .and_then(|roles| roles.as_array())
        .map(|roles| {
            roles
                .iter()
                .filter_map(|role| role.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn roles_from_mock_claims(claims: &Value) -> Vec<String> {
    if let Some(roles) = claims.get("roles").and_then(|v| v.as_array()) {
        return roles
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    claims
        .pointer("/realm_access/roles")
        .and_then(|v| v.as_array())
        .map(|roles| {
            roles
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn decode_mock_auth_payload(encoded: &str) -> Result<Vec<u8>, String> {
    // Python `urlsafe_b64decode` accepts missing padding and url-safe alphabet.
    let mut padded = encoded.replace('-', "+").replace('_', "/");
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    use base64::engine::general_purpose::STANDARD;
    STANDARD
        .decode(padded.as_bytes())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthProvider;
    use base64::engine::general_purpose::URL_SAFE;

    fn default_principal() -> Principal {
        let mut config = AuthConfig::default();
        config.auth_provider = AuthProvider::MockAuth;
        config.mock_sub = "dev-user".into();
        config.mock_username = Some("dev".into());
        config.mock_realm_access = vec!["viewer".into()];
        mock_principal(&config).expect("default mock principal")
    }

    #[test]
    fn mock_principal_stamps_org_id_claims() {
        let mut config = AuthConfig::default();
        config.auth_provider = AuthProvider::MockAuth;
        config.mock_sub = "dev-user".into();
        config.mock_org_id = "00000000-0000-4000-8000-000000000011".into();
        config.mock_realm_access = vec!["gfs_manager".into()];
        let principal = mock_principal(&config).expect("mock principal");
        assert_eq!(
            principal.claims.get("org_id").and_then(|v| v.as_str()),
            Some("00000000-0000-4000-8000-000000000011")
        );
        assert_eq!(
            principal
                .claims
                .get("organization_id")
                .and_then(|v| v.as_str()),
            Some("00000000-0000-4000-8000-000000000011")
        );
        assert_eq!(
            principal.claims.get("_tenant").and_then(|v| v.as_str()),
            Some("00000000-0000-4000-8000-000000000011")
        );
        assert_eq!(
            principal.tenant(),
            Some(uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000011").unwrap())
        );
    }

    #[test]
    fn mock_principal_prefers_mock_tenant_over_org_id() {
        let mut config = AuthConfig::default();
        config.auth_provider = AuthProvider::MockAuth;
        config.mock_sub = "dev-user".into();
        config.mock_org_id = "00000000-0000-4000-8000-000000000021".into();
        config.mock_tenant = "00000000-0000-4000-8000-000000000011".into();
        let principal = mock_principal(&config).expect("mock principal");
        assert_eq!(
            principal.claims.get("org_id").and_then(|v| v.as_str()),
            Some("00000000-0000-4000-8000-000000000021")
        );
        assert_eq!(
            principal.claims.get("_tenant").and_then(|v| v.as_str()),
            Some("00000000-0000-4000-8000-000000000011")
        );
        assert_eq!(
            principal.tenant(),
            Some(uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000011").unwrap())
        );
    }

    #[test]
    fn mock_principal_stamps_org_codes() {
        let mut config = AuthConfig::default();
        config.auth_provider = AuthProvider::MockAuth;
        config.mock_sub = "dev-user".into();
        config.mock_org_codes = vec!["NWL".into()];
        let principal = mock_principal(&config).expect("mock principal");
        assert_eq!(
            principal.claims.get("org_codes").and_then(|v| v.as_array()),
            Some(&vec![json!("NWL")])
        );
    }

    #[test]
    fn missing_header_is_anonymous() {
        let resolved = resolve_mock_principal(None).expect("ok");
        assert!(resolved.is_none());
    }

    #[test]
    fn mock_auth_header_switches_identity() {
        let claims = json!({
            "sub": "11111111-1111-1111-1111-111111111111",
            "preferred_username": "alice",
            "email": "alice@example.com",
            "realm_access": { "roles": ["widget-admin"] }
        });
        let encoded = URL_SAFE.encode(serde_json::to_vec(&claims).unwrap());
        let header = format!("MockAuth-{encoded}");
        let resolved = resolve_mock_principal(Some(&header)).expect("ok");
        assert_eq!(
            resolved.as_ref().unwrap().sub,
            "11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(
            resolved.as_ref().unwrap().preferred_username.as_deref(),
            Some("alice")
        );
        assert_eq!(
            resolved.as_ref().unwrap().roles,
            vec!["widget-admin".to_string()]
        );
        assert_eq!(
            resolved.as_ref().unwrap().iam_roles,
            vec!["widget-admin".to_string()]
        );
    }

    #[test]
    fn encode_mock_auth_header_round_trips() {
        let principal = default_principal();
        let header = encode_mock_auth_header(&principal);
        let resolved = resolve_mock_principal(Some(&header))
            .expect("ok")
            .expect("principal");
        assert_eq!(resolved.sub, principal.sub);
        assert_eq!(resolved.roles, principal.roles);
    }

    #[test]
    fn bearer_scheme_is_rejected() {
        let err = resolve_mock_principal(Some("Bearer abc")).expect_err("reject");
        assert_eq!(err.http_status, 400);
        assert_eq!(err.errcode.as_str(), "AUT-172");
    }

    #[test]
    fn claims_without_sub_are_rejected() {
        let encoded = URL_SAFE.encode(br#"{"preferred_username":"x"}"#);
        let header = format!("MockAuth-{encoded}");
        let err = resolve_mock_principal(Some(&header)).expect_err("reject");
        assert_eq!(err.errcode.as_str(), "AUT-172");
    }

    #[test]
    fn oidc_config_accepts_gitea_client_id_as_azp() {
        let mut config = AuthConfig::default();
        config.accepted_azp = vec!["gfs".into()];
        config.oauth2_client_id = "df9b55ac-8ede-4106-a4da-d34f030fe37f".into();
        let oidc = to_oidc_config(&config);
        assert!(oidc
            .accepted_azp
            .iter()
            .any(|v| v == "df9b55ac-8ede-4106-a4da-d34f030fe37f"));
        assert!(oidc.accepted_azp.iter().any(|v| v == "gfs"));
    }
}
