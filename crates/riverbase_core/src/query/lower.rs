//! Lowering: `FrontendQuery` (logical, validated against the interface) →
//! `datastore::dsl::DataQuery` (physical columns, via the binding).
//!
//! See `docs/02-design/query/10-query-engine-design.md` §5 / §6.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::binding::{FieldSource, QueryBinding};
use super::interface::field_sortable;
use super::interface::{Operator, QueryInterface};
use super::primitives::{FrontendQuery, SortDirection};
use super::statement::parse_statement;
use crate::base::RiverbaseResult;
use crate::datastore::dsl::{
    DataQuery, Expr, FieldPath, JoinSpec, OrderDirection, OrderSpec, PageSpec, PredicateOp,
    Projection,
};

/// Lower a list request into a physical `DataQuery` (with paging).
pub fn lower_list(
    fq: &FrontendQuery,
    iface: &dyn QueryInterface,
    binding: &dyn QueryBinding,
) -> RiverbaseResult<DataQuery> {
    let mut query = lower_common(fq, iface, binding)?;
    query.page = Some(PageSpec {
        offset: fq.offset(),
        limit: fq.limit,
    });
    query.count_total = fq.count;
    Ok(query)
}

/// Lower an item request (no paging; filter still applies the base/user predicate).
pub fn lower_item(
    fq: &FrontendQuery,
    iface: &dyn QueryInterface,
    binding: &dyn QueryBinding,
) -> RiverbaseResult<DataQuery> {
    lower_common(fq, iface, binding)
}

/// Constrain a lowered item query to a single identifier (AND with base/user filters).
pub fn with_identifier_filter(
    mut query: DataQuery,
    iface: &dyn QueryInterface,
    binding: &dyn QueryBinding,
    id: &str,
) -> RiverbaseResult<DataQuery> {
    let mut joins = BTreeMap::new();
    for join in &query.join {
        joins.insert(join.name.clone(), join.clone());
    }
    let id_expr = bind_expr(
        Expr::Field {
            path: FieldPath(iface.identifier_field().to_string()),
            op: PredicateOp::Eq,
            value: Value::String(id.to_string()),
        },
        binding,
        &mut joins,
    )?;
    query.join = joins.into_values().collect();
    query.filter = Some(match query.filter {
        Some(existing) => Expr::And(vec![existing, id_expr]),
        None => id_expr,
    });
    query.count_total = false;
    query.page = Some(PageSpec {
        offset: 0,
        limit: 1,
    });
    Ok(query)
}

