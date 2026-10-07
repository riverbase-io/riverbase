use async_trait::async_trait;
use serde_json::Value;

use crate::base::RiverbaseResult;

#[async_trait]
/// Message bus trait.
pub trait MessageBus: Send + Sync {
    /// Publish.
    async fn publish(&self, topic: &str, payload: Value) -> RiverbaseResult<()>;
}
