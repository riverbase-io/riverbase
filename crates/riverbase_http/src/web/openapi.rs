//! OpenAPI document generation via [aide](https://docs.rs/aide/latest/aide/openapi/index.html).

use std::sync::Arc;

use aide::{
    axum::{routing::get_with, ApiRouter, IntoApiResponse},
    openapi::{Info, MediaType, OpenApi, ReferenceOr, RequestBody, SchemaObject, Tag},
    transform::TransformOperation,
};
#[cfg(feature = "auth")]
use axum::extract::Request;
#[cfg(feature = "auth")]
use axum::{
    extract::Extension,
    response::{IntoResponse, Redirect},
    routing::get as axum_get,
    Json,
};
use indexmap::IndexMap;
use schemars::{json_schema, Schema};
use serde_json::{json, Value};

use crate::base::{success_envelope, ProblemDetails, API_CONTRACT_VERSION};
use crate::command::generic_object_schema;
use crate::command::CommandMeta;
use crate::openapi_meta::OpenApiMeta;

/// OpenAPI JSON document path (also the redirect target from `/`).
use super::api_path::{join_api_path, openapi_json_path};

/// Re-export for OpenAPI `info.version` and `api.info`.
pub use crate::base::API_CONTRACT_VERSION as OPENAPI_CONTRACT_VERSION;

/// Default OpenAPI JSON path when [`DEFAULT_API_BASE`] (`/api`) is used.
pub const OPENAPI_JSON_PATH: &str = "/api/openapi.json";

/// Riverbase `x-api-kind` category:method segment for coupled domain routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiverbaseOperationKind {
    /// Command object.
    CommandObject,
    /// Command resource.
    CommandResource,
    /// Command hook.
    CommandHook,
    /// Command link.
    CommandLink,
    /// Command meta.
    CommandMeta,
    /// Domain meta.
    DomainMeta,
    /// Query list.
    QueryList,
    /// Query item.
    QueryItem,
    /// Query meta.
    QueryMeta,
    /// Query report.
    QueryReport,
    /// Generic get.
    GenericGet,
    /// Generic post.
    GenericPost,
    /// Generic put.
    GenericPut,
    /// Generic patch.
    GenericPatch,
    /// Generic delete.
    GenericDelete,
    /// Generic options.
    GenericOptions,
    /// Generic head.
    GenericHead,
    /// Realtime websocket.
    RealtimeWebsocket,
    /// Realtime sse stream.
    RealtimeSseStream,
}

impl RiverbaseOperationKind {
    /// X kind.
    pub fn x_kind(self) -> &'static str {
        match self {
            Self::CommandObject => "command:object",
            Self::CommandResource => "command:resource",
            Self::CommandHook => "command:hook",
            Self::CommandLink => "command:link",
            Self::CommandMeta => "command:meta",
            Self::DomainMeta => "domain:meta",
            Self::QueryList => "query:list",
            Self::QueryItem => "query:item",
            Self::QueryMeta => "query:meta",
            Self::QueryReport => "query:rept",
            Self::GenericGet => "generic:get",
            Self::GenericPost => "generic:post",
            Self::GenericPut => "generic:put",
            Self::GenericPatch => "generic:patch",
            Self::GenericDelete => "generic:delete",
            Self::GenericOptions => "generic:options",
            Self::GenericHead => "generic:head",
            Self::RealtimeWebsocket => "realtime:websocket",
            Self::RealtimeSseStream => "realtime:ssestream",
        }
    }

    /// Build from http method.
    pub fn from_http_method(method: &str) -> Option<Self> {
        match method.to_ascii_lowercase().as_str() {
            "get" => Some(Self::GenericGet),
            "post" => Some(Self::GenericPost),
            "put" => Some(Self::GenericPut),
            "patch" => Some(Self::GenericPatch),
            "delete" => Some(Self::GenericDelete),
            "options" => Some(Self::GenericOptions),
            "head" => Some(Self::GenericHead),
            _ => None,
        }
    }

    /// Whether this is command.
    pub fn is_command(self) -> bool {
        matches!(
            self,
            Self::CommandObject
                | Self::CommandResource
                | Self::CommandHook
                | Self::CommandLink
                | Self::CommandMeta
        )
    }

    /// Whether this is query.
    pub fn is_query(self) -> bool {
        matches!(
            self,
            Self::QueryList | Self::QueryItem | Self::QueryMeta | Self::QueryReport
        )
    }

    /// Whether this is generic.
    pub fn is_generic(self) -> bool {
        matches!(
            self,
            Self::GenericGet
                | Self::GenericPost
                | Self::GenericPut
                | Self::GenericPatch
                | Self::GenericDelete
                | Self::GenericOptions
                | Self::GenericHead
        )
    }

    /// Whether this is realtime.
    pub fn is_realtime(self) -> bool {
        matches!(self, Self::RealtimeWebsocket | Self::RealtimeSseStream)
    }

    /// Operation id slug.
    pub fn operation_id_slug(self) -> &'static str {
        match self {
            Self::CommandObject => "command_object",
            Self::CommandResource => "command_resource",
            Self::CommandHook => "command_hook",
            Self::CommandLink => "command_link",
            Self::CommandMeta => "command_meta",
            Self::DomainMeta => "domain_meta",
            Self::QueryList => "query_list",
            Self::QueryItem => "query_item",
            Self::QueryMeta => "query_meta",
            Self::QueryReport => "query_rept",
            Self::GenericGet => "generic_get",
            Self::GenericPost => "generic_post",
            Self::GenericPut => "generic_put",
            Self::GenericPatch => "generic_patch",
            Self::GenericDelete => "generic_delete",
            Self::GenericOptions => "generic_options",
            Self::GenericHead => "generic_head",
            Self::RealtimeWebsocket => "realtime_websocket",
            Self::RealtimeSseStream => "realtime_ssestream",
        }
    }
}

