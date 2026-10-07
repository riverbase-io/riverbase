//! Optional HTTP access logging to the console (stderr via tracing).

use std::time::Instant;

use axum::{extract::Request, middleware::Next, response::Response};
use tracing::info;

/// Axum middleware: log method, path, status, and latency for each request.
pub async fn log_http_request(request: Request, next: Next) -> Response {
    let method = request.method().as_str().to_owned();
    let uri = request.uri().to_string();
    let started = Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16();
    let latency_ms = started.elapsed().as_millis();
    info!(
        method = %method,
        uri = %uri,
        status,
        latency_ms,
        "http request"
    );
    response
}
