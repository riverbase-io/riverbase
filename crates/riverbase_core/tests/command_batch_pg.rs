mod support;

use std::sync::Arc;

use riverbase_core::base::AggregateRoot;
use riverbase_core::command::{BatchItemStatus, CommandTarget, PreparedCommand};
use riverbase_core::datastore::PgDataStore;
use riverbase_core::domain::DomainRuntime;
use riverbase_core::logstore::PostgresDomainLogStore;
use riverbase_core::query::QueryAccess;
use riverbase_core::transport::{PgMessageBus, StreamBus};
use todo_domain::TodoDomain;
use uuid::Uuid;

/// Titles created by `execute_prepared_batch` in these tests, so assertions can tell a batch
/// row apart from anything else that might exist in the shared test database.
async fn todo_titles(domain: &TodoDomain<PgDataStore>) -> Vec<String> {
    let listed = domain
        .execute_query(
            domain.query().context(),
            "todo",
            QueryAccess::List,
            Default::default(),
            None,
        )
        .await
        .expect("list todos");
    listed["data"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            item.get("title")
                .and_then(|t| t.as_str())
                .map(str::to_string)
        })
        .collect()
}

async fn spawn_test_domain(tenant: Uuid) -> Option<TodoDomain<PgDataStore>> {
    let (pool, url) = support::pg::try_pg_pool().await?;
    let logstore = PostgresDomainLogStore::new(pool.clone()).into_bundle();
    let stream_bus =
        Arc::new(PgMessageBus::connect(&url).await.expect("PgMessageBus")) as Arc<dyn StreamBus>;
    let runtime = DomainRuntime::postgres_test(Arc::new(pool), url, logstore, stream_bus)
        .with_tenant_id(tenant);
    Some(
        TodoDomain::<PgDataStore>::spawn(&runtime)
            .await
            .expect("domain"),
    )
}

/// Every item succeeds: the whole batch commits and every outcome's `final_status` stays `ok`.
#[tokio::test]
async fn execute_prepared_batch_all_ok_commits_every_item() {
    let Some(domain) = spawn_test_domain(Uuid::new_v4()).await else {
        return;
    };
    let ctx = domain.command().context().clone();

    let marker = Uuid::new_v4();
    let title_a = format!("batch-ok-a-{marker}");
    let title_b = format!("batch-ok-b-{marker}");
    let items = vec![
        PreparedCommand {
            cmdkey: "create-todo".into(),
            payload: serde_json::json!({ "title": title_a }),
            target: CommandTarget::collection("todo"),
        },
        PreparedCommand {
            cmdkey: "create-todo".into(),
            payload: serde_json::json!({ "title": title_b }),
            target: CommandTarget::collection("todo"),
        },
    ];

    let result = domain
        .command()
        .inner()
        .execute_prepared_batch(&ctx, items)
        .await
        .expect("batch executes");

    assert!(result.ok);
    assert_eq!(result.items.len(), 2);
    for item in &result.items {
        assert_eq!(item.immediate_status, BatchItemStatus::Ok);
        assert_eq!(item.final_status, BatchItemStatus::Ok);
        assert!(item.result.is_some());
        assert!(item.error.is_none());
    }

    let titles = todo_titles(&domain).await;
    assert!(
        titles.contains(&title_a),
        "committed item a must be visible"
    );
    assert!(
        titles.contains(&title_b),
        "committed item b must be visible"
    );
}

/// Item 1 of 3 fails; the whole transaction rolls back. `immediate_status` still records what
/// each item's own attempt did, but `final_status` reflects that nothing persisted, and the
/// created-but-rolled-back row is genuinely absent from the store.
#[tokio::test]
async fn execute_prepared_batch_mid_batch_failure_rolls_back_and_marks_final_failed() {
    let Some(domain) = spawn_test_domain(Uuid::new_v4()).await else {
        return;
    };
    let ctx = domain.command().context().clone();

    let marker = Uuid::new_v4();
    let title_a = format!("batch-fail-a-{marker}");
    let title_c = format!("batch-fail-c-{marker}");
    let items = vec![
        PreparedCommand {
            cmdkey: "create-todo".into(),
            payload: serde_json::json!({ "title": title_a }),
            target: CommandTarget::collection("todo"),
        },
        // `update-todo` against an id that doesn't exist fails opening the aggregate
        // (CMD-007) before the handler body even runs, so this deterministically fails
        // without needing a real row.
        PreparedCommand {
            cmdkey: "update-todo".into(),
            payload: serde_json::json!({ "done": true }),
            target: CommandTarget::Object(AggregateRoot::new("todo", Uuid::new_v4().to_string())),
        },
        PreparedCommand {
            cmdkey: "create-todo".into(),
            payload: serde_json::json!({ "title": title_c }),
            target: CommandTarget::collection("todo"),
        },
    ];

    let result = domain
        .command()
        .inner()
        .execute_prepared_batch(&ctx, items)
        .await
        .expect("batch call itself does not error");

    assert!(!result.ok);
    assert_eq!(result.items.len(), 3);

    let item_a = &result.items[0];
    assert_eq!(
        item_a.immediate_status,
        BatchItemStatus::Ok,
        "item a's own attempt committed within the transaction"
    );
    assert_eq!(
        item_a.final_status,
        BatchItemStatus::Failed,
        "the transaction rolled back, so item a did not survive"
    );
    assert!(
        item_a.result.is_some(),
        "immediate result stays visible for debugging even though final_status is failed"
    );

    let item_b = &result.items[1];
    assert_eq!(item_b.immediate_status, BatchItemStatus::Failed);
    assert_eq!(item_b.final_status, BatchItemStatus::Failed);
    let errcode = item_b
        .error
        .as_ref()
        .and_then(|e| e.get("errcode"))
        .and_then(|c| c.as_str());
    assert_eq!(errcode, Some("CMD-007"));

    let item_c = &result.items[2];
    assert_eq!(item_c.immediate_status, BatchItemStatus::NotRun);
    assert_eq!(item_c.final_status, BatchItemStatus::NotRun);

    let titles = todo_titles(&domain).await;
    assert!(
        !titles.contains(&title_a),
        "rolled-back item a must not be visible in the store"
    );
    assert!(
        !titles.contains(&title_c),
        "never-run item c must not be visible in the store"
    );
}
