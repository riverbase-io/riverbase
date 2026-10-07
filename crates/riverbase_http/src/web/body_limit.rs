//! Request body size guard (`SUR-02` / `WEB-103`).

use axum::extract::Request;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::middleware::Next;
use axum::response::Response;

use crate::base::RiverbaseError;
use crate::http_response::error_into_response;

/// Body too large.
pub fn body_too_large(limit: usize) -> RiverbaseError {
    crate::errors::WEB_103.with_data(serde_json::json!({ "limit_bytes": limit }))
}

/// Reject requests whose `Content-Length` exceeds `limit` with registered `WEB-103`.
///
/// Complements [`axum::extract::DefaultBodyLimit`], which enforces the same ceiling when
/// extractors buffer the body (including chunked transfers without `Content-Length`).
pub async fn enforce_request_body_limit(
    limit: usize,
    upload_limit: usize,
    req: Request,
    next: Next,
) -> Response {
    let effective = if is_multipart(&req) {
        upload_limit.max(limit)
    } else {
        limit
    };
    if let Some(len) = content_length(&req) {
        if len > effective {
            return error_into_response(body_too_large(effective));
        }
    }
    next.run(req).await
}

fn is_multipart(req: &Request) -> bool {
    req.headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("multipart/"))
}

fn content_length(req: &Request) -> Option<usize> {
    req.headers()
        .get(CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::middleware::from_fn;
    use axum::routing::post;
    use axum::Router;
    use tower::ServiceExt;

    #[tokio::test]
    async fn oversized_content_length_returns_web_103() {
        let app = Router::new()
            .route("/", post(|| async { "ok" }))
            .layer(from_fn(|req, next| {
                enforce_request_body_limit(16, 16, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/")
                    .header(CONTENT_LENGTH, "64")
                    .body(Body::from(vec![0u8; 64]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["errcode"], "WEB-103");
    }

    #[tokio::test]
    async fn under_limit_passes() {
        let app = Router::new()
            .route("/", post(|| async { "ok" }))
            .layer(from_fn(|req, next| {
                enforce_request_body_limit(1024, 1024, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/")
                    .header(CONTENT_LENGTH, "4")
                    .body(Body::from("ping"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }
}
