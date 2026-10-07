//! `pg_domain_entity!` — generate [`ErasedEntity`](crate::datastore::postgres::entity::ErasedEntity) for domain-field tables.

/// Declare a domain-fields Postgres entity with generated [`ErasedEntity`] impl.
///
/// Requires a `Row` struct with `Queryable` and two helper fns in scope:
/// `row_to_json(Row) -> Value` and `upsert_from_json(&mut AsyncPgConnection, Uuid, &Value) -> DataResult<()>`.
///
/// Optional `order:` maps physical [`DataQuery::order`](crate::datastore::dsl::DataQuery::order)
/// field names to Diesel columns and enables list filters on those columns.
///
/// `resources:` accepts string literals or [`ResourceKey`](crate::datastore::ResourceKey)
/// types (`MemberQueryResourceKey`). A key type const-asserts
/// [`ResourceKey::SORT_COLUMNS`](crate::datastore::ResourceKey::SORT_COLUMNS) against `order:`
/// so sortable drift fails `cargo check`.
///
/// Optional enrichment (DX-03 facade absorption):
/// - `enrich_list:` `async fn(&mut AsyncPgConnection, &mut [Value]) -> DataResult<()>`
/// - `enrich_item:` `async fn(&mut AsyncPgConnection, &mut Value) -> DataResult<()>`
///
/// Optional alternate key (DX-03 composite-key absorption):
/// - `id_column:` Diesel column ident (default `_id`)
/// - `id_kind:` `uuid` (default) or `text` — text keys pass `&str` to a
///   `upsert_from_json_str(&mut AsyncPgConnection, &str, &Value)` helper instead of Uuid.
#[macro_export]
macro_rules! pg_domain_entity {
    (
        $entity:ident {
            schema: $schema:ident,
            row: $row:ty,
            resources: [$($resource:tt),+ $(,)?],
            source: $source:expr,
            row_to_json: $row_to_json:path,
            upsert_from_json: $upsert_from_json:path,
            $(id_column: $id_col:ident,)?
            $(id_kind: $id_kind:ident,)?
            $(enrich_list: $enrich_list:path,)?
            $(enrich_item: $enrich_item:path,)?
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

            async fn query_list(
                &self,
                conn: &mut diesel_async::AsyncPgConnection,
                query: &$crate::datastore::dsl::DataQuery,
            ) -> $crate::datastore::error::DataResult<(Vec<serde_json::Value>, i64)> {
                use diesel::prelude::*;
                use diesel_async::RunQueryDsl;

                if query.source != $source {
                    return Err($crate::errors::DAT_075.with_data(format!("source {} not supported", query.source)));
                }

                let mut q = $schema::table
                    .filter($schema::_deleted.is_null())
                    .into_boxed();

                $crate::pg_domain_entity!(@apply_policy_filter q, query);
                $crate::pg_domain_entity!(@apply_filter q, query $( @order [$( $order_col ),*] )? );
                $crate::pg_domain_entity!(@apply_text q, query);

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
                                return Err($crate::errors::DAT_076.with_data(format!("unsupported order column {col}")));
                            }
                        };
                    }
                )?

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
                    let mut values: Vec<serde_json::Value> = rows
                        .into_iter()
                        .map(|row| {
                            let etag = row._etag;
                            let tenant = row._tenant;
                            $crate::base::with_json_domain_meta($row_to_json(row), etag, tenant)
                        })
                        .collect();
                    $( $enrich_list(conn, &mut values).await?; )?
                    return Ok((values, -1));
                }

                let mut count_q = $schema::table
                    .filter($schema::_deleted.is_null())
                    .into_boxed();
                $crate::pg_domain_entity!(@apply_policy_filter count_q, query);
                $crate::pg_domain_entity!(@apply_filter count_q, query $( @order [$( $order_col ),*] )? );
                $crate::pg_domain_entity!(@apply_text count_q, query);
                let total: i64 =
                    diesel_async::RunQueryDsl::get_result(count_q.count(), conn).await?;

                let rows: Vec<$row> = diesel_async::RunQueryDsl::load(
                    q.select(<$row>::as_select()),
                    conn,
                )
                .await?;
                let mut values: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|row| {
                        let etag = row._etag;
                        let tenant = row._tenant;
                        $crate::base::with_json_domain_meta($row_to_json(row), etag, tenant)
                    })
                    .collect();
                $( $enrich_list(conn, &mut values).await?; )?
                Ok((values, total))
            }

            async fn fetch(
                &self,
                conn: &mut diesel_async::AsyncPgConnection,
                id: &str,
            ) -> $crate::datastore::error::DataResult<Option<serde_json::Value>> {
                use diesel::prelude::*;
                use diesel_async::RunQueryDsl;
                let row: Option<$row> = $crate::pg_domain_entity!(@fetch_row
                    $schema, $row, conn, id, $(id_column: $id_col,)? $(id_kind: $id_kind,)?
                )?;
                match row {
                    None => Ok(None),
                    Some(row) => {
                        let etag = row._etag;
                        let tenant = row._tenant;
                        let mut value =
                            $crate::base::with_json_domain_meta($row_to_json(row), etag, tenant);
                        $( $enrich_item(conn, &mut value).await?; )?
                        Ok(Some(value))
                    }
                }
            }

            async fn lock_version(
                &self,
                conn: &mut diesel_async::AsyncPgConnection,
                id: &str,
            ) -> $crate::datastore::error::DataResult<::core::option::Option<::uuid::Uuid>> {
                use diesel::prelude::*;
                use diesel_async::RunQueryDsl;
                $crate::pg_domain_entity!(@lock_version
                    $schema, conn, id, $(id_column: $id_col,)? $(id_kind: $id_kind,)?
                )
            }

            async fn upsert(
                &self,
                conn: &mut diesel_async::AsyncPgConnection,
                id: &str,
                data: serde_json::Value,
            ) -> $crate::datastore::error::DataResult<()> {
                $crate::pg_domain_entity!(@upsert
                    $upsert_from_json, conn, id, data, $(id_kind: $id_kind,)?
                )
            }

            async fn remove(
                &self,
                conn: &mut diesel_async::AsyncPgConnection,
                id: &str,
            ) -> $crate::datastore::error::DataResult<()> {
                use diesel::prelude::*;
                use diesel_async::RunQueryDsl;
                $crate::pg_domain_entity!(@remove
                    $schema, conn, id, $(id_column: $id_col,)? $(id_kind: $id_kind,)?
                )
            }

            async fn invalidate(
                &self,
                conn: &mut diesel_async::AsyncPgConnection,
                id: &str,
            ) -> $crate::datastore::error::DataResult<()> {
                use diesel::prelude::*;
                use diesel_async::RunQueryDsl;
                $crate::pg_domain_entity!(@invalidate
                    $schema, conn, id, $(id_column: $id_col,)? $(id_kind: $id_kind,)?
                )
            }
        }

        $(
            $crate::pg_domain_entity!(@assert_sort $entity, $resource);
        )+
    };

    // --- id helpers (uuid `_id` default) --------------------------------------------------------

    (@fetch_row $schema:ident, $row:ty, $conn:ident, $id:ident,) => {{
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        let row: ::core::option::Option<$row> = diesel_async::RunQueryDsl::first(
            $schema::table
                .filter($schema::_id.eq(id))
                .filter($schema::_deleted.is_null())
                .select(<$row>::as_select()),
            $conn,
        )
        .await
        .optional()?;
        Ok::<_, $crate::base::RiverbaseError>(row)
    }};

    (@fetch_row $schema:ident, $row:ty, $conn:ident, $id:ident, id_column: $id_col:ident,) => {{
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        let row: ::core::option::Option<$row> = diesel_async::RunQueryDsl::first(
            $schema::table
                .filter($schema::$id_col.eq(id))
                .filter($schema::_deleted.is_null())
                .select(<$row>::as_select()),
            $conn,
        )
        .await
        .optional()?;
        Ok::<_, $crate::base::RiverbaseError>(row)
    }};

    (@fetch_row $schema:ident, $row:ty, $conn:ident, $id:ident, id_column: $id_col:ident, id_kind: uuid,) => {{
        $crate::pg_domain_entity!(@fetch_row $schema, $row, $conn, $id, id_column: $id_col,)
    }};

    (@fetch_row $schema:ident, $row:ty, $conn:ident, $id:ident, id_column: $id_col:ident, id_kind: text,) => {{
        let row: ::core::option::Option<$row> = diesel_async::RunQueryDsl::first(
            $schema::table
                .filter($schema::$id_col.eq($id))
                .filter($schema::_deleted.is_null())
                .select(<$row>::as_select()),
            $conn,
        )
        .await
        .optional()?;
        Ok::<_, $crate::base::RiverbaseError>(row)
    }};

    (@fetch_row $schema:ident, $row:ty, $conn:ident, $id:ident, id_kind: text,) => {{
        $crate::pg_domain_entity!(@fetch_row $schema, $row, $conn, $id, id_column: _id, id_kind: text,)
    }};

    (@fetch_row $schema:ident, $row:ty, $conn:ident, $id:ident, id_kind: uuid,) => {{
        $crate::pg_domain_entity!(@fetch_row $schema, $row, $conn, $id,)
    }};

    (@lock_version $schema:ident, $conn:ident, $id:ident,) => {{
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        let etag: ::core::option::Option<::uuid::Uuid> = diesel_async::RunQueryDsl::first(
            $schema::table
                .filter($schema::_id.eq(id))
                .filter($schema::_deleted.is_null())
                .for_update()
                .select($schema::_etag),
            $conn,
        )
        .await
        .optional()?;
        Ok::<_, $crate::base::RiverbaseError>(etag)
    }};

    (@lock_version $schema:ident, $conn:ident, $id:ident, id_column: $id_col:ident,) => {{
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        let etag: ::core::option::Option<::uuid::Uuid> = diesel_async::RunQueryDsl::first(
            $schema::table
                .filter($schema::$id_col.eq(id))
                .filter($schema::_deleted.is_null())
                .for_update()
                .select($schema::_etag),
            $conn,
        )
        .await
        .optional()?;
        Ok::<_, $crate::base::RiverbaseError>(etag)
    }};

    (@lock_version $schema:ident, $conn:ident, $id:ident, id_column: $id_col:ident, id_kind: uuid,) => {{
        $crate::pg_domain_entity!(@lock_version $schema, $conn, $id, id_column: $id_col,)
    }};

    (@lock_version $schema:ident, $conn:ident, $id:ident, id_column: $id_col:ident, id_kind: text,) => {{
        let etag: ::core::option::Option<::uuid::Uuid> = diesel_async::RunQueryDsl::first(
            $schema::table
                .filter($schema::$id_col.eq($id))
                .filter($schema::_deleted.is_null())
                .for_update()
                .select($schema::_etag),
            $conn,
        )
        .await
        .optional()?;
        Ok::<_, $crate::base::RiverbaseError>(etag)
    }};

    (@lock_version $schema:ident, $conn:ident, $id:ident, id_kind: text,) => {{
        $crate::pg_domain_entity!(@lock_version $schema, $conn, $id, id_column: _id, id_kind: text,)
    }};

    (@lock_version $schema:ident, $conn:ident, $id:ident, id_kind: uuid,) => {{
        $crate::pg_domain_entity!(@lock_version $schema, $conn, $id,)
    }};

    (@upsert $upsert_from_json:path, $conn:ident, $id:ident, $data:ident,) => {{
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        $upsert_from_json($conn, id, &$data).await
    }};

    (@upsert $upsert_from_json:path, $conn:ident, $id:ident, $data:ident, id_kind: uuid,) => {{
        $crate::pg_domain_entity!(@upsert $upsert_from_json, $conn, $id, $data,)
    }};

    (@upsert $upsert_from_json:path, $conn:ident, $id:ident, $data:ident, id_kind: text,) => {{
        $upsert_from_json($conn, $id, &$data).await
    }};

    (@remove $schema:ident, $conn:ident, $id:ident,) => {{
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        let count = diesel_async::RunQueryDsl::execute(
            diesel::delete(
                $schema::table
                    .filter($schema::_id.eq(id))
                    .filter($schema::_deleted.is_null()),
            ),
            $conn,
        )
        .await?;
        if count == 0 {
            return Err($crate::errors::DAT_077.with_data(format!("resource id {}", $id)));
        }
        Ok(())
    }};

    (@remove $schema:ident, $conn:ident, $id:ident, id_column: $id_col:ident,) => {{
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        let count = diesel_async::RunQueryDsl::execute(
            diesel::delete(
                $schema::table
                    .filter($schema::$id_col.eq(id))
                    .filter($schema::_deleted.is_null()),
            ),
            $conn,
        )
        .await?;
        if count == 0 {
            return Err($crate::errors::DAT_077.with_data(format!("resource id {}", $id)));
        }
        Ok(())
    }};

    (@remove $schema:ident, $conn:ident, $id:ident, id_column: $id_col:ident, id_kind: uuid,) => {{
        $crate::pg_domain_entity!(@remove $schema, $conn, $id, id_column: $id_col,)
    }};

    (@remove $schema:ident, $conn:ident, $id:ident, id_column: $id_col:ident, id_kind: text,) => {{
        let count = diesel_async::RunQueryDsl::execute(
            diesel::delete(
                $schema::table
                    .filter($schema::$id_col.eq($id))
                    .filter($schema::_deleted.is_null()),
            ),
            $conn,
        )
        .await?;
        if count == 0 {
            return Err($crate::errors::DAT_077.with_data(format!("resource id {}", $id)));
        }
        Ok(())
    }};

    (@remove $schema:ident, $conn:ident, $id:ident, id_kind: text,) => {{
        $crate::pg_domain_entity!(@remove $schema, $conn, $id, id_column: _id, id_kind: text,)
    }};

    (@remove $schema:ident, $conn:ident, $id:ident, id_kind: uuid,) => {{
        $crate::pg_domain_entity!(@remove $schema, $conn, $id,)
    }};

    (@invalidate $schema:ident, $conn:ident, $id:ident,) => {{
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        let now = chrono::Utc::now();
        let count = diesel_async::RunQueryDsl::execute(
            diesel::update($schema::table.filter($schema::_id.eq(id)))
                .set($crate::domain_fields_soft_delete!($schema, now)),
            $conn,
        )
        .await?;
        if count == 0 {
            return Err($crate::errors::DAT_078.with_data(format!("resource id {}", $id)));
        }
        Ok(())
    }};

    (@invalidate $schema:ident, $conn:ident, $id:ident, id_column: $id_col:ident,) => {{
        let id = $crate::datastore::postgres::entity::parse_uuid_id($id)?;
        let now = chrono::Utc::now();
        let count = diesel_async::RunQueryDsl::execute(
            diesel::update(
                $schema::table
                    .filter($schema::$id_col.eq(id))
                    .filter($schema::_deleted.is_null()),
            )
            .set($crate::domain_fields_soft_delete!($schema, now)),
            $conn,
        )
        .await?;
        if count == 0 {
            return Err($crate::errors::DAT_078.with_data(format!("resource id {}", $id)));
        }
        Ok(())
    }};

    (@invalidate $schema:ident, $conn:ident, $id:ident, id_column: $id_col:ident, id_kind: uuid,) => {{
        $crate::pg_domain_entity!(@invalidate $schema, $conn, $id, id_column: $id_col,)
    }};

    (@invalidate $schema:ident, $conn:ident, $id:ident, id_column: $id_col:ident, id_kind: text,) => {{
        let now = chrono::Utc::now();
        let count = diesel_async::RunQueryDsl::execute(
            diesel::update(
                $schema::table
                    .filter($schema::$id_col.eq($id))
                    .filter($schema::_deleted.is_null()),
            )
            .set($crate::domain_fields_soft_delete!($schema, now)),
            $conn,
        )
        .await?;
        if count == 0 {
            return Err($crate::errors::DAT_078.with_data(format!("resource id {}", $id)));
        }
        Ok(())
    }};

    (@invalidate $schema:ident, $conn:ident, $id:ident, id_kind: text,) => {{
        $crate::pg_domain_entity!(@invalidate $schema, $conn, $id, id_column: _id, id_kind: text,)
    }};

    (@invalidate $schema:ident, $conn:ident, $id:ident, id_kind: uuid,) => {{
        $crate::pg_domain_entity!(@invalidate $schema, $conn, $id,)
    }};

    (@apply_filter $q:ident, $query:ident @order [$( $order_col:literal ),* $(,)?]) => {
        if let Some(filter) = $query.filter.as_ref() {
            $crate::pg_domain_entity!(@validate_filter filter, [$( $order_col ),*])?;
            let predicate = $crate::datastore::postgres::filter::expr_to_predicate(filter)?;
            $q = $q.filter(predicate);
        }
    };

    (@apply_filter $q:ident, $query:ident) => {};

    // Server-side policy restriction (physical columns, no interface validation). Applied to every
    // entity regardless of whether list filters (`order:`) are declared.
    (@apply_policy_filter $q:ident, $query:ident) => {
        if let Some(policy) = $query.policy_filter.as_ref() {
            let predicate = $crate::datastore::postgres::filter::expr_to_predicate(policy)?;
            $q = $q.filter(predicate);
        }
    };

    (@apply_text $q:ident, $query:ident) => {
        if let Some(term) = $query.text.as_deref() {
            let predicate = $crate::datastore::postgres::filter::text_search_predicate(term)?;
            $q = $q.filter(predicate);
        }
    };

    (@validate_filter $expr:ident, [$( $col:literal ),* $(,)?]) => {{
        use $crate::datastore::dsl::Expr;
        fn validate(expr: &Expr, allowed: &[&str]) -> $crate::datastore::error::DataResult<()> {
            match expr {
                Expr::Field { path, .. } => {
                    if allowed.contains(&path.0.as_str()) {
                        Ok(())
                    } else {
                        Err($crate::errors::DAT_079.with_data(format!("unsupported filter column {}", path.0)))
                    }
                }
                Expr::And(items) | Expr::Or(items) => {
                    for item in items {
                        validate(item, allowed)?;
                    }
                    Ok(())
                }
                Expr::Not(inner) => validate(inner, allowed),
            }
        }
        validate($expr, &[$( $col ),*])
    }};

    (@orderable_cols [$($col:literal),* $(,)?]) => {
        &[ $($col),* ]
    };
    (@orderable_cols) => {
        &[]
    };

    (@resource_name $name:literal) => { $name };
    (@resource_name $key:ident) => {
        <$key as $crate::datastore::ResourceKey>::NAME
    };

    (@assert_sort $entity:ident, $name:literal) => {};
    (@assert_sort $entity:ident, $key:ident) => {
        const _: () = $crate::query::assert_sort_columns_covered(
            stringify!($entity),
            stringify!($key),
            <$key as $crate::datastore::ResourceKey>::SORT_COLUMNS,
            $entity::ORDERABLE,
        );
    };
}
