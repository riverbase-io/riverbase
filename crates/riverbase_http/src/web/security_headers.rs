//! Baseline HTTP security headers ([SUR-08]).

use axum::extract::Request;
use axum::http::header::{HeaderName, HeaderValue, X_CONTENT_TYPE_OPTIONS};
use axum::middleware::Next;
use axum::response::Response;

static X_FRAME_OPTIONS: HeaderName = HeaderName::from_static("x-frame-options");
static REFERRER_POLICY: HeaderName = HeaderName::from_static("referrer-policy");
static PERMISSIONS_POLICY: HeaderName = HeaderName::from_static("permissions-policy");

/// Attach nosniff / frame-deny / referrer headers to every response.
///
/// Handlers may set `X-Frame-Options` or `Referrer-Policy` first (e.g.
/// `SAMEORIGIN` and a same-origin referrer for embeddable publication blobs).
/// This layer does not overwrite those.
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if !headers.contains_key(&X_FRAME_OPTIONS) {
        headers.insert(&X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    }
    if !headers.contains_key(&REFERRER_POLICY) {
        headers.insert(&REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    }
    headers.insert(
        &PERMISSIONS_POLICY,
        HeaderValue::from_static("interest-cohort=()"),
    );
    response
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::middleware;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    #[tokio::test]
    async fn defaults_to_frame_deny() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(super::security_headers));
        let res = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers().get("x-frame-options").unwrap(), "DENY");
    }

    #[tokio::test]
    async fn keeps_handler_sameorigin() {
        let app = Router::new()
            .route(
                "/",
                get(|| async {
                    (
                        [(
                            axum::http::HeaderName::from_static("x-frame-options"),
                            "SAMEORIGIN",
                        )],
                        "ok",
                    )
                }),
            )
            .layer(middleware::from_fn(super::security_headers));
        let res = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.headers().get("x-frame-options").unwrap(), "SAMEORIGIN");
    }

    #[tokio::test]
    async fn keeps_handler_referrer_policy() {
        let app = Router::new()
            .route(
                "/",
                get(|| async {
                    (
                        [(
                            axum::http::HeaderName::from_static("referrer-policy"),
                            "strict-origin-when-cross-origin",
                        )],
                        "ok",
                    )
                }),
            )
            .layer(middleware::from_fn(super::security_headers));
        let res = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            res.headers().get("referrer-policy").unwrap(),
            "strict-origin-when-cross-origin"
        );
    }
}
