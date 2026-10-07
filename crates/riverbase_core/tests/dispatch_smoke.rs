mod support;

use std::sync::Arc;

use riverbase_core::command::CommandTarget;
use riverbase_core::datastore::PgDataStore;
use riverbase_core::domain::{Domain, DomainQueryEngine, DomainRuntime};
use riverbase_core::logstore::PostgresDomainLogStore;
use riverbase_core::query::QueryAccess;
use riverbase_core::transport::dispatch::{
    CommandDispatchActor, QueryDispatchActor, TransportEnvelope,
};
use riverbase_core::transport::{PgMessageBus, StreamBus};
use ractor::Actor;
use todo_domain::TodoDomain;

#[tokio::test]
async fn rpc_actors_dispatch_command_and_query() {
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

    let cmd_actor = Actor::spawn(None, CommandDispatchActor, domain.command_dyn())
        .await
        .expect("command actor")
        .0;
    cmd_actor
        .send_message(TransportEnvelope::Command {
            cmdkey: "create-todo".into(),
            payload: serde_json::json!({ "title": "RPC todo" }),
            target: CommandTarget::collection("todo"),
        })
        .expect("send command");

    let query_actor = Actor::spawn(None, QueryDispatchActor, domain.query_dyn())
        .await
        .expect("query actor")
        .0;
    query_actor
        .send_message(TransportEnvelope::Query {
            resource: "todo".into(),
            access: QueryAccess::List,
            request: Default::default(),
            item_id: None,
        })
        .expect("send query");

    // Direct execute confirms the engines are live after actor dispatch.
    domain
        .execute_query(
            DomainQueryEngine::context(domain.query()),
            "todo",
            QueryAccess::List,
            Default::default(),
            None,
        )
        .await
        .expect("list");
}
