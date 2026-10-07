//! Orchestration for RxDB pull/push, mirroring `RxdbReplicationService._pull`
//! and `._push`: authorize the scope, normalize the request, invoke the
//! collection callback, and publish a change notification on push.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::base::{RiverbaseResult, JsonMap};
use crate::command::MessageBus;

use super::collection::{
    RxdbCollection, RxdbContext, RxdbPullRequest, RxdbPullResult, RxdbPushRequest, RxdbPushResult,
    DEFAULT_PULL_LIMIT, MAX_PULL_LIMIT,
};

/// Channel prefix for RxDB change notifications.
pub const RXDB_NOTIFY_PREFIX: &str = "rxdb.notify.";
/// Notification message type published on the notify channel.
pub const RXDB_NOTIFY_TYPE: &str = "rxdb.notify";

/// Notification channel name for `collection_name` (`rxdb.notify.<collection>`).
pub fn rxdb_notify_channel(collection_name: &str) -> RiverbaseResult<String> {
    if collection_name.is_empty() {
        return Err(crate::errors::RXD_001.with_data(json!({ "collection": collection_name })));
    }
    Ok(format!("{RXDB_NOTIFY_PREFIX}{collection_name}"))
}

/// Clamp a client-supplied limit into `[1, MAX_PULL_LIMIT]`, defaulting when absent.
pub fn normalize_limit(limit: Option<i64>) -> usize {
    let value = match limit {
        Some(v) if v >= 1 => v as usize,
        Some(_) => 1,
        None => DEFAULT_PULL_LIMIT,
    };
    value.clamp(1, MAX_PULL_LIMIT)
}

/// Normalize RxDB push rows to plain document bodies.
///
/// Prefers `rows` over `documents`; each row may wrap its document in
/// `newDocumentState` or `document`. Non-object rows are skipped.
pub fn normalize_push_rows(rows: Option<Vec<Value>>, documents: Option<Vec<Value>>) -> Vec<Value> {
    let source = rows.or(documents).unwrap_or_default();
    let mut normalized = Vec::with_capacity(source.len());
    for row in source {
        let Some(obj) = row.as_object() else {
            continue;
        };
        if let Some(state) = obj.get("newDocumentState").filter(|v| v.is_object()) {
            normalized.push(state.clone());
        } else if let Some(doc) = obj.get("document").filter(|v| v.is_object()) {
            normalized.push(doc.clone());
        } else {
            normalized.push(row);
        }
    }
    normalized
}

async fn resolve_scope(
    collection: &dyn RxdbCollection,
    ctx: &RxdbContext,
    action: &str,
    base_scope: Option<JsonMap>,
) -> RiverbaseResult<JsonMap> {
    let base_scope = base_scope.unwrap_or_default();
    if !collection.policy_required() {
        return Ok(base_scope);
    }
    collection.authorize_scope(ctx, action, base_scope).await
}

/// Authorize, normalize, and run a pull (`RxdbReplicationService._pull`).
pub async fn handle_pull(
    collection: &dyn RxdbCollection,
    ctx: &RxdbContext,
    request: RxdbPullRequest,
) -> RiverbaseResult<RxdbPullResult> {
    let action = format!("rxdb/{}/pull", collection.collection_name());
    let scope = resolve_scope(collection, ctx, &action, request.scope).await?;
    let limit = normalize_limit(request.limit);
    collection.pull(ctx, request.checkpoint, limit, scope).await
}

/// Authorize, normalize, run a push, and publish a change notification
/// (`RxdbReplicationService._push`). Returns the result with `notify_payload`
/// cleared (it is published, not serialized to the client).
pub async fn handle_push(
    collection: &dyn RxdbCollection,
    ctx: &RxdbContext,
    msgbus: Option<&Arc<dyn MessageBus>>,
    request: RxdbPushRequest,
) -> RiverbaseResult<RxdbPushResult> {
    let action = format!("rxdb/{}/push", collection.collection_name());
    let scope = resolve_scope(collection, ctx, &action, request.scope).await?;
    let rows = normalize_push_rows(request.rows, request.documents);

    let mut result = collection.push(ctx, rows, scope).await?;

    if result.changed > 0 || result.notify_payload.is_some() {
        if let Some(bus) = msgbus {
            let payload = result.notify_payload.clone().unwrap_or_else(|| {
                json!({
                    "collection": collection.collection_name(),
                    "changed": result.changed,
                })
            });
            publish_notification(bus.as_ref(), collection.collection_name(), payload).await?;
        }
    }

    result.notify_payload = None;
    Ok(result)
}

/// Publish an RxDB change notification on the collection's notify channel.
pub async fn publish_notification(
    msgbus: &dyn MessageBus,
    collection_name: &str,
    payload: Value,
) -> RiverbaseResult<()> {
    let channel = rxdb_notify_channel(collection_name)?;
    let envelope = json!({
        "channel": channel,
        "type": RXDB_NOTIFY_TYPE,
        "topic": collection_name,
        "payload": payload,
    });
    msgbus.publish(&channel, envelope).await
}
