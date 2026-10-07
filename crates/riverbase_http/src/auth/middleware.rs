use std::sync::Arc;

use axum::{
    extract::{Extension, Request, State},
    http::{request::Parts, HeaderMap, Uri},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use serde_json::json;
use tower_sessions::Session;

use super::context::AuthorizationContext;
use super::provider::{AuthProfileProvider, DefaultAuthProfileProvider};
use super::routes::AuthState;
use crate::api_path::is_auth_route;
use crate::auth::{IdempotencyKey, Principal};
use crate::http_response::{error_into_response, http_json_response};
use crate::web::route_auth::{normalize_request_path, RouteAuthState};

/// Axum state for JWT + OAuth session authentication middleware.
#[derive(Clone)]
pub struct AuthLayerState {
    /// OAuth / session auth routes that must not require a bearer token.
    pub auth_base_path: String,
    /// Token provider.
    pub token_provider: DefaultAuthProfileProvider,
    /// Resolves browser session cookies into authorization context (Keycloak OAuth).
    pub profile_provider: Arc<dyn AuthProfileProvider>,
    /// Public exemptions and login-redirect prefixes.
    pub route_auth: RouteAuthState,
}

/// Axum state for mock authentication (header or OAuth session).
#[derive(Clone)]
pub struct MockAuthState {
    /// Session field holding identity claims (`ses_user_field`).
    pub ses_user_field: String,
    /// Public routes may carry a bearer that is not MockAuth (the herd edge run token).
    pub route_auth: RouteAuthState,
}

/// Attach the mock [`Principal`] for this request.
///
/// Resolution order: `Authorization: MockAuth-…`, then the OAuth session (`ses_user`).
/// Missing both leaves no principal (401 on protected routes via [`require_principal`]).
pub async fn mock_auth(
    State(state): State<MockAuthState>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = normalize_request_path(request.uri());
    let authorization = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    match crate::auth::resolve_mock_principal(authorization) {
        Ok(Some(p)) => {
            request.extensions_mut().insert(p);
            return next.run(request).await;
        }
        Ok(None) => {}
        Err(err) => {
            if !defer_bearer_to_route(&path, authorization, &state.route_auth) {
                return error_into_response(err);
            }
        }
    }

    if let Some(session) = request.extensions().get::<Session>().cloned() {
        if let Ok(Some(user)) = session
            .get::<serde_json::Value>(&state.ses_user_field)
            .await
        {
            match crate::auth::principal_from_mock_claims(&user) {
                Ok(p) => {
                    request.extensions_mut().insert(p);
                    return next.run(request).await;
                }
                Err(err) => return error_into_response(err),
            }
        }
    }

    next.run(request).await
}

/// Public routes such as `/edge` authenticate with their own bearer. MockAuth must not
/// reject that header as `AUT-172` before the route reads it.
fn defer_bearer_to_route(
    path: &str,
    authorization: Option<&str>,
    route_auth: &RouteAuthState,
) -> bool {
    let Some(header) = authorization
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let scheme = header.split_whitespace().next().unwrap_or("");
    route_auth.is_public(path) && scheme.eq_ignore_ascii_case("bearer")
}

/// Roles that satisfy a portal audience gate after authentication.
#[derive(Clone)]
pub struct PortalRoleState {
    /// Profile or IAM roles that may call this process (`sys-admin` is always allowed).
    pub allowed: Vec<String>,
    /// Public exemptions — unauthenticated and public paths skip the gate.
    pub route_auth: RouteAuthState,
}

/// Whether [`Principal`] may call a portal that requires `allowed` roles.
///
/// `sys-admin` always passes. Profile roles and IAM realm roles both count.
pub fn principal_allowed_on_portal(principal: &Principal, allowed: &[String]) -> bool {
    fn has(principal: &Principal, role: &str) -> bool {
        principal.has_role(role) || principal.iam_roles.iter().any(|r| r == role)
    }
    has(principal, "sys-admin") || allowed.iter().any(|role| has(principal, role))
}

/// Reject authenticated callers who lack a portal role (403 / `AUT-196`).
///
/// Layer **inside** [`require_principal`] / [`require_bearer`] so `Principal` is set.
/// Public routes and missing principals pass through.
pub async fn require_portal_roles(
    State(state): State<PortalRoleState>,
    request: Request,
    next: Next,
) -> Response {
    let path = normalize_request_path(request.uri());
    if state.route_auth.is_public(&path) {
        return next.run(request).await;
    }
    let Some(principal) = request.extensions().get::<Principal>() else {
        return next.run(request).await;
    };
    if principal_allowed_on_portal(principal, &state.allowed) {
        return next.run(request).await;
    }
    http_json_response(crate::errors::AUT_196.with_data(json!({
        "path": path,
        "allowed": state.allowed,
        "principal": principal.to_errdata(),
    })))
}

/// Require a [`Principal`] on non-public routes (Casbin-independent auth gate).
///
/// Runs **after** mock/JWT middleware so a missing principal on a protected path
/// returns `401` / `AUT-001` before body parse or domain handlers.
pub async fn require_principal(
    State(route_auth): State<RouteAuthState>,
    request: Request,
    next: Next,
) -> Response {
    let path = normalize_request_path(request.uri());
    if route_auth.is_public(&path) {
        return next.run(request).await;
    }
    if request.extensions().get::<Principal>().is_some() {
        return next.run(request).await;
    }
    if let Some(response) = login_redirect_if_needed(&request, &route_auth) {
        return response;
    }
    unauthorized_missing_principal(&path)
}

fn unauthorized_missing_principal(path: &str) -> Response {
    http_json_response(crate::errors::AUT_182.with_data(json!({ "path": path })))
}

/// Redirect unauthenticated browsers on registered prefixes to `{auth}/sign-in?next=`.
pub fn login_redirect_if_needed(
    request: &Request,
    route_auth: &RouteAuthState,
) -> Option<Response> {
    login_redirect_if_needed_uri(request.uri(), request.headers(), route_auth)
}

fn login_redirect_if_needed_parts(parts: &Parts, route_auth: &RouteAuthState) -> Option<Response> {
    login_redirect_if_needed_uri(&parts.uri, &parts.headers, route_auth)
}

fn login_redirect_if_needed_uri(
    uri: &Uri,
    headers: &HeaderMap,
    route_auth: &RouteAuthState,
) -> Option<Response> {
    let path = normalize_request_path(uri);
    if !route_auth.should_login_redirect(&path) {
        return None;
    }
    let fallback = route_auth.public_api_prefix();
    let next = crate::auth::public_api_path(
        uri.path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or(uri.path()),
        headers,
        &fallback,
    );
    let encoded: String = url::form_urlencoded::byte_serialize(next.as_bytes()).collect();
    let sign_in = crate::auth::public_api_path(&route_auth.auth_sign_in_path(), headers, &fallback);
    Some(Redirect::temporary(&format!("{sign_in}?next={encoded}")).into_response())
}

/// Require Bearer JWT or OAuth session cookie and attach [`Principal`] to request extensions.
pub async fn require_bearer(
    State(state): State<AuthLayerState>,
    request: Request,
    next: Next,
) -> Response {
    if is_auth_route(request.uri().path(), &state.auth_base_path) {
        return next.run(request).await;
    }

    let session = request.extensions().get::<Session>().cloned();
    let jar = CookieJar::from_headers(request.headers());
    let (mut parts, body) = request.into_parts();
    match state
        .token_provider
        .get_auth_context(
            state.profile_provider.as_ref(),
            &parts,
            session.as_ref(),
            &jar,
        )
        .await
    {
        Ok(Some(ctx)) => {
            parts.extensions.insert(ctx.clone());
            if let Some(principal) = Principal::from_auth_context(&ctx) {
                parts.extensions.insert(principal);
                return next.run(Request::from_parts(parts, body)).await;
            }
        }
        Ok(None) => {
            if let Some(response) = login_redirect_if_needed_parts(&parts, &state.route_auth) {
                return response;
            }
        }
        Err(err) => return error_into_response(err),
    }

    unauthorized("missing Authorization header")
}

/// Resolve [`AuthorizationContext`] (session cookie or Bearer) and attach to extensions.
pub async fn auth_required(
    Extension(state): Extension<Arc<AuthState>>,
    session: Session,
    jar: CookieJar,
    request: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = request.into_parts();
    match state
        .provider
        .get_auth_context(
            state.profile_provider.as_ref(),
            &parts,
            Some(&session),
            &jar,
        )
        .await
    {
        Ok(Some(ctx)) => {
            parts.extensions.insert(ctx);
            next.run(Request::from_parts(parts, body)).await
        }
        Ok(None) => {
            http_json_response(crate::errors::AUT_160.with_data("User is not authenticated"))
        }
        Err(err) => http_json_response(err).into_response(),
    }
}

/// Capture and echo `Idempotency-Key` request header (Python `RiverbaseAuthMiddleware`).
pub async fn idempotency_echo(headers: HeaderMap, mut request: Request, next: Next) -> Response {
    let key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    if let Some(ref key) = key {
        if key.len() > 128 {
            return http_json_response(
                crate::errors::IDM_010.with_data(json!({ "max_length": 128, "length": key.len() })),
            );
        }
        request.extensions_mut().insert(IdempotencyKey(key.clone()));
    }
    let mut response = next.run(request).await;
    if let Some(key) = key {
        if let Ok(value) = axum::http::HeaderValue::from_str(&key) {
            response.headers_mut().insert("Idempotency-Key", value);
        }
    }
    response
}

fn unauthorized(message: impl Into<String>) -> Response {
    http_json_response(crate::errors::AUT_001.with_data(message.into()))
}

/// Extract authenticated principal from request extensions (after [`require_bearer`]).
pub fn principal_from_request(request: &Request) -> Option<Principal> {
    request.extensions().get::<Principal>().cloned()
}

/// Extract idempotency key from request extensions (after [`idempotency_echo`]).
pub fn idempotency_key_from_request(request: &Request) -> Option<String> {
    request
        .extensions()
        .get::<IdempotencyKey>()
        .map(|k| k.0.clone())
}

/// Extract authorization context from request extensions (after [`auth_required`]).
pub fn auth_context_from_request(request: &Request) -> Option<AuthorizationContext> {
    request.extensions().get::<AuthorizationContext>().cloned()
}

/// When session auth attached [`AuthorizationContext`] but no [`Principal`], synthesize one
/// so command/query handlers stamp audit `_creator` on all log channels.
pub async fn bridge_auth_to_principal(mut request: Request, next: Next) -> Response {
    if principal_from_request(&request).is_none() {
        if let Some(ctx) = auth_context_from_request(&request) {
            if let Some(principal) = Principal::from_auth_context(&ctx) {
                request.extensions_mut().insert(principal);
            }
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(roles: &[&str], iam_roles: &[&str]) -> Principal {
        Principal {
            sub: "user".into(),
            preferred_username: None,
            email: None,
            roles: roles.iter().map(|r| (*r).to_string()).collect(),
            iam_roles: iam_roles.iter().map(|r| (*r).to_string()).collect(),
            claims: json!({}),
        }
    }

    #[test]
    fn public_edge_keeps_a_bearer_run_token() {
        let routes = RouteAuthState::new();
        routes.register_public_prefix("/edge");
        assert!(defer_bearer_to_route(
            "/edge/v1/chat/completions",
            Some("Bearer v1.token"),
            &routes
        ));
        assert!(!defer_bearer_to_route(
            "/api/abx.herd/runner.list",
            Some("Bearer v1.token"),
            &routes
        ));
    }

    #[test]
    fn parent_role_is_allowed_on_parents_portal() {
        let allowed = vec!["fn1_parent".to_string()];
        assert!(principal_allowed_on_portal(
            &principal(&["fn1_parent"], &[]),
            &allowed
        ));
    }

    #[test]
    fn kid_role_is_denied_on_parents_portal() {
        let allowed = vec!["fn1_parent".to_string()];
        assert!(!principal_allowed_on_portal(
            &principal(&["fn1_kid"], &[]),
            &allowed
        ));
    }

    #[test]
    fn sys_admin_is_allowed_on_any_portal() {
        let allowed = vec!["fn1_parent".to_string()];
        assert!(principal_allowed_on_portal(
            &principal(&["sys-admin"], &[]),
            &allowed
        ));
    }

    #[test]
    fn iam_realm_role_counts_for_portal_gate() {
        let allowed = vec!["fn1_parent".to_string()];
        assert!(principal_allowed_on_portal(
            &principal(&[], &["fn1_parent"]),
            &allowed
        ));
    }
}
