use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::domain_fields_from_payload;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::postgres::entity::helpers::{bool_field, i32_field, json_field, str_field, uuid_field};
use crate::postgres::schema::form_element;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable)]
    #[diesel(table_name = form_element)]
    pub struct FormElementRow {
        pub form_id: Uuid,
        pub elem_reg_id: Uuid,
        pub elem_name: String,
        pub index: i32,
        pub required: bool,
        pub data: Option<Value>,
        pub status: String,
    }
}

pub fn form_element_row_to_json(row: FormElementRow) -> Value {
    json!({
        "id": row._id.to_string(),
        "form_id": row.form_id.to_string(),
        "elem_reg_id": row.elem_reg_id.to_string(),
        "elem_name": row.elem_name,
        "index": row.index,
        "required": row.required,
        "data": row.data,
        "status": row.status,
    })
}

pub async fn form_element_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let form_id = uuid_field(data, "form_id").unwrap_or(Uuid::nil());
    let elem_reg_id = uuid_field(data, "elem_reg_id").unwrap_or(Uuid::nil());
    let elem_name = str_field(data, "elem_name", "");
    let index = i32_field(data, "index", -1);
    let required = bool_field(data, "required", false);
    let elem_data = json_field(data, "data");
    let status = str_field(data, "status", "draft");

    let exists: bool = diesel::select(diesel::dsl::exists(
        form_element::table.filter(form_element::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;

    if exists {
        diesel::update(form_element::table.filter(form_element::_id.eq(id)))
            .set((
                form_element::form_id.eq(form_id),
                form_element::elem_reg_id.eq(elem_reg_id),
                form_element::elem_name.eq(&elem_name),
                form_element::index.eq(index),
                form_element::required.eq(required),
                form_element::data.eq(elem_data.as_ref()),
                form_element::status.eq(&status),
                form_element::_updated.eq(Some(Utc::now())),
            ))
            .execute(conn)
            .await?;
    } else {
        let domain = domain_fields_from_payload(data, id);
        diesel::insert_into(form_element::table)
            .values(riverbase_core::domain_insert_values!(
                form_element,
                domain,
                form_element::form_id.eq(form_id),
                form_element::elem_reg_id.eq(elem_reg_id),
                form_element::elem_name.eq(&elem_name),
                form_element::index.eq(index),
                form_element::required.eq(required),
                form_element::data.eq(elem_data.as_ref()),
                form_element::status.eq(&status),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    FormElementEntity {
        schema: form_element,
        row: FormElementRow,
        resources: ["form_element"],
        source: "form_element",
        row_to_json: form_element_row_to_json,
        upsert_from_json: form_element_upsert_from_json,
        order: [
            "_id" => _id,
            "form_id" => form_id,
            "elem_name" => elem_name,
            "index" => index,
            "status" => status,
        ],
    }
}
