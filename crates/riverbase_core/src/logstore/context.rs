use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::base::{AggregateContext, EngineContext, RiverbaseResult};
use crate::datastore::CommandUnitOfWork;

use super::model::{ContextLogRecord, LogRowMeta};
use super::types::DomainTransport;
use super::util::{parse_optional_uuid, scope_uuid};
use super::DomainLogStore;

fn request_id_from_engine(ctx: &EngineContext) -> Option<Uuid> {
    ctx.correlation_id
        .as_deref()
        .and_then(parse_optional_uuid)
        .or_else(|| ctx.trace_id.as_deref().and_then(parse_optional_uuid))
}

fn base_context_record(
    context_id: Uuid,
    engine: &EngineContext,
    source: Value,
) -> ContextLogRecord {
    let timestamp = Utc::now();
    ContextLogRecord {
        meta: LogRowMeta {
            id: context_id,
            created: timestamp,
            creator: engine.actor.profile_id,
        },
        domain: Some(engine.namespace.clone()),
        revision: Some(1),
        realm: None,
        dataset_id: None,
        request_id: request_id_from_engine(engine),
        user_id: engine.actor.user_id,
        profile_id: engine.actor.profile_id,
        organization_id: engine
            .claim_str("organization_id")
            .and_then(parse_optional_uuid),
        tenant: engine.actor.tenant,
        iam_roles: engine.roles.clone(),
        session: None,
        timestamp,
        transport: DomainTransport::FastApi,
        source,
        headers: None,
    }
}

/// Persist a command-scoped context row referenced by `command_log.context`.
///
/// Pass the command's unit of work while that transaction is still open. After
/// rollback, pass `None` so the row is written on a pooled connection and survives.
pub async fn append_command_context(
    logstore: &DomainLogStore,
    aggregate: &AggregateContext,
    engine: &EngineContext,
    command: &str,
    resource: &str,
    payload: &Value,
    uow: Option<&CommandUnitOfWork>,
) -> RiverbaseResult<()> {
    let source = json!({
        "kind": "command",
        "command": command,
        "resource": resource,
        "payload": payload,
        "scope": aggregate.scope,
        "causation_id": engine.causation_id,
        "parent_command_id": engine.parent_command_id,
        "invocation_depth": engine.invocation_depth,
        "invocation_mode": format!("{:?}", engine.invocation_mode),
        "service_capability": engine.service_capability,
    });
    let mut record = base_context_record(aggregate.context_id, engine, source);
    record.meta.created = aggregate.timestamp;
    record.timestamp = aggregate.timestamp;
    record.meta.creator = aggregate.actor.profile_id;
    record.user_id = aggregate.actor.user_id;
    record.profile_id = aggregate.actor.profile_id;
    record.realm = None;
    record.tenant = aggregate.actor.tenant;
    record.organization_id = aggregate
        .claims
        .get("organization_id")
        .and_then(Value::as_str)
        .and_then(parse_optional_uuid);
    record.dataset_id = scope_uuid(&aggregate.scope, "domain_sid");
    logstore.contexts.append(uow, record).await
}

/// Persist a query-scoped context row referenced by `query_log.context`.
pub async fn append_query_context(
    logstore: &DomainLogStore,
    context_id: Uuid,
    engine: &EngineContext,
    query_resource: &str,
    access: &str,
    request: &Value,
    item_id: Option<&str>,
) -> RiverbaseResult<()> {
    let source = json!({
        "kind": "query",
        "resource": query_resource,
        "access": access,
        "request": request,
        "item_id": item_id,
    });
    logstore
        .contexts
        .append(None, base_context_record(context_id, engine, source))
        .await
}
