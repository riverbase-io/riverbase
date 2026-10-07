//! HTTP adapter (axum) for command and query endpoints.

pub mod api_path;
pub mod app_base;
pub mod body_limit;
pub mod correlation;
pub mod health;
pub mod hook_token;
/// Http params; module.
pub mod http_params;
pub mod link_token;
pub mod openapi;
pub mod rate_limit;
pub mod request_log;
/// Router; module.
pub mod router;
pub mod routes;
/// Scope; module.
pub mod scope;
pub mod security_headers;
pub mod startup_checks;
pub mod startup_config;
/// State; module.
pub mod state;

#[cfg(feature = "auth")]
pub mod auth_apply;
#[cfg(feature = "auth")]
/// Auth layer; module.
pub mod auth_layer;
#[cfg(feature = "auth")]
pub mod auth_mock;
#[cfg(feature = "auth")]
pub mod casbin_layer;
#[cfg(feature = "auth")]
pub mod route_auth;

pub use api_path::{
    auth_base_path, join_api_path, normalize_api_base, openapi_json_path, rxdb_base_path,
    strip_api_base, DEFAULT_API_BASE,
};
pub use app_base::{cors_layer, RiverbaseApp, Posture, RouteAccess};
pub use hook_token::{
    consume_hook_nonce, decode_hook_token, encode_hook_token, hook_token_claims,
    payload_hash_for_params, verify_hook_payload_hash, HookTokenClaims,
};
pub use http_params::HttpQueryParams;
pub use link_token::{decode_command_token, encode_command_token, CommandTokenData};
pub use openapi::{
    api_info_path, apply_riverbase_operation, default_coupled_openapi, finish_with_openapi,
    riverbase_api_name, riverbase_command_operation, riverbase_metadata_operation,
    riverbase_query_operation, RiverbaseOperationKind, RiverbaseOperationMeta, OPENAPI_JSON_PATH,
};
pub use request_log::log_http_request;
pub use router::{command_router, coupled_router, into_router, query_router, CommandBody};
pub use routes::{
    command_exec_path, command_hook_path, command_key_meta_path, command_link_path,
    command_meta_path, command_post_path, query_item_path, query_list_path, query_meta_path,
    query_rept_path,
};
pub use scope::EMPTY_SCOPE;
pub use startup_checks::validate_mounted_query_engine;
pub use state::{CommandAppState, CoupledAppState, QueryAppState};

#[cfg(feature = "auth")]
pub use auth_apply::{
    apply_auth, auth_disabled, auth_routes_enabled, configure_auth_router,
    flrs_auth_force_disabled, jwt_validator, oauth_routes_enabled,
};
#[cfg(feature = "auth")]
pub use auth_layer::with_jwt_auth;
#[cfg(feature = "auth")]
pub use auth_mock::{
    apply_mock_auth, configure_mock_auth_routes, mock_auth_routes_enabled, with_mock_auth,
};
#[cfg(feature = "auth")]
pub use casbin_layer::{
    apply_casbin, build_casbin_state, build_http_authorizer, filter_accessible_openapi,
    load_policy_csv, maybe_apply_casbin, resolve_casbin_subject, CasbinLayerState,
};

/// Pick Postgres state store from the app runtime, then dispatch to a mount fn.
#[macro_export]
macro_rules! dispatch_backend {
    ($app:expr, $mount:ident) => {{
        use $crate::base::InvalidArgumentError;
        use $crate::domain::DbConnection;
        use $crate::RiverbaseResult;

        let runtime = $app.runtime().ok_or_else(|| {
            crate::errors::APP_002.with_data("call RiverbaseApp::init_portal or from_runtime first")
        })?;
        if matches!(runtime.dbconn, DbConnection::Postgres(_)) {
            $mount::<$crate::datastore::PgDataStore>($app).await
        } else {
            Err(crate::errors::APP_003.with_data("expected Postgres").into())
        }
    }};
}

use std::sync::Arc;

use crate::base::RiverbaseResult;
use crate::domain::Domain;
use aide::openapi::OpenApi;

/// Build coupled HTTP routes from a [`Domain`] with OpenAPI generation.
pub async fn coupled_router_for_domain<D: Domain + ?Sized>(
    domain: Arc<D>,
    api: &mut OpenApi,
) -> RiverbaseResult<axum::Router> {
    coupled_router_for_domain_with_api_base(domain, DEFAULT_API_BASE, api).await
}

/// Build coupled HTTP routes with a custom API base path.
pub async fn coupled_router_for_domain_with_api_base<D: Domain + ?Sized>(
    domain: Arc<D>,
    api_base: impl Into<String>,
    api: &mut OpenApi,
) -> RiverbaseResult<axum::Router> {
    let api_base = api_path::normalize_api_base(&api_base.into());
    let router = coupled_router(CoupledAppState::from_domain_with_api_base(
        &*domain, &api_base,
    ))
    .await?;
    Ok(finish_with_openapi(router, api, &api_base))
}

/// Convenience: default OpenAPI metadata + coupled router.
pub async fn coupled_router_for_domain_with_openapi<D: Domain + ?Sized>(
    domain: Arc<D>,
    title: impl Into<String>,
    description: impl Into<String>,
) -> RiverbaseResult<axum::Router> {
    let mut api = default_coupled_openapi(title, description);
    coupled_router_for_domain(domain, &mut api).await
}
