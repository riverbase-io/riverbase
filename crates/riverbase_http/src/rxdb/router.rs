//! [`RxdbRegistry`] — register [`RxdbCollection`]s and build their HTTP routes.
//!
//! Mirrors `RTCBridge.registerRxdbCollection` / the RxDB route block: each
//! registered collection gets `POST <prefix>/<collection>/pull` and
//! `POST <prefix>/<collection>/push` endpoints on an [`aide`] router.

use std::collections::HashSet;
use std::sync::Arc;

use aide::axum::{routing::post_with, ApiRouter};
use aide::transform::TransformOperation;
use axum::{extract::Path, response::IntoResponse, routing::get, Extension, Json};
use serde_json::json;

use crate::auth::Principal;
use crate::base::{RiverbaseError, RiverbaseResult};
use crate::command::MessageBus;
use crate::http_response::http_json_response;
use crate::transport::StreamBus;

use super::collection::{
    RxdbCollection, RxdbContext, RxdbPullRequest, RxdbPullResult, RxdbPushRequest, RxdbPushResult,
};
use super::service::{handle_pull, handle_push};
use super::stream::rxdb_sse_stream;

/// Default URL prefix for RxDB replication routes.
pub const DEFAULT_RXDB_PREFIX: &str = "/rxdb";

/// Validate an RxDB collection name (`^[a-zA-Z0-9][a-zA-Z0-9._-]*$`).
fn validate_collection_name(name: &str) -> RiverbaseResult<()> {
    let mut chars = name.chars();
    let valid = matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !valid {
        return Err(crate::errors::RXD_002.with_data(json!({ "collection": name })));
    }
    Ok(())
}

/// Builder that collects [`RxdbCollection`]s and emits their HTTP routes.
///
/// ```ignore
/// let router = RxdbRegistry::new()
///     .with_message_bus(bus)
///     .register(Arc::new(TodoCollection::new(store)))?
///     .into_router();
/// // merge `router` into your coupled domain router, then `finish_with_openapi`.
/// ```
pub struct RxdbRegistry {
    prefix: String,
    collections: Vec<Arc<dyn RxdbCollection>>,
    names: HashSet<String>,
    msgbus: Option<Arc<dyn MessageBus>>,
    stream_bus: Option<Arc<dyn StreamBus>>,
}

impl Default for RxdbRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RxdbRegistry {
    /// Construct a new value.
    pub fn new() -> Self {
        Self {
            prefix: DEFAULT_RXDB_PREFIX.to_string(),
            collections: Vec::new(),
            names: HashSet::new(),
            msgbus: None,
            stream_bus: None,
        }
    }

