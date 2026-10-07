//! Apply [`DataQuery`](crate::datastore::dsl::DataQuery) filter expressions to Diesel boxed queries.

use diesel::expression::{
    is_aggregate, AppearsOnTable, Expression, SelectableExpression, ValidGrouping,
};
use diesel::pg::Pg;
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::QueryResult;
use diesel::sql_types::{BigInt, Bool as SqlBool, Double, Text};
use serde_json::Value;

use crate::datastore::dsl::{Expr, PredicateOp};
use crate::datastore::error::DataResult;

#[derive(Debug, Clone)]
enum PredicatePart {
    Sql(String),
    Text(String),
    Bool(bool),
    I64(i64),
    F64(f64),
}

/// Dynamically lowered PostgreSQL predicate with values emitted as bind parameters.
#[derive(Debug, Clone)]
pub struct PgPredicate {
    parts: Vec<PredicatePart>,
}

impl Expression for PgPredicate {
    type SqlType = SqlBool;
}

impl QueryId for PgPredicate {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<QS> SelectableExpression<QS> for PgPredicate {}
impl<QS> AppearsOnTable<QS> for PgPredicate {}

impl<GB> ValidGrouping<GB> for PgPredicate {
    type IsAggregate = is_aggregate::Never;
}

impl QueryFragment<Pg> for PgPredicate {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, Pg>) -> QueryResult<()> {
        out.unsafe_to_cache_prepared();
        for part in &self.parts {
            match part {
                PredicatePart::Sql(sql) => out.push_sql(sql),
                PredicatePart::Text(value) => out.push_bind_param::<Text, _>(value)?,
                PredicatePart::Bool(value) => out.push_bind_param::<SqlBool, _>(value)?,
                PredicatePart::I64(value) => out.push_bind_param::<BigInt, _>(value)?,
                PredicatePart::F64(value) => out.push_bind_param::<Double, _>(value)?,
            }
        }
        Ok(())
    }
}

/// Lower a predicate with all client/server values bound separately from SQL structure.
pub fn expr_to_predicate(expr: &Expr) -> DataResult<PgPredicate> {
    let mut parts = Vec::new();
    predicate_parts(expr, &mut parts)?;
    Ok(PgPredicate { parts })
}

fn predicate_parts(expr: &Expr, parts: &mut Vec<PredicatePart>) -> DataResult<()> {
    match expr {
        Expr::Field { path, op, value } => field_parts(&path.0, op, value, parts),
        Expr::And(items) | Expr::Or(items) => {
            if items.is_empty() {
                return Err(crate::errors::DAT_057.with_data("empty boolean filter"));
            }
            parts.push(PredicatePart::Sql("(".to_string()));
            let separator = if matches!(expr, Expr::And(_)) {
                " AND "
            } else {
                " OR "
            };
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    parts.push(PredicatePart::Sql(separator.to_string()));
                }
                predicate_parts(item, parts)?;
            }
            parts.push(PredicatePart::Sql(")".to_string()));
            Ok(())
        }
        Expr::Not(inner) => {
            parts.push(PredicatePart::Sql("NOT (".to_string()));
            predicate_parts(inner, parts)?;
            parts.push(PredicatePart::Sql(")".to_string()));
            Ok(())
        }
    }
}

