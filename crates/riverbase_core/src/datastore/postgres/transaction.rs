use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use diesel_async::pooled_connection::deadpool::Object;
use diesel_async::{AsyncPgConnection, SimpleAsyncConnection};
use tokio::sync::{Mutex, OwnedMutexGuard};

use super::PgPool;
use crate::datastore::transaction::CommandUnitOfWork;
use crate::datastore::DataResult;

const ACTIVE: u8 = 0;
const COMMITTED: u8 = 1;
const ROLLED_BACK: u8 = 2;

/// One command-scoped PostgreSQL transaction shared by state and audit stores.
#[derive(Clone)]
pub struct PgTransaction {
    connection: Arc<Mutex<Object<AsyncPgConnection>>>,
    state: Arc<AtomicU8>,
}

impl PgTransaction {
    /// Begin.
    pub async fn begin(pool: &PgPool) -> DataResult<Self> {
        let mut connection = pool
            .get()
            .await
            .map_err(|error| crate::errors::DAT_024.with_data(error.to_string()))?;
        connection
            .batch_execute("BEGIN")
            .await
            .map_err(|error| crate::errors::DAT_025.with_data(error.to_string()))?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            state: Arc::new(AtomicU8::new(ACTIVE)),
        })
    }

    /// Commit.
    pub async fn commit(&self) -> DataResult<()> {
        self.finish("COMMIT", COMMITTED).await
    }

    /// Rollback.
    pub async fn rollback(&self) -> DataResult<()> {
        self.finish("ROLLBACK", ROLLED_BACK).await
    }

    async fn finish(&self, sql: &str, terminal_state: u8) -> DataResult<()> {
        self.ensure_active()?;
        let mut connection = self.connection.clone().lock_owned().await;
        connection
            .batch_execute(sql)
            .await
            .map_err(|error| crate::errors::DAT_026.with_data(error.to_string()))?;
        self.state.store(terminal_state, Ordering::Release);
        Ok(())
    }

    /// Connection.
    pub async fn connection(&self) -> DataResult<PgConnectionGuard> {
        self.ensure_active()?;
        Ok(PgConnectionGuard::Transaction(
            self.connection.clone().lock_owned().await,
        ))
    }

    fn ensure_active(&self) -> DataResult<()> {
        if self.state.load(Ordering::Acquire) != ACTIVE {
            return Err(crate::errors::DAT_027.with_data("command transaction is no longer active"));
        }
        Ok(())
    }
}

/// Pg Connection Guard enumeration.
pub enum PgConnectionGuard {
    /// Pooled.
    Pooled(Object<AsyncPgConnection>),
    /// Transaction.
    Transaction(OwnedMutexGuard<Object<AsyncPgConnection>>),
}

impl Deref for PgConnectionGuard {
    type Target = AsyncPgConnection;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Pooled(connection) => connection,
            Self::Transaction(connection) => connection,
        }
    }
}

impl DerefMut for PgConnectionGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Pooled(connection) => connection,
            Self::Transaction(connection) => connection,
        }
    }
}

/// Checkout a pooled connection for query/read paths (outside command transactions).
pub async fn pool_connection(pool: &PgPool) -> DataResult<PgConnectionGuard> {
    pool.get()
        .await
        .map(PgConnectionGuard::Pooled)
        .map_err(|error| crate::errors::DAT_028.with_data(error.to_string()))
}

/// Connection for command-path writes participating in the active unit of work.
pub async fn command_connection(uow: &CommandUnitOfWork) -> DataResult<PgConnectionGuard> {
    match uow {
        CommandUnitOfWork::Transactional(transaction) => transaction.connection().await,
        CommandUnitOfWork::NonTransactional => Err(crate::errors::DAT_029
            .with_data("postgres command-path connection requires a transactional unit of work")),
    }
}

/// Optional command-path connection: `Some` inside a postgres TX, `None` for pool reads / post-rollback logging.
pub async fn optional_command_connection(
    uow: Option<&CommandUnitOfWork>,
    pool: &PgPool,
) -> DataResult<PgConnectionGuard> {
    match uow {
        Some(uow) => command_connection(uow).await,
        None => pool_connection(pool).await,
    }
}
