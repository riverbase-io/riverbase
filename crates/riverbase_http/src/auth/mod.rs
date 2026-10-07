//! JWT authentication and OAuth2/OIDC relying-party routes (Keycloak-compatible).
//!
//! Validates bearer tokens using the provider JWKS, exposes a [`Principal`] for
//! downstream authorization, and mounts browser OAuth login at `/auth/*`.

mod config;
mod idempotency;
mod principal;

#[cfg(feature = "auth")]
mod context;
#[cfg(feature = "auth")]
mod csrf;
#[cfg(feature = "auth")]
mod session_helper;
#[cfg(feature = "auth")]
mod session_store;

#[cfg(feature = "auth")]
mod discovery;
#[cfg(feature = "auth")]
mod jwks;
#[cfg(feature = "auth")]
mod validator;

#[cfg(feature = "auth")]
/// Middleware; module.
pub mod middleware;

#[cfg(feature = "auth")]
mod callback_error;
#[cfg(feature = "auth")]
mod mock_idp;
#[cfg(feature = "auth")]
mod mock_routes;
#[cfg(feature = "auth")]
mod oauth;
#[cfg(feature = "auth")]
mod profile_resolution;
#[cfg(feature = "auth")]
mod provider;
#[cfg(feature = "auth")]
mod realm_role;
#[cfg(feature = "auth")]
mod routes;
#[cfg(feature = "auth")]
mod session_establish;

pub use config::{
    encode_mock_auth_header, mock_principal, principal_from_mock_claims, resolve_mock_principal,
    rewrite_internal_oidc_url, to_oidc_config, OidcConfig, MOCK_AUTH_SCHEME,
};
pub use idempotency::IdempotencyKey;
pub use principal::{AuthClaims, Principal};

#[cfg(feature = "auth")]
pub use crate::config::X_PROFILE_HEADER;
#[cfg(feature = "auth")]
pub use context::{
    AuthorizationContext, KeycloakTokenPayload, SessionOrganization, SessionProfile,
};
#[cfg(feature = "auth")]
pub use csrf::{generate_csrf_token, validate_csrf_token};
#[cfg(feature = "auth")]
pub use mock_routes::{mock_auth_router, MockAuthRouteState};
#[cfg(feature = "auth")]
pub use oauth::KeycloakOAuth;
#[cfg(feature = "auth")]
pub use profile_resolution::{AuthMode, SetupContextRequest};
#[cfg(feature = "auth")]
pub use provider::{AuthProfileProvider, DefaultAuthProfileProvider, EmptyAuthProfileProvider};
#[cfg(feature = "auth")]
pub use routes::{auth_router, AuthState};
#[cfg(feature = "auth")]
pub use session_helper::{
    auth_home_location, forwarded_api_prefix, is_safe_redirect_url, prefix_from_callback_uri,
    public_api_path, random_token, uri, validate_redirect_url,
};
#[cfg(feature = "auth")]
pub use session_store::PgSessionStore;
#[cfg(feature = "auth")]
pub use validator::{JwtValidator, RawTokenClaims};

#[cfg(feature = "auth")]
pub use middleware::{
    auth_context_from_request, auth_required, bridge_auth_to_principal, idempotency_echo,
    mock_auth, principal_allowed_on_portal, principal_from_request, require_bearer,
    require_portal_roles, require_principal, AuthLayerState, MockAuthState, PortalRoleState,
};

pub use crate::base::{RiverbaseError, RiverbaseResult};

#[cfg(feature = "auth")]
pub use crate::config::AuthConfig;

#[cfg(feature = "auth")]
/// Session layer type alias.
pub type SessionLayer = tower_sessions::SessionManagerLayer<PgSessionStore>;

/// Build a tower-sessions layer for OAuth browser login using Postgres.
#[cfg(feature = "auth")]
pub async fn session_layer(
    config: &AuthConfig,
    pool: std::sync::Arc<riverbase_core::datastore::PgPool>,
) -> RiverbaseResult<SessionLayer> {
    use tower_sessions::cookie::SameSite as SessionSameSite;

    let store = PgSessionStore::new(pool);
    store
        .ensure_schema()
        .await
        .map_err(|e| crate::errors::AUT_180.with_data(e.to_string()))?;

    let same_site = match config.cookie_same_site.to_ascii_lowercase().as_str() {
        "lax" => SessionSameSite::Lax,
        "none" => SessionSameSite::None,
        _ => SessionSameSite::Strict,
    };

    Ok(tower_sessions::SessionManagerLayer::new(store)
        .with_name(config.session_cookie.clone())
        .with_secure(config.cookie_https_only)
        .with_same_site(same_site)
        .with_http_only(true))
}

/// Bootstrap auth state (JWKS warmup) and return router + session layer.
#[cfg(feature = "auth")]
pub async fn configure_authentication(
    config: AuthConfig,
    profile_provider: std::sync::Arc<dyn AuthProfileProvider>,
    pool: std::sync::Arc<riverbase_core::datastore::PgPool>,
) -> RiverbaseResult<(aide::axum::ApiRouter, SessionLayer, AuthState)> {
    use std::sync::Arc;

    let state =
        AuthState::build_with_profile_provider(config.clone(), Some(profile_provider)).await?;
    let router = auth_router(Arc::new(state.clone()));
    let layer = session_layer(&config, pool).await?;
    Ok((router, layer, state))
}

/// Mount Keycloak OAuth or mock auth routes; both use a session layer.
#[cfg(feature = "auth")]
pub async fn configure_auth_routes(
    config: AuthConfig,
    profile_provider: std::sync::Arc<dyn AuthProfileProvider>,
    pool: std::sync::Arc<riverbase_core::datastore::PgPool>,
) -> RiverbaseResult<(aide::axum::ApiRouter, Option<SessionLayer>)> {
    use std::sync::Arc;

    if config.uses_keycloak_auth() {
        let (router, layer, _) = configure_authentication(config, profile_provider, pool).await?;
        Ok((router, Some(layer)))
    } else if config.uses_mock_auth() {
        let router = mock_auth_router(Arc::new(config.clone()), profile_provider);
        let layer = session_layer(&config, pool).await?;
        Ok((router, Some(layer)))
    } else {
        Err(crate::errors::AUT_171.with_data(
            serde_json::json!({ "auth_provider": format!("{:?}", config.auth_provider) }),
        ))
    }
}
