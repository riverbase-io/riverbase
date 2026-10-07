use std::collections::HashMap;
use std::sync::Arc;

use aide::axum::{
    routing::{get_with, post_with},
    ApiRouter,
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use super::hook_token::{consume_hook_nonce, decode_hook_token, verify_hook_payload_hash};
use super::http_params::{HttpQueryParams, QueryItemPath, QueryScopedItemPath};
use super::link_token::decode_command_token;
use super::openapi::{
    default_command_payload_schema, domain_meta_operation_summary, riverbase_command_operation,
    riverbase_metadata_operation, riverbase_query_operation, with_command_payload_schema,
    with_command_response_schema, RiverbaseOperationKind, RiverbaseOperationMeta,
};
use super::rate_limit::check_hook_rate;
use super::routes::{
    command_exec_path, command_hook_path, command_key_meta_path, command_link_path,
    command_meta_path, command_post_path, query_item_path, query_list_path, query_meta_path,
    query_rept_path,
};
use super::scope::{
    build_aggroot_exec, build_collection_target, normalize_scope_path, query_request_with_scope,
};
use super::state::{CommandAppState, CoupledAppState, QueryAppState};
use crate::auth::{IdempotencyKey, Principal};
use crate::base::{
    command_envelope, command_success_meta, etag_from_data, quoted_etag, unquote_etag,
    uuid_from_json, EngineContext, RiverbaseError, RiverbaseResult, ScopeMeta, TenantAccessContext,
    TenantAccessKind, DEFAULT_RESPONSE_TYPE,
};
use crate::command::{CommandKind, CommandMeta, CommandTarget};
use crate::openapi_meta::OpenApiMeta;
use crate::query::{QueryAccess, QueryRequest, QueryResourceKind};
use crate::util::api_path::join_api_path;

/// HTTP command POST body: the JSON document is the command payload.
#[derive(Debug, Deserialize)]
pub struct CommandBody(Value);

/// OpenAPI-only JSON object schema for command responses (avoids `Value` → schema `true`).
struct OpenApiJsonObject;

impl JsonSchema for OpenApiJsonObject {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CommandResponse".into()
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "additionalProperties": true
        })
    }
}

impl JsonSchema for CommandBody {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CommandBody".into()
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "additionalProperties": true
        })
    }
}

impl CommandBody {
    /// Convert into payload.
    pub fn into_payload(self) -> Value {
        self.0
    }
}

/// Build command routes with one literal path per registered command (Riverbase `:exec` / `:post`).
pub async fn command_router(state: CommandAppState) -> RiverbaseResult<ApiRouter> {
    let namespace = state.command.namespace().to_string();
    let api_base = state.api_base.clone();
    let keys = state.command.commands().await?;

    let mut router: ApiRouter<CommandAppState> = ApiRouter::new();
    for key in keys {
        let cmdkey = Arc::<str>::from(key.as_str());
        let meta = state
            .command
            .command_meta(&key)
            .unwrap_or_else(|| CommandMeta::exec(key.as_str(), key.as_str()));
        if !meta.zone_allowed(&state.api_zone) {
            continue;
        }
        let payload_schema = state
            .command
            .command_info(&key)
            .and_then(|info| info.get("schema").cloned())
            .unwrap_or_else(default_command_payload_schema);
        let response_schema = state
            .command
            .command_info(&key)
            .and_then(|info| info.get("response_schema").cloned())
            .unwrap_or_else(crate::command::generic_object_schema);

        match meta.kind {
            CommandKind::Collection => {
                router = register_command_post_routes(
                    router,
                    &api_base,
                    &namespace,
                    cmdkey.clone(),
                    meta.clone(),
                    payload_schema,
                    response_schema,
                );
            }
            CommandKind::Object | CommandKind::ObjectLink | CommandKind::ObjectHook => {
                router = register_command_exec_routes(
                    router,
                    &api_base,
                    &namespace,
                    cmdkey.clone(),
                    meta.clone(),
                    payload_schema,
                    response_schema,
                );
            }
        }

        if meta.allow_link_method {
            router = register_command_link_routes(
                router,
                &api_base,
                &namespace,
                cmdkey.clone(),
                meta.clone(),
            );
        }
        if meta.allow_hook_method {
            router = register_command_hook_routes(
                router,
                &api_base,
                &namespace,
                cmdkey.clone(),
                meta.clone(),
            );
        }

        router = register_command_key_meta_route(router, &api_base, &namespace, cmdkey, meta);
    }

    let meta_namespace = namespace.clone();
    let domain_meta_summary =
        domain_meta_operation_summary(state.command.domain_meta().title.as_str());
    router = router.api_route(
        command_meta_path(&state.api_base, &namespace).as_str(),
        get_with(command_meta, move |op| {
            riverbase_metadata_operation(
                &meta_namespace,
                domain_meta_summary.as_str(),
                "List registered command keys for the namespace.",
                "get",
                op,
            )
            .response_with::<200, Json<Value>, _>(|res| {
                res.description("Command discovery document for the namespace.")
            })
        }),
    );

    Ok(router.with_state(state))
}

fn register_command_exec_routes(
    mut router: ApiRouter<CommandAppState>,
    api_base: &str,
    namespace: &str,
    cmdkey: Arc<str>,
    meta: CommandMeta,
    payload_schema: Value,
    response_schema: Value,
) -> ApiRouter<CommandAppState> {
    let scoped = meta.scope.required;
    let path = command_exec_path(api_base, namespace, &cmdkey, scoped);
    let scope_meta = meta.scope.clone();
    let exec_key = cmdkey.clone();

    if scoped {
        router = router.api_route(
            path.as_str(),
            post_with(
                move |State(state): State<CommandAppState>,
                      principal: Option<Extension<Principal>>,
                      idempotency: Option<Extension<IdempotencyKey>>,
                      headers: HeaderMap,
                      Path((scope, resource, identifier)): Path<(String, String, String)>,
                      body: Json<CommandBody>| {
                    let cmdkey = exec_key.clone();
                    let scope_meta = scope_meta.clone();
                    async move {
                        run_command_exec(
                            &state,
                            principal.map(|Extension(p)| p),
                            idempotency.map(|Extension(k)| k.0),
                            &headers,
                            &cmdkey,
                            &resource,
                            &identifier,
                            Some(scope.as_str()),
                            &scope_meta,
                            body,
                        )
                        .await
                    }
                },
                exec_operation(
                    namespace,
                    &cmdkey,
                    &meta,
                    true,
                    payload_schema.clone(),
                    response_schema.clone(),
                ),
            ),
        );
    } else {
        router = router.api_route(
            path.as_str(),
            post_with(
                move |State(state): State<CommandAppState>,
                      principal: Option<Extension<Principal>>,
                      idempotency: Option<Extension<IdempotencyKey>>,
                      headers: HeaderMap,
                      Path((resource, identifier)): Path<(String, String)>,
                      body: Json<CommandBody>| {
                    let cmdkey = exec_key.clone();
                    let scope_meta = scope_meta.clone();
                    async move {
                        run_command_exec(
                            &state,
                            principal.map(|Extension(p)| p),
                            idempotency.map(|Extension(k)| k.0),
                            &headers,
                            &cmdkey,
                            &resource,
                            &identifier,
                            None,
                            &scope_meta,
                            body,
                        )
                        .await
                    }
                },
                exec_operation(
                    namespace,
                    &cmdkey,
                    &meta,
                    false,
                    payload_schema,
                    response_schema,
                ),
            ),
        );
    }
    router
}

