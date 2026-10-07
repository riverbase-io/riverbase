use std::sync::Arc;

use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum_extra::extract::CookieJar;
use serde_json::Value;
use tower_sessions::Session;

use super::config::AuthConfig;
use super::context::{
    AuthorizationContext, KeycloakTokenPayload, SessionOrganization, SessionProfile,
};
use super::profile_resolution::{
    resolve_profile_hint, resolve_profile_hint_from_header, AuthMode, SetupContextRequest,
};
use super::validator::JwtValidator;
use crate::base::RiverbaseResult;
use uuid::Uuid;

/// Lists profiles for the authenticated user (`GET /auth/profiles`).
#[async_trait::async_trait]
pub trait AuthProfileProvider: Send + Sync {
    /// Resolve authorization context from Keycloak token claims (profile + organization from IDM).
    async fn setup_context(
        &self,
        request: SetupContextRequest,
    ) -> RiverbaseResult<AuthorizationContext>;

    /// List profiles.
    async fn list_profiles(
        &self,
        ctx: &AuthorizationContext,
        status: Option<&str>,
    ) -> RiverbaseResult<Vec<Value>>;

    /// Ensure the authenticated user has a resolvable profile after OAuth login.
    ///
    /// Default is a no-op. Domain providers should upsert/sync the profile from
    /// Keycloak claims. Login fails when this returns an error.
    async fn upsert_on_login(&self, auth_user: &KeycloakTokenPayload) -> RiverbaseResult<()> {
        let _ = auth_user;
        Ok(())
    }

    /// Switch active profile for the authenticated user (DB + optional session).
    async fn switch_profile(
        &self,
        request: SetupContextRequest,
        profile_id: Uuid,
    ) -> RiverbaseResult<AuthorizationContext> {
        let _ = (request, profile_id);
        Err(crate::errors::AUT_170.with_data(serde_json::json!({})))
    }
}

/// No-op profile listing (until a domain-backed provider is wired).
pub struct EmptyAuthProfileProvider;

#[async_trait::async_trait]
impl AuthProfileProvider for EmptyAuthProfileProvider {
    async fn setup_context(
        &self,
        _request: SetupContextRequest,
    ) -> RiverbaseResult<AuthorizationContext> {
        Err(crate::errors::AUT_106.with_data(serde_json::json!({})))
    }

    async fn list_profiles(
        &self,
        _ctx: &AuthorizationContext,
        _status: Option<&str>,
    ) -> RiverbaseResult<Vec<Value>> {
        Ok(Vec::new())
    }
}

/// Default auth profile provider (Python `RiverbaseAuthProfileProvider`).
#[derive(Clone)]
pub struct DefaultAuthProfileProvider {
    config: AuthConfig,
    validator: Arc<JwtValidator>,
}

impl DefaultAuthProfileProvider {
    /// Construct a new value.
    pub fn new(config: AuthConfig, validator: Arc<JwtValidator>) -> Self {
        Self { config, validator }
    }

    /// Config.
    pub fn config(&self) -> &AuthConfig {
        &self.config
    }

    /// Validator.
    pub fn validator(&self) -> &JwtValidator {
        &self.validator
    }

    /// Authorize raw claims into a typed Keycloak payload.
    pub fn authorize_claims(&self, claims: Value) -> RiverbaseResult<KeycloakTokenPayload> {
        KeycloakTokenPayload::from_claims(claims)
            .map_err(|_e| crate::errors::AUT_104.with_data(serde_json::json!({})))
    }

