mod support;

use riverbase_core::base::CommandId;
use riverbase_core::command::{deliver_outbox_batch, ProcessManagerStore, ProcessState};
use riverbase_core::datastore::PostgresProcessManagerStore;
use riverbase_core::logstore::idempotency::{ClaimOutcome, IdempotencyScope};
use riverbase_core::logstore::model::OutboxRecord;
use riverbase_core::logstore::PostgresDomainLogStore;
use riverbase_core::transport::PgMessageBus;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct CountingBus {
    inner: PgMessageBus,
    published: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl riverbase_core::command::MessageBus for CountingBus {
    async fn publish(
        &self,
        topic: &str,
        payload: serde_json::Value,
    ) -> riverbase_core::base::RiverbaseResult<()> {
        self.published.fetch_add(1, Ordering::SeqCst);
        self.inner.publish(topic, payload).await
    }
}

#[tokio::test]
async fn outbox_publish_is_idempotent_per_record() {
    let Some((pool, url)) = support::pg::try_pg_pool().await else {
        return;
    };
    let logstore = PostgresDomainLogStore::new(pool.clone());
    let outbox = logstore.clone().into_bundle().outbox;
    let published = Arc::new(AtomicUsize::new(0));
    let bus = Arc::new(CountingBus {
        inner: PgMessageBus::connect(&url).await.expect("PgMessageBus"),
        published: published.clone(),
    }) as Arc<dyn riverbase_core::command::MessageBus>;

    let src_cmd = uuid::Uuid::new_v4();
    use riverbase_core::logstore::{CommandLogRecord, CommandLogStatus, LogRowMeta, LogStore};
    logstore
        .append(
            None,
            CommandLogRecord {
                meta: LogRowMeta {
                    id: src_cmd,
                    created: chrono::Utc::now(),
                    creator: None,
                },
                domain: "test".into(),
                identifier: None,
                resource: "test".into(),
                revision: 1,
                command: "test".into(),
                domain_sid: None,
                domain_iid: None,
                payload: serde_json::json!({}),
                context: uuid::Uuid::new_v4(),
                status: CommandLogStatus::Success,
                tenant: None,
            },
        )
        .await
        .expect("command_log parent for outbox FK");

    let record = OutboxRecord {
        id: uuid::Uuid::new_v4(),
        created: chrono::Utc::now(),
        src_cmd,
        topic: "test.topic".into(),
        payload: serde_json::json!({ "n": 1 }),
        attempts: 0,
    };
    outbox.enqueue(None, record).await.expect("enqueue");

    deliver_outbox_batch(outbox.as_ref(), bus.as_ref())
        .await
        .expect("first deliver");
    assert_eq!(published.load(Ordering::SeqCst), 1);

    deliver_outbox_batch(outbox.as_ref(), bus.as_ref())
        .await
        .expect("second deliver");
    assert_eq!(
        published.load(Ordering::SeqCst),
        1,
        "published record must not be delivered twice"
    );
}

#[tokio::test]
async fn idempotency_store_replays_completed_response() {
    let Some((pool, _)) = support::pg::try_pg_pool().await else {
        return;
    };
    let bundle = PostgresDomainLogStore::new(pool).into_bundle();
    let store = bundle.idempotency;
    let scope = IdempotencyScope::new("kernel.pg", "ping", format!("key-{}", uuid::Uuid::new_v4()));
    let response = serde_json::json!({ "ok": true });
    let request_hash = "hash-v1";

    let claim = match store
        .claim(&scope, None, request_hash)
        .await
        .expect("claim")
    {
        ClaimOutcome::Fresh(claim) => claim,
        other => panic!("expected fresh claim, got {other:?}"),
    };
    store
        .complete(None, &scope, &claim, &CommandId::new(), response.clone())
        .await
        .expect("complete");

    match store
        .claim(&scope, None, request_hash)
        .await
        .expect("replay")
    {
        ClaimOutcome::Completed(replayed) => assert_eq!(replayed, response),
        other => panic!("expected completed replay, got {other:?}"),
    }
}

#[tokio::test]
async fn process_manager_rejects_stale_version_writes() {
    let Some((pool, _)) = support::pg::try_pg_pool().await else {
        return;
    };
    let store = PostgresProcessManagerStore::new(pool);
    let created = store
        .create(
            None,
            ProcessState::new(
                "kernel.version",
                &uuid::Uuid::new_v4().to_string(),
                serde_json::json!({}),
            ),
        )
        .await
        .expect("create");
    let err = store
        .save(None, created.clone(), created.version - 1)
        .await
        .expect_err("stale version must fail");
    assert_eq!(
        err.errcode.as_str(),
        "PCS-003",
        "expected stale version conflict, got {err}"
    );
}