    /// Override the route prefix (default `/rxdb`). Leading slash is enforced.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        let mut prefix = prefix.into();
        if !prefix.starts_with('/') {
            prefix.insert(0, '/');
        }
        while prefix.len() > 1 && prefix.ends_with('/') {
            prefix.pop();
        }
        self.prefix = prefix;
        self
    }

    /// Set prefix to `{api_base}/rxdb`.
    pub fn with_api_base(mut self, api_base: &str) -> Self {
        self.prefix = crate::api_path::rxdb_base_path(api_base);
        self
    }

    /// Attach the message bus used to publish change notifications on push.
    pub fn with_message_bus(mut self, bus: Arc<dyn MessageBus>) -> Self {
        self.msgbus = Some(bus);
        self
    }

    /// Attach a [`StreamBus`] for push notifications and SSE streaming (required for `/stream`).
    pub fn with_stream_bus(mut self, bus: Arc<dyn StreamBus>) -> Self {
        self.stream_bus = Some(bus.clone());
        self.msgbus = Some(bus as Arc<dyn MessageBus>);
        self
    }

    /// Register a collection. Fails on an invalid or duplicate collection name.
    pub fn register(mut self, collection: Arc<dyn RxdbCollection>) -> RiverbaseResult<Self> {
        let name = collection.collection_name().to_string();
        validate_collection_name(&name)?;
        if !self.names.insert(name.clone()) {
            return Err(crate::errors::RXD_003.with_data(json!({ "collection": name })));
        }
        self.collections.push(collection);
        Ok(self)
    }

    /// Whether this is empty.
    pub fn is_empty(&self) -> bool {
        self.collections.is_empty()
    }

    /// Len.
    pub fn len(&self) -> usize {
        self.collections.len()
    }

    /// Build the [`ApiRouter`] exposing pull/push routes for every collection.
    pub fn into_router(self) -> ApiRouter {
        let RxdbRegistry {
            prefix,
            collections,
            msgbus,
            stream_bus,
            ..
        } = self;

        let mut router: ApiRouter = ApiRouter::new();
        for collection in collections {
            let name = collection.collection_name().to_string();
            let pull_path = format!("{prefix}/{name}/pull");
            let push_path = format!("{prefix}/{name}/push");
            {
                let collection = collection.clone();
                router = router.api_route(
                    pull_path.as_str(),
                    post_with(
                        move |principal: Option<Extension<Principal>>,
                              body: Json<RxdbPullRequest>| {
                            let collection = collection.clone();
                            async move {
                                let Json(request) = body;
                                let ctx = RxdbContext::new(principal.map(|Extension(p)| p));
                                handle_pull(collection.as_ref(), &ctx, request)
                                    .await
                                    .map(Json)
                                    .map_err(error_response)
                            }
                        },
                        pull_operation(&name),
                    ),
                );
            }

            {
                let collection = collection.clone();
                let msgbus = msgbus.clone();
                router = router.api_route(
                    push_path.as_str(),
                    post_with(
                        move |principal: Option<Extension<Principal>>,
                              body: Json<RxdbPushRequest>| {
                            let collection = collection.clone();
                            let msgbus = msgbus.clone();
                            async move {
                                let Json(request) = body;
                                let ctx = RxdbContext::new(principal.map(|Extension(p)| p));
                                handle_push(collection.as_ref(), &ctx, msgbus.as_ref(), request)
                                    .await
                                    .map(Json)
                                    .map_err(error_response)
                            }
                        },
                        push_operation(&name),
                    ),
                );
            }
        }

        if let Some(bus) = stream_bus {
            let stream_path = format!("{prefix}/{{collection}}/stream");
            router = router.route(
                stream_path.as_str(),
                get({
                    let bus = bus.clone();
                    move |Path(collection): Path<String>| {
                        let bus = bus.clone();
                        async move {
                            match rxdb_sse_stream(bus, collection).await {
                                Ok(sse) => sse.into_response(),
                                Err(e) => http_json_response(e),
                            }
                        }
                    }
                }),
            );
        }

        router
    }
}

fn pull_operation(collection: &str) -> impl FnOnce(TransformOperation) -> TransformOperation {
    let summary = format!("rxdb pull `{collection}`");
    let description = format!("Pull documents changed for RxDB collection `{collection}`.");
    let operation_id = format!("rxdb_{collection}_pull_post");
    move |op| {
        op.id(&operation_id)
            .tag("rxdb")
            .summary(summary.as_str())
            .description(description.as_str())
            .response_with::<200, Json<RxdbPullResult>, _>(|res| {
                res.description("Changed documents and the next checkpoint")
            })
    }
}

fn push_operation(collection: &str) -> impl FnOnce(TransformOperation) -> TransformOperation {
    let summary = format!("rxdb push `{collection}`");
    let description = format!("Push client changes to RxDB collection `{collection}`.");
    let operation_id = format!("rxdb_{collection}_push_post");
    move |op| {
        op.id(&operation_id)
            .tag("rxdb")
            .summary(summary.as_str())
            .description(description.as_str())
            .response_with::<200, Json<RxdbPushResult>, _>(|res| {
                res.description("Applied change count and conflicting documents")
            })
    }
}

fn error_response(err: RiverbaseError) -> axum::response::Response {
    crate::http_response::error_into_response(err)
}