fn register_command_post_routes(
    mut router: ApiRouter<CommandAppState>,
    api_base: &str,
    namespace: &str,
    cmdkey: Arc<str>,
    meta: CommandMeta,
    payload_schema: Value,
    response_schema: Value,
) -> ApiRouter<CommandAppState> {
    let scoped = meta.scope.required;
    let path = command_post_path(api_base, namespace, &cmdkey, scoped);
    let scope_meta = meta.scope.clone();
    let post_key = cmdkey.clone();

    if scoped {
        router = router.api_route(
            path.as_str(),
            post_with(
                move |State(state): State<CommandAppState>,
                      principal: Option<Extension<Principal>>,
                      idempotency: Option<Extension<IdempotencyKey>>,
                      headers: HeaderMap,
                      Path((scope, resource)): Path<(String, String)>,
                      body: Json<CommandBody>| {
                    let cmdkey = post_key.clone();
                    let scope_meta = scope_meta.clone();
                    async move {
                        run_command_post(
                            &state,
                            principal.map(|Extension(p)| p),
                            idempotency.map(|Extension(k)| k.0),
                            &headers,
                            &cmdkey,
                            &resource,
                            Some(scope.as_str()),
                            &scope_meta,
                            body,
                        )
                        .await
                    }
                },
                post_operation(
                    namespace,
                    &cmdkey,
                    &meta,
                    true,
                    payload_schema.clone(),
                    response_schema.clone(),
                ),
            ),
        );
    } else {
        router = router.api_route(
            path.as_str(),
            post_with(
                move |State(state): State<CommandAppState>,
                      principal: Option<Extension<Principal>>,
                      idempotency: Option<Extension<IdempotencyKey>>,
                      headers: HeaderMap,
                      Path(resource): Path<String>,
                      body: Json<CommandBody>| {
                    let cmdkey = post_key.clone();
                    let scope_meta = scope_meta.clone();
                    async move {
                        run_command_post(
                            &state,
                            principal.map(|Extension(p)| p),
                            idempotency.map(|Extension(k)| k.0),
                            &headers,
                            &cmdkey,
                            &resource,
                            None,
                            &scope_meta,
                            body,
                        )
                        .await
                    }
                },
                post_operation(
                    namespace,
                    &cmdkey,
                    &meta,
                    false,
                    payload_schema,
                    response_schema,
                ),
            ),
        );
    }
    router
}

fn register_command_link_routes(
    router: ApiRouter<CommandAppState>,
    api_base: &str,
    namespace: &str,
    cmdkey: Arc<str>,
    meta: CommandMeta,
) -> ApiRouter<CommandAppState> {
    let path = command_link_path(api_base, namespace, &cmdkey);
    let scope_meta = meta.scope.clone();
    let link_key = cmdkey.clone();

    router.api_route(
        path.as_str(),
        get_with(
            move |State(state): State<CommandAppState>,
                  principal: Option<Extension<Principal>>,
                  Path(link_token): Path<String>| {
                let cmdkey = link_key.clone();
                let scope_meta = scope_meta.clone();
                async move {
                    run_command_link(
                        &state,
                        principal.map(|Extension(p)| p),
                        &cmdkey,
                        &link_token,
                        &scope_meta,
                    )
                    .await
                }
            },
            link_operation(namespace, &cmdkey, &meta),
        ),
    )
}

fn register_command_key_meta_route(
    router: ApiRouter<CommandAppState>,
    api_base: &str,
    namespace: &str,
    cmdkey: Arc<str>,
    meta: CommandMeta,
) -> ApiRouter<CommandAppState> {
    let path = command_key_meta_path(api_base, namespace, &cmdkey);
    let meta_key = cmdkey.clone();
    let op_cmdkey = cmdkey.to_string();

    router.api_route(
        path.as_str(),
        get_with(
            move |State(state): State<CommandAppState>| {
                let cmdkey = meta_key.clone();
                async move { run_command_key_meta(&state, &cmdkey).await }
            },
            key_meta_operation(namespace, &op_cmdkey, &meta),
        ),
    )
}

fn register_command_hook_routes(
    router: ApiRouter<CommandAppState>,
    api_base: &str,
    namespace: &str,
    cmdkey: Arc<str>,
    meta: CommandMeta,
) -> ApiRouter<CommandAppState> {
    let path = command_hook_path(api_base, namespace, &cmdkey);
    let scope_meta = meta.scope.clone();
    let hook_key = cmdkey.clone();

    router.api_route(
        path.as_str(),
        get_with(
            move |State(state): State<CommandAppState>,
                  Path(hook_token): Path<String>,
                  Query(params): Query<HashMap<String, String>>| {
                let cmdkey = hook_key.clone();
                let scope_meta = scope_meta.clone();
                async move {
                    run_command_hook(
                        &state,
                        &cmdkey,
                        &hook_token,
                        params,
                        &scope_meta,
                    )
                    .await
                }
            },
            hook_operation(namespace, &cmdkey, &meta),
        ),
    )
}

fn command_operation_summary(meta: &CommandMeta) -> String {
    let title = meta.title.trim();
    if title.is_empty() {
        meta.key.clone()
    } else {
        title.to_string()
    }
}

