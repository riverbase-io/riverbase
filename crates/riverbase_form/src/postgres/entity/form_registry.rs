use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::domain_fields_from_payload;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::postgres::entity::helpers::{opt_str, str_field};
use crate::postgres::schema::form_registry;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable)]
    #[diesel(table_name = form_registry)]
    pub struct FormRegistryRow {
        #[allow(dead_code)]
        pub serial_no: i64,
        pub form_key: String,
        pub title: String,
        pub desc: Option<String>,
    }
}

pub fn form_registry_row_to_json(row: FormRegistryRow) -> Value {
    json!({
        "id": row._id.to_string(),
        "form_key": row.form_key,
        "title": row.title,
        "desc": row.desc,
    })
}

pub async fn form_registry_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let form_key = str_field(data, "form_key", "");
    let title = str_field(data, "title", "");
    let desc = opt_str(data, "desc");

    let exists: bool = diesel::select(diesel::dsl::exists(
        form_registry::table.filter(form_registry::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;

    if exists {
        diesel::update(form_registry::table.filter(form_registry::_id.eq(id)))
            .set((
                form_registry::form_key.eq(&form_key),
                form_registry::title.eq(&title),
                form_registry::desc.eq(desc.as_deref()),
                form_registry::_updated.eq(Some(Utc::now())),
            ))
            .execute(conn)
            .await?;
    } else {
        let domain = domain_fields_from_payload(data, id);
        diesel::insert_into(form_registry::table)
            .values(riverbase_core::domain_insert_values!(
                form_registry,
                domain,
                form_registry::form_key.eq(&form_key),
                form_registry::title.eq(&title),
                form_registry::desc.eq(desc.as_deref()),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    FormRegistryEntity {
        schema: form_registry,
        row: FormRegistryRow,
        resources: ["form_registry"],
        source: "form_registry",
        row_to_json: form_registry_row_to_json,
        upsert_from_json: form_registry_upsert_from_json,
        order: [
            "_id" => _id,
            "form_key" => form_key,
            "title" => title,
        ],
    }
}