/// Metadata stamped onto each Riverbase HTTP operation for api-explorer grouping.
#[derive(Debug, Clone)]
pub struct RiverbaseOperationMeta {
    /// Namespace.
    pub namespace: String,
    /// Command key or query resource name.
    pub segment: String,
    /// Kind.
    pub kind: RiverbaseOperationKind,
    /// Scoped.
    pub scoped: bool,
    /// Openapi.
    pub openapi: OpenApiMeta,
    /// Command key.
    pub command_key: Option<String>,
    /// Resources.
    pub resources: Vec<String>,
}

impl RiverbaseOperationMeta {
    /// Command.
    pub fn command(
        namespace: impl Into<String>,
        cmdkey: impl Into<String>,
        meta: &CommandMeta,
        kind: RiverbaseOperationKind,
        scoped: bool,
    ) -> Self {
        let cmdkey = cmdkey.into();
        Self {
            namespace: namespace.into(),
            segment: cmdkey.clone(),
            kind,
            scoped,
            openapi: meta.openapi.clone(),
            command_key: Some(cmdkey),
            resources: meta.resources.clone(),
        }
    }

    /// Query.
    pub fn query(
        namespace: impl Into<String>,
        resource: impl Into<String>,
        kind: RiverbaseOperationKind,
        scoped: bool,
        openapi: OpenApiMeta,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            segment: resource.into(),
            kind,
            scoped,
            openapi,
            command_key: None,
            resources: Vec::new(),
        }
    }

    /// Domain meta.
    pub fn domain_meta(namespace: impl Into<String>) -> Self {
        let namespace = namespace.into();
        Self {
            segment: namespace.clone(),
            namespace,
            kind: RiverbaseOperationKind::DomainMeta,
            scoped: false,
            openapi: OpenApiMeta::default(),
            command_key: None,
            resources: Vec::new(),
        }
    }

    /// Realtime.
    pub fn realtime(
        namespace: impl Into<String>,
        segment: impl Into<String>,
        kind: RiverbaseOperationKind,
        openapi: OpenApiMeta,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            segment: segment.into(),
            kind,
            scoped: false,
            openapi,
            command_key: None,
            resources: Vec::new(),
        }
    }

    /// X api.
    pub fn x_api(&self) -> String {
        if self.kind == RiverbaseOperationKind::DomainMeta {
            return self.namespace.clone();
        }
        if self.segment.is_empty() {
            return self.namespace.clone();
        }
        riverbase_api_name(&self.namespace, &self.segment)
    }

    /// Domain tag.
    pub fn domain_tag(&self) -> String {
        format!("domain:{}", self.namespace)
    }

    /// Queryset tag.
    pub fn queryset_tag(&self) -> String {
        format!("queryset:{}", self.namespace)
    }
}

/// Stable `{namespace}/{segment}` API entry name (matches Python `fq_name`).
pub fn riverbase_api_name(namespace: &str, segment: &str) -> String {
    format!("{namespace}/{segment}")
}

/// Convert a JSON Schema [`Value`] into a schemars [`Schema`] for OpenAPI emission.
pub fn json_schema_from_value(value: &Value) -> Schema {
    if value.is_boolean() || value.as_object().is_some() {
        value
            .clone()
            .try_into()
            .unwrap_or_else(|_| json_schema!({ "type": "object" }))
    } else {
        json_schema!({ "type": "object" })
    }
}

/// Build an OpenAPI request body for a command payload schema.
pub fn openapi_command_request_body(schema: Value) -> RequestBody {
    let json_schema = json_schema_from_value(&schema);
    RequestBody {
        description: None,
        content: IndexMap::from_iter([(
            "application/json".into(),
            MediaType {
                schema: Some(SchemaObject {
                    json_schema,
                    example: None,
                    external_docs: None,
                }),
                ..Default::default()
            },
        )]),
        required: true,
        extensions: IndexMap::default(),
    }
}

/// Stamp a command operation's request body with the typed payload schema.
pub fn with_command_payload_schema<'a>(
    op: TransformOperation<'a>,
    payload_schema: Value,
) -> TransformOperation<'a> {
    op.with(move |mut op| {
        op.inner_mut().request_body = Some(ReferenceOr::Item(openapi_command_request_body(
            payload_schema,
        )));
        op
    })
}

