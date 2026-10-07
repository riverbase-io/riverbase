use std::collections::HashMap;

use serde::Deserialize;

fn default_media_fs_root() -> String {
    "./media".to_string()
}

fn default_media_fskey() -> String {
    "file".to_string()
}

/// Blob storage for `rfx.media` (`[riverbase.media]` / `MEDIA_*` / `RIVERBASE_MEDIA_*`).
///
/// - `protocol = "fs"`: local disk via `fs_root`
/// - `protocol = "s3"`: S3-compatible via `params` (`bucket`, optional `endpoint`, …)
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MediaStorageConfig {
    /// Fs root.
    pub fs_root: String,
    /// Fskey.
    pub fskey: String,
    /// Protocol.
    pub protocol: String,
    /// Object-key prefix inside the storage root (not an OS path).
    pub root_path: String,
    #[serde(default)]
    /// Params.
    pub params: HashMap<String, String>,
}

impl Default for MediaStorageConfig {
    fn default() -> Self {
        Self {
            fs_root: default_media_fs_root(),
            fskey: default_media_fskey(),
            protocol: "fs".to_string(),
            root_path: String::new(),
            params: HashMap::new(),
        }
    }
}

impl MediaStorageConfig {
    /// Apply `MEDIA_*` / `RIVERBASE_MEDIA_*` environment overrides (wins over TOML).
    pub fn apply_env_overrides(&mut self) {
        if let Ok(value) =
            std::env::var("MEDIA_FS_ROOT").or_else(|_| std::env::var("RIVERBASE_MEDIA_FS_ROOT"))
        {
            self.fs_root = value;
        }
        if let Ok(value) =
            std::env::var("MEDIA_FSKEY").or_else(|_| std::env::var("RIVERBASE_MEDIA_FSKEY"))
        {
            self.fskey = value;
        }
        if let Ok(value) =
            std::env::var("MEDIA_PROTOCOL").or_else(|_| std::env::var("RIVERBASE_MEDIA_PROTOCOL"))
        {
            self.protocol = value;
        }
        if let Ok(value) =
            std::env::var("MEDIA_ROOT_PATH").or_else(|_| std::env::var("RIVERBASE_MEDIA_ROOT_PATH"))
        {
            self.root_path = value;
        }
        apply_media_param_env(
            &mut self.params,
            "bucket",
            &["MEDIA_BUCKET", "RIVERBASE_MEDIA_BUCKET"],
        );
        apply_media_param_env(
            &mut self.params,
            "endpoint",
            &["MEDIA_ENDPOINT", "RIVERBASE_MEDIA_ENDPOINT"],
        );
        apply_media_param_env(
            &mut self.params,
            "region",
            &["MEDIA_REGION", "RIVERBASE_MEDIA_REGION"],
        );
        apply_media_param_env(
            &mut self.params,
            "access_key_id",
            &["MEDIA_ACCESS_KEY_ID", "RIVERBASE_MEDIA_ACCESS_KEY_ID"],
        );
        apply_media_param_env(
            &mut self.params,
            "secret_access_key",
            &["MEDIA_SECRET_ACCESS_KEY", "RIVERBASE_MEDIA_SECRET_ACCESS_KEY"],
        );
        apply_media_param_env(
            &mut self.params,
            "enable_virtual_host_style",
            &[
                "MEDIA_ENABLE_VIRTUAL_HOST_STYLE",
                "RIVERBASE_MEDIA_ENABLE_VIRTUAL_HOST_STYLE",
            ],
        );
    }
}

fn apply_media_param_env(params: &mut HashMap<String, String>, key: &str, env_names: &[&str]) {
    for name in env_names {
        if let Ok(value) = std::env::var(name) {
            params.insert(key.to_string(), value);
            return;
        }
    }
}