fn field_parts(
    column: &str,
    op: &PredicateOp,
    value: &Value,
    parts: &mut Vec<PredicatePart>,
) -> DataResult<()> {
    let column = quote_ident(column);
    match op {
        PredicateOp::Eq | PredicateOp::Ne if value.is_null() => {
            parts.push(PredicatePart::Sql(format!(
                "{column} IS {}NULL",
                if matches!(op, PredicateOp::Ne) {
                    "NOT "
                } else {
                    ""
                }
            )));
        }
        PredicateOp::Eq | PredicateOp::Ne => {
            let operator = if matches!(op, PredicateOp::Eq) {
                " = "
            } else {
                " <> "
            };
            push_comparison(&column, operator, value, parts)?;
        }
        PredicateOp::Gt | PredicateOp::Gte | PredicateOp::Lt | PredicateOp::Lte => {
            let operator = match op {
                PredicateOp::Gt => " > ",
                PredicateOp::Gte => " >= ",
                PredicateOp::Lt => " < ",
                PredicateOp::Lte => " <= ",
                _ => unreachable!(),
            };
            push_comparison(&column, operator, value, parts)?;
        }
        PredicateOp::Like => {
            let escaped = as_str(value)
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            parts.push(PredicatePart::Sql(format!("{column}::text ILIKE ")));
            parts.push(PredicatePart::Text(format!("%{escaped}%")));
            parts.push(PredicatePart::Sql(" ESCAPE '\\'".to_string()));
        }
        PredicateOp::In | PredicateOp::NotIn => {
            let values = as_str_list(value);
            if values.is_empty() {
                return Err(crate::errors::DAT_058.with_data("empty membership filter"));
            }
            parts.push(PredicatePart::Sql(format!(
                "{column}::text {} (",
                if matches!(op, PredicateOp::In) {
                    "IN"
                } else {
                    "NOT IN"
                }
            )));
            for (index, value) in values.into_iter().enumerate() {
                if index > 0 {
                    parts.push(PredicatePart::Sql(", ".to_string()));
                }
                parts.push(PredicatePart::Text(value));
            }
            parts.push(PredicatePart::Sql(")".to_string()));
        }
        PredicateOp::Between => {
            let Value::Array(items) = value else {
                return Err(
                    crate::errors::DAT_059.with_data("between filter requires a two-element array")
                );
            };
            if items.len() != 2 {
                return Err(
                    crate::errors::DAT_060.with_data("between filter requires a two-element array")
                );
            }
            push_comparison(&column, " >= ", &items[0], parts)?;
            parts.push(PredicatePart::Sql(" AND ".to_string()));
            push_comparison(&column, " <= ", &items[1], parts)?;
        }
        PredicateOp::Overlap => {
            let values = as_str_list(value);
            if values.is_empty() {
                return Err(crate::errors::DAT_061.with_data("empty overlap filter"));
            }
            // Cast both sides to text[] so uuid[]/text[] columns accept text binds.
            parts.push(PredicatePart::Sql(format!("{column}::text[] && ARRAY[")));
            for (index, item) in values.into_iter().enumerate() {
                if index > 0 {
                    parts.push(PredicatePart::Sql(", ".to_string()));
                }
                parts.push(PredicatePart::Text(item));
            }
            parts.push(PredicatePart::Sql("]::text[]".to_string()));
        }
    }
    Ok(())
}

fn push_comparison(
    column: &str,
    operator: &str,
    value: &Value,
    parts: &mut Vec<PredicatePart>,
) -> DataResult<()> {
    match value {
        Value::Bool(value) => {
            parts.push(PredicatePart::Sql(format!("{column}{operator}")));
            parts.push(PredicatePart::Bool(*value));
        }
        Value::Number(number) => {
            parts.push(PredicatePart::Sql(format!("{column}{operator}")));
            if let Some(value) = number.as_i64() {
                parts.push(PredicatePart::I64(value));
            } else if number.as_u64().is_some() {
                return Err(crate::errors::DAT_062
                    .with_data("unsigned predicate exceeds PostgreSQL BIGINT range"));
            } else {
                let value = number.as_f64().ok_or_else(|| {
                    crate::errors::DAT_063.with_data("numeric predicate is outside supported range")
                })?;
                parts.push(PredicatePart::F64(value));
            }
        }
        Value::String(value) => {
            parts.push(PredicatePart::Sql(format!("{column}::text{operator}")));
            parts.push(PredicatePart::Text(value.clone()));
        }
        other => {
            parts.push(PredicatePart::Sql(format!("{column}::text{operator}")));
            parts.push(PredicatePart::Text(as_str(other)));
        }
    }
    Ok(())
}