    /// Extract auth token claims and how they were obtained.
    pub async fn get_auth_token_with_mode(
        &self,
        parts: &Parts,
        session: Option<&Session>,
        cookies: &CookieJar,
    ) -> RiverbaseResult<Option<(Value, AuthMode)>> {
        let cfg = self.config();
        if let Some(session) = session {
            if cookies.get(&cfg.ses_id_token_field).is_some() {
                if let Ok(Some(user)) = session.get::<Value>(&cfg.ses_user_field).await {
                    return Ok(Some((user, AuthMode::Session)));
                }
            }
        }
        let Some(auth_header) = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(None);
        };
        if auth_header.to_ascii_lowercase().starts_with("bearer ") {
            let token = auth_header[7..].trim();
            let claims = self.validator().decode_access_token_claims(token).await?;
            return Ok(Some((claims.claims, AuthMode::Bearer)));
        }
        Ok(None)
    }

    /// Extract auth token claims from session (cookie+user) or Bearer header.
    pub async fn get_auth_token(
        &self,
        parts: &Parts,
        session: Option<&Session>,
        cookies: &CookieJar,
    ) -> RiverbaseResult<Option<Value>> {
        Ok(self
            .get_auth_token_with_mode(parts, session, cookies)
            .await?
            .map(|(claims, _)| claims))
    }

    /// Build authorization context from resolved profile and organization (Keycloak auth).
    pub fn authorization_context_from_profile(
        auth_user: KeycloakTokenPayload,
        profile: SessionProfile,
        organization: SessionOrganization,
    ) -> AuthorizationContext {
        let realm_roles: Vec<String> = auth_user
            .realm_access
            .as_ref()
            .and_then(|v| v.get("roles"))
            .and_then(|r| serde_json::from_value(r.clone()).ok())
            .unwrap_or_default();

        let mut client_roles = Vec::new();
        if let Some(ra) = auth_user.resource_access.as_ref() {
            if let Some(obj) = ra.as_object() {
                for (_client, access) in obj {
                    if let Some(roles) = access.get("roles").and_then(|r| r.as_array()) {
                        for role in roles {
                            if let Some(s) = role.as_str() {
                                client_roles.push(s.to_string());
                            }
                        }
                    }
                }
            }
        }

        let mut all_roles: Vec<String> = realm_roles
            .iter()
            .chain(client_roles.iter())
            .cloned()
            .collect();
        all_roles.sort();
        all_roles.dedup();

        let iamroles: Vec<String> = realm_roles
            .iter()
            .filter(|r| matches!(r.as_str(), "sysadmin" | "operator" | "admin"))
            .cloned()
            .collect();

        let realm = auth_user
            .iss
            .rsplit('/')
            .next()
            .unwrap_or("default")
            .to_string();

        let mut profile = profile;
        if profile.roles.is_empty() {
            profile.roles = all_roles;
        }

        let mut auth_user = auth_user;
        auth_user.profile_id = Some(profile.id);
        let tenant = organization.id;

        AuthorizationContext {
            realm,
            user: Some(auth_user),
            profile: Some(profile),
            organization: Some(organization),
            iamroles,
            tenant: Some(tenant),
        }
    }

    /// Resolve full authorization context for a request.
    pub async fn get_auth_context(
        &self,
        profile_provider: &dyn AuthProfileProvider,
        parts: &Parts,
        session: Option<&Session>,
        cookies: &CookieJar,
    ) -> RiverbaseResult<Option<AuthorizationContext>> {
        let Some((token, auth_mode)) = self
            .get_auth_token_with_mode(parts, session, cookies)
            .await?
        else {
            return Ok(None);
        };
        let auth_user = self.authorize_claims(token)?;
        super::realm_role::require_configured_realm_role(
            &auth_user,
            &self.config().require_realm_role,
        )?;
        let profile_hint = match resolve_profile_hint_from_header(parts)? {
            Some(id) => Some(id),
            None => {
                resolve_profile_hint(&self.config, parts, session, auth_mode, &auth_user).await?
            }
        };
        profile_provider
            .setup_context(SetupContextRequest {
                auth_user,
                auth_mode,
                profile_hint,
            })
            .await
            .map(Some)
    }
}

#[async_trait::async_trait]
impl AuthProfileProvider for DefaultAuthProfileProvider {
    async fn setup_context(
        &self,
        _request: SetupContextRequest,
    ) -> RiverbaseResult<AuthorizationContext> {
        Err(crate::errors::AUT_106.with_data(serde_json::json!({})))
    }

    async fn list_profiles(
        &self,
        _ctx: &AuthorizationContext,
        _status: Option<&str>,
    ) -> RiverbaseResult<Vec<Value>> {
        Ok(Vec::new())
    }
}
#[cfg(test)]
mod profile_resolution_tests {
    use super::*;
    use crate::auth::profile_resolution::resolve_profile_hint;
    use crate::config::AuthConfig;
    use axum::http::Request;
    use uuid::Uuid;

    #[tokio::test]
    async fn bearer_mode_uses_jwt_profile_id_as_hint() {
        let cfg = AuthConfig::default();
        let profile_id = Uuid::new_v4();
        let auth_user = KeycloakTokenPayload {
            profile_id: Some(profile_id),
            ..minimal_keycloak_user()
        };
        let parts = Request::builder().body(()).unwrap().into_parts().0;
        let hint = resolve_profile_hint(&cfg, &parts, None, AuthMode::Bearer, &auth_user)
            .await
            .expect("hint");
        assert_eq!(hint, Some(profile_id));
    }

    #[tokio::test]
    async fn session_mode_ignores_jwt_profile_id_without_header() {
        let cfg = AuthConfig::default();
        let auth_user = KeycloakTokenPayload {
            profile_id: Some(Uuid::new_v4()),
            ..minimal_keycloak_user()
        };
        let parts = Request::builder().body(()).unwrap().into_parts().0;
        let hint = resolve_profile_hint(&cfg, &parts, None, AuthMode::Session, &auth_user)
            .await
            .expect("hint");
        assert_eq!(hint, None);
    }

