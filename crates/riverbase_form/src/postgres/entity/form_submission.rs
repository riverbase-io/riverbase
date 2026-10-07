use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::domain_fields_from_payload;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::postgres::entity::helpers::{bool_field, i32_field, opt_str, str_field, uuid_field};
use crate::postgres::schema::form_submission;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable)]
    #[diesel(table_name = form_submission)]
    pub struct FormSubmissionRow {
        pub document_id: Uuid,
        pub form_reg_id: Uuid,
        pub title: String,
        pub desc: Option<String>,
        pub sort_order: i32,
        pub locked: bool,
        pub status: String,
    }
}

pub fn form_submission_row_to_json(row: FormSubmissionRow) -> Value {
    json!({
        "id": row._id.to_string(),
        "document_id": row.document_id.to_string(),
        "form_reg_id": row.form_reg_id.to_string(),
        "title": row.title,
        "desc": row.desc,
        "sort_order": row.sort_order,
        "locked": row.locked,
        "status": row.status,
    })
}

pub async fn form_submission_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let document_id = uuid_field(data, "document_id").unwrap_or(Uuid::nil());
    let form_reg_id = uuid_field(data, "form_reg_id").unwrap_or(Uuid::nil());
    let title = str_field(data, "title", "");
    let desc = opt_str(data, "desc");
    let sort_order = i32_field(data, "sort_order", 0);
    let locked = bool_field(data, "locked", false);
    let status = str_field(data, "status", "draft");

    let exists: bool = diesel::select(diesel::dsl::exists(
        form_submission::table.filter(form_submission::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;

    if exists {
        diesel::update(form_submission::table.filter(form_submission::_id.eq(id)))
            .set((
                form_submission::document_id.eq(document_id),
                form_submission::form_reg_id.eq(form_reg_id),
                form_submission::title.eq(&title),
                form_submission::desc.eq(desc.as_deref()),
                form_submission::sort_order.eq(sort_order),
                form_submission::locked.eq(locked),
                form_submission::status.eq(&status),
                form_submission::_updated.eq(Some(Utc::now())),
            ))
            .execute(conn)
            .await?;
    } else {
        let domain = domain_fields_from_payload(data, id);
        diesel::insert_into(form_submission::table)
            .values(riverbase_core::domain_insert_values!(
                form_submission,
                domain,
                form_submission::document_id.eq(document_id),
                form_submission::form_reg_id.eq(form_reg_id),
                form_submission::title.eq(&title),
                form_submission::desc.eq(desc.as_deref()),
                form_submission::sort_order.eq(sort_order),
                form_submission::locked.eq(locked),
                form_submission::status.eq(&status),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    FormSubmissionEntity {
        schema: form_submission,
        row: FormSubmissionRow,
        resources: ["form_submission"],
        source: "form_submission",
        row_to_json: form_submission_row_to_json,
        upsert_from_json: form_submission_upsert_from_json,
        order: [
            "_id" => _id,
            "document_id" => document_id,
            "title" => title,
            "status" => status,
            "sort_order" => sort_order,
        ],
    }
}
