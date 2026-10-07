//! HTTP response adapters for [`riverbase_core::base`] types ([API-02]).

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use riverbase_core::base::{RiverbaseError, ProblemDetails};
use serde_json::Value;

/// Convert a [`RiverbaseError`] into an axum response (RFC 7807).
pub fn error_into_response(err: RiverbaseError) -> Response {
    error_into_response_at(err, None)
}

/// Convert a [`RiverbaseError`] with a request `instance` path.
pub fn error_into_response_at(err: RiverbaseError, instance: Option<String>) -> Response {
    problem_json_response(err, instance).into_response()
}

/// Standard HTTP error tuple: RFC 7807 `application/problem+json`.
pub fn http_json_response(err: RiverbaseError) -> Response {
    error_into_response(err)
}

/// Problem response tuple for handlers returning `(StatusCode, Json<...>)`.
pub fn problem_json_response(
    err: RiverbaseError,
    instance: Option<String>,
) -> (
    StatusCode,
    [(header::HeaderName, &'static str); 1],
    Json<ProblemDetails>,
) {
    riverbase_core::http_compat::problem_json_response(err, instance)
}

/// Render a [`ProblemDetails`] document as an HTTP response.
pub fn problem_details_into_response(problem: ProblemDetails) -> Response {
    let status = StatusCode::from_u16(problem.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        [(header::CONTENT_TYPE, "application/problem+json")],
        Json(problem),
    )
        .into_response()
}

/// Public path for RFC 9457 `instance`.
///
/// Caddy rewrites `/v1/{zone}/…` to `/api/…` and sends `X-Forwarded-Prefix`.
/// Report the browser path, not the internal Riverbase mount.
fn problem_instance_path(path: &str, headers: &HeaderMap) -> String {
    let prefix = headers
        .get("x-forwarded-prefix")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .unwrap_or_default();
    if prefix.is_empty() || (path != "/api" && !path.starts_with("/api/")) {
        return path.to_string();
    }
    format!("{prefix}{}", path.strip_prefix("/api").unwrap_or(""))
}

fn assign_problem_instance(value: &mut Value, public_path: &str) {
    match value.get("instance").and_then(Value::as_str) {
        None | Some("") => {
            value["instance"] = Value::String(public_path.to_string());
        }
        Some(current) if current == "/api" || current.starts_with("/api/") => {
            value["instance"] = Value::String(public_path.to_string());
        }
        Some(_) => {}
    }
}

/// Fill RFC 7807 `instance` from the public request path when missing.
pub async fn fill_problem_instance(request: Request<Body>, next: Next) -> Response {
    let path = problem_instance_path(request.uri().path(), request.headers());
    let response = next.run(request).await;
    let is_problem = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("application/problem+json"));
    if !is_problem {
        return response;
    }
    let (parts, body) = response.into_parts();
    let bytes = match axum::body::to_bytes(body, 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Response::from_parts(parts, Body::empty());
        }
    };
    let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    assign_problem_instance(&mut value, &path);
    let Ok(body) = serde_json::to_vec(&value) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    Response::from_parts(parts, Body::from(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problem_instance_uses_forwarded_prefix() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-prefix", "/v1/rfsentinel".parse().unwrap());
        assert_eq!(
            problem_instance_path("/api/auth/callback", &headers),
            "/v1/rfsentinel/auth/callback"
        );
        assert_eq!(
            problem_instance_path("/ui/rfsentinel/", &headers),
            "/ui/rfsentinel/"
        );
    }

    #[test]
    fn problem_instance_keeps_internal_path_without_prefix() {
        assert_eq!(
            problem_instance_path("/api/auth/callback", &HeaderMap::new()),
            "/api/auth/callback"
        );
    }

    #[test]
    fn assign_rewrites_internal_instance() {
        let mut value = serde_json::json!({"instance": "/api/auth/callback"});
        assign_problem_instance(&mut value, "/v1/rfsentinel/auth/callback");
        assert_eq!(value["instance"], "/v1/rfsentinel/auth/callback");
    }
}
