//! Casbin activity enforcement at the HTTP boundary (policy packs under `configs/policies/`).

use std::path::Path;
use std::sync::Arc;

use aide::openapi::OpenApi;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use serde_json::json;
use tracing::info;

use crate::auth::middleware::principal_from_request;
use crate::auth::Principal;
use crate::casbin::{
    build_authorizer, ActivityAuthorizer, ActivityRequest, CasbinActivityAuthorizer, DomainActivity,
};
use crate::config::CasbinConfig;
use crate::http_response::http_json_response;
use crate::RiverbaseResult;

use super::route_auth::normalize_request_path;
use super::strip_api_base;

#[derive(Clone)]
/// Casbin layer state structure.
pub struct CasbinLayerState {
    /// Authorizer.
    pub authorizer: Arc<CasbinActivityAuthorizer>,
    /// Subject used when JWT auth is disabled (`RIVERBASE_DEV_SUBJECT`, default `dev-user`).
    pub dev_subject: String,
    /// When true (auth enabled), protected routes require a principal and answer `AUT-001`
    /// instead of falling through as [`Self::dev_subject`].
    pub require_authenticated: bool,
    /// Api base.
    pub api_base: String,
    /// Known roles.
    pub known_roles: Arc<Vec<String>>,
    /// Route auth.
    pub route_auth: super::route_auth::RouteAuthState,
}

/// Resolve the Casbin subject for activity enforcement.
///
/// Picks the first role from `known_roles` (portal policy priority) that the principal holds,
/// not the first principal role that happens to match globally.
pub fn resolve_casbin_subject(
    principal: Option<&Principal>,
    dev_subject: &str,
    known_roles: &[String],
) -> String {
    match principal {
        Some(p) => known_roles
            .iter()
            .find(|role| {
                p.roles.iter().any(|r| r == *role) || p.iam_roles.iter().any(|r| r == *role)
            })
            .cloned()
            .unwrap_or_else(|| {
                if known_roles.iter().any(|role| role == "authenticated") {
                    "authenticated".into()
                } else {
                    p.sub.clone()
                }
            }),
        None => dev_subject.to_string(),
    }
}

/// Load policy CSV from `configs/policies/{pack}.csv` or inline `[riverbase.casbin] policy`.
pub fn load_policy_csv(config: &CasbinConfig, pack: Option<&str>) -> String {
    if !config.policy.trim().is_empty() {
        return config.policy.clone();
    }
    let Some(pack) = pack else {
        return String::new();
    };
    let path = Path::new("configs/policies").join(format!("{pack}.csv"));
    std::fs::read_to_string(&path).unwrap_or_else(|_| {
        tracing::warn!(path = %path.display(), "policy file not found");
        String::new()
    })
}

/// Build http authorizer.
pub async fn build_http_authorizer(
    pool: riverbase_core::datastore::PgPool,
    config: &CasbinConfig,
    pack: Option<&str>,
) -> RiverbaseResult<Arc<CasbinActivityAuthorizer>> {
    let policy = load_policy_csv(config, pack);
    let model = if config.model.trim().is_empty() {
        None
    } else {
        Some(config.model.as_str())
    };
    build_authorizer(pool, model, Some(&policy)).await
}

/// Apply Casbin when `[riverbase.casbin] enabled = true` (after JWT layer).
pub fn apply_casbin<S>(router: axum::Router<S>, state: CasbinLayerState) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(axum::middleware::from_fn_with_state(
        state,
        enforce_activity,
    ))
}

