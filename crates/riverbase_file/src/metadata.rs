use async_trait::async_trait;
use uuid::Uuid;

use riverbase_core::base::RiverbaseResult;

use super::model::{MediaEntry, MediaQuery};

#[async_trait]
pub trait MediaMetadataStore: Send + Sync {
    async fn upsert(&self, row: MediaEntry) -> RiverbaseResult<()>;
    async fn get(&self, id: &Uuid) -> RiverbaseResult<MediaEntry>;
    async fn remove(&self, id: &Uuid) -> RiverbaseResult<MediaEntry>;
    async fn list(&self, query: &MediaQuery) -> RiverbaseResult<Vec<MediaEntry>>;
}