fn link_operation(
    namespace: &str,
    cmdkey: &str,
    meta: &CommandMeta,
) -> impl FnOnce(aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    let op_meta = RiverbaseOperationMeta::command(
        namespace,
        cmdkey,
        meta,
        RiverbaseOperationKind::CommandLink,
        meta.scope.required,
    );
    let summary = command_operation_summary(meta);
    let description =
        format!("Execute `{cmdkey}` via signed link token (`GET`, payload embedded in token).");
    move |op| {
        riverbase_command_operation(&op_meta, summary.as_str(), description.as_str(), "get", op)
            .response_with::<200, Json<OpenApiJsonObject>, _>(|res| {
                res.description("Command response JSON")
            })
    }
}

fn hook_operation(
    namespace: &str,
    cmdkey: &str,
    meta: &CommandMeta,
) -> impl FnOnce(aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    let op_meta = RiverbaseOperationMeta::command(
        namespace,
        cmdkey,
        meta,
        RiverbaseOperationKind::CommandHook,
        meta.scope.required,
    );
    let summary = command_operation_summary(meta);
    let description = format!(
        "Execute `{cmdkey}` via JWT-signed hook token (GET, payload from query parameters, no session auth)."
    );
    move |op| {
        riverbase_command_operation(&op_meta, summary.as_str(), description.as_str(), "get", op)
            .response_with::<200, Json<OpenApiJsonObject>, _>(|res| {
                res.description("Command response JSON")
            })
    }
}

fn exec_operation(
    namespace: &str,
    cmdkey: &str,
    meta: &CommandMeta,
    scoped: bool,
    payload_schema: Value,
    response_schema: Value,
) -> impl FnOnce(aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    let op_meta = RiverbaseOperationMeta::command(
        namespace,
        cmdkey,
        meta,
        RiverbaseOperationKind::CommandObject,
        scoped,
    );
    let summary = command_operation_summary(meta);
    let description = if scoped {
        format!("Execute `{cmdkey}` with `{{scope}}/{{resource}}/{{identifier}}`.")
    } else {
        format!("Execute `{cmdkey}` with `{{resource}}/{{identifier}}`.")
    };
    move |op| {
        with_command_response_schema(
            with_command_payload_schema(
                riverbase_command_operation(
                    &op_meta,
                    summary.as_str(),
                    description.as_str(),
                    "post",
                    op,
                )
                .response_with::<200, Json<OpenApiJsonObject>, _>(|res| {
                    res.description("Command response JSON")
                }),
                payload_schema,
            ),
            response_schema,
        )
    }
}

fn key_meta_operation(
    namespace: &str,
    cmdkey: &str,
    meta: &CommandMeta,
) -> impl FnOnce(aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    let op_meta = RiverbaseOperationMeta::command(
        namespace,
        cmdkey,
        meta,
        RiverbaseOperationKind::CommandMeta,
        false,
    );
    let summary = command_operation_summary(meta);
    let description = format!("Command info for `{cmdkey}` (payload schema and flags).");
    move |op| {
        riverbase_command_operation(&op_meta, summary.as_str(), description.as_str(), "get", op)
            .response_with::<200, Json<OpenApiJsonObject>, _>(|res| {
                res.description("Per-command info document")
            })
    }
}

fn post_operation(
    namespace: &str,
    cmdkey: &str,
    meta: &CommandMeta,
    scoped: bool,
    payload_schema: Value,
    response_schema: Value,
) -> impl FnOnce(aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    let op_meta = RiverbaseOperationMeta::command(
        namespace,
        cmdkey,
        meta,
        RiverbaseOperationKind::CommandResource,
        scoped,
    );
    let summary = command_operation_summary(meta);
    let description = if scoped {
        format!("Run collection command `{cmdkey}` with `{{scope}}/{{resource}}`.")
    } else {
        format!("Run collection command `{cmdkey}` with `{{resource}}`.")
    };
    let created = meta.resource_init();
    move |op| {
        let op = with_command_payload_schema(
            riverbase_command_operation(&op_meta, summary.as_str(), description.as_str(), "post", op)
                .response_with::<200, Json<OpenApiJsonObject>, _>(|res| {
                    res.description("Command response JSON")
                }),
            payload_schema,
        );
        let op = if created {
            op.response_with::<201, Json<OpenApiJsonObject>, _>(|res| {
                res.description("Resource created (collection / resource_init command)")
            })
        } else {
            op
        };
        with_command_response_schema(op, response_schema)
    }
}

/// Query router.
pub async fn query_router(state: QueryAppState) -> RiverbaseResult<ApiRouter> {
    let namespace = state.query.namespace().to_string();
    let api_base = state.api_base.clone();
    let resources = state.query.query_scope_metas().await?;

    let mut router: ApiRouter<QueryAppState> = ApiRouter::new();
    for route in resources {
        if !route.zone_allowed(&state.api_zone) {
            continue;
        }
        let resource = Arc::<str>::from(route.resource.as_str());
        let title = route.title.clone();
        let openapi = route.openapi.clone();
        match route.kind {
            QueryResourceKind::Query => {
                router = register_query_list_routes(
                    router,
                    &api_base,
                    &namespace,
                    resource.clone(),
                    route.scope.clone(),
                    title.as_str(),
                    openapi.clone(),
                );
                router = register_query_item_routes(
                    router,
                    &api_base,
                    &namespace,
                    resource.clone(),
                    route.scope.clone(),
                    title.as_str(),
                    openapi.clone(),
                );
            }
            QueryResourceKind::Report => {
                router = register_query_report_routes(
                    router,
                    &api_base,
                    &namespace,
                    resource.clone(),
                    route.scope.clone(),
                    title.as_str(),
                    openapi.clone(),
                );
            }
        }
        let meta_resource = resource.clone();
        router = router.api_route(
            query_meta_path(&api_base, &namespace, &resource).as_str(),
            get_with(
                move |State(state): State<QueryAppState>,
                      principal: Option<Extension<Principal>>,
                      headers: HeaderMap| {
                    let resource = meta_resource.clone();
                    async move {
                        run_query_meta(&state, principal.map(|Extension(p)| p), &headers, &resource)
                            .await
                    }
                },
                query_operation(
                    &namespace,
                    &resource,
                    title.as_str(),
                    RiverbaseOperationKind::QueryMeta,
                    false,
                    openapi,
                ),
            ),
        );
    }

    Ok(router.with_state(state))
}

