use deadpool::Runtime;
use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::{Connection, RunQueryDsl};
use diesel_async::pooled_connection::deadpool::Pool as AsyncPool;
use diesel_async::pooled_connection::{AsyncDieselConnectionManager, ManagerConfig};
use diesel_async::{AsyncConnection, AsyncPgConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use tracing::info;

use crate::config::{clamp_dbpool_max_size, DatabasePoolConfig};
use crate::datastore::error::DataResult;

/// Async runtime pool (default for domain query/command data paths).
pub type PgPool = AsyncPool<AsyncPgConnection>;

/// Sync r2d2 pool for crates not yet on diesel-async (e.g. `riverbase_task`).
pub type SyncPgPool = Pool<ConnectionManager<PgConnection>>;

/// Audit Migrations constant.
pub const AUDIT_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Bookkeeping schema for framework audit migrations (`riverbase_audit` logstore).
pub const AUDIT_MIGRATION_SCHEMA: &str = "riverbase_audit";

fn validate_schema_name(schema: &str) -> DataResult<()> {
    if schema.is_empty()
        || !schema
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(crate::errors::DAT_012
            .with_data(format!("invalid migration schema identifier: {schema}")));
    }
    Ok(())
}

/// Ensure `schema` exists and point Diesel bookkeeping at `{schema}.__diesel_schema_migrations`.
pub fn prepare_migration_search_path(conn: &mut PgConnection, schema: &str) -> DataResult<()> {
    validate_schema_name(schema)?;
    diesel::sql_query(format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
        .execute(conn)
        .map_err(|e| crate::errors::DAT_013.from_driver(e))?;
    diesel::sql_query(format!("SET search_path TO {schema}, public"))
        .execute(conn)
        .map_err(|e| crate::errors::DAT_014.from_driver(e))?;
    Ok(())
}

/// Apply pending framework migrations (`riverbase_audit` logstore) on a sync connection.
pub fn run_pending_migrations(conn: &mut PgConnection) -> DataResult<()> {
    prepare_migration_search_path(conn, AUDIT_MIGRATION_SCHEMA)?;
    conn.run_pending_migrations(AUDIT_MIGRATIONS)
        .map_err(|e| crate::errors::DAT_015.from_driver(e))?;
    Ok(())
}

/// Session lock so concurrently starting replicas do not `CREATE TABLE` in parallel
/// (`pg_type_typname_nsp_index` UniqueViolation).
const MIGRATION_ADVISORY_LOCK: i64 = 506151857321;

fn with_migration_advisory_lock(
    conn: &mut PgConnection,
    f: impl FnOnce(&mut PgConnection) -> DataResult<()>,
) -> DataResult<()> {
    diesel::sql_query(format!(
        "SELECT pg_advisory_lock({MIGRATION_ADVISORY_LOCK})"
    ))
    .execute(conn)
    .map_err(|e| crate::errors::DAT_019.from_driver(e))?;
    let result = f(conn);
    let unlock_result = diesel::sql_query(format!(
        "SELECT pg_advisory_unlock({MIGRATION_ADVISORY_LOCK})"
    ))
    .execute(conn)
    .map_err(|e| crate::errors::DAT_020.from_driver(e));
    result.and(unlock_result.map(|_| ()))
}

/// Run arbitrary embedded migrations on a one-off sync connection (Diesel migrations are sync-only).
pub fn run_embedded_migrations(
    db_url: &str,
    migrations: EmbeddedMigrations,
    schema: &str,
) -> DataResult<()> {
    let mut conn =
        PgConnection::establish(db_url).map_err(|e| crate::errors::DAT_016.from_driver(e))?;
    with_migration_advisory_lock(&mut conn, |conn| {
        prepare_migration_search_path(conn, schema)?;
        conn.run_pending_migrations(migrations)
            .map_err(|e| crate::errors::DAT_017.from_driver(e))?;
        Ok(())
    })
}

fn run_startup_migrations(db_url: &str) -> DataResult<()> {
    let mut conn =
        PgConnection::establish(db_url).map_err(|e| crate::errors::DAT_018.from_driver(e))?;
    with_migration_advisory_lock(&mut conn, run_pending_migrations)
}

/// Connect with default pool size ([`crate::config::default_dbpool_max_size`]).
pub async fn establish_dbpool(db_url: &str) -> DataResult<PgPool> {
    establish_dbpool_with_options(db_url, DatabasePoolConfig::default()).await
}

/// Connect with a configured max pool size (async I/O via diesel-async deadpool).
pub async fn establish_dbpool_with_size(db_url: &str, max_size: u32) -> DataResult<PgPool> {
    let mut options = DatabasePoolConfig::default();
    options.max_size = max_size;
    establish_dbpool_with_options(db_url, options).await
}

/// Connect with full pool tuning ([DAT-06]).
pub async fn establish_dbpool_with_options(
    db_url: &str,
    options: DatabasePoolConfig,
) -> DataResult<PgPool> {
    let url = db_url.to_string();
    tokio::task::spawn_blocking(move || run_startup_migrations(&url))
        .await
        .map_err(|e| crate::errors::DAT_021.from_driver(e))??;
    let max_size = clamp_dbpool_max_size(options.max_size) as usize;
    let statement_timeout_ms = options.statement_timeout_ms();
    let mut manager_config = ManagerConfig::<AsyncPgConnection>::default();
    manager_config.custom_setup = Box::new(move |db_url| {
        let statement_timeout_ms = statement_timeout_ms;
        let db_url = db_url.to_string();
        Box::pin(async move {
            let mut conn = AsyncPgConnection::establish(&db_url).await?;
            let sql = format!("SET statement_timeout = '{statement_timeout_ms}ms'");
            diesel_async::RunQueryDsl::execute(diesel::sql_query(sql), &mut conn)
                .await
                .map_err(|error| diesel::ConnectionError::BadConnection(error.to_string()))?;
            Ok(conn)
        })
    });
    let manager = AsyncDieselConnectionManager::new_with_config(db_url, manager_config);
    // deadpool requires an explicit runtime whenever timeouts are configured
    // (`wait_timeout` / `recycle_timeout`); otherwise `.build()` fails with
    // "Timeouts require a runtime".
    let pool = AsyncPool::builder(manager)
        .max_size(max_size)
        .wait_timeout(Some(options.acquire_timeout()))
        .recycle_timeout(Some(options.max_lifetime()))
        .runtime(Runtime::Tokio1)
        .build()
        .map_err(|e| crate::errors::DAT_022.from_driver(e))?;
    let min_connections = options.min_connections();
    if min_connections > 0 {
        let warm = (0..min_connections)
            .map(|_| {
                let pool = pool.clone();
                async move { pool.get().await.map(|conn| drop(conn)) }
            })
            .collect::<Vec<_>>();
        for result in futures_util::future::join_all(warm).await {
            result.map_err(|e| crate::errors::DAT_086.from_driver(e))?;
        }
    }
    info!(
        max_size,
        min_connections,
        acquire_timeout_secs = options.acquire_timeout_secs,
        max_lifetime_secs = options.max_lifetime_secs,
        statement_timeout_ms = options.statement_timeout_ms,
        "framework Postgres migrations applied (async pool ready)"
    );
    Ok(pool)
}

/// Legacy sync r2d2 pool (no framework migration side effects — call [`run_startup_migrations`] first if needed).
pub fn establish_sync_dbpool_with_size(db_url: &str, max_size: u32) -> DataResult<SyncPgPool> {
    let max_size = clamp_dbpool_max_size(max_size);
    let manager = ConnectionManager::<PgConnection>::new(db_url);
    Pool::builder()
        .max_size(max_size)
        .min_idle(Some(2.min(max_size)))
        .build(manager)
        .map_err(|e| crate::errors::DAT_023.from_driver(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn pool_builder_accepts_timeouts_with_tokio_runtime() {
        // Regression for DAT-06: deadpool returns BuildError::NoRuntimeSpecified
        // ("Timeouts require a runtime") unless Runtime::Tokio1 is set when any
        // timeout is configured. `.build()` does not open connections.
        let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(
            "postgres://localhost/riverbase_pool_builder_test",
        );
        let pool = AsyncPool::builder(manager)
            .max_size(1)
            .wait_timeout(Some(Duration::from_secs(1)))
            .recycle_timeout(Some(Duration::from_secs(60)))
            .runtime(Runtime::Tokio1)
            .build();
        assert!(
            pool.is_ok(),
            "pool build with timeouts failed: {:?}",
            pool.err()
        );
    }
}
