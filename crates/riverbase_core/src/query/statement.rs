//! Filter statement grammar: parse `query`/`qbase` JSON objects into an [`Expr`] over
//! **logical** field names, validated against a [`QueryInterface`].
//!
//! See `docs/02-design/query/10-query-engine-design.md` §2.3 / §6.1.

use serde_json::Value;

use super::interface::{Operator, QueryInterface};
use crate::base::RiverbaseResult;
use crate::datastore::dsl::{Expr, FieldPath, PredicateOp};

/// Apply (`.`) vs. negate (`!`) mode for an operator statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Apply.
    Apply,
    /// Negate.
    Negate,
}

/// A parsed statement key: `<field><sep><operator>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorStatement {
    /// Logical field name; empty (`None`) for composites (`.and`/`.or`).
    pub field: Option<String>,
    /// Operator token (may be empty → default operator).
    pub operator: String,
    /// Mode.
    pub mode: Mode,
}

/// Split a statement key on the first `.` or `!` (Python `RX_PARAM_SPLIT`).
pub fn parse_operator_statement(key: &str) -> OperatorStatement {
    let sep = key.char_indices().find(|(_, c)| *c == '.' || *c == '!');
    match sep {
        Some((idx, c)) => {
            let field = &key[..idx];
            let operator = &key[idx + c.len_utf8()..];
            OperatorStatement {
                field: (!field.is_empty()).then(|| field.to_string()),
                operator: operator.to_string(),
                mode: if c == '!' { Mode::Negate } else { Mode::Apply },
            }
        }
        None => OperatorStatement {
            field: (!key.is_empty()).then(|| key.to_string()),
            operator: String::new(),
            mode: Mode::Apply,
        },
    }
}

/// Parse a statement object into an `Expr` over logical field names.
pub fn parse_statement(value: &Value, iface: &dyn QueryInterface) -> RiverbaseResult<Expr> {
    let obj = value
        .as_object()
        .ok_or_else(|| crate::errors::WEB_105.with_data(value.to_string()))?;

    let mut terms = Vec::with_capacity(obj.len());
    for (key, operand) in obj {
        let stmt = parse_operator_statement(key);
        match &stmt.field {
            None => terms.push(parse_composite(&stmt, operand, iface)?),
            Some(field) => terms.push(parse_field(&stmt, field, operand, iface)?),
        }
    }

    Ok(match terms.len() {
        0 => Expr::And(vec![]),
        1 => terms.into_iter().next().expect("one term"),
        _ => Expr::And(terms),
    })
}

fn parse_composite(
    stmt: &OperatorStatement,
    operand: &Value,
    iface: &dyn QueryInterface,
) -> RiverbaseResult<Expr> {
    let items = operand
        .as_array()
        .ok_or_else(|| crate::errors::WEB_106.with_data(operand.to_string()))?;
    let children = items
        .iter()
        .map(|item| parse_statement(item, iface))
        .collect::<RiverbaseResult<Vec<_>>>()?;

    let group = match stmt.operator.to_ascii_lowercase().as_str() {
        "and" => Expr::And(children),
        "or" => Expr::Or(children),
        other => return Err(crate::errors::QRY_110.with_data(other.to_string())),
    };

    Ok(match stmt.mode {
        Mode::Apply => group,
        Mode::Negate => Expr::Not(Box::new(group)),
    })
}

fn parse_field(
    stmt: &OperatorStatement,
    field: &str,
    operand: &Value,
    iface: &dyn QueryInterface,
) -> RiverbaseResult<Expr> {
    let def = iface
        .field(field)
        .ok_or_else(|| crate::errors::QRY_143.with_data(field.to_string()))?;

    let op = if stmt.operator.is_empty() {
        def.preset.default
    } else {
        Operator::parse(&stmt.operator)
            .ok_or_else(|| crate::errors::QRY_144.with_data(stmt.operator.clone()))?
    };

    if !def.preset.allows(op) {
        return Err(crate::errors::QRY_145.with_data(format!("{}.{}", field, op.token())));
    }

    let predicate = apply_predicate(op);
    let base = Expr::Field {
        path: FieldPath(field.to_string()),
        op: predicate,
        value: operand.clone(),
    };

    Ok(match stmt.mode {
        Mode::Apply => base,
        Mode::Negate => negate(op)
            .map(|inverse| Expr::Field {
                path: FieldPath(field.to_string()),
                op: inverse,
                value: operand.clone(),
            })
            .unwrap_or_else(|| Expr::Not(Box::new(base))),
    })
}

