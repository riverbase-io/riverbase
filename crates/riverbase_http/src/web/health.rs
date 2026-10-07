//! Process health and readiness probes ([OPS-01]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use serde_json::json;

/// Shared readiness flag flipped when the HTTP stack is ready to serve traffic.
#[derive(Clone, Default)]
pub struct ReadinessState {
    ready: Arc<AtomicBool>,
    migrations_ready: Arc<AtomicBool>,
}

impl ReadinessState {
    /// Construct a new value.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set ready.
    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    /// Set migrations ready.
    pub fn set_migrations_ready(&self, ready: bool) {
        self.migrations_ready.store(ready, Ordering::SeqCst);
    }

    /// Whether this is ready.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst) && self.migrations_ready.load(Ordering::SeqCst)
    }

    /// Whether this is migrations ready.
    pub fn is_migrations_ready(&self) -> bool {
        self.migrations_ready.load(Ordering::SeqCst)
    }
}

/// Health routes.
pub fn health_routes<S>(readiness: ReadinessState) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    axum::Router::new().route("/health", get(liveness)).route(
        "/ready",
        get({
            let readiness = readiness.clone();
            move || readiness_probe(readiness)
        }),
    )
}

async fn liveness() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

async fn readiness_probe(readiness: ReadinessState) -> impl IntoResponse {
    if readiness.is_ready() {
        (StatusCode::OK, Json(json!({ "status": "ready" })))
    } else {
        let reason = if !readiness.is_migrations_ready() {
            "migrations_pending"
        } else {
            "not_ready"
        };
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "not_ready", "reason": reason })),
        )
    }
}
