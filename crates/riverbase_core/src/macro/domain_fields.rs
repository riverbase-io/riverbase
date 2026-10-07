//! Shared Riverbase domain column definitions for Diesel schemas, row structs, and SQL migrations.
//!
//! Keep [`DOMAIN_FIELDS_DDL`] in sync with the columns in [`diesel_table_with_domain_fields`].
//!
//! | Macro | Use |
//! |-------|-----|
//! | [`diesel_table_with_domain_fields!`] | Diesel `table!` columns |
//! | [`domain_row!`] | `Queryable` row struct with standard `_id` / `_created` / … fields |
//! | [`domain_insert_values!`] | `insert_into(...).values((domain + payload, …))` from [`crate::base::DomainFields`] |
//! | [`domain_fields_touch_updated!`] | `update(...).set((..., …))` bump `_updated` |
//! | [`domain_fields_cas_touch!`] | `update(...).set((..., …))` bump `_updated` / `_updater` / `_etag` from JSON |
//! | [`domain_fields_soft_delete!`] | `update(...).set((..., …))` set `_deleted` + `_updated` |
//! | [`pg_domain_entity!`] | `ErasedEntity` impl for domain-field Postgres tables |

/// Canonical domain-column names, in Diesel / DDL order.
pub const DOMAIN_FIELD_COLUMNS: &[&str] = &[
    "_id", "_created", "_updated", "_creator", "_updater", "_tenant", "_etag", "_deleted",
];

/// SQL DDL fragment for `CREATE TABLE` (comma-separated lines). Generated from
/// [`DOMAIN_FIELD_COLUMNS`] — do not edit the column list in only one place.
pub const DOMAIN_FIELDS_DDL: &str = r#"
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ NOT NULL DEFAULT now(),
    _creator UUID,
    _updater UUID,
    _tenant UUID,
    _etag UUID NOT NULL DEFAULT gen_random_uuid(),
    _deleted TIMESTAMPTZ
"#;

#[cfg(test)]
mod domain_ddl_tests {
    use super::{DOMAIN_FIELDS_DDL, DOMAIN_FIELD_COLUMNS};

    #[test]
    fn domain_fields_ddl_lists_every_canonical_column() {
        for column in DOMAIN_FIELD_COLUMNS {
            assert!(
                DOMAIN_FIELDS_DDL.contains(column),
                "DOMAIN_FIELDS_DDL missing {column}"
            );
        }
    }

    #[test]
    fn diesel_table_macro_source_lists_every_canonical_column() {
        let src = include_str!("domain_fields.rs");
        for column in DOMAIN_FIELD_COLUMNS {
            let needle = format!("{column} ->");
            assert!(
                src.contains(&needle),
                "diesel_table_with_domain_fields missing {needle}"
            );
        }
    }

    #[test]
    fn todo_example_migration_lists_every_canonical_column() {
        let sql = include_str!(
            "../../../../examples/todo-app/domain/migrations/20260506100000_todo_items/up.sql"
        );
        for column in DOMAIN_FIELD_COLUMNS {
            assert!(
                sql.contains(column),
                "todo_items up.sql diverged from DOMAIN_FIELD_COLUMNS: missing {column}"
            );
        }
    }
}

