//! Public route exemptions and mounted namespace registry ([SEC-06], [SEC-05]).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use axum::http::Uri;

use super::{join_api_path, strip_api_base};

#[derive(Clone, Default)]
/// Route auth state structure.
pub struct RouteAuthState {
    public_paths: Arc<RwLock<HashSet<String>>>,
    public_prefixes: Arc<RwLock<Vec<String>>>,
    login_redirect_prefixes: Arc<RwLock<Vec<String>>>,
    auth_sign_in_path: Arc<RwLock<String>>,
    public_api_prefix: Arc<RwLock<String>>,
    mounted_wire_namespaces: Arc<RwLock<HashSet<String>>>,
    policy_identities: Arc<RwLock<HashMap<String, String>>>,
}

impl RouteAuthState {
    /// Construct a new value.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register public path.
    pub fn register_public_path(&self, path: impl Into<String>) {
        let mut paths = self.public_paths.write().expect("public_paths lock");
        paths.insert(path.into());
    }

    /// Register public prefix.
    pub fn register_public_prefix(&self, prefix: impl Into<String>) {
        let mut prefixes = self.public_prefixes.write().expect("public_prefixes lock");
        prefixes.push(prefix.into());
    }

    /// Register a prefix that sends unauthenticated browsers to sign-in.
    pub fn register_login_redirect_prefix(&self, prefix: impl Into<String>) {
        let mut prefixes = self
            .login_redirect_prefixes
            .write()
            .expect("login_redirect_prefixes lock");
        prefixes.push(prefix.into());
    }

    /// Sign-in path used for login redirects (`{api_base}/auth/sign-in`).
    pub fn set_auth_sign_in_path(&self, path: impl Into<String>) {
        *self
            .auth_sign_in_path
            .write()
            .expect("auth_sign_in_path lock") = path.into();
    }

    /// Public mount prefix (`/v1/reader`) used when `X-Forwarded-Prefix` is absent.
    pub fn set_public_api_prefix(&self, prefix: impl Into<String>) {
        *self
            .public_api_prefix
            .write()
            .expect("public_api_prefix lock") = prefix.into();
    }

    /// Public mount prefix for rewriting `/api/…` onto the gateway path.
    pub fn public_api_prefix(&self) -> String {
        self.public_api_prefix
            .read()
            .expect("public_api_prefix lock")
            .clone()
    }

    /// Sign-in path used for login redirects.
    pub fn auth_sign_in_path(&self) -> String {
        let path = self
            .auth_sign_in_path
            .read()
            .expect("auth_sign_in_path lock");
        if path.is_empty() {
            "/api/auth/sign-in".into()
        } else {
            path.clone()
        }
    }

    /// Whether an unauthenticated request should redirect to sign-in.
    pub fn should_login_redirect(&self, path: &str) -> bool {
        self.login_redirect_prefixes
            .read()
            .expect("login_redirect_prefixes lock")
            .iter()
            .any(|prefix| path == prefix || path.starts_with(&format!("{prefix}/")))
    }

    /// Register mounted namespace.
    pub fn register_mounted_namespace(&self, namespace: &str) {
        let wire = namespace.replace('.', "-");
        let mut namespaces = self
            .mounted_wire_namespaces
            .write()
            .expect("mounted_wire_namespaces lock");
        namespaces.insert(wire);
        namespaces.insert(namespace.to_string());
    }

    /// Register the Casbin policy identity for an HTTP wire namespace ([SEC-04]).
    pub fn register_policy_identity(&self, wire: impl Into<String>, identity: impl Into<String>) {
        let wire = wire.into();
        let identity = identity.into();
        self.register_mounted_namespace(&wire);
        self.policy_identities
            .write()
            .expect("policy_identities lock")
            .insert(wire, identity);
    }

    /// Policy identity.
    pub fn policy_identity(&self, wire: &str) -> Option<String> {
        self.policy_identities
            .read()
            .expect("policy_identities lock")
            .get(wire)
            .cloned()
    }

    /// Whether this is public.
    pub fn is_public(&self, path: &str) -> bool {
        let paths = self.public_paths.read().expect("public_paths lock");
        if paths.contains(path) {
            return true;
        }
        let prefixes = self.public_prefixes.read().expect("public_prefixes lock");
        if prefixes.iter().any(|prefix| path.starts_with(prefix)) {
            return true;
        }
        // Hook token is the credential; require_bearer must not pre-empt the route ([SEC-09]).
        path.contains(":hook/")
    }

    /// Whether this is mounted wire namespace.
    pub fn is_mounted_wire_namespace(&self, wire: &str) -> bool {
        self.mounted_wire_namespaces
            .read()
            .expect("mounted_wire_namespaces lock")
            .contains(wire)
    }

    /// Mounted namespace count.
    pub fn mounted_namespace_count(&self) -> usize {
        self.mounted_wire_namespaces
            .read()
            .expect("mounted_wire_namespaces lock")
            .len()
    }

    /// Public exemptions.
    pub fn public_exemptions(&self) -> Vec<String> {
        let paths = self.public_paths.read().expect("public_paths lock");
        let mut out: Vec<String> = paths.iter().cloned().collect();
        out.sort();
        out
    }
}

/// Wire namespace from path.
pub fn wire_namespace_from_path(path: &str, api_base: &str) -> Option<String> {
    let path = strip_api_base(api_base, path);
    let (wire_ns, _) = path.split_once('/')?;
    Some(wire_ns.to_string())
}

/// Normalize request path.
pub fn normalize_request_path(uri: &Uri) -> String {
    uri.path().to_string()
}

/// Register framework public routes.
pub fn register_framework_public_routes(state: &RouteAuthState, api_base: &str) {
    state.register_public_path("/".to_string());
    state.register_public_path("/health".to_string());
    state.register_public_path("/ready".to_string());
    state.register_public_path("/openapi.json".to_string());
    state.register_public_path(join_api_path(api_base, "/api.info"));
    state.register_public_prefix(join_api_path(api_base, "/openapi"));
    state.register_public_prefix(join_api_path(api_base, "/auth"));
    state.set_auth_sign_in_path(join_api_path(api_base, "/auth/sign-in"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framework_public_routes_include_openapi_and_api_info() {
        let state = RouteAuthState::new();
        register_framework_public_routes(&state, "/api");
        assert!(state.is_public("/openapi.json"));
        assert!(state.is_public("/api/api.info"));
        assert!(state.is_public("/api/openapi.json"));
        assert!(state.is_public("/api/auth/info"));
        assert!(!state.is_public("/api/sourcing/create-rfq:post/rfq"));
        assert!(state.is_public("/api/exp.payment/payment-webhook:hook/v2.token"));
    }

    #[test]
    fn login_redirect_prefixes_match_nested_paths() {
        let state = RouteAuthState::new();
        state.register_login_redirect_prefix("/api/gfs.publication/pkg");
        state.register_login_redirect_prefix("/api/gfs.publication/gca");
        assert!(state.should_login_redirect("/api/gfs.publication/pkg/abc/oid/index.html"));
        assert!(
            state.should_login_redirect("/api/gfs.publication/gca/acme/handbook/git:HEAD/page.md")
        );
        assert!(!state.should_login_redirect("/api/gfs.publication/publication.list"));
        assert_eq!(state.auth_sign_in_path(), "/api/auth/sign-in");
    }
}
