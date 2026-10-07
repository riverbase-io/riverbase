use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::domain_fields_from_payload;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::postgres::entity::helpers::{i32_field, opt_str, str_field};
use crate::postgres::schema::template_registry;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable)]
    #[diesel(table_name = template_registry)]
    pub struct TemplateRegistryRow {
        #[allow(dead_code)]
        pub serial_no: i64,
        pub template_key: String,
        pub template_name: String,
        pub desc: Option<String>,
        pub version: i32,
        pub types: Option<String>,
    }
}

pub fn template_registry_row_to_json(row: TemplateRegistryRow) -> Value {
    json!({
        "id": row._id.to_string(),
        "template_key": row.template_key,
        "template_name": row.template_name,
        "desc": row.desc,
        "version": row.version,
        "types": row.types,
    })
}

pub async fn template_registry_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let template_key = str_field(data, "template_key", "");
    let template_name = str_field(data, "template_name", "");
    let desc = opt_str(data, "desc");
    let version = i32_field(data, "version", 1);
    let types = opt_str(data, "types");

    let exists: bool = diesel::select(diesel::dsl::exists(
        template_registry::table.filter(template_registry::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;

    if exists {
        diesel::update(template_registry::table.filter(template_registry::_id.eq(id)))
            .set((
                template_registry::template_key.eq(&template_key),
                template_registry::template_name.eq(&template_name),
                template_registry::desc.eq(desc.as_deref()),
                template_registry::version.eq(version),
                template_registry::types.eq(types.as_deref()),
                template_registry::_updated.eq(Some(Utc::now())),
            ))
            .execute(conn)
            .await?;
    } else {
        let domain = domain_fields_from_payload(data, id);
        diesel::insert_into(template_registry::table)
            .values(riverbase_core::domain_insert_values!(
                template_registry,
                domain,
                template_registry::template_key.eq(&template_key),
                template_registry::template_name.eq(&template_name),
                template_registry::desc.eq(desc.as_deref()),
                template_registry::version.eq(version),
                template_registry::types.eq(types.as_deref()),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    TemplateRegistryEntity {
        schema: template_registry,
        row: TemplateRegistryRow,
        resources: ["template_registry"],
        source: "template_registry",
        row_to_json: template_registry_row_to_json,
        upsert_from_json: template_registry_upsert_from_json,
        order: [
            "_id" => _id,
            "template_key" => template_key,
            "template_name" => template_name,
            "version" => version,
        ],
    }
}
