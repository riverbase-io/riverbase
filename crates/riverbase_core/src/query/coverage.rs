//! Spawn-time validation: sortable / default_order fields must be backed by entity ORDER BY.

use std::collections::BTreeSet;

use super::binding::{FieldSource, QueryBinding};
use super::interface::QueryInterface;
use crate::base::RiverbaseResult;
use crate::datastore::DataStore;

/// Const-evaluable string equality.
pub const fn const_str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut i = 0;
    while i < left.len() {
        if left[i] != right[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Whether `needle` is present in `haystack` (const-evaluable).
pub const fn const_slice_contains(haystack: &[&str], needle: &str) -> bool {
    let mut i = 0;
    while i < haystack.len() {
        if const_str_eq(haystack[i], needle) {
            return true;
        }
        i += 1;
    }
    false
}

/// First required column that is missing from `orderable`, if any.
pub const fn first_missing_column<'a>(
    required: &'a [&'static str],
    orderable: &[&str],
) -> Option<&'a str> {
    let mut i = 0;
    while i < required.len() {
        if required[i].is_empty() {
            i += 1;
            continue;
        }
        if !const_slice_contains(orderable, required[i]) {
            return Some(required[i]);
        }
        i += 1;
    }
    None
}

/// Const-panic when a sortable / default-order column is absent from `order:`.
///
/// Called from `pg_domain_entity!` / `pg_readonly_entity!` when `resources:`
/// names a [`crate::datastore::ResourceKey`]. Spawn-time [`QRY-126`](crate::errors::QRY_126)
/// remains the only registered raise site.
pub const fn assert_sort_columns_covered(
    _entity: &'static str,
    _key: &'static str,
    required: &'static [&'static str],
    orderable: &'static [&'static str],
) {
    if first_missing_column(required, orderable).is_some() {
        panic!(
            "QRY-126: sortable field is not orderable on the storage entity. Add the physical column to order: or mark the field no_sort."
        );
    }
}

/// Ensure every sortable or default-order field maps to an orderable local column on the entity
/// for `binding.source()`. Mismatches fail spawn with `QRY-126`.
pub fn debug_validate_sortable_order_coverage<M: DataStore>(
    resource_name: &str,
    iface: &dyn QueryInterface,
    binding: &dyn QueryBinding,
    store: &M,
) -> RiverbaseResult<()> {
    validate_sortable_order_coverage_for_columns(
        resource_name,
        iface,
        binding,
        store.debug_orderable_columns(binding.source()),
    )
}

