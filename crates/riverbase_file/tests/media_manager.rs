use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use riverbase_core::base::{RiverbaseResult, NotFoundError};
use riverbase_file::helper::build_media_path;
use riverbase_file::{
    establish_media_dbpool, FilesystemConfig, MediaEntry, MediaManager, MediaMetadataStore,
    MediaQuery, OpenDalMediaManager, PostgresMediaMetadataStore, PutMediaRequest,
};
use tokio::sync::RwLock;
use uuid::Uuid;

fn temp_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("flrs-media-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

async fn pg_metadata_store() -> Option<Arc<PostgresMediaMetadataStore>> {
    let url = std::env::var("RIVERBASE_DB_URL")
        .or_else(|_| std::env::var("DB_URL"))
        .ok()
        .filter(|u| !u.is_empty())?;
    let dbpool = establish_media_dbpool(&url).await.ok()?;
    Some(Arc::new(PostgresMediaMetadataStore::new(dbpool)))
}

/// Test-only metadata stub for filesystem-registration checks (no DB required).
#[derive(Default)]
struct StubMetadataStore {
    rows: RwLock<HashMap<Uuid, MediaEntry>>,
}

#[async_trait]
impl MediaMetadataStore for StubMetadataStore {
    async fn upsert(&self, row: MediaEntry) -> RiverbaseResult<()> {
        self.rows.write().await.insert(row.id(), row);
        Ok(())
    }

    async fn get(&self, id: &Uuid) -> RiverbaseResult<MediaEntry> {
        self.rows.read().await.get(id).cloned().ok_or_else(|| {
            NotFoundError::with("MED-001", "Media metadata was not found.", id.to_string())
        })
    }

    async fn remove(&self, id: &Uuid) -> RiverbaseResult<MediaEntry> {
        self.rows.write().await.remove(id).ok_or_else(|| {
            NotFoundError::with("MED-001", "Media metadata was not found.", id.to_string())
        })
    }

    async fn list(&self, query: &MediaQuery) -> RiverbaseResult<Vec<MediaEntry>> {
        Ok(self
            .rows
            .read()
            .await
            .values()
            .filter(|row| {
                query
                    .resource
                    .as_ref()
                    .map(|r| row.resource.as_ref() == Some(r))
                    .unwrap_or(true)
            })
            .cloned()
            .collect())
    }
}

#[tokio::test]
async fn put_get_list_copy_delete_roundtrip() {
    let Some(metadata) = pg_metadata_store().await else {
        eprintln!("skip: set RIVERBASE_DB_URL to run media manager roundtrip");
        return;
    };
    let manager = OpenDalMediaManager::new(metadata);
    let root = temp_root();

    let mut params = HashMap::new();
    params.insert("root".to_string(), root.to_string_lossy().to_string());
    manager
        .register_filesystem(FilesystemConfig {
            fskey: "file".to_string(),
            protocol: "fs".to_string(),
            root_path: String::new(),
            params,
        })
        .await
        .expect("register fs");

    let resource_id = Uuid::new_v4();
    let row = manager
        .put(PutMediaRequest {
            filename: "hello.txt".to_string(),
            content: b"hello world".to_vec(),
            fskey: Some("file".to_string()),
            filemime: None,
            compress: None,
            resource: Some("document".to_string()),
            resource_id: Some(resource_id),
        })
        .await
        .expect("put");

    let expected_path = build_media_path("", Some("document"), &row.id(), "hello.txt");
    assert_eq!(row.fspath.as_deref(), Some(expected_path.as_str()));
    let parts: Vec<_> = expected_path.split('/').collect();
    assert_eq!(parts.len(), 4, "{expected_path}");
    assert_eq!(parts[0], "document");
    assert_eq!(parts[1].len(), 2);
    assert_eq!(parts[2].len(), 2);
    assert_eq!(parts[3], format!("{}.txt", row.id().simple()));

    let bytes = manager.get(&row.id()).await.expect("get");
    assert_eq!(bytes, b"hello world");

    assert!(manager.exists(&row.id()).await.expect("exists"));

    let listed = manager
        .list_files(MediaQuery {
            resource: Some("document".to_string()),
            resource_id: Some(resource_id),
            ..Default::default()
        })
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id(), row.id());

    let copied = manager.copy(&row.id(), Some("file")).await.expect("copy");
    assert_ne!(copied.id(), row.id());
    let copied_path = build_media_path("", Some("document"), &copied.id(), "hello.txt");
    assert_eq!(copied.fspath.as_deref(), Some(copied_path.as_str()));
    assert_eq!(
        manager.get(&copied.id()).await.expect("copied get"),
        b"hello world"
    );

    manager.delete(&row.id()).await.expect("delete");
    manager.delete(&copied.id()).await.expect("delete copy");
    assert!(!manager
        .exists(&row.id())
        .await
        .expect("exists after delete"));

    std::fs::remove_dir_all(root).expect("cleanup temp dir");
}

