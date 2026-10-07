//! Command idempotency keyed by client `Idempotency-Key` header.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use uuid::Uuid;

use crate::base::{CommandId, RiverbaseResult};
use crate::command::target::CommandTarget;

/// Scope for an idempotency record (namespace + command + client key).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyScope {
    /// Namespace.
    pub namespace: String,
    /// Command.
    pub command: String,
    /// Key.
    pub key: String,
}

impl IdempotencyScope {
    /// Construct a new value.
    pub fn new(
        namespace: impl Into<String>,
        command: impl Into<String>,
        key: impl Into<String>,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            command: command.into(),
            key: key.into(),
        }
    }
}

/// Result of attempting to claim an idempotency slot.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimOutcome {
    /// First request — proceed with command execution.
    Fresh(IdempotencyClaim),
    /// Prior request completed — replay stored response.
    Completed(Value),
    /// Same key is still being processed.
    InFlight,
    /// Same key was used with a different request body.
    Mismatch,
    /// A prior attempt reached a terminal failure.
    Failed(Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Idempotency claim structure.
pub struct IdempotencyClaim {
    /// Owner token.
    pub owner_token: Uuid,
}

impl IdempotencyClaim {
    /// Construct a new value.
    pub fn new() -> Self {
        Self {
            owner_token: Uuid::new_v4(),
        }
    }
}

impl Default for IdempotencyClaim {
    fn default() -> Self {
        Self::new()
    }
}

/// Status In Flight constant.
pub const STATUS_IN_FLIGHT: &str = "in_flight";
/// Status Completed constant.
pub const STATUS_COMPLETED: &str = "completed";
/// Status Failed constant.
pub const STATUS_FAILED: &str = "failed";
/// Default Lease constant.
pub const DEFAULT_LEASE: Duration = Duration::from_secs(5 * 60);

/// Persist and resolve command idempotency keys.
#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    /// Claim.
    async fn claim(
        &self,
        scope: &IdempotencyScope,
        actor: Option<Uuid>,
        request_hash: &str,
    ) -> RiverbaseResult<ClaimOutcome>;

    /// Complete.
    async fn complete(
        &self,
        uow: Option<&crate::datastore::CommandUnitOfWork>,
        scope: &IdempotencyScope,
        claim: &IdempotencyClaim,
        cmd_id: &CommandId,
        response: Value,
    ) -> RiverbaseResult<()>;

    /// Persist a terminal failed attempt. The key is not released because a handler may have
    /// reached an external or non-transactional side effect before returning the error.
    async fn fail(
        &self,
        uow: Option<&crate::datastore::CommandUnitOfWork>,
        scope: &IdempotencyScope,
        claim: &IdempotencyClaim,
        cmd_id: &CommandId,
        error: Value,
    ) -> RiverbaseResult<()>;
}

/// Deterministic fingerprint of command invocation for mismatch detection.
pub fn idempotency_request_hash(cmdkey: &str, payload: &Value, target: &CommandTarget) -> String {
    let target_value = match target {
        CommandTarget::Object(root) => json!({
            "kind": "object",
            "resource": root.resource,
            "identifier": root.identifier,
            "scope": root.scope,
        }),
        CommandTarget::Collection { resource, scope } => json!({
            "kind": "collection",
            "resource": resource,
            "scope": scope,
        }),
    };
    let canonical = json!({
        "cmdkey": cmdkey,
        "payload": payload,
        "target": target_value,
    });
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    format!("{:x}", Sha1::digest(bytes))
}

/// Shared idempotency store type alias.
pub type SharedIdempotencyStore = Arc<dyn IdempotencyStore>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::AggregateRoot;

    #[test]
    fn request_hash_stable_for_same_inputs() {
        let payload = json!({ "a": 1 });
        let target = CommandTarget::collection("device");
        let h1 = idempotency_request_hash("sync-device", &payload, &target);
        let h2 = idempotency_request_hash("sync-device", &payload, &target);
        assert_eq!(h1, h2);
    }

    #[test]
    fn request_hash_differs_for_different_payload() {
        let target = CommandTarget::collection("device");
        let h1 = idempotency_request_hash("sync-device", &json!({ "a": 1 }), &target);
        let h2 = idempotency_request_hash("sync-device", &json!({ "a": 2 }), &target);
        assert_ne!(h1, h2);
    }

    #[test]
    fn request_hash_differs_for_object_target() {
        let payload = json!({});
        let root = AggregateRoot {
            resource: "device".into(),
            identifier: "abc".into(),
            scope: Default::default(),
        };
        let h1 = idempotency_request_hash("x", &payload, &CommandTarget::Object(root));
        let h2 = idempotency_request_hash("x", &payload, &CommandTarget::collection("device"));
        assert_ne!(h1, h2);
    }
}