/// Overlay a command success response schema when a non-generic schema is available.
pub fn with_command_response_schema<'a>(
    op: TransformOperation<'a>,
    response_schema: Value,
) -> TransformOperation<'a> {
    if response_schema == generic_object_schema() {
        return op;
    }
    let json_schema = json_schema_from_value(&response_schema);
    op.with(move |mut op| {
        use aide::openapi::StatusCode as AideStatusCode;
        if let Some(responses) = op.inner_mut().responses.as_mut() {
            for code in [200u16, 201u16] {
                if let Some(ReferenceOr::Item(response)) =
                    responses.responses.get_mut(&AideStatusCode::Code(code))
                {
                    if let Some(media) = response.content.get_mut("application/json") {
                        media.schema = Some(SchemaObject {
                            json_schema: json_schema.clone(),
                            example: None,
                            external_docs: None,
                        });
                    }
                }
            }
        }
        op
    })
}

/// Fallback payload schema when command info is unavailable at route registration time.
pub fn default_command_payload_schema() -> Value {
    generic_object_schema()
}

fn sanitize_operation_id_segment(s: &str) -> String {
    s.replace(['.', '-'], "_")
}

/// Short unique OpenAPI `operationId` for a Riverbase HTTP operation.
pub fn riverbase_operation_id(meta: &RiverbaseOperationMeta, http_method: &str) -> String {
    let ns = sanitize_operation_id_segment(&meta.namespace);
    let seg = sanitize_operation_id_segment(&meta.segment);
    let kind = meta.kind.operation_id_slug();
    let method = http_method.to_ascii_lowercase();
    if meta.scoped {
        format!("{ns}_{seg}_{kind}_scoped_{method}")
    } else {
        format!("{ns}_{seg}_{kind}_{method}")
    }
}

/// Keep absolute OpenAPI path keys (including `/api/...`) for catalog cover parity.
///
/// Unlike an earlier strip-to-`servers.url` layout, published keys match the HTTP paths
/// clients and mugwort generators assert (e.g. `/api/sourcing/rfq.list`).
pub fn normalize_openapi_paths(api: &mut OpenApi, api_base: &str) {
    let _ = api_base;
    if let Some(paths) = api.paths.as_mut() {
        let old: Vec<_> = paths.paths.drain(..).collect();
        for (path, item) in old {
            let key = if path.starts_with('/') {
                path
            } else {
                format!("/{path}")
            };
            paths.paths.insert(key, item);
        }
    }
    // Absolute path keys — do not rewrite via a relative `servers.url`.
    api.servers.clear();
}

/// Expand `{resource}` path templates using each operation's `x-api-resources` list
/// (Python `custom_openapi` parity) so catalog cover sees `/…:exec/rfq/{identifier}`.
pub fn expand_openapi_resource_paths(api: &mut OpenApi) {
    use aide::openapi::{PathItem, ReferenceOr};

    let Some(paths) = api.paths.as_mut() else {
        return;
    };
    let old: Vec<_> = paths.paths.drain(..).collect();
    for (path, item) in old {
        if !path.contains("{resource}") {
            paths.paths.insert(path, item);
            continue;
        }
        let ReferenceOr::Item(path_item) = item else {
            paths.paths.insert(path, item);
            continue;
        };
        let mut expanded_any = false;
        for (method, operation) in path_item_operations(&path_item) {
            let resources = operation
                .extensions
                .get("x-api-resources")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if resources.is_empty() {
                continue;
            }
            for resource in resources {
                let Some(name) = resource.as_str() else {
                    continue;
                };
                let concrete = path.replace("{resource}", name);
                let mut concrete_op = operation.clone();
                concrete_op.parameters.retain(|param| match param {
                    ReferenceOr::Item(p) => parameter_name(p) != Some("resource"),
                    ReferenceOr::Reference { .. } => true,
                });
                let entry = paths
                    .paths
                    .entry(concrete)
                    .or_insert_with(|| ReferenceOr::Item(PathItem::default()));
                if let ReferenceOr::Item(item) = entry {
                    set_path_item_operation(item, method, concrete_op);
                }
                expanded_any = true;
            }
        }
        if !expanded_any {
            paths.paths.insert(path, ReferenceOr::Item(path_item));
        }
    }
}

fn path_item_operations(
    item: &aide::openapi::PathItem,
) -> Vec<(&'static str, &aide::openapi::Operation)> {
    let mut out = Vec::new();
    if let Some(op) = item.get.as_ref() {
        out.push(("get", op));
    }
    if let Some(op) = item.post.as_ref() {
        out.push(("post", op));
    }
    if let Some(op) = item.put.as_ref() {
        out.push(("put", op));
    }
    if let Some(op) = item.patch.as_ref() {
        out.push(("patch", op));
    }
    if let Some(op) = item.delete.as_ref() {
        out.push(("delete", op));
    }
    out
}

fn set_path_item_operation(
    item: &mut aide::openapi::PathItem,
    method: &str,
    operation: aide::openapi::Operation,
) {
    match method {
        "get" => item.get = Some(operation),
        "post" => item.post = Some(operation),
        "put" => item.put = Some(operation),
        "patch" => item.patch = Some(operation),
        "delete" => item.delete = Some(operation),
        _ => {}
    }
}

fn parameter_name(param: &aide::openapi::Parameter) -> Option<&str> {
    use aide::openapi::Parameter;
    Some(match param {
        Parameter::Query { parameter_data, .. }
        | Parameter::Header { parameter_data, .. }
        | Parameter::Path { parameter_data, .. }
        | Parameter::Cookie { parameter_data, .. } => parameter_data.name.as_str(),
    })
}