    #[tokio::test]
    async fn malformed_x_profile_is_aut_174() {
        let cfg = AuthConfig::default();
        let auth_user = minimal_keycloak_user();
        let parts = Request::builder()
            .header("x-profile", "not-a-uuid")
            .body(())
            .unwrap()
            .into_parts()
            .0;
        let err = resolve_profile_hint(&cfg, &parts, None, AuthMode::Bearer, &auth_user)
            .await
            .expect_err("malformed");
        assert_eq!(err.errcode.as_str(), "AUT-174");
    }

    fn minimal_keycloak_user() -> KeycloakTokenPayload {
        KeycloakTokenPayload {
            exp: 0,
            iat: 0,
            auth_time: None,
            jti: Uuid::nil(),
            iss: "https://kc/realms/test".into(),
            aud: "app".into(),
            sub: Uuid::new_v4(),
            typ: None,
            azp: None,
            nonce: None,
            session_state: None,
            at_hash: None,
            acr: None,
            sid: Uuid::nil(),
            profile_id: None,
            email_verified: None,
            name: None,
            preferred_username: None,
            given_name: None,
            family_name: None,
            email: None,
            phone: None,
            realm_access: None,
            resource_access: None,
            session_id: None,
            client_token: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn authorization_context_from_profile_maps_iamroles() {
        let user = KeycloakTokenPayload {
            exp: 0,
            iat: 0,
            auth_time: None,
            jti: uuid::Uuid::nil(),
            iss: "https://kc/realms/dev-1.riverbase.io".into(),
            aud: "app".into(),
            sub: uuid::Uuid::nil(),
            typ: None,
            azp: None,
            nonce: None,
            session_state: None,
            at_hash: None,
            acr: None,
            sid: uuid::Uuid::nil(),
            profile_id: None,
            email_verified: None,
            name: Some("Test".into()),
            preferred_username: Some("user".into()),
            given_name: None,
            family_name: Some("Org".into()),
            email: None,
            phone: None,
            realm_access: Some(json!({"roles": ["admin", "viewer"]})),
            resource_access: None,
            session_id: None,
            client_token: None,
        };
        let profile = SessionProfile {
            id: uuid::Uuid::nil(),
            name: user.name.clone(),
            family_name: user.family_name.clone(),
            given_name: user.given_name.clone(),
            email: user.email.clone(),
            username: user.preferred_username.clone(),
            roles: vec!["admin".into(), "viewer".into()],
            org_id: Some(uuid::Uuid::nil()),
            usr_id: Some(uuid::Uuid::nil()),
        };
        let organization = SessionOrganization {
            id: uuid::Uuid::nil(),
            name: Some("Org".into()),
        };
        let ctx = DefaultAuthProfileProvider::authorization_context_from_profile(
            user,
            profile,
            organization,
        );
        assert_eq!(ctx.iamroles, vec!["admin"]);
        assert_eq!(ctx.realm, "dev-1.riverbase.io");
        assert_eq!(ctx.tenant, Some(uuid::Uuid::nil()));
    }

    #[test]
    fn authorization_context_from_profile_keeps_idm_org_roles() {
        let user = KeycloakTokenPayload {
            exp: 0,
            iat: 0,
            auth_time: None,
            jti: uuid::Uuid::nil(),
            iss: "https://kc/realms/dev.entry.express".into(),
            aud: "app".into(),
            sub: uuid::Uuid::nil(),
            typ: None,
            azp: None,
            nonce: None,
            session_state: None,
            at_hash: None,
            acr: None,
            sid: uuid::Uuid::nil(),
            profile_id: None,
            email_verified: None,
            name: Some("Test".into()),
            preferred_username: Some("user".into()),
            given_name: None,
            family_name: None,
            email: None,
            phone: None,
            realm_access: Some(json!({"roles": ["offline_access", "uma_authorization"]})),
            resource_access: Some(json!({"account": {"roles": ["manage-account"]}})),
            session_id: None,
            client_token: None,
        };
        let profile = SessionProfile {
            id: uuid::Uuid::nil(),
            name: user.name.clone(),
            family_name: None,
            given_name: None,
            email: None,
            username: user.preferred_username.clone(),
            roles: vec!["coordinator_admin".into(), "seller_admin".into()],
            org_id: Some(uuid::Uuid::nil()),
            usr_id: Some(uuid::Uuid::nil()),
        };
        let organization = SessionOrganization {
            id: uuid::Uuid::nil(),
            name: Some("Org".into()),
        };
        let ctx = DefaultAuthProfileProvider::authorization_context_from_profile(
            user,
            profile,
            organization,
        );
        assert_eq!(
            ctx.profile.unwrap().roles,
            vec!["coordinator_admin", "seller_admin"]
        );
    }
}
