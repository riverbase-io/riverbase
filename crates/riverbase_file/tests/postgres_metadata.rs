use chrono::Utc;
use riverbase_core::base::DomainFields;
use riverbase_file::{
    establish_media_dbpool, MediaCompressionMethod, MediaEntry, MediaMetadataStore, MediaQuery,
    PostgresMediaMetadataStore,
};
use uuid::Uuid;

#[tokio::test]
async fn postgres_metadata_roundtrip_when_database_url_is_set() {
    let Ok(db_url) = std::env::var("DB_URL") else {
        return;
    };
    let dbpool = establish_media_dbpool(&db_url).await.expect("dbpool");
    let store = PostgresMediaMetadataStore::new(dbpool);

    let id = Uuid::new_v4();
    let now = Utc::now();
    let etag = Uuid::new_v4();
    let mut domain = DomainFields::with_id(id);
    domain.created = now;
    domain.updated = Some(now);
    domain.etag = Some(etag);
    let row = MediaEntry {
        domain,
        filename: "hello.txt".to_string(),
        filehash: Some("abc123".to_string()),
        filemime: Some("text/plain".to_string()),
        fskey: Some("file".to_string()),
        length: 11,
        fspath: Some("/tmp/hello.txt".to_string()),
        compress: Some(MediaCompressionMethod::Gzip),
        resource: Some("document".to_string()),
        resource_id: Some(Uuid::new_v4()),
        resource_sid: None,
        resource_iid: None,
        xattrs: Some("{\"a\":1}".to_string()),
        cdn_exp: None,
        cdn_url: None,
    };

    store.upsert(row.clone()).await.expect("upsert");

    let loaded = store.get(&id).await.expect("get");
    assert_eq!(loaded.id(), row.id());
    assert_eq!(loaded.filename, row.filename);
    assert_eq!(loaded.compress, row.compress);

    let listed = store
        .list(&MediaQuery {
            resource: row.resource.clone(),
            resource_id: row.resource_id,
            limit: 10,
            offset: 0,
        })
        .await
        .expect("list");
    assert!(listed.iter().any(|item| item.id() == id));

    let removed = store.remove(&id).await.expect("remove");
    assert_eq!(removed.id(), id);
}
