//! Integration tests for `[riverbase.media]` protocol/params (avoids unrelated lib test compile issues).

use riverbase_core::config::RiverbaseConfig;
use tempfile::tempdir;

#[test]
fn loads_media_s3_protocol_and_params_from_toml() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("riverbase.toml");
    std::fs::write(
        &path,
        r#"
        [riverbase]
        log_level = "info"

        [riverbase.media]
        fskey = "s3"
        protocol = "s3"
        root_path = "uploads"

        [riverbase.media.params]
        bucket = "my-bucket"
        endpoint = "http://127.0.0.1:9000"
        region = "auto"
        access_key_id = "minio"
        secret_access_key = "minio123"
        "#,
    )
    .expect("write");

    let cfg = RiverbaseConfig::from_file(&path).expect("from_file");
    assert_eq!(cfg.media.fskey, "s3");
    assert_eq!(cfg.media.protocol, "s3");
    assert_eq!(cfg.media.root_path, "uploads");
    assert_eq!(
        cfg.media.params.get("bucket").map(String::as_str),
        Some("my-bucket")
    );
    assert_eq!(
        cfg.media.params.get("endpoint").map(String::as_str),
        Some("http://127.0.0.1:9000")
    );
    assert_eq!(
        cfg.media.params.get("region").map(String::as_str),
        Some("auto")
    );
}

#[test]
fn loads_media_fs_defaults_protocol() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("riverbase.toml");
    std::fs::write(
        &path,
        r#"
        [riverbase.media]
        fs_root = "../../local/media"
        fskey = "file"
        "#,
    )
    .expect("write");

    let cfg = RiverbaseConfig::from_file(&path).expect("from_file");
    assert_eq!(cfg.media.fs_root, "../../local/media");
    assert_eq!(cfg.media.fskey, "file");
    assert_eq!(cfg.media.protocol, "fs");
    assert!(cfg.media.root_path.is_empty());
    assert!(cfg.media.params.is_empty());
}

#[test]
fn media_env_overrides_protocol_and_params() {
    let keys = [
        "RIVERBASE_MEDIA_PROTOCOL",
        "RIVERBASE_MEDIA_BUCKET",
        "RIVERBASE_MEDIA_ENDPOINT",
        "RIVERBASE_MEDIA_ROOT_PATH",
    ];
    let previous: Vec<_> = keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();
    std::env::set_var("RIVERBASE_MEDIA_PROTOCOL", "s3");
    std::env::set_var("RIVERBASE_MEDIA_BUCKET", "env-bucket");
    std::env::set_var("RIVERBASE_MEDIA_ENDPOINT", "http://minio:9000");
    std::env::set_var("RIVERBASE_MEDIA_ROOT_PATH", "pfx");

    let mut media = riverbase_core::config::MediaStorageConfig::default();
    media.apply_env_overrides();
    assert_eq!(media.protocol, "s3");
    assert_eq!(media.root_path, "pfx");
    assert_eq!(
        media.params.get("bucket").map(String::as_str),
        Some("env-bucket")
    );
    assert_eq!(
        media.params.get("endpoint").map(String::as_str),
        Some("http://minio:9000")
    );

    for (key, value) in previous {
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
}
