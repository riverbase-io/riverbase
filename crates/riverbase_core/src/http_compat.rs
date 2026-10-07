//! Minimal HTTP trait impls for [`RiverbaseError`] ([ARC-03]).
//!
//! Enabled via the `http` feature so domain crates stay axum-free while portal
//! code can use `IntoResponse` / aide `OperationOutput`.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::base::{RiverbaseError, ProblemDetails};

/// Problem json response.
pub fn problem_json_response(
    err: RiverbaseError,
    instance: Option<String>,
) -> (
    StatusCode,
    [(header::HeaderName, &'static str); 1],
    Json<ProblemDetails>,
) {
    let status = StatusCode::from_u16(err.http_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let problem = ProblemDetails::from_riverbase_error(&err, instance);
    (
        status,
        [(header::CONTENT_TYPE, "application/problem+json")],
        Json(problem),
    )
}

impl IntoResponse for RiverbaseError {
    fn into_response(self) -> Response {
        problem_json_response(self, None).into_response()
    }
}

impl aide::OperationOutput for RiverbaseError {
    type Inner = Self;
}
