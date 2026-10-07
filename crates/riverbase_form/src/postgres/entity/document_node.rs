use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::domain_fields_from_payload;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::postgres::entity::helpers::{i32_field, json_field, opt_str, str_field, uuid_field};
use crate::postgres::schema::document_node;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable)]
    #[diesel(table_name = document_node)]
    pub struct DocumentNodeRow {
        pub document_id: Uuid,
        pub parent_node: Option<Uuid>,
        pub node_key: String,
        pub form_key: Option<String>,
        pub node_type: String,
        pub sort_order: i32,
        pub title: Option<String>,
        pub desc: Option<String>,
        pub content: Option<String>,
        pub content_type: Option<String>,
        pub attrs: Option<Value>,
    }
}

pub fn document_node_row_to_json(row: DocumentNodeRow) -> Value {
    json!({
        "id": row._id.to_string(),
        "document_id": row.document_id.to_string(),
        "parent_node": row.parent_node.map(|u| u.to_string()),
        "node_key": row.node_key,
        "form_key": row.form_key,
        "node_type": row.node_type,
        "sort_order": row.sort_order,
        "title": row.title,
        "desc": row.desc,
        "content": row.content,
        "content_type": row.content_type,
        "attrs": row.attrs,
    })
}

pub async fn document_node_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let document_id = uuid_field(data, "document_id").unwrap_or(Uuid::nil());
    let parent_node = uuid_field(data, "parent_node");
    let node_key = str_field(data, "node_key", "");
    let form_key = opt_str(data, "form_key");
    let node_type = str_field(data, "node_type", "section");
    let sort_order = i32_field(data, "sort_order", 0);
    let title = opt_str(data, "title");
    let desc = opt_str(data, "desc");
    let content = opt_str(data, "content");
    let content_type = opt_str(data, "content_type");
    let attrs = json_field(data, "attrs");

    let exists: bool = diesel::select(diesel::dsl::exists(
        document_node::table.filter(document_node::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;

    if exists {
        diesel::update(document_node::table.filter(document_node::_id.eq(id)))
            .set((
                document_node::document_id.eq(document_id),
                document_node::parent_node.eq(parent_node),
                document_node::node_key.eq(&node_key),
                document_node::form_key.eq(form_key.as_deref()),
                document_node::node_type.eq(&node_type),
                document_node::sort_order.eq(sort_order),
                document_node::title.eq(title.as_deref()),
                document_node::desc.eq(desc.as_deref()),
                document_node::content.eq(content.as_deref()),
                document_node::content_type.eq(content_type.as_deref()),
                document_node::attrs.eq(attrs.as_ref()),
                document_node::_updated.eq(Some(Utc::now())),
            ))
            .execute(conn)
            .await?;
    } else {
        let domain = domain_fields_from_payload(data, id);
        diesel::insert_into(document_node::table)
            .values(riverbase_core::domain_insert_values!(
                document_node,
                domain,
                document_node::document_id.eq(document_id),
                document_node::parent_node.eq(parent_node),
                document_node::node_key.eq(&node_key),
                document_node::form_key.eq(form_key.as_deref()),
                document_node::node_type.eq(&node_type),
                document_node::sort_order.eq(sort_order),
                document_node::title.eq(title.as_deref()),
                document_node::desc.eq(desc.as_deref()),
                document_node::content.eq(content.as_deref()),
                document_node::content_type.eq(content_type.as_deref()),
                document_node::attrs.eq(attrs.as_ref()),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    DocumentNodeEntity {
        schema: document_node,
        row: DocumentNodeRow,
        resources: ["document_node"],
        source: "document_node",
        row_to_json: document_node_row_to_json,
        upsert_from_json: document_node_upsert_from_json,
        order: [
            "_id" => _id,
            "document_id" => document_id,
            "node_key" => node_key,
            "node_type" => node_type,
            "sort_order" => sort_order,
        ],
    }
}
