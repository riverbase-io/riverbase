//! Backend **binding** — the only place storage mapping, joins, and operand coercion
//! live. Logical interface field names resolve to physical columns (local or via a
//! binding-declared join). There is **no** `resource:field` wire syntax.
//!
//! See `docs/02-design/query/10-query-engine-design.md` §3.2.

use serde_json::Value;

use super::interface::Operator;
use crate::base::RiverbaseResult;
use crate::datastore::dsl::JoinKind;

/// Where a logical field's data physically lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldSource {
    /// A column on the base `source()`.
    Local(String),
    /// A column reached via a named join declared in [`QueryBinding::joins`].
    Joined {
        /// Join name declared on the query binding.
        join: String,
        /// Physical column on the joined relation.
        column: String,
    },
}

impl FieldSource {
    /// Local.
    pub fn local(column: impl Into<String>) -> Self {
        FieldSource::Local(column.into())
    }

    /// Joined.
    pub fn joined(join: impl Into<String>, column: impl Into<String>) -> Self {
        FieldSource::Joined {
            join: join.into(),
            column: column.into(),
        }
    }
}

/// A join declared by a binding (never introduced by client input).
#[derive(Debug, Clone)]
pub struct JoinDef {
    /// Name.
    pub name: &'static str,
    /// Foreign source.
    pub foreign_source: &'static str,
    /// Foreign column.
    pub foreign_column: &'static str,
    /// Local column.
    pub local_column: &'static str,
    /// Kind.
    pub kind: JoinKind,
}

/// Maps a resource's logical fields to physical storage.
pub trait QueryBinding: Send + Sync {
    /// Base table / read-model name.
    fn source(&self) -> &'static str;

    /// Declared joins (usually none).
    fn joins(&self) -> &'static [JoinDef] {
        &[]
    }

    /// Resolve a logical field to its physical source. Default: identity local column.
    fn resolve(&self, field: &str) -> FieldSource {
        FieldSource::Local(field.to_string())
    }

    /// Physical column for the identifier field.
    fn identifier_column(&self) -> &str {
        "id"
    }

    /// Validate/coerce an operand before it becomes an expression.
    fn coerce(&self, _field: &str, _op: Operator, value: Value) -> RiverbaseResult<Value> {
        Ok(value)
    }
}