#[cfg(feature = "aws-s3")]
#[tokio::test]
async fn s3_register_requires_bucket_param() {
    let manager = OpenDalMediaManager::new(Arc::new(StubMetadataStore::default()));
    let err = manager
        .register_filesystem(FilesystemConfig {
            fskey: "s3".to_string(),
            protocol: "s3".to_string(),
            root_path: String::new(),
            params: HashMap::new(),
        })
        .await
        .expect_err("missing bucket must fail");
    assert_eq!(err.errcode.as_str(), "MED-009");
}

#[cfg(feature = "aws-s3")]
#[tokio::test]
async fn s3_register_builds_operator_with_minio_style_params() {
    let manager = OpenDalMediaManager::new(Arc::new(StubMetadataStore::default()));
    let mut params = HashMap::new();
    params.insert("bucket".to_string(), "test-bucket".to_string());
    params.insert("endpoint".to_string(), "http://127.0.0.1:9000".to_string());
    params.insert("access_key_id".to_string(), "minio".to_string());
    params.insert("secret_access_key".to_string(), "minio123".to_string());
    // region omitted → build_operator defaults to "auto" when endpoint is set
    manager
        .register_filesystem(FilesystemConfig {
            fskey: "s3".to_string(),
            protocol: "s3".to_string(),
            root_path: String::new(),
            params,
        })
        .await
        .expect("register s3 operator");
}

/// Live MinIO roundtrip. Enable with `RIVERBASE_MEDIA_S3_TEST=1` and a reachable endpoint.
///
/// Expected env (in addition to the gate):
/// - `RIVERBASE_MEDIA_ENDPOINT` (default `http://127.0.0.1:9000`)
/// - `RIVERBASE_MEDIA_BUCKET`
/// - `RIVERBASE_MEDIA_ACCESS_KEY_ID` / `RIVERBASE_MEDIA_SECRET_ACCESS_KEY`
#[cfg(feature = "aws-s3")]
#[tokio::test]
async fn s3_put_get_delete_roundtrip_minio() {
    if std::env::var("RIVERBASE_MEDIA_S3_TEST").ok().as_deref() != Some("1") {
        eprintln!("skipping: set RIVERBASE_MEDIA_S3_TEST=1 to run MinIO roundtrip");
        return;
    }

    let Some(metadata) = pg_metadata_store().await else {
        eprintln!("skip: set RIVERBASE_DB_URL to run MinIO media roundtrip");
        return;
    };

    let endpoint =
        std::env::var("RIVERBASE_MEDIA_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:9000".into());
    let bucket = std::env::var("RIVERBASE_MEDIA_BUCKET").expect("RIVERBASE_MEDIA_BUCKET");
    let access_key_id =
        std::env::var("RIVERBASE_MEDIA_ACCESS_KEY_ID").expect("RIVERBASE_MEDIA_ACCESS_KEY_ID");
    let secret_access_key =
        std::env::var("RIVERBASE_MEDIA_SECRET_ACCESS_KEY").expect("RIVERBASE_MEDIA_SECRET_ACCESS_KEY");

    let mut params = HashMap::new();
    params.insert("bucket".to_string(), bucket);
    params.insert("endpoint".to_string(), endpoint);
    params.insert("region".to_string(), "auto".to_string());
    params.insert("access_key_id".to_string(), access_key_id);
    params.insert("secret_access_key".to_string(), secret_access_key);

    let manager = OpenDalMediaManager::new(metadata).with_default_fskey("s3");
    manager
        .register_filesystem(FilesystemConfig {
            fskey: "s3".to_string(),
            protocol: "s3".to_string(),
            root_path: format!("flrs-media-test/{}", Uuid::new_v4()),
            params,
        })
        .await
        .expect("register minio");

    let row = manager
        .put(PutMediaRequest {
            filename: "hello.txt".to_string(),
            content: b"hello minio".to_vec(),
            fskey: Some("s3".to_string()),
            filemime: None,
            compress: None,
            resource: Some("document".to_string()),
            resource_id: Some(Uuid::new_v4()),
        })
        .await
        .expect("put");

    assert_eq!(manager.get(&row.id()).await.expect("get"), b"hello minio");
    manager.delete(&row.id()).await.expect("delete");
}