fn insert_extension<'a>(
    op: TransformOperation<'a>,
    key: &str,
    value: Value,
) -> TransformOperation<'a> {
    op.with(|mut t| {
        t.inner_mut().extensions.insert(key.to_string(), value);
        t
    })
}

/// Default primary Riverbase catalog tag for an operation kind.
pub fn default_primary_tag(kind: RiverbaseOperationKind) -> &'static str {
    match kind {
        RiverbaseOperationKind::DomainMeta => "riverbase:metadata",
        kind if kind.is_command() => "riverbase:command",
        kind if kind.is_query() => "riverbase:query",
        kind if kind.is_realtime() => "riverbase:rtc",
        kind if kind.is_generic() => "riverbase:request",
        _ => "riverbase:request",
    }
}

/// Resolved primary catalog tag (meta override or kind default).
pub fn resolve_primary_tag(kind: RiverbaseOperationKind, openapi: &OpenApiMeta) -> String {
    openapi
        .tag
        .clone()
        .unwrap_or_else(|| default_primary_tag(kind).to_string())
}

fn apply_openapi_extensions<'a>(
    op: TransformOperation<'a>,
    openapi: &OpenApiMeta,
) -> TransformOperation<'a> {
    let mut op = op;
    if openapi.explorer == Some(false) {
        op = insert_extension(op, "x-explorer", json!(false));
    }
    if openapi.internal == Some(true) {
        op = insert_extension(op, "x-internal", json!(true));
    }
    if openapi.deprecated {
        op = op.with(|mut t| {
            t.inner_mut().deprecated = true;
            t
        });
    }
    op
}

/// Apply Riverbase catalog tags and `x-*` extensions to an OpenAPI operation.
pub fn apply_riverbase_operation<'a>(
    op: TransformOperation<'a>,
    meta: &RiverbaseOperationMeta,
    http_method: &str,
) -> TransformOperation<'a> {
    let operation_id = riverbase_operation_id(meta, http_method);
    let primary_tag = resolve_primary_tag(meta.kind, &meta.openapi);
    let mut op = op.id(&operation_id).with(|mut t| {
        t.inner_mut().tags.clear();
        t
    });

    op = op.tag(&primary_tag);
    match meta.kind {
        RiverbaseOperationKind::DomainMeta
        | RiverbaseOperationKind::CommandObject
        | RiverbaseOperationKind::CommandResource
        | RiverbaseOperationKind::CommandHook
        | RiverbaseOperationKind::CommandLink
        | RiverbaseOperationKind::CommandMeta => {
            op = op.tag(&meta.domain_tag());
        }
        RiverbaseOperationKind::QueryList
        | RiverbaseOperationKind::QueryItem
        | RiverbaseOperationKind::QueryMeta
        | RiverbaseOperationKind::QueryReport => {
            op = op.tag(&meta.queryset_tag());
        }
        RiverbaseOperationKind::RealtimeWebsocket | RiverbaseOperationKind::RealtimeSseStream => {
            op = op.tag("riverbase:rtc");
            op = op.tag(&meta.domain_tag());
        }
        _ => {}
    }

    op = insert_extension(op, "x-api-kind", json!(meta.kind.x_kind()));
    op = insert_extension(op, "x-api-name", json!(meta.x_api()));

    if meta.kind.is_command() {
        op = insert_extension(op, "x-domain", json!(meta.namespace));
        if let Some(ref cmdkey) = meta.command_key {
            op = insert_extension(op, "x-command", json!(cmdkey));
        }
        if !meta.resources.is_empty() {
            op = insert_extension(op, "x-api-resources", json!(meta.resources));
        }
        op = insert_extension(op, "x-scoped", json!(meta.scoped));
    }

    if meta.kind.is_query() {
        op = insert_extension(op, "x-queryset", json!(meta.namespace));
        op = insert_extension(op, "x-scoped", json!(meta.scoped));
    }

    apply_openapi_extensions(op, &meta.openapi)
}

/// Command route operation metadata (tags, `x-kind`, `x-api`, …) plus summary/description.
pub fn riverbase_command_operation<'a>(
    meta: &RiverbaseOperationMeta,
    summary: &str,
    description: &str,
    http_method: &str,
    op: TransformOperation<'a>,
) -> TransformOperation<'a> {
    apply_riverbase_operation(op, meta, http_method)
        .summary(summary)
        .description(description)
}

/// Query route operation metadata plus summary/description.
pub fn riverbase_query_operation<'a>(
    meta: &RiverbaseOperationMeta,
    summary: &str,
    description: &str,
    http_method: &str,
    op: TransformOperation<'a>,
) -> TransformOperation<'a> {
    apply_riverbase_operation(op, meta, http_method)
        .summary(summary)
        .description(description)
}

/// OpenAPI summary for `GET /{namespace}/domain.meta`.
pub fn domain_meta_operation_summary(domain_title: &str) -> String {
    format!("{} Domain", domain_title.trim())
}

/// Namespace command discovery (`GET /{namespace}/domain.meta`).
pub fn riverbase_metadata_operation<'a>(
    namespace: &str,
    summary: &str,
    description: &str,
    http_method: &str,
    op: TransformOperation<'a>,
) -> TransformOperation<'a> {
    let meta = RiverbaseOperationMeta::domain_meta(namespace);
    riverbase_command_operation(&meta, summary, description, http_method, op)
}