async fn enforce_activity(
    State(state): State<CasbinLayerState>,
    request: Request,
    next: Next,
) -> Response {
    let path = normalize_request_path(request.uri());
    if state.route_auth.is_public(&path) {
        return next.run(request).await;
    }

    let activity = match activity_from_path(&path, &state.api_base, &state.route_auth) {
        Some(activity) => activity,
        None => return forbidden_unmapped(&path),
    };

    let principal = principal_from_request(&request);
    if principal.is_none() && state.require_authenticated {
        if let Some(response) =
            crate::auth::middleware::login_redirect_if_needed(&request, &state.route_auth)
        {
            return response;
        }
        return unauthorized_missing_principal(&path);
    }
    let subject =
        resolve_casbin_subject(principal.as_ref(), &state.dev_subject, &state.known_roles);

    let req = ActivityRequest::new(subject, activity);
    if state.authorizer.enforce(&req).await.is_err() {
        return forbidden(&path, &req);
    }
    next.run(request).await
}

/// Resolve the Casbin policy identity for an HTTP wire namespace ([SEC-04]).
pub(crate) fn policy_namespace_from_wire(
    wire: &str,
    route_auth: &super::route_auth::RouteAuthState,
) -> Option<String> {
    route_auth.policy_identity(wire)
}

/// Map Riverbase HTTP paths to domain activities for Casbin.
pub(crate) fn activity_from_path(
    path: &str,
    api_base: &str,
    route_auth: &super::route_auth::RouteAuthState,
) -> Option<DomainActivity> {
    let path = strip_api_base(api_base, path);
    let (wire_ns, rest) = path.split_once('/')?;
    let namespace = policy_namespace_from_wire(wire_ns, route_auth)?;
    if rest == "domain.meta" {
        return Some(
            DomainActivity::new(namespace, "domain.meta")
                .with_resource("domain")
                .with_object_id("*"),
        );
    }
    if let Some(resource) = rest.strip_suffix(".meta") {
        if !resource.contains(':') && !resource.is_empty() {
            return Some(
                DomainActivity::new(namespace, format!("{resource}.query"))
                    .with_resource(resource)
                    .with_object_id("*"),
            );
        }
    }
    if let Some(segment) = rest.split('/').next() {
        if segment.ends_with("~ssestream") || segment.ends_with("~websocket") {
            return Some(
                DomainActivity::new(namespace, "rtc.stream")
                    .with_resource("rtc")
                    .with_object_id("*"),
            );
        }
    }
    if rest == "fleet.list" {
        return Some(
            DomainActivity::new(namespace, "fleet.query")
                .with_resource("fleet")
                .with_object_id("*"),
        );
    }
    if let Some(action) = rest.split('/').next() {
        if action == "session.stream" || action == "session.observe" {
            return Some(
                DomainActivity::new(namespace, "session.stream")
                    .with_resource("session")
                    .with_object_id("*"),
            );
        }
        if action == "session.cast" {
            return Some(
                DomainActivity::new(namespace, "session.cast")
                    .with_resource("session")
                    .with_object_id("*"),
            );
        }
    }
    // Raw blob GET: `/api/{ns}/blob/{org}/{slug}/…` (not `{resource}.list`).
    if rest == "blob" || rest.starts_with("blob/") {
        return Some(
            DomainActivity::new(namespace, "blob.query")
                .with_resource("blob")
                .with_object_id("*"),
        );
    }
    if rest == "pkg" || rest.starts_with("pkg/") {
        return Some(
            DomainActivity::new(namespace, "pkg.query")
                .with_resource("pkg")
                .with_object_id("*"),
        );
    }
    if rest == "gca" || rest.starts_with("gca/") {
        return Some(
            DomainActivity::new(namespace, "gca.query")
                .with_resource("gca")
                .with_object_id("*"),
        );
    }
    if rest == "media-entry" || rest.starts_with("media-entry/") {
        return Some(
            DomainActivity::new(namespace, "media-entry.query")
                .with_resource("media-entry")
                .with_object_id("*"),
        );
    }
    if rest.contains(":post/") || rest.contains(":exec/") || rest.contains(":hook/") {
        let cmdkey = rest.split([':', '/']).next()?;
        let segments: Vec<&str> = rest.split('/').collect();
        let resource = if rest.contains(":hook/") {
            segments
                .get(2)
                .copied()
                .filter(|s| !s.is_empty() && *s != "~")
                .or_else(|| {
                    segments
                        .get(1)
                        .copied()
                        .filter(|s| !s.is_empty() && *s != "~" && !s.contains(':'))
                })
                .unwrap_or("*")
        } else {
            segments
                .get(1)
                .copied()
                .filter(|s| !s.is_empty() && *s != "~")
                .unwrap_or("*")
        };
        return Some(
            DomainActivity::new(namespace, format!("{cmdkey}.execute"))
                .with_resource(resource)
                .with_object_id("*"),
        );
    }
    if let Some((resource, _method)) = rest.split_once('.') {
        return Some(
            DomainActivity::new(namespace, format!("{resource}.query"))
                .with_resource(resource)
                .with_object_id("*"),
        );
    }
    None
}

