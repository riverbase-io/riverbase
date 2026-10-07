use std::marker::PhantomData;

use async_trait::async_trait;

use super::model::{CommandLogRecord, ResponseRecord};
use super::response::ResponseLogStore;
use super::store::{CommandStatusLogStore, LogStore};
use super::types::CommandLogStatus;
use crate::base::{CommandId, RiverbaseResult};
use crate::datastore::CommandUnitOfWork;
use uuid::Uuid;

/// Append-only store that discards all records (channel disabled).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpLogStore<R>(PhantomData<R>);

impl<R> NoOpLogStore<R> {
    /// Construct a new value.
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

#[async_trait]
impl<R: Send + Sync> LogStore<R> for NoOpLogStore<R> {
    async fn append(&self, _uow: Option<&CommandUnitOfWork>, _record: R) -> RiverbaseResult<()> {
        Ok(())
    }
}

#[async_trait]
impl CommandStatusLogStore for NoOpLogStore<CommandLogRecord> {
    async fn set_status(
        &self,
        _uow: Option<&CommandUnitOfWork>,
        _command_id: Uuid,
        _status: CommandLogStatus,
    ) -> RiverbaseResult<()> {
        Ok(())
    }
}

/// Response store that discards writes; reads are not available.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpResponseLogStore;

#[async_trait]
impl LogStore<ResponseRecord> for NoOpResponseLogStore {
    async fn append(
        &self,
        _uow: Option<&CommandUnitOfWork>,
        _record: ResponseRecord,
    ) -> RiverbaseResult<()> {
        Ok(())
    }
}

#[async_trait]
impl ResponseLogStore for NoOpResponseLogStore {
    async fn get(&self, cmd_id: &CommandId) -> RiverbaseResult<ResponseRecord> {
        Err(crate::errors::LOG_002
            .with_data(format!("response log disabled (command {})", cmd_id.0)))
    }
}