fn riverbase_catalog_tags() -> Vec<Tag> {
    [
        ("riverbase:agent", "Agentic API"),
        ("riverbase:process", "Process Management API"),
        ("riverbase:command", "Command Endpoints"),
        ("riverbase:query", "Query Endpoints"),
        ("riverbase:metadata", "Metadata Endpoints"),
        ("riverbase:mcp", "MCP Tool Command"),
        ("riverbase:rtc", "Realtime Endpoints"),
        ("riverbase:rule", "Rule Engine Endpoints"),
        ("riverbase:setting", "Setting Manager Endpoints"),
        ("riverbase:media", "Media Endpoints"),
        ("riverbase:auth", "Authentication Endpoints"),
        ("riverbase:task", "Task Endpoints"),
        ("riverbase:document", "Document Endpoints"),
        ("riverbase:audit", "Audit Endpoints"),
    ]
    .into_iter()
    .map(|(name, description)| Tag {
        name: name.into(),
        description: Some(description.into()),
        ..Tag::default()
    })
    .collect()
}

/// Build a starter [`OpenApi`] document for a coupled flrs HTTP service.
pub fn default_coupled_openapi(
    title: impl Into<String>,
    description: impl Into<String>,
) -> OpenApi {
    OpenApi {
        info: Info {
            title: title.into(),
            description: Some(description.into()),
            version: API_CONTRACT_VERSION.into(),
            ..Info::default()
        },
        tags: riverbase_catalog_tags(),
        ..OpenApi::default()
    }
}

/// `GET {api_base}/api.info` — application metadata path (mirrors Python `/api.info`).
pub fn api_info_path(api_base: &str) -> String {
    join_api_path(api_base, "/api.info")
}

/// Build the application info payload served at `GET {api_base}/api.info`.
///
/// Mirrors Python `riverbase.fastapi.create_app`'s `application_metadata` handler:
/// `name`/`description`/`version` come from the OpenAPI document, `riverbase` is the
/// `riverbase_core` crate version, and `root_path` is the configured API base path.
fn application_info(api: &OpenApi, api_base: &str) -> Value {
    json!({
        "name": api.info.title,
        "description": api.info.description,
        "version": API_CONTRACT_VERSION,
        "contract": API_CONTRACT_VERSION,
        "riverbase": env!("CARGO_PKG_VERSION"),
        "root_path": api_base,
    })
}

/// Attach `/` → OpenAPI redirect, `{api_base}/openapi.json` + root `/openapi.json`,
/// `/api.info`, and finalize the spec.
pub fn finish_with_openapi<S>(
    router: ApiRouter<S>,
    api: &mut OpenApi,
    api_base: &str,
) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let openapi_path = openapi_json_path(api_base);
    let redirect_target = openapi_path.clone();
    let info_path = api_info_path(api_base);
    let info = application_info(api, api_base);
    let router = router
        .route(
            "/",
            axum_get(move || redirect_to_openapi(redirect_target.clone())),
        )
        // Catalog/cover + mugwort read `{api_base}/openapi.json` (single OpenAPI entry).
        .api_route(
            openapi_path.as_str(),
            get_with(serve_openapi, stamp_openapi_document_op),
        )
        // Origin-root compatibility alias — not stamped again (avoids duplicate x-api-name).
        .route("/openapi.json", axum_get(serve_openapi_compat))
        .api_route(
            info_path.as_str(),
            get_with(move || serve_api_info(info.clone()), stamp_api_info_op),
        )
        .finish_api_with(api, |api| {
            api.default_response_with::<Json<ProblemDetails>, _>(|res| {
                res.description("RFC 7807 problem+json error")
            })
        });
    normalize_openapi_paths(api, api_base);
    expand_openapi_resource_paths(api);
    ensure_probe_openapi_paths(api, api_base);
    router
        // Arc: axum clones router state per request; cloning the full OpenApi tree was ~1s/request.
        .layer(Extension(Arc::new(std::mem::take(api))))
}

fn stamp_openapi_document_op(op: TransformOperation) -> TransformOperation {
    apply_riverbase_operation(
        op,
        &RiverbaseOperationMeta {
            namespace: "openapi.json".into(),
            segment: String::new(),
            kind: RiverbaseOperationKind::GenericGet,
            scoped: false,
            openapi: OpenApiMeta::default(),
            command_key: None,
            resources: Vec::new(),
        },
        "get",
    )
    .summary("OpenAPI document")
    .description("Live OpenAPI 3 document for this portal.")
}

fn stamp_api_info_op(op: TransformOperation) -> TransformOperation {
    apply_riverbase_operation(
        op,
        &RiverbaseOperationMeta {
            namespace: "api.info".into(),
            segment: String::new(),
            kind: RiverbaseOperationKind::GenericGet,
            scoped: false,
            openapi: OpenApiMeta::default(),
            command_key: None,
            resources: Vec::new(),
        },
        "get",
    )
    .summary("Application info")
    .description("Application metadata envelope (contract version, name, …).")
}

