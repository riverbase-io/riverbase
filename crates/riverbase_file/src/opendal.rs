use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use opendal::Operator;
use tokio::sync::RwLock;
use uuid::Uuid;

use riverbase_core::base::RiverbaseResult;

use super::helper::{build_media_path, guess_mime, hash_n_length, storage_object_key};
use super::metadata::MediaMetadataStore;
use super::model::{FilesystemConfig, MediaEntry, MediaQuery, PutMediaRequest};
use super::store::MediaManager;

struct RegisteredFilesystem {
    config: FilesystemConfig,
    operator: Operator,
}

#[derive(Clone)]
pub struct OpenDalMediaManager {
    default_fskey: String,
    metadata: Arc<dyn MediaMetadataStore>,
    filesystems: Arc<RwLock<HashMap<String, RegisteredFilesystem>>>,
}

impl OpenDalMediaManager {
    pub fn new(metadata: Arc<dyn MediaMetadataStore>) -> Self {
        Self {
            default_fskey: "file".to_string(),
            metadata,
            filesystems: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_store(metadata: Arc<dyn MediaMetadataStore>) -> Self {
        Self::new(metadata)
    }

    pub fn with_default_fskey(mut self, fskey: impl Into<String>) -> Self {
        self.default_fskey = fskey.into();
        self
    }

    async fn resolve_filesystem(
        &self,
        fskey: Option<&str>,
    ) -> RiverbaseResult<(FilesystemConfig, Operator)> {
        let key = fskey.unwrap_or(&self.default_fskey);
        let systems = self.filesystems.read().await;
        let row = systems
            .get(key)
            .ok_or_else(|| crate::errors::MED_007.with_data(format!("filesystem {key}")))?;
        Ok((row.config.clone(), row.operator.clone()))
    }

    fn build_operator(config: &FilesystemConfig) -> RiverbaseResult<Operator> {
        match config.protocol.as_str() {
            "fs" | "file" => {
                #[cfg(not(feature = "fs"))]
                {
                    let _ = config;
                    return Err(
                        crate::errors::MED_013.with_data("Rebuild riverbase_file with feature `fs`.")
                    );
                }
                #[cfg(feature = "fs")]
                {
                    let builder = opendal::services::Fs::default();
                    let root = config
                        .params
                        .get("root")
                        .cloned()
                        .unwrap_or_else(|| ".".to_string());
                    let builder = builder.root(&root);
                    Ok(Operator::new(builder)
                        .map_err(|e| crate::errors::MED_008.with_data(e.to_string()))?
                        .finish())
                }
            }
            "s3" => {
                #[cfg(not(feature = "aws-s3"))]
                {
                    let _ = config;
                    return Err(crate::errors::MED_034
                        .with_data("Rebuild riverbase_file with feature `aws-s3`."));
                }
                #[cfg(feature = "aws-s3")]
                {
                    let mut builder = opendal::services::S3::default()
                        .bucket(&config.params.get("bucket").cloned().ok_or_else(|| {
                            crate::errors::MED_009.with_data("s3 requires bucket")
                        })?)
                        // Prefer explicit FilesystemConfig params over ambient AWS env/config.
                        .disable_config_load();
                    if let Some(v) = config.params.get("endpoint") {
                        builder = builder.endpoint(v);
                    }
                    match config.params.get("region") {
                        Some(v) => builder = builder.region(v),
                        None if config.params.contains_key("endpoint") => {
                            // OpenDAL MinIO guidance: region "auto" when using a custom endpoint.
                            builder = builder.region("auto");
                        }
                        None => {}
                    }
                    if let Some(v) = config.params.get("access_key_id") {
                        builder = builder.access_key_id(v);
                    }
                    if let Some(v) = config.params.get("secret_access_key") {
                        builder = builder.secret_access_key(v);
                    }
                    if parse_bool_param(config.params.get("enable_virtual_host_style")) {
                        builder = builder.enable_virtual_host_style();
                    }
                    Ok(Operator::new(builder)
                        .map_err(|e| crate::errors::MED_010.with_data(e.to_string()))?
                        .finish())
                }
            }
            "http" | "https" => {
                #[cfg(not(feature = "http"))]
                {
                    let _ = config;
                    return Err(crate::errors::MED_035
                        .with_data("Rebuild riverbase_file with feature `http`."));
                }
                #[cfg(feature = "http")]
                {
                    let builder = opendal::services::Http::default();
                    let endpoint = config.params.get("endpoint").cloned().ok_or_else(|| {
                        crate::errors::MED_011.with_data("http requires endpoint")
                    })?;
                    let builder = builder.endpoint(&endpoint);
                    Ok(Operator::new(builder)
                        .map_err(|e| crate::errors::MED_012.with_data(e.to_string()))?
                        .finish())
                }
            }
            other => Err(crate::errors::MED_036.with_data(format!("unsupported protocol {other}"))),
        }
    }
}

#[cfg(feature = "aws-s3")]
fn parse_bool_param(value: Option<&String>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

#[async_trait]
impl MediaManager for OpenDalMediaManager {
    async fn register_filesystem(&self, config: FilesystemConfig) -> RiverbaseResult<()> {
        let operator = Self::build_operator(&config)?;
        self.filesystems.write().await.insert(
            config.fskey.clone(),
            RegisteredFilesystem { config, operator },
        );
        Ok(())
    }

    async fn put(&self, request: PutMediaRequest) -> RiverbaseResult<MediaEntry> {
        let (config, operator) = self.resolve_filesystem(request.fskey.as_deref()).await?;
        let now = Utc::now();
        let mut domain = riverbase_core::base::DomainFields::new();
        domain.created = now;
        let path = build_media_path(
            &config.root_path,
            request.resource.as_deref(),
            &domain.id,
            &request.filename,
        );
        operator
            .write(&path, request.content.clone())
            .await
            .map_err(|e| crate::errors::MED_014.with_data(e.to_string()))?;
        let (filehash, length) = hash_n_length(&request.content);
        let row = MediaEntry {
            domain,
            filename: request.filename,
            filehash: Some(filehash),
            filemime: request.filemime.or_else(|| Some(guess_mime(&path))),
            fskey: Some(config.fskey),
            length,
            fspath: Some(path),
            compress: request.compress,
            resource: request.resource,
            resource_id: request.resource_id,
            resource_sid: None,
            resource_iid: None,
            xattrs: None,
            cdn_exp: None,
            cdn_url: None,
        };
        self.metadata.upsert(row.clone()).await?;
        Ok(row)
    }

    async fn get(&self, file_id: &Uuid) -> RiverbaseResult<Vec<u8>> {
        let row = self.metadata.get(file_id).await?;
        let (_, operator) = self.resolve_filesystem(row.fskey.as_deref()).await?;
        let path = row
            .fspath
            .ok_or_else(|| crate::errors::MED_015.with_data(file_id.to_string()))?;
        let key = storage_object_key(&path);
        let bytes = operator
            .read(&key)
            .await
            .map_err(|e| crate::errors::MED_016.with_data(e.to_string()))?;
        Ok(bytes.to_vec())
    }

    async fn stream(&self, file_id: &Uuid, chunk_size: usize) -> RiverbaseResult<Vec<Vec<u8>>> {
        if chunk_size == 0 {
            return Err(crate::errors::MED_017.with_data("chunk_size must be > 0"));
        }
        let bytes = self.get(file_id).await?;
        Ok(bytes
            .chunks(chunk_size)
            .map(|chunk| chunk.to_vec())
            .collect())
    }

    async fn delete(&self, file_id: &Uuid) -> RiverbaseResult<()> {
        let row = self.metadata.remove(file_id).await?;
        let (_, operator) = self.resolve_filesystem(row.fskey.as_deref()).await?;
        if let Some(path) = row.fspath {
            let key = storage_object_key(&path);
            operator
                .delete(&key)
                .await
                .map_err(|e| crate::errors::MED_018.with_data(e.to_string()))?;
        }
        Ok(())
    }

    async fn exists(&self, file_id: &Uuid) -> RiverbaseResult<bool> {
        let row = match self.metadata.get(file_id).await {
            Ok(row) => row,
            Err(_) => return Ok(false),
        };
        let (_, operator) = self.resolve_filesystem(row.fskey.as_deref()).await?;
        let path = match row.fspath {
            Some(path) => path,
            None => return Ok(false),
        };
        let key = storage_object_key(&path);
        operator
            .exists(&key)
            .await
            .map_err(|e| crate::errors::MED_019.with_data(e.to_string()))
    }

    async fn copy(&self, file_id: &Uuid, dest_fskey: Option<&str>) -> RiverbaseResult<MediaEntry> {
        let row = self.metadata.get(file_id).await?;
        let content = self.get(file_id).await?;
        self.put(PutMediaRequest {
            filename: row.filename,
            content,
            fskey: dest_fskey
                .map(str::to_string)
                .or_else(|| row.fskey.as_ref().map(ToString::to_string)),
            filemime: row.filemime,
            compress: row.compress,
            resource: row.resource,
            resource_id: row.resource_id,
        })
        .await
    }

    async fn get_metadata(&self, file_id: &Uuid) -> RiverbaseResult<MediaEntry> {
        self.metadata.get(file_id).await
    }

    async fn list_files(&self, query: MediaQuery) -> RiverbaseResult<Vec<MediaEntry>> {
        self.metadata.list(&query).await
    }
}