fn lower_common(
    fq: &FrontendQuery,
    iface: &dyn QueryInterface,
    binding: &dyn QueryBinding,
) -> RiverbaseResult<DataQuery> {
    let mut joins: BTreeMap<String, JoinSpec> = BTreeMap::new();

    // 4. process_query — base ∧ user, two independent trees joined at the top level.
    let base = parse_side(fq.base_query.as_ref(), iface)?;
    let user = parse_side(fq.user_query.as_ref(), iface)?;
    let filter = match (base, user) {
        (Some(b), Some(u)) => Some(Expr::And(vec![b, u])),
        (Some(b), None) => Some(b),
        (None, Some(u)) => Some(u),
        (None, None) => None,
    };
    let filter = filter
        .map(|expr| bind_expr(expr, binding, &mut joins))
        .transpose()?;

    // 5. process_sort — logical sort, fall back to default_order.
    let order = process_sort(fq, iface, binding, &mut joins)?;

    // 6. process_select — logical include/exclude → physical projection + alias map.
    let (projection, alias) = process_select(fq, iface, binding, &mut joins);

    Ok(DataQuery {
        source: binding.source().to_string(),
        join: joins.into_values().collect(),
        filter,
        policy_filter: None,
        order,
        projection,
        alias,
        page: None,
        count_total: fq.count,
        text: fq
            .text
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

fn parse_side(value: Option<&Value>, iface: &dyn QueryInterface) -> RiverbaseResult<Option<Expr>> {
    let Some(value) = value else {
        return Ok(None);
    };
    // `{}` is no predicate. `parse_statement` would emit `AND ()`, which Postgres cannot lower.
    if value.as_object().is_some_and(|obj| obj.is_empty()) {
        return Ok(None);
    }
    Ok(Some(parse_statement(value, iface)?))
}

fn process_sort(
    fq: &FrontendQuery,
    iface: &dyn QueryInterface,
    binding: &dyn QueryBinding,
    joins: &mut BTreeMap<String, JoinSpec>,
) -> RiverbaseResult<Vec<OrderSpec>> {
    let logical: Vec<(String, SortDirection)> = match fq.sort.as_ref() {
        Some(entries) if !entries.is_empty() => {
            let mut out = Vec::with_capacity(entries.len());
            for entry in entries {
                let (field, dir) = parse_sort_entry(entry);
                if field_sortable(iface, &field) {
                    out.push((field, dir));
                } else {
                    return Err(crate::errors::QRY_146.with_data(field));
                }
            }
            out
        }
        _ => iface
            .default_order()
            .iter()
            .map(|(f, d)| (f.to_string(), *d))
            .collect(),
    };

    Ok(logical
        .into_iter()
        .map(|(field, dir)| OrderSpec {
            field: FieldPath(bind_column(&field, binding, joins)),
            direction: match dir {
                SortDirection::Asc => OrderDirection::Asc,
                SortDirection::Desc => OrderDirection::Desc,
            },
        })
        .collect())
}

fn parse_sort_entry(entry: &str) -> (String, SortDirection) {
    match entry.split_once('.') {
        Some((field, dir)) => {
            let direction = if dir.eq_ignore_ascii_case("desc") {
                SortDirection::Desc
            } else {
                SortDirection::Asc
            };
            (field.to_string(), direction)
        }
        None => (entry.to_string(), SortDirection::Asc),
    }
}

fn process_select(
    fq: &FrontendQuery,
    iface: &dyn QueryInterface,
    binding: &dyn QueryBinding,
    joins: &mut BTreeMap<String, JoinSpec>,
) -> (Option<Projection>, BTreeMap<String, String>) {
    // Alias is built for every declared field so output is always relabeled to logical
    // names, regardless of whether the projection restricts the column set.
    let mut alias = BTreeMap::new();
    for field in iface.fields() {
        let column = bind_column(field.name, binding, joins);
        if column != field.name {
            alias.insert(column, field.name.to_string());
        }
    }

    let identifier = iface.identifier_field().to_string();
    let include_present = fq.include.as_ref().is_some_and(|i| !i.is_empty());
    let exclude_present = fq.exclude.as_ref().is_some_and(|e| !e.is_empty());

    if !include_present && !exclude_present {
        return (None, alias);
    }

    // Start from the include set (whitelisted to declared fields) or all selectable fields.
    let mut selected: Vec<String> = if include_present {
        fq.include
            .as_ref()
            .unwrap()
            .iter()
            .filter(|name| iface.field(name).is_some())
            .cloned()
            .collect()
    } else {
        iface
            .fields()
            .iter()
            .filter(|f| !f.hidden)
            .map(|f| f.name.to_string())
            .collect()
    };

    // The identifier is always retained when an explicit include is given.
    if include_present && !selected.iter().any(|name| name == &identifier) {
        selected.push(identifier.clone());
    }

    if let Some(exclude) = fq.exclude.as_ref() {
        selected.retain(|name| name == &identifier || !exclude.contains(name));
    }

    let fields: Vec<FieldPath> = selected
        .iter()
        .map(|name| FieldPath(bind_column(name, binding, joins)))
        .collect();

    (Some(Projection { fields }), alias)
}

/// Resolve a logical field to its physical column, collecting any referenced join.
fn bind_column(
    field: &str,
    binding: &dyn QueryBinding,
    joins: &mut BTreeMap<String, JoinSpec>,
) -> String {
    match binding.resolve(field) {
        FieldSource::Local(column) => column,
        FieldSource::Joined { join, column } => {
            collect_join(&join, binding, joins);
            format!("{join}.{column}")
        }
    }
}

fn collect_join(name: &str, binding: &dyn QueryBinding, joins: &mut BTreeMap<String, JoinSpec>) {
    if joins.contains_key(name) {
        return;
    }
    if let Some(def) = binding.joins().iter().find(|j| j.name == name) {
        joins.insert(
            name.to_string(),
            JoinSpec {
                name: def.name.to_string(),
                foreign_source: def.foreign_source.to_string(),
                foreign_column: def.foreign_column.to_string(),
                local_column: def.local_column.to_string(),
                kind: def.kind,
            },
        );
    }
}

/// Rewrite an `Expr` tree from logical field names to physical columns and coerce operands.
fn bind_expr(
    expr: Expr,
    binding: &dyn QueryBinding,
    joins: &mut BTreeMap<String, JoinSpec>,
) -> RiverbaseResult<Expr> {
    match expr {
        Expr::Field { path, op, value } => {
            let logical = path.0;
            let column = bind_column(&logical, binding, joins);
            let value = binding.coerce(&logical, predicate_to_operator(&op), value)?;
            Ok(Expr::Field {
                path: FieldPath(column),
                op,
                value,
            })
        }
        Expr::And(items) => Ok(Expr::And(
            items
                .into_iter()
                .map(|e| bind_expr(e, binding, joins))
                .collect::<RiverbaseResult<Vec<_>>>()?,
        )),
        Expr::Or(items) => Ok(Expr::Or(
            items
                .into_iter()
                .map(|e| bind_expr(e, binding, joins))
                .collect::<RiverbaseResult<Vec<_>>>()?,
        )),
        Expr::Not(inner) => Ok(Expr::Not(Box::new(bind_expr(*inner, binding, joins)?))),
    }
}

fn predicate_to_operator(op: &PredicateOp) -> Operator {
    match op {
        PredicateOp::Eq => Operator::Eq,
        PredicateOp::Ne => Operator::Ne,
        PredicateOp::Gt => Operator::Gt,
        PredicateOp::Gte => Operator::Gte,
        PredicateOp::Lt => Operator::Lt,
        PredicateOp::Lte => Operator::Lte,
        PredicateOp::In => Operator::In,
        PredicateOp::NotIn => Operator::NotIn,
        PredicateOp::Like => Operator::Has,
        PredicateOp::Overlap => Operator::Ov,
        PredicateOp::Between => Operator::Between,
    }
}

/// Apply the projection + alias contract to result rows (§6): keep only projected
/// columns (when set), then relabel physical columns to their logical names.
pub fn project_and_relabel(rows: Vec<Value>, query: &DataQuery) -> Vec<Value> {
    let keep: Option<Vec<&str>> = query
        .projection
        .as_ref()
        .map(|p| p.fields.iter().map(|f| f.0.as_str()).collect());

    rows.into_iter()
        .map(|row| relabel_row(row, keep.as_deref(), &query.alias))
        .collect()
}

fn relabel_row(row: Value, keep: Option<&[&str]>, alias: &BTreeMap<String, String>) -> Value {
    let Value::Object(obj) = row else {
        return row;
    };
    let mut out = Map::new();
    for (key, value) in obj {
        if let Some(keep) = keep {
            if !keep.contains(&key.as_str()) {
                continue;
            }
        }
        let logical = alias.get(&key).cloned().unwrap_or(key);
        out.insert(logical, value);
    }
    Value::Object(out)
}

/// Build the Riverbase pagination envelope (§7). `total` is `null` when not counted.
pub fn pagination_meta(fq: &FrontendQuery, total: i64, count_max_rows: Option<u64>) -> Value {
    let limit = if fq.limit == 0 { 1 } else { fq.limit };
    let (total_value, pages, total_capped) = if total < 0 {
        (Value::Null, 0_u64, false)
    } else if total == 0 {
        (json!(0), 0, false)
    } else if let Some(cap) = count_max_rows.filter(|cap| total as u64 > *cap) {
        (json!(cap), (cap).div_ceil(limit), true)
    } else {
        (json!(total), (total as u64).div_ceil(limit), false)
    };
    json!({
        "limit": fq.limit,
        "offset": fq.offset(),
        "page": fq.page,
        "total": total_value,
        "pages": pages,
        "count": fq.count,
        "total_capped": total_capped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::interface::{
        FieldDef, PRESET_BOOLEAN, PRESET_DATETIME, PRESET_STRING, PRESET_UUID,
    };

    struct TestInterface;
    static FIELDS: &[FieldDef] = &[
        FieldDef::new("id", "ID", &PRESET_UUID).identifier(),
        FieldDef::new("title", "Title", &PRESET_STRING),
        FieldDef::new("done", "Done", &PRESET_BOOLEAN),
        FieldDef::new("created", "Created", &PRESET_DATETIME),
    ];
    impl QueryInterface for TestInterface {
        fn fields(&self) -> &'static [FieldDef] {
            FIELDS
        }
    }

    /// Non-identity binding: `id`/`created` map to `_id`/`_created`.
    struct TestBinding;
    impl QueryBinding for TestBinding {
        fn source(&self) -> &'static str {
            "todo_examples.todo_items"
        }
        fn identifier_column(&self) -> &str {
            "_id"
        }
        fn resolve(&self, field: &str) -> FieldSource {
            match field {
                "id" => FieldSource::local("_id"),
                "created" => FieldSource::local("_created"),
                other => FieldSource::local(other),
            }
        }
    }

    fn example_query() -> FrontendQuery {
        FrontendQuery {
            limit: 10,
            page: 1,
            include: Some(vec!["title".into()]),
            sort: Some(vec!["created.desc".into()]),
            base_query: Some(json!({ "done.eq": false })),
            user_query: Some(json!({
                ".or": [ { "title.has": "buy" }, { "title.eq": "Ship" } ],
                "created.gte": "2026-01-01"
            })),
            ..Default::default()
        }
    }

    #[test]
    fn process_sort_keeps_multiple_keys_in_order() {
        let fq = FrontendQuery {
            sort: Some(vec!["title.asc".into(), "created.desc".into()]),
            ..Default::default()
        };
        let dq = lower_list(&fq, &TestInterface, &TestBinding).unwrap();
        assert_eq!(dq.order.len(), 2);
        assert_eq!(dq.order[0].field.0, "title");
        assert!(matches!(dq.order[0].direction, OrderDirection::Asc));
        assert_eq!(dq.order[1].field.0, "_created");
        assert!(matches!(dq.order[1].direction, OrderDirection::Desc));
    }

    #[test]
    fn empty_query_object_is_no_filter() {
        let fq = FrontendQuery {
            user_query: Some(json!({})),
            base_query: Some(json!({})),
            ..Default::default()
        };
        let dq = lower_list(&fq, &TestInterface, &TestBinding).unwrap();
        assert!(
            dq.filter.is_none(),
            "an empty filter object must not lower to AND ()"
        );
    }

    #[test]
    fn base_and_user_combine_at_top_level() {
        let dq = lower_list(&example_query(), &TestInterface, &TestBinding).unwrap();
        match dq.filter.as_ref().unwrap() {
            Expr::And(items) => assert_eq!(items.len(), 2, "base ∧ user"),
            other => panic!("expected top-level And, got {other:?}"),
        }
    }

    #[test]
    fn select_resolves_physical_projection_and_alias() {
        let dq = lower_list(&example_query(), &TestInterface, &TestBinding).unwrap();
        let proj: Vec<&str> = dq
            .projection
            .as_ref()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.0.as_str())
            .collect();
        assert!(proj.contains(&"title"));
        assert!(proj.contains(&"_id"), "identifier forced into projection");
        assert_eq!(dq.alias.get("_id").map(String::as_str), Some("id"));
        assert_eq!(dq.order[0].field.0, "_created");
        assert!(matches!(dq.order[0].direction, OrderDirection::Desc));
    }

    #[test]
    fn project_and_relabel_keeps_projection_and_renames() {
        let dq = lower_list(&example_query(), &TestInterface, &TestBinding).unwrap();
        let rows = vec![json!({ "_id": "x", "title": "buy milk", "_created": "c", "done": false })];
        let out = project_and_relabel(rows, &dq);
        let row = &out[0];
        assert_eq!(row.get("id").and_then(Value::as_str), Some("x"));
        assert_eq!(row.get("title").and_then(Value::as_str), Some("buy milk"));
        assert!(row.get("done").is_none(), "non-projected field dropped");
        assert!(row.get("_id").is_none(), "physical key relabeled");
    }

    #[test]
    fn pagination_pages_from_total() {
        let fq = FrontendQuery {
            limit: 50,
            page: 1,
            ..Default::default()
        };
        let meta = pagination_meta(&fq, 7, None);
        assert_eq!(meta["total"], 7);
        assert_eq!(meta["pages"], 1);
    }

    #[test]
    fn pagination_offset_from_page() {
        let fq = FrontendQuery {
            limit: 10,
            page: 3,
            ..Default::default()
        };
        assert_eq!(fq.offset(), 20);
        let meta = pagination_meta(&fq, -1, None);
        assert_eq!(meta["offset"], 20);
        assert!(meta["total"].is_null());
        assert_eq!(meta["pages"], 0);
    }

    #[test]
    fn pagination_caps_expensive_counts() {
        let fq = FrontendQuery {
            limit: 10,
            page: 1,
            count: true,
            ..Default::default()
        };
        let meta = pagination_meta(&fq, 50_000, Some(10_000));
        assert_eq!(meta["total"], 10_000);
        assert_eq!(meta["total_capped"], true);
    }

    #[test]
    fn lower_list_passes_trimmed_text() {
        let fq = FrontendQuery {
            text: Some("  tunnels  ".into()),
            ..Default::default()
        };
        let dq = lower_list(&fq, &TestInterface, &TestBinding).unwrap();
        assert_eq!(dq.text.as_deref(), Some("tunnels"));
    }

    #[test]
    fn lower_list_omits_blank_text() {
        let fq = FrontendQuery {
            text: Some("   ".into()),
            ..Default::default()
        };
        let dq = lower_list(&fq, &TestInterface, &TestBinding).unwrap();
        assert!(dq.text.is_none());
    }

    #[test]
    fn with_identifier_filter_adds_physical_id_predicate() {
        let fq = FrontendQuery::default();
        let dq = lower_item(&fq, &TestInterface, &TestBinding).unwrap();
        let dq = with_identifier_filter(
            dq,
            &TestInterface,
            &TestBinding,
            "3b5a94e7-9d46-5e66-83de-bffc2bc8057c",
        )
        .unwrap();
        assert_eq!(dq.page.as_ref().map(|p| p.limit), Some(1));
        assert!(!dq.count_total);
        match dq.filter.as_ref().unwrap() {
            Expr::Field { path, op, value } => {
                assert_eq!(path.0, "_id");
                assert!(matches!(op, PredicateOp::Eq));
                assert_eq!(value.as_str(), Some("3b5a94e7-9d46-5e66-83de-bffc2bc8057c"));
            }
            other => panic!("expected id predicate, got {other:?}"),
        }
    }
}
