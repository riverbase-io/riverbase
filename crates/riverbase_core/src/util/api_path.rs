//! HTTP API base path helpers — prefix for OpenAPI, auth, domain, and RxDB routes.

/// Default mount prefix for all Riverbase HTTP API routes.
pub const DEFAULT_API_BASE: &str = "/api";

/// Normalize a base path: leading `/`, no trailing `/`, empty → [`DEFAULT_API_BASE`].
pub fn normalize_api_base(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() || path == "/" {
        return DEFAULT_API_BASE.to_string();
    }
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    path.trim_end_matches('/').to_string()
}

/// Join a normalized API base with a relative suffix (`/openapi.json`, `/auth`, …).
pub fn join_api_path(base: &str, suffix: &str) -> String {
    let base = normalize_api_base(base);
    let suffix = suffix.trim();
    let suffix = if suffix.starts_with('/') {
        suffix.to_string()
    } else {
        format!("/{suffix}")
    };
    format!("{base}{suffix}")
}

/// `GET {base}/openapi.json`
pub fn openapi_json_path(base: &str) -> String {
    join_api_path(base, "/openapi.json")
}

/// Auth route prefix: `{base}/auth`
pub fn auth_base_path(base: &str) -> String {
    join_api_path(base, "/auth")
}

/// True when `path` is the auth prefix or a subpath (OAuth login, callback, etc.).
pub fn is_auth_route(path: &str, auth_base: &str) -> bool {
    let base = auth_base.trim().trim_end_matches('/');
    if base.is_empty() {
        return false;
    }
    let path = path.trim_end_matches('/');
    path == base || path.starts_with(&format!("{base}/"))
}

/// RxDB replication prefix: `{base}/rxdb`
pub fn rxdb_base_path(base: &str) -> String {
    join_api_path(base, "/rxdb")
}

/// Strip a configured API base prefix from an incoming request path.
pub fn strip_api_base<'a>(base: &str, path: &'a str) -> &'a str {
    let base = normalize_api_base(base);
    let trimmed = path.trim();
    if trimmed == base {
        return "/";
    }
    if let Some(rest) = trimmed.strip_prefix(&format!("{base}/")) {
        return rest;
    }
    trimmed.trim_start_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_and_join() {
        assert_eq!(normalize_api_base(""), "/api");
        assert_eq!(normalize_api_base("/v1/"), "/v1");
        assert_eq!(openapi_json_path("/api"), "/api/openapi.json");
        assert_eq!(auth_base_path("/v1"), "/v1/auth");
    }

    #[test]
    fn is_auth_route_matches_prefix_and_subpaths() {
        assert!(is_auth_route("/api/auth", "/api/auth"));
        assert!(is_auth_route("/api/auth/login", "/api/auth"));
        assert!(is_auth_route("/api/auth/sign-in", "/api/auth"));
        assert!(!is_auth_route("/api/exp.catalog/todo.list", "/api/auth"));
    }

    #[test]
    fn strip_api_base_prefix() {
        assert_eq!(
            strip_api_base("/api", "/api/exp.catalog/todo.list"),
            "exp.catalog/todo.list"
        );
        assert_eq!(
            strip_api_base("/api", "/riverbase.todo/todo.list"),
            "riverbase.todo/todo.list"
        );
    }
}
