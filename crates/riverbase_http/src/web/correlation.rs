//! Correlation / request id propagation ([OPS-03]).

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, Response};
use axum::middleware::Next;
use uuid::Uuid;

/// Request Id Header constant.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

static REQUEST_ID_HEADER_NAME: std::sync::OnceLock<HeaderName> = std::sync::OnceLock::new();

fn request_id_header_name() -> &'static HeaderName {
    REQUEST_ID_HEADER_NAME.get_or_init(|| HeaderName::from_static(REQUEST_ID_HEADER))
}

/// Extension key for the resolved correlation id on each HTTP request.
#[derive(Clone, Debug)]
pub struct CorrelationId(pub String);

/// Correlation id from request.
pub fn correlation_id_from_request(request: &Request<Body>) -> String {
    request
        .headers()
        .get(request_id_header_name())
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

/// Correlation middleware.
pub async fn correlation_middleware(mut request: Request<Body>, next: Next) -> Response<Body> {
    let correlation_id = correlation_id_from_request(&request);
    request
        .extensions_mut()
        .insert(CorrelationId(correlation_id.clone()));
    if let Ok(value) = HeaderValue::from_str(&correlation_id) {
        request
            .headers_mut()
            .insert(request_id_header_name().clone(), value);
    }

    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&correlation_id) {
        response
            .headers_mut()
            .insert(request_id_header_name().clone(), value);
    }
    response
}