/// Text search predicate.
pub fn text_search_predicate(term: &str) -> DataResult<PgPredicate> {
    let term = term.trim();
    if term.is_empty() {
        return Err(crate::errors::DAT_064.with_data("empty text search term"));
    }
    let parts = term
        .split_whitespace()
        .filter_map(tsquery_prefix_token)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return Err(crate::errors::DAT_065.with_data("empty text search term"));
    }
    Ok(PgPredicate {
        parts: vec![
            PredicatePart::Sql("_txt @@ to_tsquery('english', ".to_string()),
            PredicatePart::Text(parts.join(" & ")),
            PredicatePart::Sql(")".to_string()),
        ],
    })
}

/// Borrow as r.
pub fn as_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => value.to_string(),
    }
}

/// Borrow as r list.
pub fn as_str_list(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items.iter().map(as_str).collect(),
        Value::String(s) => vec![s.clone()],
        other => vec![as_str(other)],
    }
}

/// Unsupported.
pub fn unsupported(col: &str, op: &PredicateOp) -> crate::base::RiverbaseError {
    crate::errors::DAT_066.with_data(format!("unsupported filter {col}.{op:?}"))
}

/// Render a filter expression tree as a SQL boolean clause (test/debug only; [SUR-11]).
#[cfg(test)]
pub fn expr_to_sql(expr: &Expr) -> DataResult<String> {
    expr_sql(expr)
}

#[cfg(test)]
fn expr_sql(expr: &Expr) -> DataResult<String> {
    match expr {
        Expr::Field { path, op, value } => field_sql(&path.0, op, value),
        Expr::And(items) => {
            if items.is_empty() {
                return Err(crate::errors::DAT_067.with_data("empty AND filter"));
            }
            let parts: DataResult<Vec<String>> = items.iter().map(expr_sql).collect();
            let parts = parts?;
            Ok(parts.join(" AND "))
        }
        Expr::Or(items) => {
            if items.is_empty() {
                return Err(crate::errors::DAT_068.with_data("empty OR filter"));
            }
            let parts: DataResult<Vec<String>> = items.iter().map(expr_sql).collect();
            let parts = parts?;
            Ok(format!("({})", parts.join(" OR ")))
        }
        Expr::Not(inner) => Ok(format!("NOT ({})", expr_sql(inner)?)),
    }
}

#[cfg(test)]
fn field_sql(col: &str, op: &PredicateOp, value: &Value) -> DataResult<String> {
    let col = quote_ident(col);
    Ok(match op {
        // Cast to text so comparisons work on enum and other non-text column types
        // without PostgreSQL rejecting unknown enum literals. Boolean operands use
        // native `bool = bool` because `bool::text = true` fails in PostgreSQL.
        PredicateOp::Eq => eq_sql(&col, value),
        PredicateOp::Ne => ne_sql(&col, value),
        PredicateOp::Gt => format!("{col} > {}", quote_value(value)),
        PredicateOp::Gte => format!("{col} >= {}", quote_value(value)),
        PredicateOp::Lt => format!("{col} < {}", quote_value(value)),
        PredicateOp::Lte => format!("{col} <= {}", quote_value(value)),
        // Cast to text so ILIKE works on enum and other non-text column types.
        PredicateOp::Like => format!("{col}::text ILIKE {}", quote_like(value)),
        PredicateOp::In => {
            let values = as_str_list(value);
            if values.is_empty() {
                return Err(crate::errors::DAT_069.with_data("empty IN filter"));
            }
            let list = values
                .iter()
                .map(|v| quote_value(&Value::String(v.clone())))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{col}::text IN ({list})")
        }
        PredicateOp::NotIn => {
            let values = as_str_list(value);
            if values.is_empty() {
                return Err(crate::errors::DAT_070.with_data("empty NOT IN filter"));
            }
            let list = values
                .iter()
                .map(|v| quote_value(&Value::String(v.clone())))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{col}::text NOT IN ({list})")
        }
        PredicateOp::Between => {
            let items = match value {
                Value::Array(items) if items.len() == 2 => items,
                _ => {
                    return Err(crate::errors::DAT_071
                        .with_data("between filter requires a two-element array"));
                }
            };
            format!(
                "{col} BETWEEN {} AND {}",
                quote_value(&items[0]),
                quote_value(&items[1])
            )
        }
        PredicateOp::Overlap => {
            let values = as_str_list(value);
            if values.is_empty() {
                return Err(crate::errors::DAT_072.with_data("empty overlap filter"));
            }
            let list = values
                .iter()
                .map(|v| quote_value(&Value::String(v.clone())))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{col}::text[] && ARRAY[{list}]::text[]")
        }
    })
}

