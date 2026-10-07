mod support;

use std::sync::Arc;

use riverbase_core::command::CommandTarget;
use riverbase_core::datastore::PgDataStore;
use riverbase_core::domain::{Domain, DomainQueryEngine, DomainRuntime};
use riverbase_core::logstore::PostgresDomainLogStore;
use riverbase_core::query::{QueryAccess, QueryRequest};
use riverbase_core::transport::{PgMessageBus, StreamBus};
use todo_domain::TodoDomain;

#[tokio::test]
async fn query_engine_pool_serves_concurrent_lists() {
    let Some((pool, url)) = support::pg::try_pg_pool().await else {
        return;
    };
    let logstore = PostgresDomainLogStore::new(pool.clone()).into_bundle();
    let stream_bus =
        Arc::new(PgMessageBus::connect(&url).await.expect("PgMessageBus")) as Arc<dyn StreamBus>;
    let runtime = DomainRuntime::postgres_test(Arc::new(pool), url, logstore, stream_bus);

    let domain = TodoDomain::<PgDataStore>::spawn(&runtime)
        .await
        .expect("domain");

    domain
        .execute_command(
            "create-todo",
            serde_json::json!({ "title": "A" }),
            CommandTarget::collection("todo"),
        )
        .await
        .expect("seed todo");

    let mut handles = Vec::with_capacity(16);
    for _ in 0..16 {
        let query = domain.query_dyn();
        handles.push(tokio::spawn(async move {
            query
                .execute(
                    DomainQueryEngine::context(query.as_ref()),
                    "todo",
                    QueryAccess::List,
                    QueryRequest::default(),
                    None,
                )
                .await
        }));
    }

    for handle in handles {
        handle.await.expect("join").expect("list");
    }
}
