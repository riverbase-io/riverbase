use async_trait::async_trait;

use super::model::ResponseRecord;
use super::store::LogStore;
use crate::base::{CommandId, RiverbaseResult};

/// Response log (append + lookup by command id).
#[async_trait]
pub trait ResponseLogStore: LogStore<ResponseRecord> {
    /// Get.
    async fn get(&self, cmd_id: &CommandId) -> RiverbaseResult<ResponseRecord>;
}