pub(crate) fn validate_sortable_order_coverage_for_columns(
    resource_name: &str,
    iface: &dyn QueryInterface,
    binding: &dyn QueryBinding,
    cols: Option<&'static [&'static str]>,
) -> RiverbaseResult<()> {
    let Some(cols) = cols else {
        return Ok(());
    };

    let mut to_check = BTreeSet::new();
    for field in iface.fields() {
        if super::interface::field_sortable(iface, field.name) {
            to_check.insert(field.name);
        }
    }
    for (name, _) in iface.default_order() {
        to_check.insert(name);
    }

    for field_name in to_check {
        match binding.resolve(field_name) {
            FieldSource::Local(col) => {
                if !cols.contains(&col.as_str()) {
                    return Err(crate::errors::QRY_126.with_data(serde_json::json!({
                        "resource": resource_name,
                        "field": field_name,
                        "column": col.to_string(),
                        "source": binding.source(),
                    })));
                }
            }
            FieldSource::Joined { join, column } => {
                return Err(crate::errors::QRY_125.with_data(serde_json::json!({
                    "resource": resource_name,
                    "field": field_name,
                    "join": join,
                    "column": column,
                    "source": binding.source(),
                })));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::binding::QueryBinding;
    use crate::query::interface::{FieldDef, QueryInterface, PRESET_STRING};
    use crate::query::SortDirection;

    struct TestIface {
        fields: &'static [FieldDef],
        default_order: &'static [(&'static str, SortDirection)],
    }

    impl QueryInterface for TestIface {
        fn fields(&self) -> &'static [FieldDef] {
            self.fields
        }

        fn default_order(&self) -> &'static [(&'static str, SortDirection)] {
            self.default_order
        }
    }

    struct TestBinding {
        source: &'static str,
    }

    impl QueryBinding for TestBinding {
        fn source(&self) -> &'static str {
            self.source
        }
    }

    static FIELDS: [FieldDef; 2] = [
        FieldDef::new("code", "Code", &PRESET_STRING),
        FieldDef::new("notes", "Notes", &PRESET_STRING),
    ];

    #[test]
    fn local_sortable_column_in_entity_passes() {
        let iface = TestIface {
            fields: &FIELDS,
            default_order: &[],
        };
        let binding = TestBinding { source: "item" };
        validate_sortable_order_coverage_for_columns(
            "item",
            &iface,
            &binding,
            Some(&["code", "notes"]),
        )
        .unwrap();
    }

    #[test]
    fn local_sortable_column_missing_from_entity_fails() {
        let iface = TestIface {
            fields: &FIELDS,
            default_order: &[],
        };
        let binding = TestBinding { source: "item" };
        let err =
            validate_sortable_order_coverage_for_columns("item", &iface, &binding, Some(&["code"]))
                .unwrap_err();
        assert_eq!(err.errcode.as_str(), "QRY-126");
    }

    #[test]
    fn default_order_field_is_checked() {
        static FIELDS_ONE: [FieldDef; 1] =
            [FieldDef::new("status", "Status", &PRESET_STRING).no_sort()];
        let iface = TestIface {
            fields: &FIELDS_ONE,
            default_order: &[("status", SortDirection::Asc)],
        };
        let binding = TestBinding { source: "item" };
        let err =
            validate_sortable_order_coverage_for_columns("item", &iface, &binding, Some(&["code"]))
                .unwrap_err();
        assert_eq!(err.errcode.as_str(), "QRY-126");
    }

    #[test]
    fn missing_orderable_columns_skips_check() {
        let iface = TestIface {
            fields: &FIELDS,
            default_order: &[],
        };
        let binding = TestBinding { source: "item" };
        validate_sortable_order_coverage_for_columns("item", &iface, &binding, None).unwrap();
    }

    #[test]
    fn order_fields_supersedes_field_coverage() {
        struct OrderIface;
        impl QueryInterface for OrderIface {
            fn fields(&self) -> &'static [FieldDef] {
                &FIELDS
            }
            fn default_order(&self) -> &'static [(&'static str, SortDirection)] {
                &[]
            }
            fn order_fields(&self) -> &'static [&'static str] {
                &["code"]
            }
        }
        let binding = TestBinding { source: "item" };
        validate_sortable_order_coverage_for_columns(
            "item",
            &OrderIface,
            &binding,
            Some(&["code"]),
        )
        .unwrap();
    }

    #[test]
    fn joined_sortable_field_fails() {
        struct JoinBinding;

        impl QueryBinding for JoinBinding {
            fn source(&self) -> &'static str {
                "item"
            }

            fn resolve(&self, field: &str) -> FieldSource {
                FieldSource::joined("profile", field)
            }
        }

        static FIELDS_JOIN: [FieldDef; 1] = [FieldDef::new("display_name", "Name", &PRESET_STRING)];
        let iface = TestIface {
            fields: &FIELDS_JOIN,
            default_order: &[],
        };
        let err = validate_sortable_order_coverage_for_columns(
            "item",
            &iface,
            &JoinBinding,
            Some(&["display_name"]),
        )
        .unwrap_err();
        assert_eq!(err.errcode.as_str(), "QRY-125");
    }

    #[test]
    fn const_slice_contains_and_first_missing() {
        assert!(const_slice_contains(&["id", "title"], "title"));
        assert!(!const_slice_contains(&["id", "title"], "notes"));
        assert_eq!(
            first_missing_column(&["id", "notes"], &["id", "title"]),
            Some("notes")
        );
        assert_eq!(first_missing_column(&["id"], &["id", "title"]), None);
    }

    #[test]
    fn sort_columns_binding_flags_and_order_fields() {
        #![allow(dead_code)]
        use crate::datastore::ResourceKey;

        crate::query_resource! {
            SortProbeQueryResource name "sort_probe" {
                meta {
                    title: "Probe",
                    default_order: [created.desc],
                    scope: none,
                }
                fields {
                    field id      { preset: Uuid,     label: "ID", identifier }
                    field title   { preset: String,   label: "Title" }
                    field notes   { preset: String,   label: "Notes", no_sort }
                    field payload { preset: Json,     label: "Payload" }
                    field created { preset: Datetime, label: "Created" }
                }
                binding source "sort_probe" {
                    id      => "_id",
                    created => "_created",
                }
            }
        }

        assert!(SortProbeQueryResourceKey::SORT_COLUMNS.contains(&"_id"));
        assert!(SortProbeQueryResourceKey::SORT_COLUMNS.contains(&"title"));
        assert!(SortProbeQueryResourceKey::SORT_COLUMNS.contains(&"_created"));
        assert!(!SortProbeQueryResourceKey::SORT_COLUMNS.contains(&"notes"));
        assert!(!SortProbeQueryResourceKey::SORT_COLUMNS.contains(&"payload"));
        assert!(!SortProbeQueryResourceKey::SORT_COLUMNS.contains(&"id"));

        crate::query_resource! {
            SortAllowQueryResource name "sort_allow" {
                meta {
                    title: "Allow",
                    default_order: [created.desc],
                    order_fields: [title, payload],
                    scope: none,
                }
                fields {
                    field id      { preset: Uuid,     label: "ID", identifier }
                    field title   { preset: String,   label: "Title" }
                    field notes   { preset: String,   label: "Notes" }
                    field payload { preset: Json,     label: "Payload" }
                    field created { preset: Datetime, label: "Created" }
                }
                binding source "sort_allow" {
                    id      => "_id",
                    created => "_created",
                }
            }
        }

        assert_eq!(
            SortAllowQueryResourceKey::SORT_COLUMNS,
            &["title", "payload", "_created"][..]
        );
    }
}