#[cfg(test)]
fn parse_bool_operand(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "t" | "1" | "yes" => Some(true),
            "false" | "f" | "0" | "no" => Some(false),
            _ => None,
        },
        Value::Number(n) => n.as_i64().map(|i| i != 0),
        _ => None,
    }
}

#[cfg(test)]
fn quote_bool(b: bool) -> &'static str {
    if b {
        "TRUE"
    } else {
        "FALSE"
    }
}

#[cfg(test)]
fn eq_sql(col: &str, value: &Value) -> String {
    if value.is_null() {
        format!("{col} IS NULL")
    } else if let Some(b) = parse_bool_operand(value) {
        format!("{col} = {}", quote_bool(b))
    } else {
        format!("{col}::text = {}", quote_value(value))
    }
}

#[cfg(test)]
fn ne_sql(col: &str, value: &Value) -> String {
    if value.is_null() {
        format!("{col} IS NOT NULL")
    } else if let Some(b) = parse_bool_operand(value) {
        format!("{col} <> {}", quote_bool(b))
    } else {
        format!("{col}::text <> {}", quote_value(value))
    }
}

fn quote_ident(ident: &str) -> String {
    if ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        ident.to_string()
    } else {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }
}

#[cfg(test)]
fn quote_value(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        other => format!("'{}'", as_str(other).replace('\'', "''")),
    }
}

#[cfg(test)]
fn quote_like(value: &Value) -> String {
    let escaped = as_str(value)
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .replace('\'', "''");
    format!("'%{escaped}%' ESCAPE '\\'")
}

#[cfg(test)]
fn quote_tsquery_term(term: &str) -> String {
    format!("'{}'", term.replace('\'', "''"))
}

fn tsquery_prefix_token(token: &str) -> Option<String> {
    let cleaned: String = token.chars().filter(|c| c.is_alphanumeric()).collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(format!("{cleaned}:*"))
    }
}

