//! MockAuth HTTP wiring — session/header identity and `/api/auth/*` routes.

use std::sync::Arc;

use aide::axum::ApiRouter;
use axum::Router;
use tracing::info;

use crate::auth::middleware::{mock_auth, require_principal, MockAuthState};
use crate::auth::{mock_auth_router, AuthProfileProvider};
use crate::config::AuthConfig;
use crate::web::route_auth::RouteAuthState;

/// Apply mock authentication from `Authorization: MockAuth-…` or the OAuth session.
///
/// Also layers [`require_principal`] so missing auth yields `401` on protected routes
/// (independent of Casbin).
pub fn with_mock_auth<S>(router: Router<S>, route_auth: RouteAuthState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    with_mock_auth_field(router, route_auth, "user")
}

fn with_mock_auth_field<S>(
    router: Router<S>,
    route_auth: RouteAuthState,
    ses_user_field: impl Into<String>,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let state = MockAuthState {
        ses_user_field: ses_user_field.into(),
        route_auth: route_auth.clone(),
    };
    // Inner: require_principal; outer: mock_auth (so principal is attached first).
    let router = router.layer(axum::middleware::from_fn_with_state(
        route_auth,
        require_principal,
    ));
    router.layer(axum::middleware::from_fn_with_state(state, mock_auth))
}

fn flrs_auth_force_disabled() -> bool {
    std::env::var("RIVERBASE_AUTH_DISABLED")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Whether MockAuth `/api/auth/*` routes should be mounted.
pub fn mock_auth_routes_enabled(config: &AuthConfig) -> bool {
    !flrs_auth_force_disabled() && !config.auth_disabled() && config.uses_mock_auth()
}

/// Apply the mock bearer/session layer. Picker identities stay on `[riverbase.auth] mock_*`.
pub fn apply_mock_auth<S>(
    router: Router<S>,
    config: &AuthConfig,
    route_auth: RouteAuthState,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    info!("Mock auth enabled (session or Authorization: MockAuth-…)");
    with_mock_auth_field(router, route_auth, config.ses_user_field.clone())
}

/// Mount MockAuth browser routes (`/api/auth/info`, IdP picker, etc.).
pub fn configure_mock_auth_routes(
    config: &AuthConfig,
    profile_provider: Arc<dyn AuthProfileProvider>,
) -> ApiRouter {
    mock_auth_router(Arc::new(config.clone()), profile_provider)
}
