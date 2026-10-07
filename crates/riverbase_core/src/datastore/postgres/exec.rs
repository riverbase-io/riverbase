use diesel::pg::PgConnection;
use diesel_async::AsyncPgConnection;

use super::dbpool::{PgPool, SyncPgPool};
use crate::datastore::error::DataResult;

/// Acquire an async connection and run Diesel work without blocking the runtime thread pool.
pub async fn with_connection<T, F, Fut>(pool: &PgPool, f: F) -> DataResult<T>
where
    F: FnOnce(&mut AsyncPgConnection) -> Fut,
    Fut: std::future::Future<Output = DataResult<T>>,
{
    let mut conn = pool
        .get()
        .await
        .map_err(|e| crate::errors::DAT_030.from_driver(e))?;
    f(&mut conn).await
}

/// Same as [`with_connection`] but returns [`crate::base::RiverbaseResult`].
pub async fn with_connection_riverbase<T, F, Fut>(
    pool: &PgPool,
    f: F,
) -> crate::base::RiverbaseResult<T>
where
    F: FnOnce(&mut AsyncPgConnection) -> Fut,
    Fut: std::future::Future<Output = crate::base::RiverbaseResult<T>>,
{
    let mut conn = pool
        .get()
        .await
        .map_err(|e| crate::errors::DAT_010.from_driver(e))?;
    f(&mut conn).await
}

/// Legacy: run sync Diesel on the blocking thread pool (sync r2d2 pool only).
pub async fn run_blocking<T, F>(pool: &SyncPgPool, f: F) -> DataResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut PgConnection) -> DataResult<T> + Send + 'static,
{
    let pool = pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool
            .get()
            .map_err(|e| crate::errors::DAT_031.from_driver(e))?;
        f(&mut conn)
    })
    .await
    .map_err(|e| crate::errors::DAT_032.from_driver(e))?
}

/// Legacy alias for sync-pool callers (`riverbase_task`, `river_lotus`, …).
pub async fn run_blocking_riverbase<T, F>(pool: &SyncPgPool, f: F) -> crate::base::RiverbaseResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut PgConnection) -> crate::base::RiverbaseResult<T> + Send + 'static,
{
    let pool = pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool
            .get()
            .map_err(|e| crate::errors::DAT_010.from_driver(e))?;
        f(&mut conn)
    })
    .await
    .map_err(|e| crate::errors::DAT_011.from_driver(e))?
}
