//! Postgres persistence smoke test.
//!
//! Skips gracefully when no database is reachable.

#![cfg(feature = "postgres")]

use diesel_async::SimpleAsyncConnection;
use riverbase_form::postgres::{establish_form_dbpool, run_form_migrations, FORM_MIGRATIONS};

fn db_url() -> Option<String> {
    if let Ok(url) = std::env::var("RIVERBASE_FORM_DB_URL") {
        return Some(url);
    }
    riverbase_core::config::RiverbaseConfig::load()
        .ok()
        .map(|c| c.db_url)
        .filter(|s| !s.is_empty())
}

#[tokio::test]
#[ignore = "requires Postgres (RIVERBASE_CONFIG / RIVERBASE_FORM_DB_URL)"]
async fn migrations_create_form_tables() {
    let Some(url) = db_url() else {
        eprintln!("skip: no database URL");
        return;
    };
    run_form_migrations(&url).expect("migrations");
    let pool = match establish_form_dbpool(&url).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("skip: cannot reach database: {}", e.errmesg);
            return;
        }
    };
    let mut conn = pool.get().await.expect("connection");
    conn.batch_execute("SELECT 1 FROM riverbase_form.document LIMIT 1")
        .await
        .expect("document table exists");
    conn.batch_execute("SELECT 1 FROM riverbase_form.text_input_data LIMIT 1")
        .await
        .expect("element table exists");
    let _ = FORM_MIGRATIONS;
}
