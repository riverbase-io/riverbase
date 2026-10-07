use async_trait::async_trait;
use uuid::Uuid;

use riverbase_core::base::RiverbaseResult;

use super::model::{FilesystemConfig, MediaEntry, MediaQuery, PutMediaRequest};

/// Unified media management interface for local/remote stores.
#[async_trait]
pub trait MediaManager: Send + Sync {
    async fn register_filesystem(&self, config: FilesystemConfig) -> RiverbaseResult<()>;
    async fn put(&self, request: PutMediaRequest) -> RiverbaseResult<MediaEntry>;
    async fn get(&self, file_id: &Uuid) -> RiverbaseResult<Vec<u8>>;
    async fn stream(&self, file_id: &Uuid, chunk_size: usize) -> RiverbaseResult<Vec<Vec<u8>>>;
    async fn delete(&self, file_id: &Uuid) -> RiverbaseResult<()>;
    async fn exists(&self, file_id: &Uuid) -> RiverbaseResult<bool>;
    async fn copy(&self, file_id: &Uuid, dest_fskey: Option<&str>) -> RiverbaseResult<MediaEntry>;
    async fn get_metadata(&self, file_id: &Uuid) -> RiverbaseResult<MediaEntry>;
    async fn list_files(&self, query: MediaQuery) -> RiverbaseResult<Vec<MediaEntry>>;
}
