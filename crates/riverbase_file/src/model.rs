use std::collections::HashMap;

use chrono::{DateTime, Utc};
use riverbase_core::base::DomainFields;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// Compression methods aligned with `riverbase.media.FsSpecCompressionMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaCompressionMethod {
    Bz2,
    Gzip,
    Lz4,
    Lzma,
    Snappy,
    Xz,
    Zip,
    Zstd,
}

/// Standard file metadata aligned with `riverbase.media.MediaEntry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaEntry {
    #[serde(flatten)]
    pub domain: DomainFields,
    pub filename: String,
    pub filehash: Option<String>,
    pub filemime: Option<String>,
    pub fskey: Option<String>,
    pub length: i64,
    pub fspath: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_compress"
    )]
    pub compress: Option<MediaCompressionMethod>,
    pub resource: Option<String>,
    pub resource_id: Option<Uuid>,
    pub resource_sid: Option<Uuid>,
    pub resource_iid: Option<Uuid>,
    pub xattrs: Option<String>,
    pub cdn_exp: Option<DateTime<Utc>>,
    pub cdn_url: Option<String>,
}

impl MediaEntry {
    pub fn id(&self) -> Uuid {
        self.domain.id
    }

    pub fn created(&self) -> DateTime<Utc> {
        self.domain.created
    }

    pub fn updated(&self) -> Option<DateTime<Utc>> {
        self.domain.updated
    }

    pub fn etag(&self) -> Option<Uuid> {
        self.domain.etag
    }

    pub fn deleted(&self) -> Option<DateTime<Utc>> {
        self.domain.deleted
    }
}

/// Filesystem backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemConfig {
    pub fskey: String,
    pub protocol: String,
    pub root_path: String,
    #[serde(default)]
    pub params: HashMap<String, String>,
}

impl Default for FilesystemConfig {
    fn default() -> Self {
        Self {
            fskey: "file".to_string(),
            protocol: "fs".to_string(),
            root_path: "root".to_string(),
            params: HashMap::new(),
        }
    }
}

/// Request to put a file into managed storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutMediaRequest {
    pub filename: String,
    pub content: Vec<u8>,
    pub fskey: Option<String>,
    pub filemime: Option<String>,
    pub compress: Option<MediaCompressionMethod>,
    pub resource: Option<String>,
    pub resource_id: Option<Uuid>,
}

/// Query options for metadata listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaQuery {
    pub resource: Option<String>,
    pub resource_id: Option<Uuid>,
    pub limit: usize,
    pub offset: usize,
}

impl Default for MediaQuery {
    fn default() -> Self {
        Self {
            resource: None,
            resource_id: None,
            limit: 100,
            offset: 0,
        }
    }
}

fn deserialize_optional_compress<'de, D>(
    deserializer: D,
) -> Result<Option<MediaCompressionMethod>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => MediaCompressionMethod::deserialize(v)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}