fn unauthorized_missing_principal(path: &str) -> Response {
    http_json_response(crate::errors::AUT_182.with_data(json!({ "path": path })))
}

fn forbidden_unmapped(path: &str) -> Response {
    http_json_response(crate::errors::CAS_015.with_data(json!({
        "path": path,
        "detail": "unmapped activity for mounted namespace",
    })))
}

fn forbidden(path: &str, req: &ActivityRequest) -> Response {
    let activity = &req.activity;
    let resource = if activity.resource.is_empty() {
        "*".to_string()
    } else {
        activity.resource.clone()
    };
    let object_id = if activity.object_id.is_empty() {
        "*".to_string()
    } else {
        activity.object_id.clone()
    };
    http_json_response(riverbase_core::errors::AUT_003.with_data(json!({
        "path": path,
        "subject": req.subject,
        "namespace": activity.namespace,
        "activity_type": activity.activity_type,
        "resource": resource,
        "object_id": object_id,
        "check": format!(
            "enforce({}, {}, {}, {}, {})",
            req.subject, activity.namespace, activity.activity_type, resource, object_id
        ),
    })))
}

/// Build Casbin layer state when `[riverbase.casbin] enabled = true`.
pub async fn build_casbin_state(
    pool: riverbase_core::datastore::PgPool,
    config: &CasbinConfig,
    pack: Option<&str>,
    api_base: &str,
    known_roles: &[String],
    route_auth: super::route_auth::RouteAuthState,
    require_authenticated: bool,
) -> RiverbaseResult<Option<CasbinLayerState>> {
    if !config.enabled {
        return Ok(None);
    }
    let authorizer = build_http_authorizer(pool, config, pack).await?;
    let policy_rules = authorizer.policy_rule_count().await;
    let mounted = route_auth.mounted_namespace_count();
    if mounted > 0 && policy_rules == 0 {
        tracing::error!(
            ?pack,
            mounted_namespaces = mounted,
            "Casbin enabled but policy pack has zero activity rules for mounted namespaces"
        );
        return Err(
            crate::errors::CAS_014.with_data(format!("pack={pack:?} mounted_namespaces={mounted}"))
        );
    }
    info!(?pack, policy_rules, "Casbin activity enforcement enabled");
    let dev_subject = std::env::var("RIVERBASE_DEV_SUBJECT").unwrap_or_else(|_| "dev-user".into());
    Ok(Some(CasbinLayerState {
        authorizer,
        dev_subject,
        require_authenticated,
        api_base: api_base.to_string(),
        known_roles: Arc::new(known_roles.to_vec()),
        route_auth,
    }))
}

