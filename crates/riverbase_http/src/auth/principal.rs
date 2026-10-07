use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::base::AuditActor;

/// Normalized authenticated subject for authorization and audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    /// Subject identifier (`sub` claim).
    pub sub: String,
    /// Preferred username when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    /// Email when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Profile-store roles used for command/query `roles_required` ([D12]).
    #[serde(default)]
    pub roles: Vec<String>,
    /// IAM realm roles from the token (`realm_access.roles`). Reserved for
    /// `api_zone` authorization — must not grant command authority.
    #[serde(default)]
    pub iam_roles: Vec<String>,
    /// Raw JWT claims for advanced policies (full token payload).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub claims: Value,
}

impl Principal {
    /// Subject.
    pub fn subject(&self) -> &str {
        &self.sub
    }

    /// Has role.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Tenant identifier for data scoping (`_tenant` claim).
    ///
    /// When `_tenant` is omitted, uses the logged-in organization id
    /// (`organization_id` / `org_id`).
    pub fn tenant(&self) -> Option<Uuid> {
        if let Some(value) = self.claims.get("_tenant") {
            return value.as_str().and_then(|s| Uuid::parse_str(s).ok());
        }
        self.claims
            .get("organization_id")
            .and_then(|value| value.as_str())
            .or_else(|| self.claims.get("org_id").and_then(|value| value.as_str()))
            .and_then(|s| Uuid::parse_str(s).ok())
    }

    /// Build an [`AuditActor`] for command execution audit columns.
    ///
    /// Prefers explicit profile claims (`profile_id`, …) then parses `sub` as UUID.
    /// `tenant` is stamped into `_tenant` on persisted rows.
    pub fn to_audit_actor(&self, tenant: Option<Uuid>) -> AuditActor {
        AuditActor::from_subject_and_claims(
            &self.sub,
            &self.claims,
            tenant.or_else(|| self.tenant()),
        )
    }

    #[cfg(feature = "auth")]
    /// Build from auth context.
    pub fn from_auth_context(ctx: &super::context::AuthorizationContext) -> Option<Self> {
        let profile_id = ctx.audit_profile_id()?;
        Some(Self {
            sub: profile_id.to_string(),
            preferred_username: ctx.profile.as_ref().and_then(|p| p.username.clone()),
            email: ctx.profile.as_ref().and_then(|p| p.email.clone()),
            roles: ctx
                .profile
                .as_ref()
                .map(|p| p.roles.clone())
                .unwrap_or_default(),
            iam_roles: ctx.iamroles.clone(),
            claims: {
                let mut claims = ctx
                    .user
                    .as_ref()
                    .and_then(|u| serde_json::to_value(u).ok())
                    .unwrap_or_else(|| json!({}));
                if let Some(obj) = claims.as_object_mut() {
                    obj.insert("profile_id".into(), json!(profile_id.to_string()));
                    let org_id = ctx
                        .profile
                        .as_ref()
                        .and_then(|p| p.org_id)
                        .or_else(|| ctx.organization.as_ref().map(|o| o.id));
                    if let Some(org_id) = org_id {
                        let org = org_id.to_string();
                        obj.insert("org_id".into(), json!(org));
                        obj.insert("organization_id".into(), json!(org));
                    }
                    if let Some(tenant) = ctx.tenant.or(org_id) {
                        obj.insert("_tenant".into(), json!(tenant.to_string()));
                    }
                }
                claims
            },
        })
    }

    /// Minimal identity for AUT-196 `errdata.principal`.
    pub fn to_errdata(&self) -> Value {
        json!({
            "sub": self.sub,
            "preferred_username": self.preferred_username,
            "email": self.email,
        })
    }

    /// Build a principal from validated JWT claims, preserving the full payload for audit resolution.
    pub fn from_token_claims(claims: AuthClaims, raw: Value) -> Self {
        let mut principal = Self::from(claims);
        principal.claims = raw;
        principal
    }
}

/// Standard OIDC claims used when validating access tokens.
#[derive(Debug, Deserialize)]
pub struct AuthClaims {
    /// Token subject.
    pub sub: String,
    /// Token issuer.
    pub iss: String,
    #[serde(default = "empty_audience")]
    /// Token audience.
    pub aud: Audience,
    /// Expiration time as Unix seconds.
    pub exp: i64,
    #[serde(default)]
    /// Preferred username.
    pub preferred_username: Option<String>,
    #[serde(default)]
    /// Email address.
    pub email: Option<String>,
    #[serde(default)]
    /// Active profile identifier.
    pub profile_id: Option<String>,
    #[serde(default)]
    /// Realm-level role claims.
    pub realm_access: Option<RealmAccess>,
    #[serde(default)]
    /// Resource-level role claims.
    pub resource_access: Option<Value>,
}

