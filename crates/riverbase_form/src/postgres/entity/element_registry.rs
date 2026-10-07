use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::domain_fields_from_payload;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::postgres::entity::helpers::{json_field, opt_str, str_field};
use crate::postgres::schema::element_registry;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable)]
    #[diesel(table_name = element_registry)]
    pub struct ElementRegistryRow {
        #[allow(dead_code)]
        pub serial_no: i64,
        pub element_key: String,
        pub element_label: Option<String>,
        pub element_schema: Value,
    }
}

pub fn element_registry_row_to_json(row: ElementRegistryRow) -> Value {
    json!({
        "id": row._id.to_string(),
        "element_key": row.element_key,
        "element_label": row.element_label,
        "element_schema": row.element_schema,
    })
}

pub async fn element_registry_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let element_key = str_field(data, "element_key", "");
    let element_label = opt_str(data, "element_label");
    let element_schema = json_field(data, "element_schema").unwrap_or_else(|| json!({}));

    let exists: bool = diesel::select(diesel::dsl::exists(
        element_registry::table.filter(element_registry::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;

    if exists {
        diesel::update(element_registry::table.filter(element_registry::_id.eq(id)))
            .set((
                element_registry::element_key.eq(&element_key),
                element_registry::element_label.eq(element_label.as_deref()),
                element_registry::element_schema.eq(&element_schema),
                element_registry::_updated.eq(Some(Utc::now())),
            ))
            .execute(conn)
            .await?;
    } else {
        let domain = domain_fields_from_payload(data, id);
        diesel::insert_into(element_registry::table)
            .values(riverbase_core::domain_insert_values!(
                element_registry,
                domain,
                element_registry::element_key.eq(&element_key),
                element_registry::element_label.eq(element_label.as_deref()),
                element_registry::element_schema.eq(&element_schema),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    ElementRegistryEntity {
        schema: element_registry,
        row: ElementRegistryRow,
        resources: ["element_registry"],
        source: "element_registry",
        row_to_json: element_registry_row_to_json,
        upsert_from_json: element_registry_upsert_from_json,
        order: [
            "_id" => _id,
            "element_key" => element_key,
            "element_label" => element_label,
        ],
    }
}
