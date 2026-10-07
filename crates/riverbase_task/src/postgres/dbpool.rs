use diesel::pg::PgConnection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

use riverbase_core::base::RiverbaseResult;
use riverbase_core::config::default_dbpool_max_size;
use riverbase_core::datastore::postgres::{prepare_migration_search_path, run_embedded_migrations};
use riverbase_core::datastore::{establish_sync_dbpool_with_size, SyncPgPool};

pub const TRACKER_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub fn run_tracker_migrations(conn: &mut PgConnection) -> RiverbaseResult<()> {
    prepare_migration_search_path(conn, "riverbase_task")
        .map_err(|e| crate::errors::TRK_001.with_data(e.to_string()))?;
    conn.run_pending_migrations(TRACKER_MIGRATIONS)
        .map_err(|e| crate::errors::TRK_001.with_data(e.to_string()))?;
    Ok(())
}

pub fn run_tracker_migrations_url(db_url: &str) -> RiverbaseResult<()> {
    run_embedded_migrations(db_url, TRACKER_MIGRATIONS, "riverbase_task")
        .map_err(|e| crate::errors::TRK_005.with_data(e.to_string()))
}

pub fn establish_tracker_dbpool(db_url: &str) -> RiverbaseResult<SyncPgPool> {
    let dbpool = establish_sync_dbpool_with_size(db_url, default_dbpool_max_size())
        .map_err(|e| crate::errors::TRK_002.with_data(e.to_string()))?;
    let mut conn = dbpool
        .get()
        .map_err(|e| crate::errors::TRK_003.with_data(e.to_string()))?;
    run_tracker_migrations(&mut conn)?;
    Ok(dbpool)
}