/// Filter an OpenAPI document to operations the subject is permitted to call.
///
/// When `principal` is absent, returns `AUT-001` (401) — filtering requires an authenticated
/// subject.
pub async fn filter_accessible_openapi(
    api: &OpenApi,
    state: &CasbinLayerState,
    principal: Option<&Principal>,
) -> RiverbaseResult<OpenApi> {
    let principal = principal.ok_or_else(|| {
        crate::errors::AUT_183
            .with_data("omit_inaccessible_openapi requires an authenticated principal")
    })?;
    let subject = resolve_casbin_subject(Some(principal), &state.dev_subject, &state.known_roles);
    let mut filtered = api.clone();
    let Some(paths) = filtered.paths.as_mut() else {
        return Ok(filtered);
    };

    let keys: Vec<String> = paths.paths.keys().cloned().collect();
    let mut allowed = std::collections::HashSet::new();
    for key in keys {
        match activity_from_path(&key, &state.api_base, &state.route_auth) {
            None => {
                // Unmapped paths are not accessible in filtered OpenAPI (fail-closed).
            }
            Some(activity) => {
                let req = ActivityRequest::new(subject.clone(), activity);
                if state.authorizer.enforce(&req).await.is_ok() {
                    allowed.insert(key);
                }
            }
        }
    }
    paths.paths.retain(|key, _| allowed.contains(key));
    Ok(filtered)
}

