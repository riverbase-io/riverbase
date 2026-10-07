use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::domain_fields_from_payload;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::postgres::entity::helpers::{opt_str, str_field, uuid_field};
use crate::postgres::schema::collection;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable)]
    #[diesel(table_name = collection)]
    pub struct CollectionRow {
        pub collection_key: String,
        pub collection_name: String,
        pub desc: Option<String>,
        pub owner_id: Option<Uuid>,
        pub organization_id: Option<String>,
    }
}

pub fn collection_row_to_json(row: CollectionRow) -> Value {
    json!({
        "id": row._id.to_string(),
        "collection_key": row.collection_key,
        "collection_name": row.collection_name,
        "desc": row.desc,
        "owner_id": row.owner_id.map(|u| u.to_string()),
        "organization_id": row.organization_id,
    })
}

pub async fn collection_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let collection_key = str_field(data, "collection_key", "");
    let collection_name = str_field(data, "collection_name", "");
    let desc = opt_str(data, "desc");
    let owner_id = uuid_field(data, "owner_id");
    let organization_id = opt_str(data, "organization_id");

    let exists: bool = diesel::select(diesel::dsl::exists(
        collection::table.filter(collection::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;

    if exists {
        diesel::update(collection::table.filter(collection::_id.eq(id)))
            .set((
                collection::collection_key.eq(&collection_key),
                collection::collection_name.eq(&collection_name),
                collection::desc.eq(desc.as_deref()),
                collection::owner_id.eq(owner_id),
                collection::organization_id.eq(organization_id.as_deref()),
                collection::_updated.eq(Some(Utc::now())),
            ))
            .execute(conn)
            .await?;
    } else {
        let domain = domain_fields_from_payload(data, id);
        diesel::insert_into(collection::table)
            .values(riverbase_core::domain_insert_values!(
                collection,
                domain,
                collection::collection_key.eq(&collection_key),
                collection::collection_name.eq(&collection_name),
                collection::desc.eq(desc.as_deref()),
                collection::owner_id.eq(owner_id),
                collection::organization_id.eq(organization_id.as_deref()),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    CollectionEntity {
        schema: collection,
        row: CollectionRow,
        resources: ["collection"],
        source: "collection",
        row_to_json: collection_row_to_json,
        upsert_from_json: collection_upsert_from_json,
        order: [
            "_id" => _id,
            "collection_key" => collection_key,
            "collection_name" => collection_name,
        ],
    }
}
