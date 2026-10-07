use async_trait::async_trait;
use uuid::Uuid;

use crate::base::RiverbaseResult;
use crate::datastore::CommandUnitOfWork;
use crate::logstore::{CommandLogRecord, CommandLogStatus};

/// Generic append-only audit log store for a single record type.
#[async_trait]
pub trait LogStore<R>: Send + Sync
where
    R: Send + Sync,
{
    /// `uow` is required on the command path; query and background workers pass `None`.
    async fn append(&self, uow: Option<&CommandUnitOfWork>, record: R) -> RiverbaseResult<()>;
}

/// Mutable lifecycle operations for command audit records.
#[async_trait]
pub trait CommandStatusLogStore: LogStore<CommandLogRecord> {
    /// Set status.
    async fn set_status(
        &self,
        uow: Option<&CommandUnitOfWork>,
        command_id: Uuid,
        status: CommandLogStatus,
    ) -> RiverbaseResult<()>;
}

/// Append multiple records to any [`LogStore`].
pub async fn append_many<S, R>(
    store: &S,
    uow: Option<&CommandUnitOfWork>,
    records: impl IntoIterator<Item = R>,
) -> RiverbaseResult<()>
where
    S: LogStore<R> + ?Sized,
    R: Send + Sync,
{
    for record in records {
        store.append(uow, record).await?;
    }
    Ok(())
}
