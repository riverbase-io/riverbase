use std::time::Duration;

use async_trait::async_trait;
use uuid::Uuid;

use super::model::OutboxRecord;
use crate::base::RiverbaseResult;
use crate::datastore::CommandUnitOfWork;

/// Durable, leased integration-message queue.
#[async_trait]
pub trait OutboxStore: Send + Sync {
    /// Enqueue.
    async fn enqueue(
        &self,
        uow: Option<&CommandUnitOfWork>,
        record: OutboxRecord,
    ) -> RiverbaseResult<()>;

    /// Atomically lease due records for one publisher.
    async fn claim_due(&self, limit: i64, lease: Duration) -> RiverbaseResult<Vec<OutboxRecord>>;

    /// Mark published.
    async fn mark_published(&self, id: Uuid) -> RiverbaseResult<()>;

    /// Return a record to the retry queue or dead-letter it after `max_attempts`.
    async fn mark_failed(
        &self,
        id: Uuid,
        error: &str,
        retry_after: Duration,
        max_attempts: i32,
    ) -> RiverbaseResult<()>;
}

#[derive(Debug, Clone, Copy, Default)]
/// No op outbox store; structure.
pub struct NoOpOutboxStore;

#[async_trait]
impl OutboxStore for NoOpOutboxStore {
    async fn enqueue(
        &self,
        _uow: Option<&CommandUnitOfWork>,
        _record: OutboxRecord,
    ) -> RiverbaseResult<()> {
        Ok(())
    }

    async fn claim_due(&self, _limit: i64, _lease: Duration) -> RiverbaseResult<Vec<OutboxRecord>> {
        Ok(Vec::new())
    }

    async fn mark_published(&self, _id: Uuid) -> RiverbaseResult<()> {
        Ok(())
    }

    async fn mark_failed(
        &self,
        _id: Uuid,
        _error: &str,
        _retry_after: Duration,
        _max_attempts: i32,
    ) -> RiverbaseResult<()> {
        Ok(())
    }
}
