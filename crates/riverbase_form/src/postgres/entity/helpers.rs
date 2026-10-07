//! Shared JSON field helpers for entity upserts.

use serde_json::Value;
use uuid::Uuid;

pub fn str_field(data: &Value, key: &str, default: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string()
}

pub fn opt_str(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

pub fn bool_field(data: &Value, key: &str, default: bool) -> bool {
    data.get(key).and_then(Value::as_bool).unwrap_or(default)
}

pub fn i32_field(data: &Value, key: &str, default: i32) -> i32 {
    data.get(key)
        .and_then(Value::as_i64)
        .map(|v| v as i32)
        .unwrap_or(default)
}

pub fn uuid_field(data: &Value, key: &str) -> Option<Uuid> {
    data.get(key)
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
}

pub fn json_field(data: &Value, key: &str) -> Option<Value> {
    data.get(key).cloned()
}
