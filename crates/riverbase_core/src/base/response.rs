//! Success envelope and RFC 7807 problem document types ([API-01], [API-02]).

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{json, Map, Value};

use super::error::RiverbaseError;

/// Riverbase HTTP API contract version (OpenAPI `info.version` / `api.info`).
pub const API_CONTRACT_VERSION: &str = "2.0.0";

/// RFC 7807 `type` URI prefix ([riverbase.io error catalogue](https://riverbase.io/~/rs/error/)).
pub use super::error::ERROR_TYPE_BASE;

/// Envelope ``type`` discriminators (shared with Python riverbase).
pub const ENVELOPE_GENERIC: i64 = 800;
/// Envelope Query Item constant.
pub const ENVELOPE_QUERY_ITEM: i64 = 100;
/// Envelope Query List constant.
pub const ENVELOPE_QUERY_LIST: i64 = 200;
/// Envelope Command constant.
pub const ENVELOPE_COMMAND: i64 = 300;
/// Envelope Report constant.
pub const ENVELOPE_REPORT: i64 = 400;

/// Default command response map key when ``CommandMeta.response_type`` is unset.
pub const DEFAULT_RESPONSE_TYPE: &str = "default-response";

/// Standard success envelope applied by the framework.
pub fn success_envelope(data: Value, meta: Value) -> Value {
    success_envelope_typed(ENVELOPE_GENERIC, data, meta)
}

/// Typed success envelope (`type` + `data`, plus `meta` when non-empty).
pub fn success_envelope_typed(envelope_type: i64, data: Value, meta: Value) -> Value {
    let mut body = json!({ "type": envelope_type, "data": data });
    if meta_has_entries(&meta) {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("meta".to_string(), meta);
        }
    }
    body
}

fn meta_has_entries(meta: &Value) -> bool {
    match meta {
        Value::Object(map) => !map.is_empty(),
        Value::Null => false,
        Value::Array(items) => !items.is_empty(),
        Value::String(s) => !s.is_empty(),
        Value::Bool(_) | Value::Number(_) => true,
    }
}

/// Wrap list query output (`data` rows + pagination meta).
pub fn list_envelope(rows: Value, pagination: Value) -> Value {
    success_envelope_typed(
        ENVELOPE_QUERY_LIST,
        rows,
        json!({ "pagination": pagination }),
    )
}

/// Wrap a query item in the v2 envelope.
pub fn item_envelope(data: Value, meta: Value) -> Value {
    success_envelope_typed(ENVELOPE_QUERY_ITEM, data, meta)
}

/// Wrap a report document/result in the v2 envelope.
pub fn report_envelope(data: Value, meta: Value) -> Value {
    success_envelope_typed(ENVELOPE_REPORT, data, meta)
}

/// Wrap a command response, nesting ``data`` under ``response_type``.
pub fn command_envelope(response_type: &str, data: Value, meta: Value) -> Value {
    let key = if response_type.is_empty() {
        DEFAULT_RESPONSE_TYPE
    } else {
        response_type
    };
    let nested = json!({ key: data });
    success_envelope_typed(ENVELOPE_COMMAND, nested, meta)
}

/// Build command success envelope `meta` with command key and optional resource identifiers.
/// Reads ``id`` / ``_etag`` from a flat object or a single-keyed nested map.
pub fn command_success_meta(cmdkey: &str, data: &Value) -> Value {
    let mut meta = Map::new();
    meta.insert("command".to_string(), json!(cmdkey));
    let source = match data.as_object() {
        Some(obj) if obj.contains_key("id") || obj.contains_key("_etag") => Some(obj),
        Some(obj) if obj.len() == 1 => obj.values().next().and_then(Value::as_object),
        _ => None,
    };
    if let Some(obj) = source {
        if let Some(id) = obj.get("id") {
            meta.insert("resource_id".to_string(), id.clone());
        }
        if let Some(etag) = obj.get("_etag") {
            meta.insert("etag".to_string(), etag.clone());
        }
    }
    Value::Object(meta)
}

