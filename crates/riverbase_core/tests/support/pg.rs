use riverbase_core::datastore::postgres::establish_dbpool;
use riverbase_core::datastore::PgPool;

pub fn db_url() -> Option<String> {
    std::env::var("RIVERBASE_DB_URL")
        .or_else(|_| std::env::var("DB_URL"))
        .ok()
        .filter(|u| !u.is_empty())
}

/// Connect when a database URL is set; skip (return `None`) otherwise.
/// Panics in CI when the URL is missing or unreachable.
pub async fn try_pg_pool() -> Option<(PgPool, String)> {
    let url = match db_url() {
        Some(url) => url,
        None if std::env::var("CI").is_ok() => {
            panic!("RIVERBASE_DB_URL or DB_URL must be set in CI for kernel Postgres tests");
        }
        None => {
            eprintln!("skip: set RIVERBASE_DB_URL or DB_URL to run Postgres tests");
            return None;
        }
    };
    match establish_dbpool(&url).await {
        Ok(pool) => Some((pool, url)),
        Err(e) if std::env::var("CI").is_ok() => {
            panic!("postgres connect failed in CI: {e}");
        }
        Err(e) => {
            eprintln!("skip: cannot connect to postgres: {e}");
            None
        }
    }
}
