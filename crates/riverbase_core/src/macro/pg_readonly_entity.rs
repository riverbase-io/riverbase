//! `pg_readonly_entity!` — generate read-only / append-only [`ErasedEntity`](crate::datastore::postgres::entity::ErasedEntity).
//!
//! Targets audit/log projections and API-read-only tables without soft-delete domain fields.
//! Writes (`upsert` / `remove` / `invalidate`) always return a
//! [`DataError`](crate::datastore::DataError) constructed with
//! [`DataError::unsupported`](crate::datastore::DataError::unsupported).

/// Declare a read-only Postgres entity (no soft-delete filter; rejects writes).
///
/// Requires a `Row` struct with `Queryable`/`Selectable` and `row_to_json(Row) -> Value` in scope.
///
/// Optional `order:` maps physical order/filter columns. Optional `default_order:` applies when the
/// query carries no order specs. Optional `id_column:` / `id_kind:` select the fetch key
/// (`uuid` default on `_id`, or `text` for string keys).
///
/// `resources:` accepts string literals or [`ResourceKey`](crate::datastore::ResourceKey)
/// types. A key type const-asserts sortable columns against `order:`.
#[macro_export]
macro_rules! pg_readonly_entity {
    (
        $entity:ident {
            schema: $schema:ident,
            row: $row:ty,
            resources: [$($resource:tt),+ $(,)?],
            source: $source:literal,
            row_to_json: $row_to_json:path,
            write_error: $write_error:literal,
            $(id_column: $id_col:ident,)?
            $(id_kind: $id_kind:ident,)?
            $(default_order: $def_col:literal => $def_field:ident $def_dir:ident,)?
            $(order: [$( $order_col:literal => $order_field:ident ),* $(,)?])? $(,)?
        }
    ) => {
        pub struct $entity;

        impl $entity {
            /// Physical columns listed in `order:`.
            pub const ORDERABLE: &'static [&'static str] =
                $crate::pg_domain_entity!(@orderable_cols $( [$( $order_col ),*] )? );
        }

        #[::async_trait::async_trait]
        impl $crate::datastore::postgres::entity::ErasedEntity for $entity {
            fn resources(&self) -> &[&'static str] {
                &[$($crate::pg_domain_entity!(@resource_name $resource)),+]
            }

            fn source(&self) -> &'static str {
                $source
            }

            fn enforces_policy_filter(&self) -> bool {
                true
            }

            fn debug_orderable_columns(&self) -> &'static [&'static str] {
                Self::ORDERABLE
            }

            async fn remove(
                &self,
                _conn: &mut diesel_async::AsyncPgConnection,
                _id: &str,
            ) -> $crate::datastore::error::DataResult<()> {
                Err($crate::errors::DAT_080.with_data($write_error))
            }

            async fn invalidate(
                &self,
                _conn: &mut diesel_async::AsyncPgConnection,
                _id: &str,
            ) -> $crate::datastore::error::DataResult<()> {
                Err($crate::errors::DAT_081.with_data($write_error))
            }

            async fn query_list(
                &self,
                conn: &mut diesel_async::AsyncPgConnection,
                query: &$crate::datastore::dsl::DataQuery,
            ) -> $crate::datastore::error::DataResult<(Vec<serde_json::Value>, i64)> {
                use diesel::ExpressionMethods;
                use diesel::QueryDsl;
                use diesel::SelectableHelper;

                if query.source != $source {
                    return Err($crate::errors::DAT_082.with_data(format!("source {} not supported", query.source)));
                }

                let mut q = $schema::table.into_boxed();
                $crate::pg_domain_entity!(@apply_policy_filter q, query);
                $crate::pg_domain_entity!(@apply_filter q, query $( @order [$( $order_col ),*] )? );

                if query.order.is_empty() {
                    $(
                        q = match stringify!($def_dir) {
                            "asc" | "Asc" => q.order($schema::$def_field.asc()),
                            _ => q.order($schema::$def_field.desc()),
                        };
                    )?
                } else {
                    $(
                        for (idx, spec) in query.order.iter().enumerate() {
                            q = match (idx == 0, spec.field.0.as_str(), spec.direction) {
                                $(
                                    (true, $order_col, $crate::datastore::dsl::OrderDirection::Asc) => {
                                        q.order($schema::$order_field.asc())
                                    }
                                    (true, $order_col, $crate::datastore::dsl::OrderDirection::Desc) => {
                                        q.order($schema::$order_field.desc())
                                    }
                                    (false, $order_col, $crate::datastore::dsl::OrderDirection::Asc) => {
                                        q.then_order_by($schema::$order_field.asc())
                                    }
                                    (false, $order_col, $crate::datastore::dsl::OrderDirection::Desc) => {
                                        q.then_order_by($schema::$order_field.desc())
                                    }
                                )*
                                (_, col, _) => {
                                    return Err($crate::errors::DAT_083.with_data(format!("unsupported order column {col}")));
                                }
                            };
                        }
                    )?
                }

                if let Some(page) = query.page.clone() {
                    q = q.offset(page.offset as i64).limit(page.limit as i64);
                } else {
                    q = q.limit(100);
                }

                if !query.count_total {
                    let rows: Vec<$row> = diesel_async::RunQueryDsl::load(
                        q.select(<$row>::as_select()),
                        conn,
                    )
                    .await?;
                    return Ok((rows.into_iter().map($row_to_json).collect(), -1));
                }

                let mut count_q = $schema::table.into_boxed();
                $crate::pg_domain_entity!(@apply_policy_filter count_q, query);
                $crate::pg_domain_entity!(@apply_filter count_q, query $( @order [$( $order_col ),*] )? );
                let total: i64 =
                    diesel_async::RunQueryDsl::get_result(count_q.count(), conn).await?;
                let rows: Vec<$row> = diesel_async::RunQueryDsl::load(
                    q.select(<$row>::as_select()),
                    conn,
                )
                .await?;
                Ok((rows.into_iter().map($row_to_json).collect(), total))
            }

            async fn fetch(
                &self,
                conn: &mut diesel_async::AsyncPgConnection,
                id: &str,
            ) -> $crate::datastore::error::DataResult<::core::option::Option<serde_json::Value>> {
                use diesel::prelude::*;
                use diesel_async::RunQueryDsl;
                $crate::pg_readonly_entity!(@fetch_by_id
                    $schema, $row, $row_to_json, conn, id,
                    $(id_column: $id_col,)?
                    $(id_kind: $id_kind,)?
                )
            }

            async fn lock_version(
                &self,
                _conn: &mut diesel_async::AsyncPgConnection,
                _id: &str,
            ) -> $crate::datastore::error::DataResult<::core::option::Option<::uuid::Uuid>> {
                $crate::datastore::postgres::entity::lock_version_none()
            }

            async fn upsert(
                &self,
                _conn: &mut diesel_async::AsyncPgConnection,
                _id: &str,
                _data: serde_json::Value,
            ) -> $crate::datastore::error::DataResult<()> {
                Err($crate::errors::DAT_084.with_data($write_error))
            }
        }

        $(
            $crate::pg_domain_entity!(@assert_sort $entity, $resource);
        )+
    };

    // Default: UUID `_id`
    (@fetch_by_id $schema:ident, $row:ty, $row_to_json:path, $conn:ident, $id:ident,) => {{
        use diesel::ExpressionMethods;
        use diesel::OptionalExtension;
        use diesel::QueryDsl;
        use diesel::SelectableHelper;
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        let row: ::core::option::Option<$row> = diesel_async::RunQueryDsl::first(
            $schema::table
                .filter($schema::_id.eq(id))
                .select(<$row>::as_select()),
            $conn,
        )
        .await
        .optional()?;
        Ok(row.map($row_to_json))
    }};

    (@fetch_by_id $schema:ident, $row:ty, $row_to_json:path, $conn:ident, $id:ident, id_column: $id_col:ident,) => {{
        use diesel::ExpressionMethods;
        use diesel::OptionalExtension;
        use diesel::QueryDsl;
        use diesel::SelectableHelper;
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        let row: ::core::option::Option<$row> = diesel_async::RunQueryDsl::first(
            $schema::table
                .filter($schema::$id_col.eq(id))
                .select(<$row>::as_select()),
            $conn,
        )
        .await
        .optional()?;
        Ok(row.map($row_to_json))
    }};

    (@fetch_by_id $schema:ident, $row:ty, $row_to_json:path, $conn:ident, $id:ident, id_column: $id_col:ident, id_kind: uuid,) => {{
        $crate::pg_readonly_entity!(@fetch_by_id $schema, $row, $row_to_json, $conn, $id, id_column: $id_col,)
    }};

    (@fetch_by_id $schema:ident, $row:ty, $row_to_json:path, $conn:ident, $id:ident, id_column: $id_col:ident, id_kind: text,) => {{
        use diesel::ExpressionMethods;
        use diesel::OptionalExtension;
        use diesel::QueryDsl;
        use diesel::SelectableHelper;
        let row: ::core::option::Option<$row> = diesel_async::RunQueryDsl::first(
            $schema::table
                .filter($schema::$id_col.eq($id))
                .select(<$row>::as_select()),
            $conn,
        )
        .await
        .optional()?;
        Ok(row.map($row_to_json))
    }};

    (@fetch_by_id $schema:ident, $row:ty, $row_to_json:path, $conn:ident, $id:ident, id_kind: text,) => {{
        $crate::pg_readonly_entity!(@fetch_by_id $schema, $row, $row_to_json, $conn, $id, id_column: _id, id_kind: text,)
    }};

    (@fetch_by_id $schema:ident, $row:ty, $row_to_json:path, $conn:ident, $id:ident, id_kind: uuid,) => {{
        $crate::pg_readonly_entity!(@fetch_by_id $schema, $row, $row_to_json, $conn, $id,)
    }};
}