/// RFC 7807 problem document with Riverbase extensions.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    #[schemars(rename = "type")]
    /// Problem type.
    pub problem_type: String,
    /// Title.
    pub title: String,
    /// Status.
    pub status: u16,
    /// Detail.
    pub detail: String,
    /// Instance.
    pub instance: Option<String>,
    /// Errcode.
    pub errcode: String,
    /// Errdata.
    pub errdata: Value,
    /// Errhint. Omitted from JSON when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errhint: Option<String>,
}

impl ProblemDetails {
    /// Build from riverbase error.
    pub fn from_riverbase_error(err: &RiverbaseError, instance: Option<String>) -> Self {
        Self {
            problem_type: format!("{}{}", err.error_type_base(), err.errcode.as_str()),
            title: err.http_title.clone(),
            status: err.http_status,
            detail: err.errmesg.clone(),
            instance,
            errcode: err.errcode.as_str().to_string(),
            errdata: err.errdata.clone(),
            errhint: err.errhint.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use super::super::error::{RiverbaseError, RiverbaseErrorCode};

    #[test]
    fn problem_details_serializes_errhint() {
        let err = RiverbaseError::new(
            401,
            "Unauthorized Request",
            RiverbaseErrorCode::new("AUT-146"),
            "Access token has expired.",
            json!(null),
        )
        .with_errhint("Sign in again.");
        let problem = ProblemDetails::from_riverbase_error(&err, Some("/api/example".into()));
        let value = serde_json::to_value(&problem).unwrap();
        assert_eq!(value["errhint"], "Sign in again.");
        assert_eq!(value["type"], format!("{ERROR_TYPE_BASE}AUT-146"));
    }

    #[test]
    fn problem_details_omits_null_errhint() {
        let err = RiverbaseError::new(
            404,
            "Not Found",
            RiverbaseErrorCode::new("DAT-001"),
            "Resource was not found.",
            json!(null),
        );
        let problem = ProblemDetails::from_riverbase_error(&err, None);
        let value = serde_json::to_value(&problem).unwrap();
        assert!(value.get("errhint").is_none());
    }

    #[test]
    fn command_envelope_nests_by_response_type() {
        let body = command_envelope(
            "rfq-response",
            json!({"id": "1", "title": "t", "_etag": "e"}),
            command_success_meta("create-rfq", &json!({"id": "1", "_etag": "e"})),
        );
        assert_eq!(body["type"], 300);
        assert_eq!(body["data"]["rfq-response"]["title"], "t");
        assert_eq!(body["meta"]["resource_id"], "1");
        assert_eq!(body["meta"]["etag"], "e");
    }

    #[test]
    fn generic_envelope_uses_type_800_and_omits_empty_meta() {
        let body = success_envelope(json!({"version": "2.0.0"}), json!({}));
        assert_eq!(body["type"], 800);
        assert_eq!(body["data"]["version"], "2.0.0");
        assert!(body.get("meta").is_none());
    }
}

/// Merge command metadata into an envelope `meta` object.
pub fn command_meta(base: Map<String, Value>) -> Value {
    Value::Object(base)
}

/// Format a weak/strong ETag header value (quoted).
pub fn quoted_etag(etag: &str) -> String {
    let trimmed = etag.trim().trim_matches('"');
    format!("\"{trimmed}\"")
}

/// Extract `_etag` from a resource document or success envelope `data`.
pub fn etag_from_data(value: &Value) -> Option<String> {
    value
        .get("_etag")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("data")
                .and_then(|data| {
                    data.get("_etag").or_else(|| {
                        data.as_object()
                            .and_then(|obj| obj.values().next())
                            .and_then(|inner| inner.get("_etag"))
                    })
                })
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

/// Strip quotes from an `If-Match` / ETag header value.
pub fn unquote_etag(value: &str) -> &str {
    value.trim().trim_matches('"')
}
