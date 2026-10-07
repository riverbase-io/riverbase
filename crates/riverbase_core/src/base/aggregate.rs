use serde::{Deserialize, Serialize};

use super::scope::ScopeMap;

/// Locates the aggregate instance a command operates on (Riverbase `AggregateRoot`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregateRoot {
    /// Resource.
    pub resource: String,
    /// Identifier.
    pub identifier: String,
    #[serde(default, skip_serializing_if = "ScopeMap::is_empty")]
    /// Scope.
    pub scope: ScopeMap,
}

impl AggregateRoot {
    /// Construct a new value.
    pub fn new(resource: impl Into<String>, identifier: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            identifier: identifier.into(),
            scope: ScopeMap::new(),
        }
    }

    /// Set scope and return self.
    pub fn with_scope(mut self, scope: ScopeMap) -> Self {
        self.scope = scope;
        self
    }

    /// Set scope entry and return self.
    pub fn with_scope_entry(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.scope.insert(key.into(), value.into());
        self
    }
}