/// Declare a Diesel table with shared domain metadata columns plus table-specific columns.
///
/// Use the schema-qualified form (`riverbase_media.entry (_id) { … }`) so Diesel emits
/// `"schema"."table"`; a plain `#[sql_name = "schema.table"]` would be quoted as a single
/// identifier and never resolve. The module name is the `table_name` segment; add
/// `#[sql_name = "…"]` to override the SQL table name independently of the module.
#[macro_export]
macro_rules! diesel_table_with_domain_fields {
    // Schema-qualified table, e.g. `riverbase_media.entry (_id) { … }`.
    (
        $(#[$attr:meta])*
        $schema:ident . $table_name:ident ($pk:ident) {
            $($extra:tt)*
        }
    ) => {
        diesel::table! {
            use diesel::sql_types::*;

            $(#[$attr])*
            $schema.$table_name ($pk) {
                _id -> Uuid,
                _created -> Timestamptz,
                _updated -> Nullable<Timestamptz>,
                _creator -> Nullable<Uuid>,
                _updater -> Nullable<Uuid>,
                _tenant -> Nullable<Uuid>,
                _etag -> Uuid,
                _deleted -> Nullable<Timestamptz>,
                $($extra)*
            }
        }
    };
    // Unqualified table (default/public schema).
    (
        $(#[$attr:meta])*
        $table_name:ident ($pk:ident) {
            $($extra:tt)*
        }
    ) => {
        diesel::table! {
            use diesel::sql_types::*;

            $(#[$attr])*
            $table_name ($pk) {
                _id -> Uuid,
                _created -> Timestamptz,
                _updated -> Nullable<Timestamptz>,
                _creator -> Nullable<Uuid>,
                _updater -> Nullable<Uuid>,
                _tenant -> Nullable<Uuid>,
                _etag -> Uuid,
                _deleted -> Nullable<Timestamptz>,
                $($extra)*
            }
        }
    };
}

/// Diesel row struct with shared domain-metadata columns plus table-specific fields.
#[macro_export]
macro_rules! domain_row {
    (
        $(#[$struct_meta:meta])*
        $vis:vis struct $name:ident {
            $($rest:tt)*
        }
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            pub _id: ::uuid::Uuid,
            pub _created: ::chrono::DateTime<::chrono::Utc>,
            pub _updated: Option<::chrono::DateTime<::chrono::Utc>>,
            pub _creator: Option<::uuid::Uuid>,
            pub _updater: Option<::uuid::Uuid>,
            pub _tenant: Option<::uuid::Uuid>,
            pub _etag: ::uuid::Uuid,
            pub _deleted: Option<::chrono::DateTime<::chrono::Utc>>,
            $($rest)*
        }
    };
}

/// Diesel `insert_into(...).values((domain columns, …))` from [`crate::base::DomainFields`].
#[macro_export]
macro_rules! domain_insert_values {
    (
        $schema:ident,
        $domain:expr
        $(, $extra:expr)*
        $(,)?
    ) => {
        (
            $schema::_id.eq($domain.id),
            $schema::_created.eq($domain.created),
            $schema::_updated.eq($domain.updated.unwrap_or($domain.created)),
            $schema::_creator.eq($domain.creator),
            $schema::_updater.eq($domain.updater),
            $schema::_tenant.eq($domain.tenant),
            $schema::_etag.eq($domain.etag.unwrap_or_else(::uuid::Uuid::new_v4)),
            $schema::_deleted.eq($domain.deleted),
            $($extra),*
        )
    };
}

/// Set `_updated` on `update(...).set((payload, …))`.
#[macro_export]
macro_rules! domain_fields_touch_updated {
    ($schema:ident, $now:expr) => {
        $schema::_updated.eq($now)
    };
}

/// Persist optimistic-concurrency audit fields from a JSON upsert payload.
///
/// Prefer this on update paths so `_etag` / `_updater` / `_updated` from
/// [`crate::command::aggregate::Aggregate::update`] (or upsert audit patches) are written.
#[macro_export]
macro_rules! domain_fields_cas_touch {
    ($schema:ident, $data:expr) => {{
        let __domain = $crate::base::domain_fields_from_payload($data, ::uuid::Uuid::nil());
        (
            $schema::_updated.eq(__domain.updated.unwrap_or_else(::chrono::Utc::now)),
            $schema::_updater.eq(__domain.updater),
            $schema::_etag.eq(__domain.etag.unwrap_or_else(::uuid::Uuid::new_v4)),
        )
    }};
}

/// Soft-delete: set `_deleted` and `_updated`.
#[macro_export]
macro_rules! domain_fields_soft_delete {
    ($schema:ident, $now:expr) => {
        ($schema::_deleted.eq(Some($now)), $schema::_updated.eq($now))
    };
}

/// Generate `row_to_json` / `upsert_from_json` from a [`domain_row!`] struct plus field kinds.
///
/// Domain columns (`id`, `_etag`, `_updated`, …) are handled automatically. Extra payload
/// fields use: `text`, `text_or("default")`, `opt_text`, `f64`, `json_text`.
///
/// ```ignore
/// riverbase_core::domain_json_converters! {
///     WorkerJobRow, worker_jobs {
///         to_json: worker_job_row_to_json,
///         upsert: worker_job_upsert_from_json,
///         op: text,
///         status: text_or("SUBMITTED"),
///         progress: f64,
///         result: json_text,
///         error: opt_text,
///         args: json_text,
///     }
/// }
/// ```
#[macro_export]
macro_rules! domain_json_converters {
    (
        $row:ty, $schema:ident {
            to_json: $to_json:ident,
            upsert: $upsert:ident,
            $($field:ident : $kind:ident $( ( $kind_arg:literal ) )? ),+ $(,)?
        }
    ) => {
        fn $to_json(row: $row) -> ::serde_json::Value {
            ::serde_json::json!({
                "id": row._id.to_string(),
                $(
                    stringify!($field): $crate::domain_json_converters!(@to_json $kind $( ( $kind_arg ) )?, row.$field),
                )+
                "_etag": row._etag.to_string(),
            })
        }

        async fn $upsert(
            conn: &mut diesel_async::AsyncPgConnection,
            id: ::uuid::Uuid,
            data: &::serde_json::Value,
        ) -> $crate::datastore::error::DataResult<()> {
            use diesel::{ExpressionMethods, QueryDsl};

            $(
                let $field = $crate::domain_json_converters!(@from_json $kind $( ( $kind_arg ) )?, data, stringify!($field));
            )+
            let exists: bool = diesel_async::RunQueryDsl::get_result(
                diesel::select(diesel::dsl::exists(
                    $schema::table.filter($schema::_id.eq(id)),
                )),
                conn,
            )
            .await?;
            if exists {
                let domain = $crate::base::domain_fields_from_payload(data, id);
                diesel_async::RunQueryDsl::execute(
                    diesel::update($schema::table.filter($schema::_id.eq(id))).set((
                        $( $schema::$field.eq($field), )+
                        $schema::_updated.eq(domain.updated.or(Some(::chrono::Utc::now()))),
                        $schema::_updater.eq(domain.updater),
                        $schema::_etag.eq(domain.etag.unwrap_or_else(::uuid::Uuid::new_v4)),
                    )),
                    conn,
                )
                .await?;
            } else {
                let domain = $crate::base::domain_fields_from_payload(data, id);
                diesel_async::RunQueryDsl::execute(
                    diesel::insert_into($schema::table).values($crate::domain_insert_values!(
                        $schema,
                        domain,
                        $( $schema::$field.eq($field), )+
                    )),
                    conn,
                )
                .await?;
            }
            Ok(())
        }
    };

    (@to_json text, $value:expr) => { $value };
    (@to_json opt_text, $value:expr) => { $value };
    (@to_json f64, $value:expr) => { $value };
    (@to_json json_text, $value:expr) => {{
        $value
            .as_ref()
            .and_then(|text| ::serde_json::from_str::<::serde_json::Value>(text).ok())
    }};
    (@to_json text_or ($default:literal), $value:expr) => { $value };

    (@from_json text, $data:expr, $key:expr) => {
        $data.get($key).and_then(::serde_json::Value::as_str).unwrap_or_default()
    };
    (@from_json text_or ($default:literal), $data:expr, $key:expr) => {
        $data.get($key).and_then(::serde_json::Value::as_str).unwrap_or($default)
    };
    (@from_json opt_text, $data:expr, $key:expr) => {
        $data.get($key).and_then(::serde_json::Value::as_str).map(str::to_string)
    };
    (@from_json f64, $data:expr, $key:expr) => {
        $data.get($key).and_then(::serde_json::Value::as_f64).unwrap_or(0.0)
    };
    (@from_json json_text, $data:expr, $key:expr) => {{
        $data.get($key).and_then(|value| {
            if value.is_null() {
                None
            } else {
                Some(value.to_string())
            }
        })
    }};
}
