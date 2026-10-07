use std::collections::HashMap;

use serde_json::Value;

/// Key-value scope segments (replaces fixed domain_sid / domain_iid on [`AggregateRoot`](crate::AggregateRoot)).
pub type ScopeMap = HashMap<String, String>;

/// Scope segment metadata for HTTP endpoints (permission / restriction).
#[derive(Debug, Clone, Default)]
pub struct ScopeMeta {
    /// Required.
    pub required: bool,
    /// Schema.
    pub schema: Option<Value>,
}

impl ScopeMeta {
    /// None.
    pub fn none() -> Self {
        Self::default()
    }

    /// Required.
    pub fn required(schema: Option<Value>) -> Self {
        Self {
            required: true,
            schema,
        }
    }
}
