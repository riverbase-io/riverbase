use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::base::profile_id_from_claims_and_sub;

/// Keycloak ID / access token claims (Python `KeycloakTokenPayload`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KeycloakTokenPayload {
    /// Expiration time as Unix seconds.
    pub exp: i64,
    /// Issued-at time as Unix seconds.
    pub iat: i64,
    #[serde(default)]
    /// Authentication time as Unix seconds, if present.
    pub auth_time: Option<i64>,
    /// JWT identifier.
    pub jti: Uuid,
    /// Token issuer.
    pub iss: String,
    /// Token audience.
    pub aud: String,
    /// Token subject.
    pub sub: Uuid,
    #[serde(default)]
    /// Token type.
    pub typ: Option<String>,
    #[serde(default)]
    /// Authorized party.
    pub azp: Option<String>,
    #[serde(default)]
    /// OIDC nonce.
    pub nonce: Option<String>,
    #[serde(default)]
    /// Keycloak session state.
    pub session_state: Option<Uuid>,
    #[serde(default)]
    /// Access-token hash.
    pub at_hash: Option<String>,
    #[serde(default)]
    /// Authentication context class reference.
    pub acr: Option<String>,
    /// Session identifier.
    pub sid: Uuid,
    #[serde(default)]
    /// Active profile identifier.
    pub profile_id: Option<Uuid>,
    #[serde(default)]
    /// Whether the email address is verified.
    pub email_verified: Option<bool>,
    #[serde(default)]
    /// Name.
    pub name: Option<String>,
    #[serde(default)]
    /// Preferred username.
    pub preferred_username: Option<String>,
    #[serde(default)]
    /// Given name.
    pub given_name: Option<String>,
    #[serde(default)]
    /// Family name.
    pub family_name: Option<String>,
    #[serde(default)]
    /// Email address.
    pub email: Option<String>,
    #[serde(default)]
    /// Phone number.
    pub phone: Option<String>,
    #[serde(default)]
    /// Realm-level role claims.
    pub realm_access: Option<Value>,
    #[serde(default)]
    /// Resource-level role claims.
    pub resource_access: Option<Value>,
    #[serde(default)]
    /// Session identifier string.
    pub session_id: Option<String>,
    #[serde(default)]
    /// Client token, if present.
    pub client_token: Option<String>,
}

fn gid_user_sub(raw: &str) -> Uuid {
    if let Ok(parsed) = Uuid::parse_str(raw) {
        return parsed;
    }
    let digest = md5::compute(format!("user:{raw}"));
    Uuid::from_bytes(digest.0)
}

impl KeycloakTokenPayload {
    /// Parse from raw JWT claims JSON (applies Keycloak cluster `node-id:uuid` jti normalization).
    /// Gitea OIDC tokens use a numeric `sub`; those map to `md5('user:' || sub)::uuid`.
    pub fn from_claims(mut claims: Value) -> Result<Self, serde_json::Error> {
        if let Some(obj) = claims.as_object_mut() {
            if let Some(sub) = obj.get("sub").cloned() {
                let raw = match sub {
                    Value::String(s) => s,
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                };
                if !raw.is_empty() && Uuid::parse_str(&raw).is_err() {
                    obj.insert("sub".into(), serde_json::json!(gid_user_sub(&raw)));
                }
            }
            if let Some(jti) = obj.get("jti").and_then(|v| v.as_str()).map(str::to_string) {
                let uuid_part = jti.split(':').next_back().unwrap_or(jti.as_str());
                if let Ok(parsed) = Uuid::parse_str(uuid_part) {
                    obj.insert("jti".into(), serde_json::json!(parsed));
                } else {
                    obj.insert("jti".into(), serde_json::json!(Uuid::nil()));
                }
            } else {
                obj.insert("jti".into(), serde_json::json!(Uuid::nil()));
            }
            match obj.get("sid") {
                Some(Value::String(s)) if Uuid::parse_str(s).is_ok() => {}
                _ => {
                    obj.insert("sid".into(), serde_json::json!(Uuid::nil()));
                }
            }
            if let Some(Value::Array(aud)) = obj.get("aud").cloned() {
                if let Some(first) = aud.first().and_then(|v| v.as_str()) {
                    obj.insert("aud".into(), serde_json::json!(first));
                }
            }
        }
        serde_json::from_value(claims)
    }
}

