//! Migration integrity checks ([TST-04]).

use diesel::pg::PgConnection;
use diesel::prelude::*;
use riverbase_core::datastore::postgres::{
    prepare_migration_search_path, run_pending_migrations, AUDIT_MIGRATION_SCHEMA,
};

fn require_db_url() -> String {
    if std::env::var("RIVERBASE_TEST_REQUIRE_DB").is_ok() || std::env::var("CI").is_ok() {
        std::env::var("DB_URL").expect("DB_URL required when RIVERBASE_TEST_REQUIRE_DB or CI is set")
    } else {
        match std::env::var("DB_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping migration_integrity: set DB_URL to run");
                std::process::exit(0);
            }
        }
    }
}

fn applied_versions(conn: &mut PgConnection, schema: &str) -> Vec<String> {
    prepare_migration_search_path(conn, schema).expect("search_path");
    diesel::sql_query(format!(
        "SELECT version FROM {schema}.__diesel_schema_migrations ORDER BY version"
    ))
    .load::<VersionRow>(conn)
    .map(|rows| rows.into_iter().map(|r| r.version).collect())
    .unwrap_or_default()
}

#[derive(QueryableByName)]
struct VersionRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    version: String,
}

#[test]
fn framework_migrations_apply_idempotently() {
    let url = require_db_url();
    let mut conn = PgConnection::establish(&url).expect("connect");
    run_pending_migrations(&mut conn).expect("apply framework migrations");
    let first = applied_versions(&mut conn, AUDIT_MIGRATION_SCHEMA);
    assert!(!first.is_empty(), "framework migrations should be recorded");
    run_pending_migrations(&mut conn).expect("re-apply framework migrations");
    let second = applied_versions(&mut conn, AUDIT_MIGRATION_SCHEMA);
    assert_eq!(first, second, "re-applying must not change version table");
}

#[test]
fn embedded_migrations_record_every_folder_version() {
    let url = require_db_url();
    let mut conn = PgConnection::establish(&url).expect("connect");
    run_pending_migrations(&mut conn).expect("apply");
    let versions = applied_versions(&mut conn, AUDIT_MIGRATION_SCHEMA);
    for version in [
        "20260506000000",
        "20260530100000",
        "20260619180000",
        "20260721200000",
    ] {
        assert!(
            versions.iter().any(|v| v == version),
            "migration version {version} missing from bookkeeping table"
        );
    }
}
