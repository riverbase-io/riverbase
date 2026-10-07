use diesel_migrations::{embed_migrations, EmbeddedMigrations};

use riverbase_core::base::RiverbaseResult;
use riverbase_core::config::default_dbpool_max_size;
use riverbase_core::datastore::postgres::{
    establish_dbpool_with_size, run_embedded_migrations, PgPool,
};

pub const MEDIA_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub fn run_media_migrations(db_url: &str) -> RiverbaseResult<()> {
    run_embedded_migrations(db_url, MEDIA_MIGRATIONS, "riverbase_media")
        .map_err(|e| crate::errors::MED_033.with_data(e.to_string()))?;
    Ok(())
}

pub async fn establish_media_dbpool(db_url: &str) -> RiverbaseResult<PgPool> {
    run_media_migrations(db_url)?;
    establish_dbpool_with_size(db_url, default_dbpool_max_size())
        .await
        .map_err(|e| crate::errors::MED_032.with_data(e.to_string()))
}
