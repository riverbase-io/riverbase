//! Auth orchestration for HTTP services — dispatches to mock or JWT paths.

use std::sync::Arc;

use aide::axum::ApiRouter;
use axum::Router;
use tracing::info;

use crate::auth::{
    configure_auth_routes, AuthProfileProvider, DefaultAuthProfileProvider, JwtValidator,
    SessionLayer,
};
use crate::config::{AuthConfig, AuthProvider};
use crate::RiverbaseResult;

use super::auth_layer::with_jwt_auth;
use super::auth_mock::apply_mock_auth;
use super::route_auth::RouteAuthState;

/// Whether JWT/OAuth auth is fully disabled via `RIVERBASE_AUTH_DISABLED=1`.
pub fn flrs_auth_force_disabled() -> bool {
    std::env::var("RIVERBASE_AUTH_DISABLED")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Whether authentication is disabled (`RIVERBASE_AUTH_DISABLED=1` or `auth_provider = None`).
pub fn auth_disabled(config: &AuthConfig) -> bool {
    if flrs_auth_force_disabled() {
        return true;
    }
    config.auth_disabled()
}

/// Whether `/api/auth/*` routes should be mounted (MockAuth or Keycloak).
pub fn auth_routes_enabled(config: &AuthConfig) -> bool {
    !auth_disabled(config) && (config.uses_mock_auth() || config.uses_keycloak_auth())
}

/// Whether OAuth2 browser login routes need a session layer.
pub fn oauth_routes_enabled(config: &AuthConfig) -> bool {
    !flrs_auth_force_disabled()
        && (config.uses_keycloak_auth() || config.uses_mock_auth())
        && !config.oauth2_client_id.trim().is_empty()
}

/// Build a JWT validator when Keycloak auth is selected and issuer is configured.
pub fn jwt_validator(config: &AuthConfig) -> Option<Arc<JwtValidator>> {
    if flrs_auth_force_disabled() || !config.uses_keycloak_auth() {
        return None;
    }
    let oidc = crate::auth::to_oidc_config(config);
    if oidc.issuer.trim().is_empty() {
        return None;
    }
    JwtValidator::new(oidc).ok().map(Arc::new)
}

/// Apply mock or JWT bearer + session auth when configured; otherwise return the router unchanged.
pub async fn apply_auth<S>(
    router: Router<S>,
    config: &AuthConfig,
    profile_provider: Arc<dyn AuthProfileProvider>,
    route_auth: RouteAuthState,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    if flrs_auth_force_disabled() || config.auth_provider == AuthProvider::None {
        return router;
    }

    if config.uses_mock_auth() {
        return apply_mock_auth(router, config, route_auth);
    }

    let Some(validator) = jwt_validator(config) else {
        if config.uses_keycloak_auth() && config.effective_issuer().trim().is_empty() {
            info!("JWT auth skipped: issuer not configured");
        }
        return router;
    };
    match validator.warmup().await {
        Ok(()) => info!("JWT OIDC discovery and JWKS cache warmed up"),
        Err(e) => {
            tracing::warn!(error = %e, "JWT warmup failed; requests may fail until JWKS is reachable")
        }
    }
    info!("JWT and OAuth session authentication enabled");
    let token_provider = DefaultAuthProfileProvider::new(config.clone(), validator.clone());
    with_jwt_auth(
        router,
        validator,
        config.base_path.clone(),
        token_provider,
        profile_provider,
        route_auth,
    )
}

/// Build `/api/auth` routes for merging into the app ApiRouter (before domain routes).
pub async fn configure_auth_router(
    config: &AuthConfig,
    profile_provider: Arc<dyn AuthProfileProvider>,
    pool: Arc<riverbase_core::datastore::PgPool>,
) -> RiverbaseResult<(ApiRouter, Option<SessionLayer>)> {
    let (auth_api, session_layer) =
        configure_auth_routes(config.clone(), profile_provider, pool).await?;
    info!(
        base = %config.base_path,
        auth_provider = ?config.auth_provider,
        "Auth routes mounted"
    );
    Ok((auth_api, session_layer))
}