/// Maybe apply casbin.
pub async fn maybe_apply_casbin<S>(
    router: axum::Router<S>,
    pool: riverbase_core::datastore::PgPool,
    config: &CasbinConfig,
    pack: Option<&str>,
    api_base: &str,
    known_roles: &[String],
    route_auth: super::route_auth::RouteAuthState,
    require_authenticated: bool,
) -> RiverbaseResult<axum::Router<S>>
where
    S: Clone + Send + Sync + 'static,
{
    let Some(state) = build_casbin_state(
        pool,
        config,
        pack,
        api_base,
        known_roles,
        route_auth,
        require_authenticated,
    )
    .await?
    else {
        return Ok(router);
    };
    Ok(apply_casbin(router, state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::route_auth::RouteAuthState;

    const API: &str = "/api";

    fn auth_with(identities: &[(&str, &str)]) -> RouteAuthState {
        let auth = RouteAuthState::new();
        for (wire, identity) in identities {
            auth.register_policy_identity(*wire, *identity);
        }
        auth
    }

    fn rfx_auth() -> RouteAuthState {
        auth_with(&[
            ("exp.catalog", "exp.catalog"),
            ("exp.rtc", "exp.rtc"),
            ("identity-manager", "rfx.idm"),
            ("user-profile", "rfx.user"),
            ("setting-manager", "rfx.setting"),
            ("domain-audit", "rfx.audit"),
            ("form-engine", "rfx.form"),
            ("workflow-manager", "rfx.flow"),
            ("task-manager", "rfx.task"),
            ("rule-engine", "rfx.rule"),
            ("media-manager", "rfx.media"),
            ("rfx.media", "rfx.media"),
            ("sourcing", "sourcing"),
        ])
    }

    #[test]
    fn unregistered_wire_namespace_has_no_activity() {
        let auth = RouteAuthState::new();
        assert!(activity_from_path("/api/identity-manager/user.list", API, &auth).is_none());
    }

    #[test]
    fn flat_conformance_namespace_resolves() {
        let auth = rfx_auth();
        let activity =
            activity_from_path("/api/sourcing/rfq.list", API, &auth).expect("sourcing rfq");
        assert_eq!(activity.namespace, "sourcing");
        assert_eq!(activity.activity_type, "rfq.query");
    }

    #[test]
    fn meta_paths_map_to_query_activity() {
        let auth = rfx_auth();
        let activity =
            activity_from_path("/api/exp.catalog/product.meta", API, &auth).expect("product.meta");
        assert_eq!(activity.namespace, "exp.catalog");
        assert_eq!(activity.activity_type, "product.query");
        assert_eq!(activity.resource, "product");
        let domain_meta =
            activity_from_path("/api/exp.catalog/domain.meta", API, &auth).expect("domain.meta");
        assert_eq!(domain_meta.activity_type, "domain.meta");
        assert!(
            !activity_from_path("/api/exp.catalog/product.list", API, &auth)
                .unwrap()
                .activity_type
                .is_empty()
        );
    }

    #[test]
    fn content_blob_get_maps_to_blob_query() {
        let auth = auth_with(&[("gfs.content", "gfs.content")]);
        let activity = activity_from_path(
            "/api/gfs.content/blob/abx/dev-bootstrap/branch/gfs%2Fchanges%2Fa5d1e687-514e-4dac-aed1-d172b70ccca1/docs/43434",
            API,
            &auth,
        )
        .expect("blob activity");
        assert_eq!(activity.namespace, "gfs.content");
        assert_eq!(activity.activity_type, "blob.query");
        assert_eq!(activity.resource, "blob");
    }

    #[test]
    fn publication_pkg_and_gca_map_to_query_activity() {
        let auth = auth_with(&[("gfs.publication", "gfs.publication")]);
        let pkg = activity_from_path(
            "/api/gfs.publication/pkg/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa/deadbeef/content/html/index.html",
            API,
            &auth,
        )
        .expect("pkg activity");
        assert_eq!(pkg.namespace, "gfs.publication");
        assert_eq!(pkg.activity_type, "pkg.query");
        assert_eq!(pkg.resource, "pkg");
        let gca = activity_from_path(
            "/api/gfs.publication/gca/acme/handbook/git:HEAD/content/html/index.html",
            API,
            &auth,
        )
        .expect("gca activity");
        assert_eq!(gca.activity_type, "gca.query");
        assert_eq!(gca.resource, "gca");
    }

    #[test]
    fn realtime_sse_maps_to_rtc_stream_activity() {
        let auth = rfx_auth();
        let activity = activity_from_path(
            "/api/exp.rtc/default~ssestream/exp.device%2Fvenue%2Fabc",
            API,
            &auth,
        )
        .expect("rtc sse activity");
        assert_eq!(activity.namespace, "exp.rtc");
        assert_eq!(activity.activity_type, "rtc.stream");
        let org = activity_from_path(
            "/api/exp.rtc/default~ssestream/exp.device/org/00000000-0000-4000-8000-000000000011",
            API,
            &auth,
        )
        .expect("rtc org sse activity");
        assert_eq!(org.activity_type, "rtc.stream");
        let ws = activity_from_path("/api/exp.rtc/default~websocket", API, &auth).expect("rtc ws");
        assert_eq!(ws.activity_type, "rtc.stream");
    }

    #[test]
    fn realtime_sse_without_policy_identity_is_unmapped() {
        let auth = RouteAuthState::new();
        assert!(activity_from_path(
            "/api/exp.rtc/default~ssestream/exp.device/org/00000000-0000-4000-8000-000000000011",
            API,
            &auth,
        )
        .is_none());
    }

    #[test]
    fn public_route_registry_skips_enforcement() {
        let state = RouteAuthState::new();
        state.register_public_path("/api/exp.catalog/domain.meta");
        assert!(state.is_public("/api/exp.catalog/domain.meta"));
        assert!(!state.is_public("/api/exp.catalog/product.list"));
    }

    #[test]
    fn domain_meta_is_not_anonymous_by_default() {
        let state = RouteAuthState::new();
        crate::web::route_auth::register_framework_public_routes(&state, API);
        assert!(
            !state.is_public("/api/exp.catalog/domain.meta"),
            "domain.meta stays a distinct, authorized activity ([SEC-12])"
        );
        let activity =
            activity_from_path("/api/exp.catalog/domain.meta", API, &rfx_auth()).expect("mapped");
        assert_eq!(activity.activity_type, "domain.meta");
    }

    #[test]
    fn command_key_meta_maps_to_query_activity() {
        let path = "/api/exp.catalog/create-product.meta";
        let activity = activity_from_path(path, API, &rfx_auth()).expect("create-product.meta");
        assert_eq!(activity.activity_type, "create-product.query");
    }

    #[test]
    fn setting_display_maps_to_query_activity() {
        let activity = activity_from_path(
            "/api/setting-manager/setting-display.list",
            API,
            &rfx_auth(),
        )
        .expect("setting-display activity");
        assert_eq!(activity.namespace, "rfx.setting");
        assert_eq!(activity.activity_type, "setting-display.query");
        assert_eq!(activity.resource, "setting-display");
    }

    #[test]
    fn identity_manager_maps_to_rfx_idm_policy_namespace() {
        let activity = activity_from_path(
            "/api/identity-manager/create-user:post/user",
            API,
            &rfx_auth(),
        )
        .expect("activity");
        assert_eq!(activity.namespace, "rfx.idm");
        assert_eq!(activity.activity_type, "create-user.execute");
        assert_eq!(activity.resource, "user");
    }

    #[test]
    fn user_profile_maps_to_rfx_user_policy_namespace() {
        let activity = activity_from_path("/api/user-profile/profile.list", API, &rfx_auth())
            .expect("activity");
        assert_eq!(activity.namespace, "rfx.user");
        assert_eq!(activity.activity_type, "profile.query");
    }

    #[test]
    fn platform_domains_map_to_rfx_policy_namespaces() {
        let cases = [
            (
                "/api/domain-audit/command-log.list",
                "rfx.audit",
                "command-log.query",
            ),
            (
                "/api/form-engine/form-definition.list",
                "rfx.form",
                "form-definition.query",
            ),
            (
                "/api/workflow-manager/workflow-definition.list",
                "rfx.flow",
                "workflow-definition.query",
            ),
            ("/api/task-manager/worker.list", "rfx.task", "worker.query"),
            (
                "/api/rule-engine/rule-definition.list",
                "rfx.rule",
                "rule-definition.query",
            ),
            ("/api/media-manager/media.list", "rfx.media", "media.query"),
            (
                "/api/rfx.media/media-entry/export_attempt/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb/download",
                "rfx.media",
                "media-entry.query",
            ),
            (
                "/api/rfx.media/media-entry/import_attempt/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb/download",
                "rfx.media",
                "media-entry.query",
            ),
        ];
        for (path, namespace, activity_type) in cases {
            let activity =
                activity_from_path(path, API, &rfx_auth()).unwrap_or_else(|| panic!("{path}"));
            assert_eq!(activity.namespace, namespace, "{path}");
            assert_eq!(activity.activity_type, activity_type, "{path}");
        }
    }

    #[test]
    fn resolve_subject_prefers_portal_role_order_over_profile_role_order() {
        let roles = vec!["seller_admin".into(), "seller_staff".into()];
        let p = Principal {
            sub: "user-123".into(),
            preferred_username: None,
            email: None,
            roles: vec!["coordinator_admin".into(), "seller_admin".into()],
            iam_roles: vec![],
            claims: serde_json::json!({}),
        };
        assert_eq!(
            resolve_casbin_subject(Some(&p), "dev-user", &roles),
            "seller_admin"
        );
    }

    #[test]
    fn resolve_subject_prefers_known_role() {
        let roles = vec!["customer".into(), "seller_admin".into()];
        let p = Principal {
            sub: "user-123".into(),
            preferred_username: None,
            email: None,
            roles: vec!["customer".into(), "seller_admin".into()],
            iam_roles: vec![],
            claims: serde_json::json!({}),
        };
        assert_eq!(
            resolve_casbin_subject(Some(&p), "dev-user", &roles),
            "customer"
        );
    }

    #[test]
    fn resolve_subject_falls_back_to_authenticated_when_listed() {
        let roles = vec!["gfs_reader".into(), "authenticated".into()];
        let p = Principal {
            sub: "user-123".into(),
            preferred_username: None,
            email: None,
            roles: vec!["staff".into()],
            iam_roles: vec![],
            claims: serde_json::json!({}),
        };
        assert_eq!(
            resolve_casbin_subject(Some(&p), "dev-user", &roles),
            "authenticated"
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres (PgCasbinAdapter)"]
    async fn filter_accessible_openapi_without_principal_errors() {
        let _ = filter_accessible_openapi;
    }

    #[tokio::test]
    #[ignore = "requires Postgres (PgCasbinAdapter)"]
    async fn filter_accessible_openapi_drops_forbidden_paths() {
        let _ = filter_accessible_openapi;
    }
}
