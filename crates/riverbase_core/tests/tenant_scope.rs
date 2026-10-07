mod support;

use std::sync::Arc;

use riverbase_core::base::{AggregateRoot, EngineContext};
use riverbase_core::command::CommandTarget;
use riverbase_core::datastore::PgDataStore;
use riverbase_core::domain::{DomainCommandEngine, DomainQueryEngine, DomainRuntime};
use riverbase_core::logstore::PostgresDomainLogStore;
use riverbase_core::query::QueryAccess;
use riverbase_core::transport::{PgMessageBus, StreamBus};
use todo_domain::TodoDomain;
use uuid::Uuid;

#[tokio::test]
async fn tenant_scope_hides_other_tenant_rows_and_requires_context() {
    let Some((pool, url)) = support::pg::try_pg_pool().await else {
        return;
    };
    let tenant_a = Uuid::new_v4();
    let tenant_b = Uuid::new_v4();
    let logstore = PostgresDomainLogStore::new(pool.clone()).into_bundle();
    let stream_bus =
        Arc::new(PgMessageBus::connect(&url).await.expect("PgMessageBus")) as Arc<dyn StreamBus>;
    let runtime = DomainRuntime::postgres_test(Arc::new(pool), url, logstore, stream_bus)
        .with_tenant_id(tenant_a);

    let domain = TodoDomain::<PgDataStore>::spawn(&runtime)
        .await
        .expect("domain");

    let created = domain
        .execute_command(
            "create-todo",
            serde_json::json!({ "title": "tenant-a" }),
            CommandTarget::collection("todo"),
        )
        .await
        .expect("create");
    let id = created.get("id").and_then(|v| v.as_str()).expect("id");

    let ctx_b =
        EngineContext::for_domain(TodoDomain::<PgDataStore>::NAMESPACE).with_tenant_id(tenant_b);
    let listed_b = domain
        .execute_query(&ctx_b, "todo", QueryAccess::List, Default::default(), None)
        .await
        .expect("list b");
    let items_b = listed_b["data"].as_array().cloned().unwrap_or_default();
    assert!(
        !items_b
            .iter()
            .any(|item| item.get("id").and_then(|v| v.as_str()) == Some(id)),
        "tenant B must not see tenant A row {id}"
    );

    let item_b = domain
        .execute_query(
            &ctx_b,
            "todo",
            QueryAccess::Item,
            Default::default(),
            Some(id),
        )
        .await;
    assert!(
        item_b.is_err(),
        "item fetch for another tenant must fail closed"
    );

    let update_b = DomainCommandEngine::execute(
        domain.command(),
        &ctx_b,
        "update-todo",
        serde_json::json!({ "done": true }),
        CommandTarget::Object(AggregateRoot::new("todo", id)),
    )
    .await
    .expect_err("cross-tenant object command");
    assert_eq!(update_b.errcode.as_str(), "CMD-007");

    let update_a = domain
        .execute_command(
            "update-todo",
            serde_json::json!({ "done": true }),
            CommandTarget::Object(AggregateRoot::new("todo", id)),
        )
        .await
        .expect("same-tenant update must succeed even when row_to_json omits _tenant");
    assert_eq!(update_a.get("done"), Some(&serde_json::json!(true)));

    let listed_a = domain
        .execute_query(
            DomainQueryEngine::context(domain.query()),
            "todo",
            QueryAccess::List,
            Default::default(),
            None,
        )
        .await
        .expect("list a");
    let items_a = listed_a["data"].as_array().cloned().unwrap_or_default();
    assert!(
        items_a
            .iter()
            .any(|item| item.get("id").and_then(|v| v.as_str()) == Some(id)),
        "tenant A must see its own row {id}"
    );

    let missing_cmd = DomainCommandEngine::execute(
        domain.command(),
        &EngineContext::new(TodoDomain::<PgDataStore>::NAMESPACE),
        "create-todo",
        serde_json::json!({ "title": "no-tenant" }),
        CommandTarget::collection("todo"),
    )
    .await
    .expect_err("DOM-045");
    assert_eq!(missing_cmd.errcode.as_str(), "DOM-045");

    let missing_qry = domain
        .execute_query(
            &EngineContext::new(TodoDomain::<PgDataStore>::NAMESPACE),
            "todo",
            QueryAccess::List,
            Default::default(),
            None,
        )
        .await
        .expect_err("QRY-141");
    assert_eq!(missing_qry.errcode.as_str(), "QRY-141");
}