fn register_query_list_routes(
    mut router: ApiRouter<QueryAppState>,
    api_base: &str,
    namespace: &str,
    resource: Arc<str>,
    scope_meta: ScopeMeta,
    title: &str,
    openapi: OpenApiMeta,
) -> ApiRouter<QueryAppState> {
    let scoped = scope_meta.required;
    let path = query_list_path(api_base, namespace, &resource, scoped);
    let scope_meta = scope_meta.clone();
    let list_resource = resource.clone();

    if scoped {
        router = router.api_route(
            path.as_str(),
            get_with(
                move |State(state): State<QueryAppState>,
                      principal: Option<Extension<Principal>>,
                      headers: HeaderMap,
                      Path(scope): Path<String>,
                      params: axum::extract::Query<HttpQueryParams>| {
                    let resource = list_resource.clone();
                    let scope_meta = scope_meta.clone();
                    async move {
                        run_query_list(
                            &state,
                            principal.map(|Extension(p)| p),
                            &headers,
                            &resource,
                            Some(scope.as_str()),
                            &scope_meta,
                            params,
                        )
                        .await
                    }
                },
                query_operation(
                    namespace,
                    &resource,
                    title,
                    RiverbaseOperationKind::QueryList,
                    true,
                    openapi.clone(),
                ),
            ),
        );
    } else {
        router = router.api_route(
            path.as_str(),
            get_with(
                move |State(state): State<QueryAppState>,
                      principal: Option<Extension<Principal>>,
                      headers: HeaderMap,
                      params: axum::extract::Query<HttpQueryParams>| {
                    let resource = list_resource.clone();
                    let scope_meta = scope_meta.clone();
                    async move {
                        run_query_list(
                            &state,
                            principal.map(|Extension(p)| p),
                            &headers,
                            &resource,
                            None,
                            &scope_meta,
                            params,
                        )
                        .await
                    }
                },
                query_operation(
                    namespace,
                    &resource,
                    title,
                    RiverbaseOperationKind::QueryList,
                    false,
                    openapi,
                ),
            ),
        );
    }
    router
}

fn register_query_item_routes(
    mut router: ApiRouter<QueryAppState>,
    api_base: &str,
    namespace: &str,
    resource: Arc<str>,
    scope_meta: ScopeMeta,
    title: &str,
    openapi: OpenApiMeta,
) -> ApiRouter<QueryAppState> {
    let scoped = scope_meta.required;
    let path = query_item_path(api_base, namespace, &resource, scoped);
    let scope_meta = scope_meta.clone();
    let item_resource = resource.clone();

    if scoped {
        router = router.api_route(
            path.as_str(),
            get_with(
                move |State(state): State<QueryAppState>,
                      principal: Option<Extension<Principal>>,
                      headers: HeaderMap,
                      Path(path): Path<QueryScopedItemPath>,
                      params: axum::extract::Query<HttpQueryParams>| {
                    let resource = item_resource.clone();
                    let scope_meta = scope_meta.clone();
                    async move {
                        run_query_item(
                            &state,
                            principal.map(|Extension(p)| p),
                            &headers,
                            &resource,
                            &path.identifier,
                            Some(path.scope.as_str()),
                            &scope_meta,
                            params,
                        )
                        .await
                    }
                },
                query_operation(
                    namespace,
                    &resource,
                    title,
                    RiverbaseOperationKind::QueryItem,
                    true,
                    openapi.clone(),
                ),
            ),
        );
    } else {
        router = router.api_route(
            path.as_str(),
            get_with(
                move |State(state): State<QueryAppState>,
                      principal: Option<Extension<Principal>>,
                      headers: HeaderMap,
                      Path(path): Path<QueryItemPath>,
                      params: axum::extract::Query<HttpQueryParams>| {
                    let resource = item_resource.clone();
                    let scope_meta = scope_meta.clone();
                    async move {
                        run_query_item(
                            &state,
                            principal.map(|Extension(p)| p),
                            &headers,
                            &resource,
                            &path.identifier,
                            None,
                            &scope_meta,
                            params,
                        )
                        .await
                    }
                },
                query_operation(
                    namespace,
                    &resource,
                    title,
                    RiverbaseOperationKind::QueryItem,
                    false,
                    openapi,
                ),
            ),
        );
    }
    router
}

fn register_query_report_routes(
    mut router: ApiRouter<QueryAppState>,
    api_base: &str,
    namespace: &str,
    resource: Arc<str>,
    scope_meta: ScopeMeta,
    title: &str,
    openapi: OpenApiMeta,
) -> ApiRouter<QueryAppState> {
    let scoped = scope_meta.required;
    let path = query_rept_path(api_base, namespace, &resource, scoped);
    let scope_meta = scope_meta.clone();
    let rept_resource = resource.clone();

    if scoped {
        router = router.api_route(
            path.as_str(),
            post_with(
                move |State(state): State<QueryAppState>,
                      principal: Option<Extension<Principal>>,
                      headers: HeaderMap,
                      Path(scope): Path<String>,
                      body: Json<CommandBody>| {
                    let resource = rept_resource.clone();
                    let scope_meta = scope_meta.clone();
                    async move {
                        run_query_report(
                            &state,
                            principal.map(|Extension(p)| p),
                            &headers,
                            &resource,
                            Some(scope.as_str()),
                            &scope_meta,
                            body,
                        )
                        .await
                    }
                },
                query_operation(
                    namespace,
                    &resource,
                    title,
                    RiverbaseOperationKind::QueryReport,
                    true,
                    openapi.clone(),
                ),
            ),
        );
    } else {
        router = router.api_route(
            path.as_str(),
            post_with(
                move |State(state): State<QueryAppState>,
                      principal: Option<Extension<Principal>>,
                      headers: HeaderMap,
                      body: Json<CommandBody>| {
                    let resource = rept_resource.clone();
                    let scope_meta = scope_meta.clone();
                    async move {
                        run_query_report(
                            &state,
                            principal.map(|Extension(p)| p),
                            &headers,
                            &resource,
                            None,
                            &scope_meta,
                            body,
                        )
                        .await
                    }
                },
                query_operation(
                    namespace,
                    &resource,
                    title,
                    RiverbaseOperationKind::QueryReport,
                    false,
                    openapi,
                ),
            ),
        );
    }
    router
}

fn query_operation_summary(title: &str, resource: &str) -> String {
    let t = title.trim();
    if t.is_empty() {
        resource.to_string()
    } else {
        t.to_string()
    }
}

