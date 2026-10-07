//! [`RxdbCollection`] — the abstraction for RxDB replication endpoints.
//!
//! Mirrors the Python `riverbase.rxdb.RxdbReplicationService`: a collection
//! exposes `pull` / `push` callbacks, an optional `authorize_scope` hook, and
//! a few metadata accessors. The HTTP wiring (pull/push routes + change
//! notifications) lives in [`super::router`] and [`super::service`].

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::Principal;
use crate::base::{RiverbaseResult, JsonMap};

/// Default page size for a pull when the client omits `limit`.
pub const DEFAULT_PULL_LIMIT: usize = 100;
/// Upper bound for a pull page size.
pub const MAX_PULL_LIMIT: usize = 1_000;
/// Default resource name used in authorization messages.
pub const DEFAULT_RESOURCE_NAME: &str = "rxdb-replication";

/// Authentication / authorization context handed to collection callbacks.
///
/// The optional [`Principal`] is populated from the request extensions when an
/// authentication layer (e.g. [`crate::web::with_jwt_auth`]) is mounted ahead
/// of the RxDB routes; otherwise it is `None`.
#[derive(Debug, Clone, Default)]
pub struct RxdbContext {
    principal: Option<Principal>,
}

impl RxdbContext {
    /// Construct a new value.
    pub fn new(principal: Option<Principal>) -> Self {
        Self { principal }
    }

    /// Authenticated subject, when present.
    pub fn principal(&self) -> Option<&Principal> {
        self.principal.as_ref()
    }

    /// Subject identifier (`sub`) of the authenticated principal, when present.
    pub fn subject(&self) -> Option<&str> {
        self.principal.as_ref().map(|p| p.subject())
    }
}

/// Result of a [`RxdbCollection::pull`]: changed documents and the next checkpoint.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct RxdbPullResult {
    /// Documents changed since the requested checkpoint.
    pub documents: Vec<Value>,
    /// Opaque checkpoint the client should send on its next pull.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<Value>,
}

impl RxdbPullResult {
    /// Construct a new value.
    pub fn new(documents: Vec<Value>, checkpoint: Option<Value>) -> Self {
        Self {
            documents,
            checkpoint,
        }
    }
}

/// Result of a [`RxdbCollection::push`]: applied change count and conflicts.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct RxdbPushResult {
    /// Number of rows successfully applied.
    pub changed: u64,
    /// Documents the server rejected as conflicting (RxDB conflict handling).
    #[serde(default)]
    pub conflicts: Vec<Value>,
    /// Optional explicit notification payload. When set (or when `changed > 0`)
    /// a change notification is published on the collection's notify channel.
    /// Not serialized in the HTTP response.
    #[serde(skip)]
    pub notify_payload: Option<Value>,
}

impl RxdbPushResult {
    /// Changed.
    pub fn changed(changed: u64) -> Self {
        Self {
            changed,
            ..Default::default()
        }
    }

    /// Set conflicts and return self.
    pub fn with_conflicts(mut self, conflicts: Vec<Value>) -> Self {
        self.conflicts = conflicts;
        self
    }

    /// Set notify payload and return self.
    pub fn with_notify_payload(mut self, payload: Value) -> Self {
        self.notify_payload = Some(payload);
        self
    }
}

/// Replication endpoint backing a single RxDB collection.
///
/// Implement this trait, then register it on a [`super::RxdbRegistry`] to mount
/// `…/<collection>/pull` and `…/<collection>/push` HTTP routes.
#[async_trait]
pub trait RxdbCollection: Send + Sync {
    /// Logical collection name; also the URL path segment for the routes.
    fn collection_name(&self) -> &str;

    /// Resource name used when composing authorization messages.
    fn resource_name(&self) -> &str {
        DEFAULT_RESOURCE_NAME
    }

    /// Whether [`RxdbCollection::authorize_scope`] should be consulted.
    fn policy_required(&self) -> bool {
        true
    }

    /// Pull documents changed since `checkpoint`, returning at most `limit`
    /// rows and the next checkpoint.
    async fn pull(
        &self,
        ctx: &RxdbContext,
        checkpoint: Option<Value>,
        limit: usize,
        scope: JsonMap,
    ) -> RiverbaseResult<RxdbPullResult>;

    /// Apply client `rows` (already normalized to document bodies).
    async fn push(
        &self,
        ctx: &RxdbContext,
        rows: Vec<Value>,
        scope: JsonMap,
    ) -> RiverbaseResult<RxdbPushResult>;

    /// Resolve the effective scope for `action` (`rxdb/<collection>/pull` or
    /// `…/push`). The default returns `base_scope` unchanged; override to merge
    /// policy-derived restrictions or to deny by returning an error.
    async fn authorize_scope(
        &self,
        _ctx: &RxdbContext,
        _action: &str,
        base_scope: JsonMap,
    ) -> RiverbaseResult<JsonMap> {
        Ok(base_scope)
    }
}

/// Request body for a pull (`POST …/<collection>/pull`).
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct RxdbPullRequest {
    /// Opaque checkpoint returned by the previous pull (omit for the first pull).
    #[serde(default)]
    pub checkpoint: Option<Value>,
    /// Maximum number of documents to return (clamped to [`MAX_PULL_LIMIT`]).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Caller-supplied base scope, merged with policy restrictions.
    #[serde(default)]
    pub scope: Option<JsonMap>,
}

/// Request body for a push (`POST …/<collection>/push`).
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct RxdbPushRequest {
    /// RxDB change rows; each entry may wrap the document in `newDocumentState`
    /// or `document`.
    #[serde(default)]
    pub rows: Option<Vec<Value>>,
    /// Alternative document list (used when `rows` is absent).
    #[serde(default)]
    pub documents: Option<Vec<Value>>,
    /// Caller-supplied base scope, merged with policy restrictions.
    #[serde(default)]
    pub scope: Option<JsonMap>,
}
