use crate::base::{AggregateRoot, ScopeMap, ScopeMeta};
use crate::command::CommandTarget;
use crate::query::QueryRequest;
use serde_json::{json, Value};

/// Placeholder scope path segment when scoping is not required (`:hook`, hidden `:exec`).
pub const EMPTY_SCOPE: &str = "~";

/// Map `~` or empty scope path segments to `None` for unscoped endpoints.
pub fn normalize_scope_path(scope: &str) -> Option<&str> {
    if scope.is_empty() || scope == EMPTY_SCOPE {
        None
    } else {
        Some(scope)
    }
}

/// Validate the scope path segment against endpoint metadata.
pub fn validate_scope(scope: Option<&str>, meta: &ScopeMeta) -> Result<(), String> {
    if meta.required && scope.as_ref().is_none_or(|s| s.is_empty()) {
        return Err("scope path segment is required for this endpoint".into());
    }
    if scope.is_some() && !meta.required {
        return Err("scope path segment is not used for this endpoint".into());
    }
    if let (Some(scope), Some(schema)) = (scope, meta.schema.as_ref()) {
        validate_scope_against_schema(scope, schema)?;
    }
    Ok(())
}

fn validate_scope_against_schema(scope: &str, schema: &Value) -> Result<(), String> {
    if scope.is_empty() {
        return Err("scope path segment must not be empty".into());
    }
    if let Some(desc) = schema.get("description").and_then(|v| v.as_str()) {
        if desc.is_empty() {
            return Ok(());
        }
    }
    Ok(())
}

fn scope_map_from_path(scope: Option<&str>) -> ScopeMap {
    let mut map = ScopeMap::new();
    let Some(s) = scope else {
        return map;
    };
    if let Value::Object(obj) = decode_scope_token(s) {
        for (k, v) in obj {
            if let Some(text) = scope_value_as_string(&v) {
                map.insert(k, text);
            }
        }
    }
    map
}

fn scope_value_as_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Build aggroot exec.
pub fn build_aggroot_exec(
    resource: &str,
    identifier: &str,
    scope: Option<&str>,
    scope_meta: &ScopeMeta,
) -> Result<AggregateRoot, String> {
    validate_scope(scope, scope_meta)?;
    let aggroot = AggregateRoot::new(resource, identifier).with_scope(scope_map_from_path(scope));
    Ok(aggroot)
}

/// Build collection target.
pub fn build_collection_target(
    resource: &str,
    scope: Option<&str>,
    scope_meta: &ScopeMeta,
) -> Result<CommandTarget, String> {
    validate_scope(scope, scope_meta)?;
    Ok(CommandTarget::Collection {
        resource: resource.to_string(),
        scope: scope_map_from_path(scope),
    })
}

/// Query request with scope.
pub fn query_request_with_scope(
    mut request: QueryRequest,
    scope: Option<&str>,
    scope_meta: &ScopeMeta,
) -> Result<QueryRequest, String> {
    validate_scope(scope, scope_meta)?;
    if let Some(s) = scope {
        request.scope = Some(decode_scope_token(s));
    }
    Ok(request)
}

/// Decode a JSON-URL scope path segment into a structured object (documented subset):
/// `<domain_sid>:key=value:…` → `{ "domain_sid": …, "key": value, … }`.
pub fn decode_scope_token(token: &str) -> Value {
    let mut map = serde_json::Map::new();
    for (idx, segment) in token.split(':').filter(|s| !s.is_empty()).enumerate() {
        match segment.split_once('=') {
            Some((key, value)) => {
                map.insert(key.to_string(), decode_scope_value(value));
            }
            None if idx == 0 => {
                map.insert("domain_sid".to_string(), Value::String(segment.to_string()));
            }
            None => {
                map.insert(segment.to_string(), Value::Bool(true));
            }
        }
    }
    if map.is_empty() {
        Value::String(token.to_string())
    } else {
        Value::Object(map)
    }
}

fn decode_scope_value(value: &str) -> Value {
    if value.eq_ignore_ascii_case("true") {
        Value::Bool(true)
    } else if value.eq_ignore_ascii_case("false") {
        Value::Bool(false)
    } else if let Ok(n) = value.parse::<i64>() {
        json!(n)
    } else {
        Value::String(value.to_string())
    }
}