fn query_operation(
    namespace: &str,
    resource: &str,
    title: &str,
    kind: RiverbaseOperationKind,
    scoped: bool,
    openapi: OpenApiMeta,
) -> impl FnOnce(aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    let method = match kind {
        RiverbaseOperationKind::QueryList => ".list",
        RiverbaseOperationKind::QueryItem => ".item",
        RiverbaseOperationKind::QueryMeta => ".meta",
        RiverbaseOperationKind::QueryReport => ".rept",
        _ => ".query",
    };
    let op_meta = RiverbaseOperationMeta::query(namespace, resource, kind, scoped, openapi);
    let summary = query_operation_summary(title, resource);
    let description = if scoped {
        format!("Query `{resource}`{method} (scoped).")
    } else {
        format!("Query `{resource}`{method}.")
    };
    let http_method = if kind == RiverbaseOperationKind::QueryReport {
        "post"
    } else {
        "get"
    };
    move |op| {
        riverbase_query_operation(
            &op_meta,
            summary.as_str(),
            description.as_str(),
            http_method,
            op,
        )
        .response_with::<200, Json<Value>, _>(|res| res.description("Query JSON result"))
    }
}

/// Coupled router.
pub async fn coupled_router(state: CoupledAppState) -> RiverbaseResult<ApiRouter> {
    let command = command_router(state.command).await?;
    let query = query_router(state.query).await?;
    Ok(ApiRouter::new().merge(command).merge(query))
}

/// Convert into router.
pub fn into_router<S>(router: ApiRouter<S>) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.into()
}

async fn run_command_exec(
    state: &CommandAppState,
    principal: Option<Principal>,
    idempotency_key: Option<String>,
    headers: &HeaderMap,
    cmdkey: &str,
    resource: &str,
    identifier: &str,
    scope: Option<&str>,
    scope_meta: &ScopeMeta,
    Json(body): Json<CommandBody>,
) -> Result<Response, Response> {
    let aggroot = build_aggroot_exec(resource, identifier, scope, scope_meta)
        .map_err(invalid_request_response)?;
    let mut payload = body.into_payload();
    let mut ctx = request_context(
        state.command.context(),
        principal.as_ref(),
        idempotency_key,
        device_token_from_headers(headers),
        Some(correlation_id_from_request_parts(headers)),
    )
    .map_err(riverbase_error_response)?;
    let if_match = inject_if_match(&mut ctx, &mut payload, headers);
    state
        .command
        .execute(&ctx, cmdkey, payload, CommandTarget::Object(aggroot))
        .await
        .map(|value| command_success_response(state, cmdkey, value, StatusCode::OK, None))
        .map_err(|err| riverbase_error_response_for_command(err, if_match))
}

async fn run_command_post(
    state: &CommandAppState,
    principal: Option<Principal>,
    idempotency_key: Option<String>,
    headers: &HeaderMap,
    cmdkey: &str,
    resource: &str,
    scope: Option<&str>,
    scope_meta: &ScopeMeta,
    Json(body): Json<CommandBody>,
) -> Result<Response, Response> {
    let target =
        build_collection_target(resource, scope, scope_meta).map_err(invalid_request_response)?;
    let ctx = request_context(
        state.command.context(),
        principal.as_ref(),
        idempotency_key,
        device_token_from_headers(headers),
        Some(correlation_id_from_request_parts(headers)),
    )
    .map_err(riverbase_error_response)?;
    let is_create = state
        .command
        .command_meta(cmdkey)
        .map(|meta| meta.resource_init())
        .unwrap_or(false);
    match state
        .command
        .execute(&ctx, cmdkey, body.into_payload(), target)
        .await
    {
        Ok(value) => {
            if is_create {
                let location = resource_created_location(
                    &state.api_base,
                    state.command.namespace(),
                    resource,
                    &value,
                );
                Ok(command_success_response(
                    state,
                    cmdkey,
                    value,
                    StatusCode::CREATED,
                    location,
                ))
            } else {
                Ok(command_success_response(
                    state,
                    cmdkey,
                    value,
                    StatusCode::OK,
                    None,
                ))
            }
        }
        Err(err) => Err(riverbase_error_response(err)),
    }
}

async fn run_command_link(
    state: &CommandAppState,
    principal: Option<Principal>,
    cmdkey: &str,
    link_token: &str,
    scope_meta: &ScopeMeta,
) -> Result<Response, Response> {
    let token =
        decode_command_token(link_token, &state.link_token).map_err(riverbase_error_response)?;
    let scope = token.scope.as_deref().and_then(normalize_scope_path);
    let payload = token.payload.unwrap_or_else(|| json!({}));
    let aggroot = build_aggroot_exec(&token.resource, &token.identifier, scope, scope_meta)
        .map_err(invalid_request_response)?;
    let ctx = request_context(
        state.command.context(),
        principal.as_ref(),
        None,
        None,
        Some(correlation_id_from_optional_headers(None)),
    )
    .map_err(riverbase_error_response)?;
    state
        .command
        .execute(&ctx, cmdkey, payload, CommandTarget::Object(aggroot))
        .await
        .map(|value| command_success_response(state, cmdkey, value, StatusCode::OK, None))
        .map_err(riverbase_error_response)
}

async fn run_command_hook(
    state: &CommandAppState,
    cmdkey: &str,
    hook_token: &str,
    params: HashMap<String, String>,
    scope_meta: &ScopeMeta,
) -> Result<Response, Response> {
    check_hook_rate(&format!("hook:{cmdkey}")).map_err(riverbase_error_response)?;
    let claims =
        decode_hook_token(hook_token, &state.hook_token).map_err(riverbase_error_response)?;
    if claims.cmdkey != cmdkey {
        return Err(invalid_request_response(
            "Hook token command key does not match route.".to_string(),
        ));
    }
    consume_hook_nonce(&claims.nonce, claims.expires_at).map_err(riverbase_error_response)?;
    verify_hook_payload_hash(&claims, &params).map_err(riverbase_error_response)?;
    let scope = claims.scope.as_deref().and_then(normalize_scope_path);
    let payload = serde_json::to_value(&params).unwrap_or_else(|_| json!({}));
    let aggroot = build_aggroot_exec(&claims.resource, &claims.identifier, scope, scope_meta)
        .map_err(invalid_request_response)?;
    let mut claims_json = json!({ "hook": true, "cmdkey": cmdkey });
    if let Some(tenant) = claims.tenant {
        if let Some(obj) = claims_json.as_object_mut() {
            obj.insert("_tenant".into(), json!(tenant.to_string()));
        }
    }
    let principal = Principal {
        sub: format!("hook:{cmdkey}"),
        preferred_username: Some(format!("hook:{cmdkey}")),
        email: None,
        roles: vec!["hook".into()],
        iam_roles: vec![],
        claims: claims_json,
    };
    let ctx = request_context(
        state.command.context(),
        Some(&principal),
        None,
        None,
        Some(correlation_id_from_optional_headers(None)),
    )
    .map_err(riverbase_error_response)?;
    state
        .command
        .execute(&ctx, cmdkey, payload, CommandTarget::Object(aggroot))
        .await
        .map(|value| command_success_response(state, cmdkey, value, StatusCode::OK, None))
        .map_err(riverbase_error_response)
}

