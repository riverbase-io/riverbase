use std::sync::Arc;

use crate::auth::middleware::{require_bearer, AuthLayerState};
use crate::auth::{AuthProfileProvider, DefaultAuthProfileProvider, JwtValidator};
use crate::web::route_auth::RouteAuthState;
use axum::Router;

/// Apply JWT bearer + OAuth session authentication to API routes.
pub fn with_jwt_auth<S>(
    router: Router<S>,
    _validator: Arc<JwtValidator>,
    auth_base_path: impl Into<String>,
    token_provider: DefaultAuthProfileProvider,
    profile_provider: Arc<dyn AuthProfileProvider>,
    route_auth: RouteAuthState,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let state = AuthLayerState {
        auth_base_path: auth_base_path.into(),
        token_provider,
        profile_provider,
        route_auth,
    };
    router.layer(axum::middleware::from_fn_with_state(state, require_bearer))
}