/// Build a `_txt @@ to_tsquery(...)` clause with prefix matching per token (test/debug only).
#[cfg(test)]
pub fn text_search_sql(term: &str) -> DataResult<String> {
    let term = term.trim();
    if term.is_empty() {
        return Err(crate::errors::DAT_073.with_data("empty text search term"));
    }
    let parts: Vec<String> = term
        .split_whitespace()
        .filter_map(tsquery_prefix_token)
        .collect();
    if parts.is_empty() {
        return Err(crate::errors::DAT_074.with_data("empty text search term"));
    }
    let tsquery = parts.join(" & ");
    Ok(format!(
        "_txt @@ to_tsquery('english', {})",
        quote_tsquery_term(&tsquery)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    diesel::table! {
        predicate_test (id) {
            id -> BigInt,
            name -> Text,
            active -> Bool,
        }
    }

    #[test]
    fn field_sql_has_uses_ilike() {
        let sql = field_sql("status", &PredicateOp::Like, &Value::String("paid".into())).unwrap();
        assert_eq!(sql, "status::text ILIKE '%paid%' ESCAPE '\\'");
    }

    #[test]
    fn field_sql_eq_casts_to_text_for_enums() {
        let sql = field_sql(
            "product_type",
            &PredicateOp::Eq,
            &Value::String("event".into()),
        )
        .unwrap();
        assert_eq!(sql, "product_type::text = 'event'");
    }

    #[test]
    fn field_sql_eq_uses_native_bool_for_boolean_operands() {
        let sql = field_sql("active", &PredicateOp::Eq, &Value::Bool(true)).unwrap();
        assert_eq!(sql, "active = TRUE");

        let sql = field_sql("active", &PredicateOp::Eq, &Value::String("true".into())).unwrap();
        assert_eq!(sql, "active = TRUE");

        let sql = field_sql("active", &PredicateOp::Ne, &Value::Bool(false)).unwrap();
        assert_eq!(sql, "active <> FALSE");
    }

    #[test]
    fn null_equality_uses_null_predicates() {
        assert_eq!(
            field_sql("deleted_at", &PredicateOp::Eq, &Value::Null).unwrap(),
            "deleted_at IS NULL"
        );
        assert_eq!(
            field_sql("deleted_at", &PredicateOp::Ne, &Value::Null).unwrap(),
            "deleted_at IS NOT NULL"
        );
    }

    #[test]
    fn like_treats_wildcards_as_literals() {
        assert_eq!(
            field_sql(
                "name",
                &PredicateOp::Like,
                &Value::String(r#"50%_off\sale"#.into())
            )
            .unwrap(),
            r#"name::text ILIKE '%50\%\_off\\sale%' ESCAPE '\'"#
        );
    }

    #[test]
    fn field_sql_overlap_uses_array_overlap() {
        let sql = field_sql(
            "venue_scope",
            &PredicateOp::Overlap,
            &Value::String("11111111-1111-1111-1111-111111111111".into()),
        )
        .unwrap();
        assert_eq!(
            sql,
            "venue_scope::text[] && ARRAY['11111111-1111-1111-1111-111111111111']::text[]"
        );

        let sql = field_sql(
            "venue_scope",
            &PredicateOp::Overlap,
            &Value::Array(vec![Value::String("a".into()), Value::String("b".into())]),
        )
        .unwrap();
        assert_eq!(sql, "venue_scope::text[] && ARRAY['a', 'b']::text[]");
    }

    #[test]
    fn and_sql_joins_clauses() {
        let expr = Expr::And(vec![
            Expr::Field {
                path: crate::datastore::dsl::FieldPath("status".into()),
                op: PredicateOp::Like,
                value: Value::String("paid".into()),
            },
            Expr::Field {
                path: crate::datastore::dsl::FieldPath("channel".into()),
                op: PredicateOp::Eq,
                value: Value::String("VCB".into()),
            },
        ]);
        let sql = expr_to_sql(&expr).unwrap();
        assert_eq!(
            sql,
            "status::text ILIKE '%paid%' ESCAPE '\\' AND channel::text = 'VCB'"
        );
    }

    #[test]
    fn text_search_sql_builds_prefix_tsquery() {
        let sql = text_search_sql("tunnels").unwrap();
        assert_eq!(sql, "_txt @@ to_tsquery('english', 'tunnels:*')");
    }

    #[test]
    fn text_search_sql_prefix_matches_partial_words() {
        let sql = text_search_sql("tunn").unwrap();
        assert_eq!(sql, "_txt @@ to_tsquery('english', 'tunn:*')");
    }

    #[test]
    fn text_search_sql_joins_multi_word_prefixes() {
        let sql = text_search_sql("cu tunn").unwrap();
        assert_eq!(sql, "_txt @@ to_tsquery('english', 'cu:* & tunn:*')");
    }

    #[test]
    fn text_search_sql_strips_non_alphanumeric_tokens() {
        let sql = text_search_sql("it's").unwrap();
        assert_eq!(sql, "_txt @@ to_tsquery('english', 'its:*')");
    }

    #[test]
    fn text_search_sql_rejects_empty_term() {
        assert!(text_search_sql("").is_err());
        assert!(text_search_sql("   ").is_err());
    }

    #[test]
    fn predicate_values_are_database_binds_not_sql_literals() {
        use diesel::QueryDsl;

        let attacker = "x' OR TRUE --";
        let predicate = expr_to_predicate(&Expr::Field {
            path: crate::datastore::dsl::FieldPath("name".into()),
            op: PredicateOp::Eq,
            value: Value::String(attacker.into()),
        })
        .expect("predicate");
        let query = predicate_test::table.filter(predicate);
        let debug = diesel::debug_query::<Pg, _>(&query).to_string();
        assert!(debug.contains("$1"));
        assert!(!debug
            .split("-- binds:")
            .next()
            .unwrap_or_default()
            .contains(attacker));
        assert!(debug.contains(attacker));
    }
}