/// Operator → predicate op in apply (`.`) mode.
pub fn apply_predicate(op: Operator) -> PredicateOp {
    match op {
        Operator::Eq => PredicateOp::Eq,
        Operator::Ne => PredicateOp::Ne,
        Operator::Gt => PredicateOp::Gt,
        Operator::Gte => PredicateOp::Gte,
        Operator::Lt => PredicateOp::Lt,
        Operator::Lte => PredicateOp::Lte,
        Operator::In => PredicateOp::In,
        Operator::NotIn => PredicateOp::NotIn,
        Operator::Has => PredicateOp::Like,
        Operator::Ov => PredicateOp::Overlap,
        Operator::Between => PredicateOp::Between,
    }
}

/// Inverse predicate op for negate (`!`) mode, when a clean inverse exists.
/// Operators without an inverse (`Has`/`Ov`/`Between`) return `None` and the caller
/// wraps the expression in `Expr::Not`.
pub fn negate(op: Operator) -> Option<PredicateOp> {
    Some(match op {
        Operator::Eq => PredicateOp::Ne,
        Operator::Ne => PredicateOp::Eq,
        Operator::Gt => PredicateOp::Lte,
        Operator::Gte => PredicateOp::Lt,
        Operator::Lt => PredicateOp::Gte,
        Operator::Lte => PredicateOp::Gt,
        Operator::In => PredicateOp::NotIn,
        Operator::NotIn => PredicateOp::In,
        Operator::Has | Operator::Ov | Operator::Between => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::interface::{FieldDef, PRESET_DATETIME, PRESET_STRING, PRESET_UUID};

    struct TestInterface;
    static TEST_FIELDS: &[FieldDef] = &[
        FieldDef::new("id", "ID", &PRESET_UUID).identifier(),
        FieldDef::new("name", "Name", &PRESET_STRING),
        FieldDef::new("created", "Created", &PRESET_DATETIME),
    ];
    impl QueryInterface for TestInterface {
        fn fields(&self) -> &'static [FieldDef] {
            TEST_FIELDS
        }
    }

    #[test]
    fn omitted_operator_uses_preset_default() {
        // `id` uses the uuid preset (default `eq`); `name` uses string (default `has`).
        let id = parse_statement(&serde_json::json!({ "id": "abc" }), &TestInterface).unwrap();
        match id {
            Expr::Field { op, .. } => assert_eq!(op, PredicateOp::Eq),
            _ => panic!("expected field"),
        }
        let name =
            parse_statement(&serde_json::json!({ "name": "Harry" }), &TestInterface).unwrap();
        match name {
            Expr::Field { op, .. } => assert_eq!(op, PredicateOp::Like),
            _ => panic!("expected field"),
        }
    }

    #[test]
    fn negate_inverts_operator() {
        let v = serde_json::json!({ "name!eq": "Harry" });
        let expr = parse_statement(&v, &TestInterface).unwrap();
        match expr {
            Expr::Field { op, .. } => assert_eq!(op, PredicateOp::Ne),
            _ => panic!("expected field"),
        }
    }

    #[test]
    fn nested_or_group() {
        let v = serde_json::json!({
            ".or": [ { "name.has": "harry" }, { "name.has": "ron" } ]
        });
        let expr = parse_statement(&v, &TestInterface).unwrap();
        match expr {
            Expr::Or(items) => assert_eq!(items.len(), 2),
            _ => panic!("expected or-group"),
        }
    }

    #[test]
    fn unknown_field_errors() {
        let v = serde_json::json!({ "missing.eq": 1 });
        assert!(parse_statement(&v, &TestInterface).is_err());
    }
}