/// Merge request principal into the domain engine context for command/query execution.
fn request_context(
    base: &EngineContext,
    principal: Option<&Principal>,
    idempotency_key: Option<String>,
    device_token: Option<String>,
    correlation_id: Option<String>,
) -> RiverbaseResult<EngineContext> {
    let mut ctx = base.clone();
    if let Some(p) = principal {
        ctx.actor = p.to_audit_actor(None);
        ctx.profile_id = ctx.actor.profile_id;
        ctx.user_id = ctx.actor.user_id;
        ctx.roles = p.roles.clone();
        if ctx.roles.is_empty() {
            ctx.roles = p.iam_roles.clone();
        }
        if let Some(claims) = p.claims.as_object() {
            ctx.set_jwt_claims(claims);
        }
        if let Some(org) = p
            .claims
            .get("organization_id")
            .and_then(uuid_from_json)
            .or_else(|| p.claims.get("org_id").and_then(uuid_from_json))
        {
            ctx.organization_id = Some(org);
            ctx.set_claim("organization_id", org.to_string());
        }
    }
    ctx.idempotency_key = idempotency_key;
    if let Some(token) = device_token.filter(|t| !t.is_empty()) {
        ctx.set_claim("device_token", token);
    }
    if let Some(id) = correlation_id.filter(|value| !value.is_empty()) {
        ctx.correlation_id = Some(id);
    }
    if let Some(policies) = ctx.tenant_policies.clone() {
        let has_identity = principal.is_some()
            || ctx.organization_id.is_some()
            || ctx.profile_id.is_some()
            || ctx.user_id.is_some();
        if has_identity {
            let namespace = ctx.namespace().to_string();
            policies.apply(
                &mut ctx,
                &TenantAccessContext {
                    kind: TenantAccessKind::Query,
                    namespace: &namespace,
                    resource: "",
                },
            )?;
        } else if ctx.tenant().is_none() {
            return Err(crate::errors::AUT_173.with_data(serde_json::json!({})));
        }
    } else if let Some(p) = principal {
        let tenant = p.tenant();
        if tenant.is_none() {
            return Err(crate::errors::AUT_173.with_data(serde_json::json!({})));
        }
        if let Some(tenant) = tenant {
            ctx.set_tenant_id(tenant);
        }
    }
    if let Some(tenant) = ctx.tenant() {
        ctx.set_claim("_tenant", tenant.to_string());
    }
    if ctx.actor.tenant.is_none() {
        return Err(crate::errors::AUT_173.with_data(serde_json::json!({})));
    }
    Ok(ctx)
}

