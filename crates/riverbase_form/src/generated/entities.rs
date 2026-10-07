//! Generated entity wiring for element data tables.

use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::domain_fields_from_payload;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::generated::schema::*;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable)]
    #[diesel(table_name = text_input_data)]
    pub struct TextInputDataRow {
        pub value: String,
    }
}

pub fn text_input_data_row_to_json(row: TextInputDataRow) -> Value {
    json!({
        "id": row._id.to_string(),
        "value": row.value,
    })
}

pub async fn text_input_data_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let value = data
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let exists: bool = diesel::select(diesel::dsl::exists(
        text_input_data::table.filter(text_input_data::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;
    if exists {
        diesel::update(text_input_data::table.filter(text_input_data::_id.eq(id)))
            .set((text_input_data::value.eq(value),))
            .execute(conn)
            .await?;
    } else {
        let domain = domain_fields_from_payload(data, id);
        diesel::insert_into(text_input_data::table)
            .values(riverbase_core::domain_insert_values!(
                text_input_data,
                domain,
                text_input_data::value.eq(value),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    TextInputDataEntity {
        schema: text_input_data,
        row: TextInputDataRow,
        resources: ["TXT-0001", "text_input_data"],
        source: "text_input_data",
        row_to_json: text_input_data_row_to_json,
        upsert_from_json: text_input_data_upsert_from_json,
        order: [
            "_id" => _id,
            "value" => value,
        ],
    }
}

pub fn register_element_entities() -> Vec<std::sync::Arc<dyn riverbase_core::datastore::ErasedEntity>>
{
    vec![std::sync::Arc::new(TextInputDataEntity)
        as std::sync::Arc<dyn riverbase_core::datastore::ErasedEntity>]
}