/// Document `{api_base}/health` and `{api_base}/ready` in OpenAPI.
///
/// HTTP handlers remain at origin-root `/health` and `/ready`; these keys exist
/// so catalog cover and Mugwort L1 see probes under the same mount as the API.
fn ensure_probe_openapi_paths(api: &mut OpenApi, api_base: &str) {
    use aide::openapi::{Operation, PathItem, Paths, ReferenceOr};

    let paths = api.paths.get_or_insert_with(Paths::default);
    for (suffix, name, summary) in [
        ("/health", "health", "Liveness probe"),
        ("/ready", "ready", "Readiness probe"),
    ] {
        let path = join_api_path(api_base, suffix);
        if paths.paths.contains_key(&path) {
            continue;
        }
        let mut op = Operation::default();
        op.operation_id = Some(format!("probe_{name}_get"));
        op.summary = Some(summary.into());
        op.tags = vec!["riverbase:request".into()];
        op.extensions
            .insert("x-api-kind".into(), json!("generic:get"));
        op.extensions.insert("x-api-name".into(), json!(name));
        let mut item = PathItem::default();
        item.get = Some(op);
        paths.paths.insert(path, ReferenceOr::Item(item));
    }
}

async fn redirect_to_openapi(path: String) -> impl IntoResponse {
    Redirect::temporary(&path)
}

async fn serve_openapi(
    Extension(api): Extension<Arc<OpenApi>>,
    #[cfg(feature = "auth")] request: Request,
) -> impl IntoApiResponse {
    #[cfg(feature = "auth")]
    {
        use super::casbin_layer::{filter_accessible_openapi, CasbinLayerState};
        use crate::auth::middleware::principal_from_request;

        if let Some(state) = request.extensions().get::<CasbinLayerState>() {
            let principal = principal_from_request(&request);
            return match filter_accessible_openapi(&api, state, principal.as_ref()).await {
                Ok(filtered) => Json(filtered).into_response(),
                Err(err) => openapi_error_response(err),
            };
        }
    }
    Json((*api).clone()).into_response()
}

/// Compatibility mount at origin-root `/openapi.json` (not stamped in OpenAPI).
async fn serve_openapi_compat(Extension(api): Extension<Arc<OpenApi>>) -> impl IntoResponse {
    Json((*api).clone())
}

#[cfg(feature = "auth")]
fn openapi_error_response(err: crate::base::RiverbaseError) -> axum::response::Response {
    crate::http_response::error_into_response(err)
}

async fn serve_api_info(info: Value) -> impl IntoApiResponse {
    Json(success_envelope(info, json!({})))
}

