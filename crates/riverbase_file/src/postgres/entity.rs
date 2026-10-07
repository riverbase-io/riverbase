use chrono::{DateTime, Utc};

use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use riverbase_core::base::DomainFields;
use riverbase_core::datastore::error::DataResult;
use serde_json::{json, Value};
use uuid::Uuid;

use super::schema::media_entries;

riverbase_core::domain_row! {
    #[derive(Debug, Clone, Queryable, Selectable, Insertable, AsChangeset, Identifiable)]
    #[diesel(table_name = media_entries, primary_key(_id))]
    pub struct MediaEntryRow {
        pub filename: String,
        pub filehash: Option<String>,
        pub filemime: Option<String>,
        pub fskey: Option<String>,
        pub length: i64,
        pub fspath: Option<String>,
        pub compress: Option<String>,
        pub resource: Option<String>,
        pub resource_id: Option<Uuid>,
        pub resource_sid: Option<Uuid>,
        pub resource_iid: Option<Uuid>,
        pub xattrs: Option<String>,
        pub cdn_exp: Option<DateTime<Utc>>,
        pub cdn_url: Option<String>,
    }
}

pub fn media_row_to_json(row: MediaEntryRow) -> Value {
    let mut value = json!({
        "id": row._id.to_string(),
        "created": row._created,
        "updated": row._updated,
        "creator": row._creator.map(|v| v.to_string()),
        "updater": row._updater.map(|v| v.to_string()),
        "_tenant": row._tenant,
        "etag": row._etag.to_string(),
        "deleted": row._deleted,
        "filename": row.filename,
        "filehash": row.filehash,
        "filemime": row.filemime,
        "fskey": row.fskey,
        "length": row.length,
        "fspath": row.fspath,
        "resource": row.resource,
        "resource_id": row.resource_id.map(|v| v.to_string()),
        "resource_sid": row.resource_sid.map(|v| v.to_string()),
        "resource_iid": row.resource_iid.map(|v| v.to_string()),
        "xattrs": row.xattrs,
        "cdn_exp": row.cdn_exp,
        "cdn_url": row.cdn_url,
    });
    if let Some(compress) = &row.compress {
        value["compress"] = json!(compress);
    }
    value
}

pub async fn media_upsert_from_json(
    conn: &mut diesel_async::AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> DataResult<()> {
    let filename = data
        .get("filename")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let filehash = data
        .get("filehash")
        .and_then(Value::as_str)
        .map(str::to_string);
    let filemime = data
        .get("filemime")
        .and_then(Value::as_str)
        .map(str::to_string);
    let fskey = data
        .get("fskey")
        .and_then(Value::as_str)
        .map(str::to_string);
    let length = data.get("length").and_then(Value::as_i64).unwrap_or(0);
    let fspath = data
        .get("fspath")
        .and_then(Value::as_str)
        .map(str::to_string);
    let compress = data.get("compress").and_then(|v| {
        v.as_str()
            .map(str::to_string)
            .or_else(|| serde_json::to_string(v).ok())
    });
    let resource = data
        .get("resource")
        .and_then(Value::as_str)
        .map(str::to_string);
    let resource_id = data
        .get("resource_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());
    let resource_sid = data
        .get("resource_sid")
        .and_then(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()));
    let resource_iid = data
        .get("resource_iid")
        .and_then(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()));
    let xattrs = data
        .get("xattrs")
        .and_then(Value::as_str)
        .map(str::to_string);
    let cdn_url = data
        .get("cdn_url")
        .and_then(Value::as_str)
        .map(str::to_string);

    let now = Utc::now();
    let exists: bool = diesel::select(diesel::dsl::exists(
        media_entries::table.filter(media_entries::_id.eq(id)),
    ))
    .get_result(conn)
    .await?;

    if exists {
        diesel::update(media_entries::table.filter(media_entries::_id.eq(id)))
            .set((
                media_entries::filename.eq(&filename),
                media_entries::filehash.eq(&filehash),
                media_entries::filemime.eq(&filemime),
                media_entries::fskey.eq(&fskey),
                media_entries::length.eq(length),
                media_entries::fspath.eq(&fspath),
                media_entries::compress.eq(compress.clone()),
                media_entries::resource.eq(&resource),
                media_entries::resource_id.eq(resource_id),
                media_entries::resource_sid.eq(resource_sid),
                media_entries::resource_iid.eq(resource_iid),
                media_entries::xattrs.eq(&xattrs),
                media_entries::cdn_url.eq(&cdn_url),
                riverbase_core::domain_fields_touch_updated!(media_entries, now),
            ))
            .execute(conn)
            .await?;
    } else {
        let domain = DomainFields::with_id(id);
        diesel::insert_into(media_entries::table)
            .values(riverbase_core::domain_insert_values!(
                media_entries,
                domain,
                media_entries::filename.eq(&filename),
                media_entries::filehash.eq(&filehash),
                media_entries::filemime.eq(&filemime),
                media_entries::fskey.eq(&fskey),
                media_entries::length.eq(length),
                media_entries::fspath.eq(&fspath),
                media_entries::compress.eq(compress),
                media_entries::resource.eq(&resource),
                media_entries::resource_id.eq(resource_id),
                media_entries::resource_sid.eq(resource_sid),
                media_entries::resource_iid.eq(resource_iid),
                media_entries::xattrs.eq(&xattrs),
                media_entries::cdn_url.eq(&cdn_url),
            ))
            .execute(conn)
            .await?;
    }
    Ok(())
}

riverbase_core::pg_domain_entity! {
    MediaEntity {
        schema: media_entries,
        row: MediaEntryRow,
        resources: ["media_entry"],
        source: "media_entries",
        row_to_json: media_row_to_json,
        upsert_from_json: media_upsert_from_json,
        order: [
            "_id" => _id,
            "_created" => _created,
            "_creator" => _creator,
            "filename" => filename,
            "filehash" => filehash,
            "filemime" => filemime,
            "fskey" => fskey,
            "length" => length,
            "fspath" => fspath,
            "compress" => compress,
            "resource" => resource,
            "resource_id" => resource_id,
            "resource_sid" => resource_sid,
            "resource_iid" => resource_iid,
            "xattrs" => xattrs,
            "cdn_exp" => cdn_exp,
            "cdn_url" => cdn_url,
        ],
    }
}
