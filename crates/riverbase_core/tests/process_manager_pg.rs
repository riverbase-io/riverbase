mod support;

use riverbase_core::command::{ProcessManagerStore, ProcessState, ProcessStatus};
use riverbase_core::datastore::PostgresProcessManagerStore;

#[tokio::test]
async fn claim_due_leases_process_rows() {
    let Some((pool, _)) = support::pg::try_pg_pool().await else {
        return;
    };
    let store = PostgresProcessManagerStore::new(pool);
    let correlation = uuid::Uuid::new_v4().to_string();
    let mut process = ProcessState::new(
        "test.workflow",
        &correlation,
        serde_json::json!({ "step": 0 }),
    );
    process.status = ProcessStatus::Pending;
    process.next_attempt_at = Some(chrono::Utc::now() - chrono::Duration::seconds(5));
    store.create(None, process).await.expect("create");

    let claimed = store.claim_due(10).await.expect("claim");
    assert!(
        claimed.iter().any(|row| row.correlation_key == correlation),
        "expected claimed workflow for {correlation}"
    );
    let second = store.claim_due(10).await.expect("second claim");
    assert!(
        !second.iter().any(|row| row.correlation_key == correlation),
        "leased workflow must not be claimed twice"
    );
}

#[tokio::test]
async fn completed_workflow_persists_terminal_status() {
    let Some((pool, _)) = support::pg::try_pg_pool().await else {
        return;
    };
    let store = PostgresProcessManagerStore::new(pool);
    let correlation = uuid::Uuid::new_v4().to_string();
    let mut process = ProcessState::new(
        "test.workflow",
        &correlation,
        serde_json::json!({ "done": true }),
    );
    process.status = ProcessStatus::Completed;
    process.next_attempt_at = None;
    let saved = store.create(None, process).await.expect("create");
    let loaded = store.load(saved.id).await.expect("load").expect("row");
    assert_eq!(loaded.status, ProcessStatus::Completed);
    assert_eq!(loaded.correlation_key, correlation);
}
