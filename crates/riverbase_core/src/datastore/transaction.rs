use std::future::Future;

use super::error::DataResult;
use super::postgres::transaction::{command_connection, PgConnectionGuard, PgTransaction};

/// Postgres command transaction controlled by the command engine.
#[derive(Clone)]
pub struct CommandTransaction(pub PgTransaction);

impl std::fmt::Debug for CommandTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CommandTransaction")
    }
}

impl CommandTransaction {
    /// Construct a new value.
    pub fn new(transaction: PgTransaction) -> Self {
        Self(transaction)
    }

    /// Commit.
    pub async fn commit(&self) -> DataResult<()> {
        self.0.commit().await
    }

    /// Rollback.
    pub async fn rollback(&self) -> DataResult<()> {
        self.0.rollback().await
    }

    /// Connection.
    pub async fn connection(&self) -> DataResult<PgConnectionGuard> {
        self.0.connection().await
    }
}

impl From<PgTransaction> for CommandTransaction {
    fn from(transaction: PgTransaction) -> Self {
        Self(transaction)
    }
}

/// Explicit command-scoped unit of work passed through handlers and stores.
///
/// The command engine begins one per command and threads it through
/// [`crate::base::AggregateContext`]. Command-path writes must use this handle rather than
/// checking out pooled connections directly.
#[derive(Clone)]
pub enum CommandUnitOfWork {
    /// Transactional.
    Transactional(CommandTransaction),
    /// Backends with [`crate::datastore::DataStore::supports_command_transactions`] == `false`.
    /// PostgreSQL write connections are rejected.
    NonTransactional,
}

impl std::fmt::Debug for CommandUnitOfWork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transactional(tx) => f.debug_tuple("Transactional").field(tx).finish(),
            Self::NonTransactional => f.write_str("NonTransactional"),
        }
    }
}

impl CommandUnitOfWork {
    /// Transactional.
    pub fn transactional(transaction: CommandTransaction) -> Self {
        Self::Transactional(transaction)
    }

    /// Non transactional.
    pub fn non_transactional() -> Self {
        Self::NonTransactional
    }

    /// Whether this is transactional.
    pub fn is_transactional(&self) -> bool {
        matches!(self, Self::Transactional(_))
    }

    /// Commit.
    pub async fn commit(&self) -> DataResult<()> {
        match self {
            Self::Transactional(transaction) => transaction.commit().await,
            Self::NonTransactional => Ok(()),
        }
    }

    /// Rollback.
    pub async fn rollback(&self) -> DataResult<()> {
        match self {
            Self::Transactional(transaction) => transaction.rollback().await,
            Self::NonTransactional => Ok(()),
        }
    }

    /// PostgreSQL connection for command-path writes and audit/idempotency rows in the same TX.
    pub async fn postgres_connection(&self) -> DataResult<PgConnectionGuard> {
        command_connection(self).await
    }

    /// Run a future inside the ambient transaction scope (deprecated — prefer passing this handle).
    pub async fn scope<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        future.await
    }
}
