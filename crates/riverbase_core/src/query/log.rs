use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::base::{EngineContext, RiverbaseResult};
use crate::logstore::{
    append_query_context, new_log_id, parse_optional_uuid, DomainLogStore, LogRowMeta,
    QueryLogRecord, QueryLogStatus,
};

use super::primitives::QueryRequest;
use super::resource::QueryAccess;

fn scope_uuid_from_value(scope: Option<&Value>, key: &str) -> Option<Uuid> {
    scope
        .and_then(Value::as_object)
        .and_then(|obj| obj.get(key))
        .and_then(Value::as_str)
        .and_then(parse_optional_uuid)
}

fn query_access_label(access: QueryAccess) -> &'static str {
    match access {
        QueryAccess::List => "list",
        QueryAccess::Item => "item",
        QueryAccess::Meta => "meta",
        QueryAccess::Report => "rept",
    }
}

fn query_request_json(request: &QueryRequest) -> Value {
    json!({
        "limit": request.limit,
        "page": request.page,
        "include": request.include,
        "exclude": request.exclude,
        "sort": request.sort,
        "query": request.user_query,
        "qbase": request.base_query,
        "scope": request.scope,
        "text": request.text,
    })
}

/// Append context + query audit rows when the corresponding channels are enabled.
pub async fn append_query_log(
    logstore: &DomainLogStore,
    ctx: &EngineContext,
    query_resource: &str,
    access: QueryAccess,
    request: &QueryRequest,
    item_id: Option<&str>,
    status: QueryLogStatus,
    result_count: Option<i32>,
    error_code: Option<String>,
    context_id: Option<Uuid>,
) -> RiverbaseResult<()> {
    let identifier = item_id.and_then(parse_optional_uuid);
    let access_label = query_access_label(access);
    let request_json = query_request_json(request);
    let context_id = context_id.unwrap_or_else(Uuid::new_v4);

    append_query_context(
        logstore,
        context_id,
        ctx,
        query_resource,
        access_label,
        &request_json,
        item_id,
    )
    .await?;

    logstore
        .queries
        .append(
            None,
            QueryLogRecord {
                meta: LogRowMeta {
                    id: new_log_id(),
                    created: Utc::now(),
                    creator: ctx.actor.profile_id,
                },
                domain: ctx.namespace.clone(),
                resource: query_resource.to_string(),
                access: access_label.to_string(),
                identifier,
                domain_sid: scope_uuid_from_value(request.scope.as_ref(), "domain_sid"),
                domain_iid: scope_uuid_from_value(request.scope.as_ref(), "domain_iid"),
                request: request_json,
                context: context_id,
                status,
                result_count,
                error_code,
                tenant: ctx.actor.tenant,
            },
        )
        .await
}
