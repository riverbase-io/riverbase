use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Field path structure.
pub struct FieldPath(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Predicate Op enumeration.
pub enum PredicateOp {
    /// Eq.
    Eq,
    /// Ne.
    Ne,
    /// Gt.
    Gt,
    /// Gte.
    Gte,
    /// Lt.
    Lt,
    /// Lte.
    Lte,
    /// Like.
    Like,
    /// In.
    In,
    /// Not in.
    NotIn,
    /// Between.
    Between,
    /// Overlap.
    Overlap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Expr enumeration.
pub enum Expr {
    /// Field.
    Field {
        /// Path.
        path: FieldPath,
        /// Op.
        op: PredicateOp,
        /// Value.
        value: Value,
    },
    /// And.
    And(Vec<Expr>),
    /// Or.
    Or(Vec<Expr>),
    /// Not.
    Not(Box<Expr>),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
/// Order Direction enumeration.
pub enum OrderDirection {
    /// Asc.
    Asc,
    /// Desc.
    Desc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Order spec structure.
pub struct OrderSpec {
    /// Field.
    pub field: FieldPath,
    /// Direction.
    pub direction: OrderDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Projection structure.
pub struct Projection {
    /// Fields.
    pub fields: Vec<FieldPath>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Page spec structure.
pub struct PageSpec {
    /// Offset.
    pub offset: u64,
    /// Limit.
    pub limit: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
/// Join Kind enumeration.
pub enum JoinKind {
    /// Inner.
    Inner,
    /// Left.
    Left,
}

/// A binding-declared join carried on a [`DataQuery`]. Joins are never introduced by
/// client input — only by a resource's `QueryBinding`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinSpec {
    /// Join handle referenced by qualified field paths (`<join>.<column>`).
    pub name: String,
    /// Foreign source.
    pub foreign_source: String,
    /// Foreign column.
    pub foreign_column: String,
    /// Local column.
    pub local_column: String,
    /// Kind.
    pub kind: JoinKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
/// Data query structure.
pub struct DataQuery {
    /// Source.
    pub source: String,
    /// Joins referenced by bound fields (empty for single-source resources).
    #[serde(default)]
    pub join: Vec<JoinSpec>,
    /// Filter.
    pub filter: Option<Expr>,
    /// Order.
    pub order: Vec<OrderSpec>,
    /// Physical column names to select (no logical names). Empty/`None` selects all.
    pub projection: Option<Projection>,
    /// Output relabel applied **after** projection: `physical_column → logical_name`.
    #[serde(default)]
    pub alias: BTreeMap<String, String>,
    /// Page.
    pub page: Option<PageSpec>,
    /// When false, list queries skip exact total count and return `total = -1`.
    #[serde(default = "default_count_total")]
    pub count_total: bool,
    /// Full-text search term; applied against `_txt` when the resource allows it.
    #[serde(default)]
    pub text: Option<String>,
    /// Server-side authorization restriction applied **before** [`Self::filter`]; computed from
    /// engine context (scope policy + casbin) and **not** validated against the public interface.
    #[serde(default)]
    pub policy_filter: Option<Expr>,
}

fn default_count_total() -> bool {
    true
}

impl DataQuery {
    /// List.
    pub fn list(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            join: vec![],
            filter: None,
            order: vec![],
            projection: None,
            alias: BTreeMap::new(),
            page: None,
            count_total: true,
            text: None,
            policy_filter: None,
        }
    }

    /// Set page and return self.
    pub fn with_page(mut self, offset: u64, limit: u64) -> Self {
        self.page = Some(PageSpec { offset, limit });
        self
    }
}

impl Expr {
    /// Build a `Field` predicate.
    pub fn field(path: impl Into<String>, op: PredicateOp, value: Value) -> Self {
        Expr::Field {
            path: FieldPath(path.into()),
            op,
            value,
        }
    }

    /// Build an equality predicate (`path = value`).
    pub fn eq(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Expr::field(path, PredicateOp::Eq, value.into())
    }

    /// Build an `IN` predicate (`path IN values`).
    pub fn in_list(path: impl Into<String>, values: impl Into<Value>) -> Self {
        Expr::field(path, PredicateOp::In, values.into())
    }

    /// Combine two optional predicates with logical AND, flattening `None`s.
    pub fn and_opt(a: Option<Expr>, b: Option<Expr>) -> Option<Expr> {
        match (a, b) {
            (Some(a), Some(b)) => Some(Expr::And(vec![a, b])),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn and_opt_combines_two_predicates() {
        let a = Some(Expr::eq("org", "a"));
        let b = Some(Expr::eq("status", "open"));
        match Expr::and_opt(a, b).unwrap() {
            Expr::And(items) => assert_eq!(items.len(), 2),
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn and_opt_flattens_none() {
        let only = Expr::eq("x", "1");
        assert!(matches!(
            Expr::and_opt(Some(only.clone()), None),
            Some(Expr::Field { .. })
        ));
        assert!(Expr::and_opt(None, None).is_none());
    }

    #[test]
    fn eq_builds_field_predicate() {
        match Expr::eq("organization_id", json!("org-1")) {
            Expr::Field { path, op, value } => {
                assert_eq!(path.0, "organization_id");
                assert_eq!(op, PredicateOp::Eq);
                assert_eq!(value, json!("org-1"));
            }
            other => panic!("expected Field, got {other:?}"),
        }
    }
}