/// Session profile derived from token claims.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionProfile {
    /// Row identifier.
    pub id: Uuid,
    #[serde(default)]
    /// Name.
    pub name: Option<String>,
    #[serde(default)]
    /// Family name.
    pub family_name: Option<String>,
    #[serde(default)]
    /// Given name.
    pub given_name: Option<String>,
    #[serde(default)]
    /// Email address.
    pub email: Option<String>,
    #[serde(default)]
    /// Username.
    pub username: Option<String>,
    #[serde(default)]
    /// Roles.
    pub roles: Vec<String>,
    #[serde(default)]
    /// Org id.
    pub org_id: Option<Uuid>,
    #[serde(default)]
    /// Usr id.
    pub usr_id: Option<Uuid>,
}

/// Session organization (Python uses `family_name` as org name).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionOrganization {
    /// Row identifier.
    pub id: Uuid,
    #[serde(default)]
    /// Name.
    pub name: Option<String>,
}

/// Resolved authorization context for handlers and query engines.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthorizationContext {
    #[serde(default = "default_realm")]
    /// Realm.
    pub realm: String,
    #[serde(default)]
    /// User.
    pub user: Option<KeycloakTokenPayload>,
    #[serde(default)]
    /// Profile.
    pub profile: Option<SessionProfile>,
    #[serde(default)]
    /// Organization.
    pub organization: Option<SessionOrganization>,
    #[serde(default)]
    /// Iamroles.
    pub iamroles: Vec<String>,
    #[serde(default)]
    /// Tenant data scope. Set by [`super::AuthProfileProvider::setup_context`].
    /// Persisted on rows as `_tenant`.
    pub tenant: Option<Uuid>,
}

fn default_realm() -> String {
    "default-realm".into()
}

impl AuthorizationContext {
    /// Profile UUID for audit `_creator` / `_updater` columns.
    pub fn audit_profile_id(&self) -> Option<Uuid> {
        if let Some(user) = &self.user {
            if let Some(id) = user.profile_id {
                return Some(id);
            }
            if let Ok(raw) = serde_json::to_value(user) {
                if let Some(id) = profile_id_from_claims_and_sub(&raw, &user.sub.to_string()) {
                    return Some(id);
                }
            }
        }
        self.profile.as_ref().map(|p| p.id)
    }

    /// Tenant identifier for data scoping (persisted as `_tenant`).
    pub fn tenant(&self) -> Option<Uuid> {
        self.tenant
    }

    /// Build an [`AuditActor`] for command/query audit columns.
    pub fn to_audit_actor(&self) -> crate::base::AuditActor {
        let mut actor = crate::base::AuditActor::new();
        actor.profile_id = self.audit_profile_id();
        if let Some(user) = &self.user {
            actor.user_id = Some(user.sub);
        }
        if actor.user_id.is_none() {
            actor.user_id = self.profile.as_ref().and_then(|p| p.usr_id);
        }
        actor.tenant = self.tenant;
        actor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jti_strips_node_prefix() {
        let claims = serde_json::json!({
            "exp": 9999999999_i64,
            "iat": 1,
            "jti": "node-1:550e8400-e29b-41d4-a716-446655440000",
            "iss": "https://kc/realms/r",
            "aud": "app",
            "sub": "550e8400-e29b-41d4-a716-446655440001",
            "sid": "550e8400-e29b-41d4-a716-446655440002"
        });
        let payload = KeycloakTokenPayload::from_claims(claims).expect("parse");
        assert_eq!(
            payload.jti,
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
        );
    }
}