fn correlation_id_from_request_parts(headers: &HeaderMap) -> String {
    headers
        .get(super::correlation::REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn correlation_id_from_optional_headers(headers: Option<&HeaderMap>) -> String {
    headers
        .map(correlation_id_from_request_parts)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn device_token_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("x-device-token").and_then(|v| v.to_str().ok()) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(value) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        let trimmed = value.trim();
        if let Some(rest) = trimmed.strip_prefix("Device ") {
            if let Some((_id, token)) = rest.split_once(':') {
                let token = token.trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }
    None
}

async fn run_command_key_meta(
    state: &CommandAppState,
    cmdkey: &str,
) -> Result<Json<Value>, axum::response::Response> {
    let doc = state.command.command_info(cmdkey).ok_or_else(|| {
        riverbase_core::errors::CMD_018
            .with_data(json!({ "command": cmdkey }))
            .into_response()
    })?;
    Ok(Json(doc))
}

async fn command_meta(
    State(state): State<CommandAppState>,
) -> Result<Json<Value>, axum::response::Response> {
    let commands = state
        .command
        .commands()
        .await
        .map_err(riverbase_error_response)?;
    Ok(Json(json!({
        "namespace": state.command.namespace(),
        "commands": commands,
    })))
}

async fn run_query_list(
    state: &QueryAppState,
    principal: Option<Principal>,
    headers: &HeaderMap,
    resource: &str,
    scope: Option<&str>,
    scope_meta: &ScopeMeta,
    params: axum::extract::Query<HttpQueryParams>,
) -> Result<Json<Value>, axum::response::Response> {
    let request = params
        .0
        .into_frontend_query()
        .map_err(riverbase_error_response)?;
    let request =
        query_request_with_scope(request, scope, scope_meta).map_err(invalid_request_response)?;
    let ctx = request_context(
        state.query.context(),
        principal.as_ref(),
        None,
        None,
        Some(correlation_id_from_request_parts(headers)),
    )
    .map_err(riverbase_error_response)?;
    state
        .query
        .execute(&ctx, resource, QueryAccess::List, request, None)
        .await
        .map(Json)
        .map_err(riverbase_error_response)
}

async fn run_query_item(
    state: &QueryAppState,
    principal: Option<Principal>,
    headers: &HeaderMap,
    resource: &str,
    identifier: &str,
    scope: Option<&str>,
    scope_meta: &ScopeMeta,
    params: axum::extract::Query<HttpQueryParams>,
) -> Result<Response, Response> {
    let request = params
        .0
        .into_frontend_query()
        .map_err(riverbase_error_response)?;
    let request =
        query_request_with_scope(request, scope, scope_meta).map_err(invalid_request_response)?;
    let ctx = request_context(
        state.query.context(),
        principal.as_ref(),
        None,
        None,
        Some(correlation_id_from_request_parts(headers)),
    )
    .map_err(riverbase_error_response)?;
    state
        .query
        .execute(&ctx, resource, QueryAccess::Item, request, Some(identifier))
        .await
        .map(|value| {
            // Engine already applied the success envelope ([API-01]).
            let etag = etag_from_data(&value);
            let mut response = Json(value).into_response();
            if let Some(etag) = etag {
                if let Ok(header_value) = HeaderValue::from_str(&quoted_etag(&etag)) {
                    response.headers_mut().insert(header::ETAG, header_value);
                }
            }
            response
        })
        .map_err(riverbase_error_response)
}

async fn run_query_report(
    state: &QueryAppState,
    principal: Option<Principal>,
    headers: &HeaderMap,
    resource: &str,
    scope: Option<&str>,
    scope_meta: &ScopeMeta,
    Json(body): Json<CommandBody>,
) -> Result<Json<Value>, axum::response::Response> {
    let request: QueryRequest = serde_json::from_value(body.into_payload())
        .map_err(|e| invalid_request_response(format!("Invalid report query body: {e}")))?;
    let request =
        query_request_with_scope(request, scope, scope_meta).map_err(invalid_request_response)?;
    let ctx = request_context(
        state.query.context(),
        principal.as_ref(),
        None,
        None,
        Some(correlation_id_from_request_parts(headers)),
    )
    .map_err(riverbase_error_response)?;
    state
        .query
        .execute(&ctx, resource, QueryAccess::Report, request, None)
        .await
        .map(Json)
        .map_err(riverbase_error_response)
}

async fn run_query_meta(
    state: &QueryAppState,
    principal: Option<Principal>,
    headers: &HeaderMap,
    resource: &str,
) -> Result<Json<Value>, axum::response::Response> {
    let ctx = request_context(
        state.query.context(),
        principal.as_ref(),
        None,
        None,
        Some(correlation_id_from_request_parts(headers)),
    )
    .map_err(riverbase_error_response)?;
    state
        .query
        .execute(&ctx, resource, QueryAccess::Meta, Default::default(), None)
        .await
        .map(Json)
        .map_err(riverbase_error_response)
}

fn riverbase_error_response(err: RiverbaseError) -> Response {
    crate::http_response::error_into_response(err)
}

fn riverbase_error_response_for_command(err: RiverbaseError, _if_match_present: bool) -> Response {
    riverbase_error_response(err)
}

fn invalid_request_response(detail: String) -> Response {
    crate::http_response::error_into_response(riverbase_core::errors::WEB_001.with_data(detail))
}

fn command_success_response(
    state: &CommandAppState,
    cmdkey: &str,
    value: Value,
    status: StatusCode,
    location: Option<String>,
) -> Response {
    let response_type = state
        .command
        .command_meta(cmdkey)
        .map(|meta| meta.response_type.clone())
        .unwrap_or_else(|| DEFAULT_RESPONSE_TYPE.to_string());
    let meta = command_success_meta(cmdkey, &value);
    let body = command_envelope(&response_type, value, meta);
    let etag = etag_from_data(response_data_for_etag(&body));
    let mut response = (status, Json(body)).into_response();
    if let Some(etag) = etag {
        if let Ok(header_value) = HeaderValue::from_str(&quoted_etag(&etag)) {
            response.headers_mut().insert(header::ETAG, header_value);
        }
    }
    if let Some(location) = location {
        if let Ok(header_value) = HeaderValue::from_str(&location) {
            response
                .headers_mut()
                .insert(header::LOCATION, header_value);
        }
    }
    response
}

fn response_data_for_etag(body: &Value) -> &Value {
    body.get("data").unwrap_or(body)
}

fn resource_created_location(
    api_base: &str,
    namespace: &str,
    resource: &str,
    value: &Value,
) -> Option<String> {
    let id = value
        .get("id")
        .or_else(|| {
            value
                .as_object()
                .and_then(|obj| obj.values().next())
                .and_then(|inner| inner.get("id"))
        })
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            other => other.as_str().map(str::to_string).or_else(|| {
                if other.is_null() {
                    None
                } else {
                    Some(other.to_string().trim_matches('"').to_string())
                }
            }),
        })?;
    Some(join_api_path(
        api_base,
        &format!("/{namespace}/{resource}.item/{id}"),
    ))
}

/// Inject `If-Match` into engine claims for aggregate CAS ([API-06]).
///
/// Claims only — do not mutate the command JSON body (payloads may enable
/// `deny_unknown_fields` and reject framework-injected keys with 422 `WEB-001`).
fn inject_if_match(ctx: &mut EngineContext, payload: &mut Value, headers: &HeaderMap) -> bool {
    let _ = payload;
    let Some(raw) = headers.get(header::IF_MATCH).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let etag = unquote_etag(raw);
    if etag.is_empty() {
        return false;
    }
    ctx.set_claim("_if_match", etag);
    ctx.set_claim("_etag", etag);
    true
}

#[cfg(test)]
mod if_match_tests {
    use super::*;
    use crate::base::EngineContext;
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;

    #[test]
    fn inject_if_match_sets_claims_without_mutating_body() {
        let mut ctx = EngineContext::new("test");
        let mut payload = json!({ "title": "Bolts" });
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"etag-1\""));
        assert!(inject_if_match(&mut ctx, &mut payload, &headers));
        assert_eq!(
            ctx.claims.get("_if_match").and_then(|v| v.as_str()),
            Some("etag-1")
        );
        assert_eq!(payload, json!({ "title": "Bolts" }));
    }
}

#[cfg(test)]
mod zone_tests {
    use super::*;
    use crate::base::{EngineContext, ScopeMeta};
    use crate::command::CommandMeta;
    use crate::domain::{DomainCommandEngine, DomainQueryEngine};
    use crate::openapi_meta::OpenApiMeta;
    use crate::query::{QueryAccess, QueryRequest, QueryResourceKind, QueryRouteMeta};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;

    struct MockCommandEngine {
        ctx: EngineContext,
        metas: HashMap<String, CommandMeta>,
    }

    #[async_trait]
    impl DomainCommandEngine for MockCommandEngine {
        fn context(&self) -> &EngineContext {
            &self.ctx
        }

        async fn commands(&self) -> RiverbaseResult<Vec<String>> {
            Ok(self.metas.keys().cloned().collect())
        }

        fn command_meta(&self, cmdkey: &str) -> Option<CommandMeta> {
            self.metas.get(cmdkey).cloned()
        }

        async fn execute(
            &self,
            _ctx: &EngineContext,
            _cmdkey: &str,
            _payload: Value,
            _target: CommandTarget,
        ) -> RiverbaseResult<Value> {
            Ok(json!({}))
        }
    }

