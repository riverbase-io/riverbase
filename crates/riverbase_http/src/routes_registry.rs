//! Registry for per-domain custom HTTP routes ([ARC-03]).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use axum::Router;

type RoutesBuilder = Box<dyn Fn(&str) -> Option<Router> + Send + Sync>;

static REGISTRY: OnceLock<Mutex<HashMap<&'static str, RoutesBuilder>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<&'static str, RoutesBuilder>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register handcrafted routes for a domain namespace (idempotent per process).
pub fn register_domain_http_routes(
    namespace: &'static str,
    builder: impl Fn(&str) -> Option<Router> + Send + Sync + 'static,
) {
    let mut guard = registry()
        .lock()
        .expect("domain http route registry poisoned");
    guard.insert(namespace, Box::new(builder));
}

/// Resolve registered routes for a mounted domain, if any.
pub fn domain_http_routes(namespace: &str, api_base: &str) -> Option<Router> {
    registry()
        .lock()
        .ok()?
        .get(namespace)
        .and_then(|builder| builder(api_base))
}
