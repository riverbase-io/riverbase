use crate::base::{AggregateRoot, ScopeMap};
use serde::{Deserialize, Serialize};

/// Command dispatch target: single object or collection scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandTarget {
    /// Object.
    Object(AggregateRoot),
    /// Collection-scoped command (no single aggregate id).
    Collection {
        /// Resource name the collection command applies to.
        resource: String,
        /// Scope map attached to the collection command.
        scope: ScopeMap,
    },
}

impl CommandTarget {
    /// Collection.
    pub fn collection(resource: impl Into<String>) -> Self {
        Self::Collection {
            resource: resource.into(),
            scope: ScopeMap::new(),
        }
    }

    /// Collection scoped.
    pub fn collection_scoped(resource: impl Into<String>, scope: ScopeMap) -> Self {
        Self::Collection {
            resource: resource.into(),
            scope,
        }
    }
}
