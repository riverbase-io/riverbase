use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::domain_fields_from_payload;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::postgres::entity::helpers::{i32_field, opt_str, str_field, uuid_field};
use crate::postgres::schema::document;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable)]
    #[diesel(table_name = document)]
    pub struct DocumentRow {
        pub template_id: Option<Uuid>,
        pub document_key: String,
        pub document_name: String,
        pub desc: Option<String>,
        pub version: i32,
        pub owner_id: Option<Uuid>,
        pub organization_id: Option<String>,
        pub resource_id: Option<Uuid>,
        pub resource_name: Option<String>,
    }
}

pub fn document_row_to_json(row: DocumentRow) -> Value {
    json!({
        "id": row._id.to_string(),
        "template_id": row.template_id.map(|u| u.to_string()),
        "document_key": row.document_key,
        "document_name": row.document_name,
        "desc": row.desc,
        "version": row.version,
        "owner_id": row.owner_id.map(|u| u.to_string()),
        "organization_id": row.organization_id,
        "resource_id": row.resource_id.map(|u| u.to_string()),
        "resource_name": row.resource_name,
    })
}

pub async fn document_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let template_id = uuid_field(data, "template_id");
    let document_key = str_field(data, "document_key", "");
    let document_name = str_field(data, "document_name", "");
    let desc = opt_str(data, "desc");
    let version = i32_field(data, "version", 1);
    let owner_id = uuid_field(data, "owner_id");
    let organization_id = opt_str(data, "organization_id");
    let resource_id = uuid_field(data, "resource_id");
    let resource_name = opt_str(data, "resource_name");

    let exists: bool = diesel::select(diesel::dsl::exists(
        document::table.filter(document::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;

    if exists {
        diesel::update(document::table.filter(document::_id.eq(id)))
            .set((
                document::template_id.eq(template_id),
                document::document_key.eq(&document_key),
                document::document_name.eq(&document_name),
                document::desc.eq(desc.as_deref()),
                document::version.eq(version),
                document::owner_id.eq(owner_id),
                document::organization_id.eq(organization_id.as_deref()),
                document::resource_id.eq(resource_id),
                document::resource_name.eq(resource_name.as_deref()),
                document::_updated.eq(Some(Utc::now())),
            ))
            .execute(conn)
            .await?;
    } else {
        let domain = domain_fields_from_payload(data, id);
        diesel::insert_into(document::table)
            .values(riverbase_core::domain_insert_values!(
                document,
                domain,
                document::template_id.eq(template_id),
                document::document_key.eq(&document_key),
                document::document_name.eq(&document_name),
                document::desc.eq(desc.as_deref()),
                document::version.eq(version),
                document::owner_id.eq(owner_id),
                document::organization_id.eq(organization_id.as_deref()),
                document::resource_id.eq(resource_id),
                document::resource_name.eq(resource_name.as_deref()),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    DocumentEntity {
        schema: document,
        row: DocumentRow,
        resources: ["document"],
        source: "document",
        row_to_json: document_row_to_json,
        upsert_from_json: document_upsert_from_json,
        order: [
            "_id" => _id,
            "document_key" => document_key,
            "document_name" => document_name,
            "version" => version,
        ],
    }
}