    struct MockQueryEngine {
        ctx: EngineContext,
        routes: Vec<QueryRouteMeta>,
    }

    #[async_trait]
    impl DomainQueryEngine for MockQueryEngine {
        fn context(&self) -> &EngineContext {
            &self.ctx
        }

        async fn queries(&self) -> RiverbaseResult<Vec<String>> {
            Ok(self.routes.iter().map(|r| r.resource.clone()).collect())
        }

        async fn query_scope_metas(&self) -> RiverbaseResult<Vec<QueryRouteMeta>> {
            Ok(self.routes.clone())
        }

        async fn execute(
            &self,
            _ctx: &EngineContext,
            _resource: &str,
            _access: QueryAccess,
            _request: QueryRequest,
            _item_id: Option<&str>,
        ) -> RiverbaseResult<Value> {
            Ok(json!({}))
        }
    }

    fn openapi_paths<S>(router: ApiRouter<S>) -> Vec<String>
    where
        S: Clone + Send + Sync + 'static,
    {
        let mut api = aide::openapi::OpenApi::default();
        let _ = router.finish_api(&mut api);
        api.paths
            .map(|paths| paths.paths.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn command_router_skips_zone_restricted_commands() {
        let namespace = "test.domain";
        let mut metas = HashMap::new();
        metas.insert(
            "open-cmd".into(),
            CommandMeta::collection("open-cmd", "Open").with_resources(["item"]),
        );
        metas.insert(
            "seller-cmd".into(),
            CommandMeta::collection("seller-cmd", "Seller")
                .with_resources(["item"])
                .with_allowed_zones(["seller"]),
        );

        let mut state = CommandAppState::with_api_base(
            Arc::new(MockCommandEngine {
                ctx: EngineContext::new(namespace),
                metas,
            }),
            "/api",
        );
        state.api_zone = Arc::new(vec!["coordinator".into()]);

        let paths = openapi_paths(command_router(state).await.expect("router"));
        assert!(paths.iter().any(|p| p.contains("open-cmd:post")));
        assert!(!paths.iter().any(|p| p.contains("seller-cmd:post")));
    }

    #[tokio::test]
    async fn query_router_skips_zone_restricted_resources() {
        let namespace = "test.domain";
        let routes = vec![
            QueryRouteMeta {
                resource: "open-item".into(),
                scope: ScopeMeta::none(),
                title: "Open".into(),
                openapi: OpenApiMeta::default(),
                kind: QueryResourceKind::Query,
                allowed_zones: Vec::new(),
            },
            QueryRouteMeta {
                resource: "seller-item".into(),
                scope: ScopeMeta::none(),
                title: "Seller".into(),
                openapi: OpenApiMeta::default(),
                kind: QueryResourceKind::Query,
                allowed_zones: vec!["seller".into()],
            },
        ];

        let mut state = QueryAppState::with_api_base(
            Arc::new(MockQueryEngine {
                ctx: EngineContext::new(namespace),
                routes,
            }),
            "/api",
        );
        state.api_zone = Arc::new(vec!["coordinator".into()]);

        let paths = openapi_paths(query_router(state).await.expect("router"));
        assert!(paths.iter().any(|p| p.contains("open-item.list")));
        assert!(!paths.iter().any(|p| p.contains("seller-item.list")));
    }
}

#[cfg(test)]
mod request_context_tests {
    use super::*;
    use uuid::Uuid;

    fn principal_with_claims(claims: Value) -> Principal {
        Principal {
            sub: "user".into(),
            preferred_username: None,
            email: None,
            roles: vec![],
            iam_roles: vec![],
            claims,
        }
    }

    #[test]
    fn missing_principal_without_base_tenant_is_aut_173() {
        let err = request_context(&EngineContext::new("test"), None, None, None, None)
            .expect_err("tenant required");
        assert_eq!(err.errcode.as_str(), "AUT-173");
    }

    #[test]
    fn principal_without_tenant_is_aut_173() {
        let err = request_context(
            &EngineContext::new("test"),
            Some(&principal_with_claims(json!({}))),
            None,
            None,
            None,
        )
        .expect_err("tenant required");
        assert_eq!(err.errcode.as_str(), "AUT-173");
    }

    #[test]
    fn non_uuid_tenant_claim_is_aut_173() {
        let err = request_context(
            &EngineContext::new("test"),
            Some(&principal_with_claims(json!({ "_tenant": "not-a-uuid" }))),
            None,
            None,
            None,
        )
        .expect_err("invalid tenant");
        assert_eq!(err.errcode.as_str(), "AUT-173");
    }

    #[test]
    fn principal_tenant_is_copied() {
        let tenant = Uuid::new_v4();
        let ctx = request_context(
            &EngineContext::new("test"),
            Some(&principal_with_claims(
                json!({ "_tenant": tenant.to_string() }),
            )),
            None,
            None,
            None,
        )
        .expect("ok");
        assert_eq!(ctx.tenant(), Some(tenant));
        assert_eq!(
            ctx.claim_str("_tenant"),
            Some(tenant.to_string()).as_deref()
        );
    }

    #[test]
    fn policies_stamp_and_access_from_organization() {
        let org = Uuid::new_v4();
        let mut base = EngineContext::new("test");
        base.tenant_policies = Some(crate::base::TenantPolicyResolver::from_defaults());
        let ctx = request_context(
            &base,
            Some(&principal_with_claims(json!({
                "organization_id": org.to_string()
            }))),
            None,
            None,
            None,
        )
        .expect("ok");
        assert_eq!(ctx.tenant(), Some(org));
        assert_eq!(
            ctx.tenant_access,
            crate::base::TenantAccess::tenants(vec![org])
        );
    }

    #[test]
    fn system_access_keeps_org_stamp() {
        let org = Uuid::new_v4();
        let mut base = EngineContext::new("test");
        base.tenant_policies = Some(crate::base::TenantPolicyResolver::new(
            crate::base::ACCESS_SYSTEM,
            crate::base::STAMP_PROFILE_ORGANIZATION,
        ));
        let ctx = request_context(
            &base,
            Some(&principal_with_claims(json!({
                "organization_id": org.to_string()
            }))),
            None,
            None,
            None,
        )
        .expect("ok");
        assert_eq!(ctx.tenant(), Some(org));
        assert!(ctx.tenant_access.is_all());
    }
}
