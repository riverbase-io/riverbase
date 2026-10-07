use diesel_migrations::{embed_migrations, EmbeddedMigrations};
use riverbase_core::base::RiverbaseResult;
use riverbase_core::datastore::postgres::{establish_dbpool, run_embedded_migrations, PgPool};

pub const FORM_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub fn run_form_migrations(db_url: &str) -> RiverbaseResult<()> {
    run_embedded_migrations(db_url, FORM_MIGRATIONS, "riverbase_form")
        .map_err(|e| crate::errors::FRM_060.with_data(e.to_string()))
}

pub async fn establish_form_dbpool(db_url: &str) -> RiverbaseResult<PgPool> {
    let dbpool = establish_dbpool(db_url)
        .await
        .map_err(|e| crate::errors::FRM_061.with_data(e.to_string()))?;
    run_form_migrations(db_url)?;
    Ok(dbpool)
}