fn empty_audience() -> Audience {
    Audience::Many(Vec::new())
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    pub fn contains(&self, expected: &str) -> bool {
        match self {
            Audience::One(a) => a == expected,
            Audience::Many(list) => list.iter().any(|a| a == expected),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RealmAccess {
    #[serde(default)]
    pub roles: Vec<String>,
}

impl From<AuthClaims> for Principal {
    fn from(claims: AuthClaims) -> Self {
        // Bearer-path tokens carry IAM realm roles only. Profile roles are
        // populated by AuthProfileProvider → from_auth_context ([D12]).
        let iam_roles = claims.realm_access.map(|r| r.roles).unwrap_or_default();
        let mut claims_value = json!({
            "sub": claims.sub,
            "iss": claims.iss,
            "preferred_username": claims.preferred_username,
            "email": claims.email,
            "roles": iam_roles,
        });
        if let Some(profile_id) = &claims.profile_id {
            if let Some(obj) = claims_value.as_object_mut() {
                obj.insert("profile_id".into(), json!(profile_id));
            }
        }
        Self {
            sub: claims.sub,
            preferred_username: claims.preferred_username,
            email: claims.email,
            roles: Vec::new(),
            iam_roles,
            claims: claims_value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn audit_actor_prefers_profile_id_claim_over_opaque_sub() {
        let profile_id = Uuid::new_v4();
        let principal = Principal {
            sub: "opaque-keycloak-subject".into(),
            preferred_username: None,
            email: None,
            roles: vec![],
            iam_roles: vec![],
            claims: json!({ "profile_id": profile_id.to_string() }),
        };
        let tenant = Uuid::new_v4();
        let actor = principal.to_audit_actor(Some(tenant));
        assert_eq!(actor.profile_id, Some(profile_id));
        assert_eq!(actor.tenant, Some(tenant));
    }

    #[test]
    fn audit_actor_parses_uuid_sub_when_no_profile_claim() {
        let id = Uuid::new_v4();
        let principal = Principal {
            sub: id.to_string(),
            preferred_username: None,
            email: None,
            roles: vec![],
            iam_roles: vec![],
            claims: json!({ "sub": id.to_string() }),
        };
        assert_eq!(principal.to_audit_actor(None).profile_id, Some(id));
    }

    #[test]
    fn from_auth_context_stamps_org_id_from_profile() {
        let org_id = Uuid::parse_str("00000000-0000-4000-8000-000000000011").unwrap();
        let profile_id = Uuid::parse_str("b5db55cf-bd20-450e-9555-dd7ba125e9d2").unwrap();
        let ctx = crate::auth::context::AuthorizationContext {
            realm: "mock".into(),
            user: None,
            profile: Some(crate::auth::context::SessionProfile {
                id: profile_id,
                name: Some("Gfs Manager".into()),
                family_name: None,
                given_name: None,
                email: None,
                username: Some("gfs-manager".into()),
                roles: vec!["gfs_manager".into()],
                org_id: Some(org_id),
                usr_id: Some(profile_id),
            }),
            organization: Some(crate::auth::context::SessionOrganization {
                id: org_id,
                name: Some("GitsFusion Dev".into()),
            }),
            iamroles: vec![],
            tenant: Some(org_id),
        };
        let principal = Principal::from_auth_context(&ctx).expect("principal");
        assert_eq!(
            principal.claims.get("org_id").and_then(|v| v.as_str()),
            Some(org_id.to_string()).as_deref()
        );
        assert_eq!(
            principal
                .claims
                .get("organization_id")
                .and_then(|v| v.as_str()),
            Some(org_id.to_string()).as_deref()
        );
        assert_eq!(principal.tenant(), Some(org_id));
    }

    #[test]
    fn tenant_rejects_non_uuid_claim() {
        let principal = Principal {
            sub: "user".into(),
            preferred_username: None,
            email: None,
            roles: vec![],
            iam_roles: vec![],
            claims: json!({ "_tenant": "not-a-uuid" }),
        };
        assert!(principal.tenant().is_none());
    }

    #[test]
    fn tenant_falls_back_to_organization_id() {
        let org_id = Uuid::new_v4();
        let principal = Principal {
            sub: "user".into(),
            preferred_username: None,
            email: None,
            roles: vec![],
            iam_roles: vec![],
            claims: json!({ "organization_id": org_id.to_string() }),
        };
        assert_eq!(principal.tenant(), Some(org_id));
    }

    #[test]
    fn custom_auth_tenant_is_not_forced_to_organization_id() {
        let org_id = Uuid::new_v4();
        let tenant = Uuid::new_v4();
        let profile_id = Uuid::new_v4();
        let ctx = crate::auth::context::AuthorizationContext {
            realm: "mock".into(),
            user: None,
            profile: Some(crate::auth::context::SessionProfile {
                id: profile_id,
                name: None,
                family_name: None,
                given_name: None,
                email: None,
                username: None,
                roles: vec![],
                org_id: Some(org_id),
                usr_id: Some(profile_id),
            }),
            organization: Some(crate::auth::context::SessionOrganization {
                id: org_id,
                name: Some("Org".into()),
            }),
            iamroles: vec![],
            tenant: Some(tenant),
        };
        let principal = Principal::from_auth_context(&ctx).expect("principal");
        assert_eq!(principal.tenant(), Some(tenant));
        assert_eq!(ctx.to_audit_actor().tenant, Some(tenant));
    }

    #[test]
    fn from_token_claims_preserves_full_jwt_payload() {
        let profile_id = Uuid::new_v4();
        let raw = json!({
            "sub": "opaque-sub",
            "iss": "https://issuer",
            "aud": "app",
            "exp": 9999999999_i64,
            "profile_id": profile_id.to_string(),
        });
        let typed: AuthClaims = serde_json::from_value(raw.clone()).expect("claims");
        let principal = Principal::from_token_claims(typed, raw);
        assert_eq!(principal.to_audit_actor(None).profile_id, Some(profile_id));
    }
}