/// Back-compat alias — prefer [`apply_riverbase_operation`] with explicit metadata.
#[deprecated(note = "use riverbase_command_operation / riverbase_query_operation instead")]
pub fn flrs_operation<'a>(op: TransformOperation<'a>) -> TransformOperation<'a> {
    op.tag("flrs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aide::openapi::Operation;
    use aide::transform::TransformOperation;

    fn apply_and_read(meta: RiverbaseOperationMeta, http_method: &str) -> Operation {
        let mut op = Operation::default();
        let t = TransformOperation::new(&mut op);
        let _ = apply_riverbase_operation(t, &meta, http_method);
        op
    }

    #[test]
    fn riverbase_api_name_format() {
        assert_eq!(
            riverbase_api_name("exp.catalog", "create-product"),
            "exp.catalog/create-product"
        );
    }

    #[test]
    fn operation_metadata_table() {
        let exec_meta =
            CommandMeta::exec("create-product", "Create Product").with_resources(["product"]);
        let collection_meta = CommandMeta::collection("checkout", "Checkout");

        let cases: Vec<(RiverbaseOperationMeta, &[&str], &str, Option<&str>)> = vec![
            (
                RiverbaseOperationMeta::command(
                    "exp.catalog",
                    "create-product",
                    &exec_meta,
                    RiverbaseOperationKind::CommandObject,
                    false,
                ),
                &["riverbase:command", "domain:exp.catalog"],
                "command:object",
                Some("exp.catalog/create-product"),
            ),
            (
                RiverbaseOperationMeta::command(
                    "exp.order",
                    "checkout",
                    &collection_meta,
                    RiverbaseOperationKind::CommandResource,
                    true,
                ),
                &["riverbase:command", "domain:exp.order"],
                "command:resource",
                Some("exp.order/checkout"),
            ),
            (
                RiverbaseOperationMeta::command(
                    "exp.payment",
                    "stripe-webhook",
                    &collection_meta,
                    RiverbaseOperationKind::CommandHook,
                    true,
                ),
                &["riverbase:command", "domain:exp.payment"],
                "command:hook",
                Some("exp.payment/stripe-webhook"),
            ),
            (
                RiverbaseOperationMeta::command(
                    "exp.ticket",
                    "share-link",
                    &exec_meta,
                    RiverbaseOperationKind::CommandLink,
                    false,
                ),
                &["riverbase:command", "domain:exp.ticket"],
                "command:link",
                Some("exp.ticket/share-link"),
            ),
            (
                RiverbaseOperationMeta::query(
                    "exp.catalog",
                    "product",
                    RiverbaseOperationKind::QueryList,
                    false,
                    OpenApiMeta::default(),
                ),
                &["riverbase:query", "queryset:exp.catalog"],
                "query:list",
                Some("exp.catalog/product"),
            ),
            (
                RiverbaseOperationMeta::query(
                    "exp.catalog",
                    "product",
                    RiverbaseOperationKind::QueryItem,
                    true,
                    OpenApiMeta::default(),
                ),
                &["riverbase:query", "queryset:exp.catalog"],
                "query:item",
                Some("exp.catalog/product"),
            ),
            (
                RiverbaseOperationMeta::query(
                    "exp.catalog",
                    "product",
                    RiverbaseOperationKind::QueryMeta,
                    false,
                    OpenApiMeta::default(),
                ),
                &["riverbase:query", "queryset:exp.catalog"],
                "query:meta",
                Some("exp.catalog/product"),
            ),
            (
                RiverbaseOperationMeta::domain_meta("exp.catalog"),
                &["riverbase:metadata", "domain:exp.catalog"],
                "domain:meta",
                Some("exp.catalog"),
            ),
            (
                RiverbaseOperationMeta::command(
                    "exp.catalog",
                    "create-product",
                    &exec_meta,
                    RiverbaseOperationKind::CommandMeta,
                    false,
                ),
                &["riverbase:command", "domain:exp.catalog"],
                "command:meta",
                Some("exp.catalog/create-product"),
            ),
        ];

        for (meta, expected_tags, expected_kind, expected_api) in cases {
            let method = if meta.kind.is_query() || meta.kind == RiverbaseOperationKind::DomainMeta {
                "get"
            } else {
                "post"
            };
            let op = apply_and_read(meta, method);
            assert_eq!(op.tags, expected_tags);
            assert_eq!(
                op.extensions.get("x-api-kind").and_then(|v| v.as_str()),
                Some(expected_kind)
            );
            assert_eq!(
                op.extensions.get("x-api-name").and_then(|v| v.as_str()),
                expected_api
            );
        }
    }

    #[test]
    fn command_extensions_include_domain_and_scoped() {
        let meta =
            CommandMeta::exec("create-product", "Create Product").with_resources(["product"]);
        let op_meta = RiverbaseOperationMeta::command(
            "exp.catalog",
            "create-product",
            &meta,
            RiverbaseOperationKind::CommandObject,
            false,
        );
        let op = apply_and_read(op_meta, "post");

        assert_eq!(
            op.extensions.get("x-domain").and_then(|v| v.as_str()),
            Some("exp.catalog")
        );
        assert_eq!(
            op.extensions.get("x-command").and_then(|v| v.as_str()),
            Some("create-product")
        );
        assert_eq!(
            op.extensions.get("x-scoped").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            op.extensions.get("x-api-resources"),
            Some(&json!(["product"]))
        );
    }

    #[test]
    fn query_extensions_include_queryset() {
        let op_meta = RiverbaseOperationMeta::query(
            "exp.catalog",
            "product",
            RiverbaseOperationKind::QueryList,
            false,
            OpenApiMeta::default(),
        );
        let op = apply_and_read(op_meta, "get");
        assert_eq!(
            op.extensions.get("x-queryset").and_then(|v| v.as_str()),
            Some("exp.catalog")
        );
    }

    #[test]
    fn domain_meta_operation_summary_uses_domain_title() {
        assert_eq!(
            domain_meta_operation_summary("Reporting"),
            "Reporting Domain"
        );
    }

    #[test]
    fn audit_query_uses_audit_tag() {
        let op_meta = RiverbaseOperationMeta::query(
            "rfx.domain",
            "command-log",
            RiverbaseOperationKind::QueryList,
            false,
            OpenApiMeta::default().with_tag("riverbase:audit"),
        );
        let op = apply_and_read(op_meta, "get");
        assert_eq!(op.tags, &["riverbase:audit", "queryset:rfx.domain"]);
    }

    #[test]
    fn rule_query_uses_rule_tag() {
        let op_meta = RiverbaseOperationMeta::query(
            "rule-engine",
            "knowledge-base",
            RiverbaseOperationKind::QueryList,
            false,
            OpenApiMeta::default().with_tag("riverbase:rule"),
        );
        let op = apply_and_read(op_meta, "get");
        assert_eq!(op.tags, &["riverbase:rule", "queryset:rule-engine"]);
    }

    #[test]
    fn setting_query_uses_setting_tag() {
        let op_meta = RiverbaseOperationMeta::query(
            "setting-manager",
            "setting-group",
            RiverbaseOperationKind::QueryList,
            false,
            OpenApiMeta::default().with_tag("riverbase:setting"),
        );
        let op = apply_and_read(op_meta, "get");
        assert_eq!(op.tags, &["riverbase:setting", "queryset:setting-manager"]);
    }

    #[test]
    fn command_openapi_tag_override() {
        let meta =
            CommandMeta::exec("create-product", "Create Product").with_openapi_tag("riverbase:audit");
        let op_meta = RiverbaseOperationMeta::command(
            "exp.catalog",
            "create-product",
            &meta,
            RiverbaseOperationKind::CommandObject,
            false,
        );
        let op = apply_and_read(op_meta, "post");
        assert_eq!(op.tags, &["riverbase:audit", "domain:exp.catalog"]);
    }

    #[test]
    fn openapi_extensions_apply_explorer_and_internal() {
        let op_meta = RiverbaseOperationMeta::query(
            "exp.catalog",
            "product",
            RiverbaseOperationKind::QueryList,
            false,
            OpenApiMeta {
                explorer: Some(false),
                internal: Some(true),
                ..OpenApiMeta::default()
            },
        );
        let op = apply_and_read(op_meta, "get");
        assert_eq!(
            op.extensions.get("x-explorer").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            op.extensions.get("x-internal").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn default_openapi_includes_catalog_tags() {
        let api = default_coupled_openapi("Test", "Desc");
        assert!(api.tags.iter().any(|t| t.name == "riverbase:command"));
        assert!(api.tags.iter().any(|t| t.name == "riverbase:query"));
        assert!(api.tags.iter().any(|t| t.name == "riverbase:metadata"));
    }

    #[test]
    fn api_info_path_is_prefixed_by_base() {
        assert_eq!(api_info_path("/api"), "/api/api.info");
        assert_eq!(api_info_path("/v1"), "/v1/api.info");
    }

    #[tokio::test]
    async fn openapi_document_is_mounted_under_api_base() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let mut api = default_coupled_openapi("Test", "Desc");
        let router = finish_with_openapi(aide::axum::ApiRouter::<()>::new(), &mut api, "/v1");

        let get = |uri: &'static str| {
            let router = router.clone();
            async move {
                router
                    .oneshot(
                        Request::builder()
                            .uri(uri)
                            .body(Body::empty())
                            .expect("request"),
                    )
                    .await
                    .expect("response")
            }
        };

        let mounted = get("/v1/openapi.json").await;
        assert_eq!(mounted.status(), StatusCode::OK);
        let body = axum::body::to_bytes(mounted.into_body(), usize::MAX)
            .await
            .expect("body");
        let document: Value = serde_json::from_slice(&body).expect("openapi json");
        let paths = document["paths"].as_object().expect("paths");
        assert!(paths.contains_key("/v1/openapi.json"));
        assert!(paths.contains_key("/v1/api.info"));
        assert!(paths.contains_key("/v1/health"));
        assert!(paths.contains_key("/v1/ready"));
        // Origin-root alias is served but not stamped (avoids duplicate x-api-name).
        assert!(!paths.contains_key("/openapi.json"));
        assert!(!paths.contains_key("/health"));
        assert!(!paths.contains_key("/ready"));

        assert_eq!(get("/openapi.json").await.status(), StatusCode::OK);

        let redirect = get("/").await;
        assert_eq!(redirect.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            redirect
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok()),
            Some("/v1/openapi.json")
        );
    }

    #[test]
    fn application_info_reports_app_and_riverbase_metadata() {
        let api = default_coupled_openapi("entry.express Seller API", "Seller portal");
        let info = application_info(&api, "/api");
        assert_eq!(info["name"], "entry.express Seller API");
        assert_eq!(info["description"], "Seller portal");
        assert_eq!(info["root_path"], "/api");
        assert_eq!(info["riverbase"], env!("CARGO_PKG_VERSION"));
        assert_eq!(info["version"], API_CONTRACT_VERSION);
        assert_eq!(info["contract"], API_CONTRACT_VERSION);
        assert_eq!(api.info.version, API_CONTRACT_VERSION);
    }

    #[test]
    fn riverbase_operation_id_is_short_and_unique() {
        let exec_meta = CommandMeta::exec("create-product", "Create Product");
        let exec = RiverbaseOperationMeta::command(
            "exp.catalog",
            "create-product",
            &exec_meta,
            RiverbaseOperationKind::CommandObject,
            false,
        );
        assert_eq!(
            riverbase_operation_id(&exec, "post"),
            "exp_catalog_create_product_command_object_post"
        );

        let item = RiverbaseOperationMeta::query(
            "identity-manager",
            "organization",
            RiverbaseOperationKind::QueryItem,
            false,
            OpenApiMeta::default(),
        );
        assert_eq!(
            riverbase_operation_id(&item, "get"),
            "identity_manager_organization_query_item_get"
        );

        let scoped_item = RiverbaseOperationMeta::query(
            "exp.catalog",
            "product",
            RiverbaseOperationKind::QueryItem,
            true,
            OpenApiMeta::default(),
        );
        assert_eq!(
            riverbase_operation_id(&scoped_item, "get"),
            "exp_catalog_product_query_item_scoped_get"
        );

        let mut op = Operation::default();
        let t = TransformOperation::new(&mut op);
        let _ = apply_riverbase_operation(t, &exec, "post");
        assert_eq!(
            op.operation_id.as_deref(),
            Some("exp_catalog_create_product_command_object_post")
        );
    }

    #[test]
    fn normalize_openapi_paths_keeps_absolute_api_keys() {
        use aide::openapi::{PathItem, Paths, ReferenceOr};

        let mut api = default_coupled_openapi("Test", "Desc");
        let mut paths = Paths::default();
        paths.paths.insert(
            "/api/exp.catalog/product.list".into(),
            ReferenceOr::Item(PathItem::default()),
        );
        paths.paths.insert(
            "/api/identity-manager/organization.item/{identifier}".into(),
            ReferenceOr::Item(PathItem::default()),
        );
        api.paths = Some(paths);

        normalize_openapi_paths(&mut api, "/api");

        let paths = api.paths.as_ref().unwrap();
        assert!(paths.paths.contains_key("/api/exp.catalog/product.list"));
        assert!(paths
            .paths
            .contains_key("/api/identity-manager/organization.item/{identifier}"));
        assert!(api.servers.is_empty());
    }
}
