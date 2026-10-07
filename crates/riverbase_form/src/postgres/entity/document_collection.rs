use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::domain_fields_from_payload;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::postgres::entity::helpers::{i32_field, uuid_field};
use crate::postgres::schema::document_collection;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable)]
    #[diesel(table_name = document_collection)]
    pub struct DocumentCollectionRow {
        pub document_id: Uuid,
        pub collection_id: Uuid,
        pub sort_order: i32,
    }
}

pub fn document_collection_row_to_json(row: DocumentCollectionRow) -> Value {
    json!({
        "id": row._id.to_string(),
        "document_id": row.document_id.to_string(),
        "collection_id": row.collection_id.to_string(),
        "sort_order": row.sort_order,
    })
}

pub async fn document_collection_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let document_id = uuid_field(data, "document_id").unwrap_or(Uuid::nil());
    let collection_id = uuid_field(data, "collection_id").unwrap_or(Uuid::nil());
    let sort_order = i32_field(data, "sort_order", 0);

    let exists: bool = diesel::select(diesel::dsl::exists(
        document_collection::table.filter(document_collection::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;

    if exists {
        diesel::update(document_collection::table.filter(document_collection::_id.eq(id)))
            .set((
                document_collection::document_id.eq(document_id),
                document_collection::collection_id.eq(collection_id),
                document_collection::sort_order.eq(sort_order),
                document_collection::_updated.eq(Some(Utc::now())),
            ))
            .execute(conn)
            .await?;
    } else {
        let domain = domain_fields_from_payload(data, id);
        diesel::insert_into(document_collection::table)
            .values(riverbase_core::domain_insert_values!(
                document_collection,
                domain,
                document_collection::document_id.eq(document_id),
                document_collection::collection_id.eq(collection_id),
                document_collection::sort_order.eq(sort_order),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    DocumentCollectionEntity {
        schema: document_collection,
        row: DocumentCollectionRow,
        resources: ["document_collection"],
        source: "document_collection",
        row_to_json: document_collection_row_to_json,
        upsert_from_json: document_collection_upsert_from_json,
        order: [
            "_id" => _id,
            "document_id" => document_id,
            "collection_id" => collection_id,
            "sort_order" => sort_order,
        ],
    }
}
