//! Cross-domain command and query routing.
//!
//! [`CommandInvoker`] and [`QueryInvoker`] are in-process registries of mounted domain engines,
//! keyed by namespace. Saga handlers use them to invoke sibling domains. Both are carried by
//! [`DomainRuntime`](crate::domain::DomainRuntime) and threaded into every domain spawn.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::base::{EngineContext, RiverbaseResult, InvocationMode};
use crate::command::{BatchExecuteResult, CommandMeta, CommandTarget, PreparedCommand};
use crate::domain::{DomainCommandEngine, DomainQueryEngine};
use crate::query::{QueryAccess, QueryRequest};

/// Re-evaluates command authority against the *target* domain ([SEC-08], [SEC-11], ADR 006).
#[async_trait::async_trait]
pub trait CommandActivityGate: Send + Sync {
    /// Authorize.
    async fn authorize(
        &self,
        ctx: &EngineContext,
        namespace: &str,
        cmdkey: &str,
        meta: Option<&CommandMeta>,
    ) -> RiverbaseResult<()>;
}

const MAX_INVOCATION_DEPTH: u16 = 32;

/// In-process registry of mounted domain command engines, keyed by namespace.
///
/// Populated as domains are mounted; saga handlers use it to invoke commands on sibling domains.
#[derive(Default)]
pub struct CommandInvoker {
    engines: RwLock<HashMap<String, Arc<dyn DomainCommandEngine>>>,
    activity_gate: RwLock<Option<Arc<dyn CommandActivityGate>>>,
}

impl CommandInvoker {
    /// Construct a new value.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Register.
    pub fn register(&self, namespace: &str, engine: Arc<dyn DomainCommandEngine>) {
        self.engines
            .write()
            .expect("command invoker lock")
            .insert(namespace.to_string(), engine);
    }

    /// Install the target-domain activity gate used by [`Self::execute`] ([SEC-11]).
    pub fn set_activity_gate(&self, gate: Arc<dyn CommandActivityGate>) {
        *self.activity_gate.write().expect("command invoker lock") = Some(gate);
    }

    /// Snapshot mounted commands for startup Casbin cross-check ([SEC-08]).
    pub async fn command_catalog(&self) -> RiverbaseResult<Vec<(String, CommandMeta)>> {
        let engines: Vec<(String, Arc<dyn DomainCommandEngine>)> = self
            .engines
            .read()
            .expect("command invoker lock")
            .iter()
            .map(|(namespace, engine)| (namespace.clone(), engine.clone()))
            .collect();
        let mut catalog = Vec::new();
        for (namespace, engine) in engines {
            for key in engine.commands().await? {
                let meta = engine
                    .command_meta(&key)
                    .unwrap_or_else(|| CommandMeta::object(&key, &key));
                catalog.push((namespace.clone(), meta));
            }
        }
        Ok(catalog)
    }

    /// Execute.
    pub async fn execute(
        &self,
        ctx: &EngineContext,
        namespace: &str,
        cmdkey: &str,
        payload: Value,
        target: CommandTarget,
    ) -> RiverbaseResult<Value> {
        let engine = self
            .engines
            .read()
            .expect("command invoker lock")
            .get(namespace)
            .cloned()
            .ok_or_else(|| crate::errors::FLR_010.with_data(namespace))?;

        if ctx.invocation_depth >= MAX_INVOCATION_DEPTH {
            return Err(crate::errors::FLR_011.with_data(serde_json::json!({
                "namespace": namespace,
                "command": cmdkey,
                "depth": ctx.invocation_depth,
            })));
        }
        if ctx.invocation_mode == InvocationMode::Service && ctx.service_capability.is_none() {
            return Err(crate::errors::FLR_012
                .with_data(serde_json::json!({ "namespace": namespace, "command": cmdkey })));
        }

        let meta = engine.command_meta(cmdkey);
        let gate = self
            .activity_gate
            .read()
            .expect("command invoker lock")
            .clone();
        if let Some(gate) = gate {
            gate.authorize(ctx, namespace, cmdkey, meta.as_ref())
                .await?;
        }

        let target_context = engine.context();
        let mut child = ctx.clone();
        child.namespace = engine.namespace().to_string();
        child.title = target_context.title.clone();
        child.causation_id = ctx
            .parent_command_id
            .clone()
            .or_else(|| ctx.causation_id.clone());
        child.parent_command_id = child.causation_id.clone();
        child.invocation_depth = ctx.invocation_depth + 1;
        child.idempotency_key = ctx
            .idempotency_key
            .as_deref()
            .map(|parent_key| child_idempotency_key(parent_key, namespace, cmdkey, &target));

        engine.execute(&child, cmdkey, payload, target).await
    }

    /// Execute a prepared command batch on a target domain in one host transaction.
    pub async fn execute_prepared_batch(
        &self,
        ctx: &EngineContext,
        namespace: &str,
        items: Vec<PreparedCommand>,
    ) -> RiverbaseResult<BatchExecuteResult> {
        let engine = self
            .engines
            .read()
            .expect("command invoker lock")
            .get(namespace)
            .cloned()
            .ok_or_else(|| crate::errors::FLR_010.with_data(namespace))?;

        if ctx.invocation_depth >= MAX_INVOCATION_DEPTH {
            return Err(crate::errors::FLR_011.with_data(serde_json::json!({
                "namespace": namespace,
                "depth": ctx.invocation_depth,
                "batch_size": items.len(),
            })));
        }
        if ctx.invocation_mode == InvocationMode::Service && ctx.service_capability.is_none() {
            return Err(crate::errors::FLR_012.with_data(serde_json::json!({
                "namespace": namespace,
                "batch_size": items.len(),
            })));
        }

        let gate = self
            .activity_gate
            .read()
            .expect("command invoker lock")
            .clone();
        if let Some(gate) = gate {
            for item in &items {
                let meta = engine.command_meta(&item.cmdkey);
                gate.authorize(ctx, namespace, &item.cmdkey, meta.as_ref())
                    .await?;
            }
        }

        let target_context = engine.context();
        let mut child = ctx.clone();
        child.namespace = engine.namespace().to_string();
        child.title = target_context.title.clone();
        child.causation_id = ctx
            .parent_command_id
            .clone()
            .or_else(|| ctx.causation_id.clone());
        child.parent_command_id = child.causation_id.clone();
        child.invocation_depth = ctx.invocation_depth + 1;
        child.idempotency_key = None;

        engine.execute_prepared_batch(&child, items).await
    }

    /// Invoke a target command as a declared service capability.
    pub async fn execute_as_service(
        &self,
        ctx: &EngineContext,
        capability: impl Into<String>,
        namespace: &str,
        cmdkey: &str,
        payload: Value,
        target: CommandTarget,
    ) -> RiverbaseResult<Value> {
        let service_ctx = ctx.clone().as_service(capability);
        self.execute(&service_ctx, namespace, cmdkey, payload, target)
            .await
    }
}

/// In-process registry of mounted domain query engines, keyed by namespace.
///
/// Used for decision reads and report resources across bounded contexts (for example checkout
/// quoting) without exposing sibling write commands.
#[derive(Default)]
pub struct QueryInvoker {
    engines: RwLock<HashMap<String, Arc<dyn DomainQueryEngine>>>,
}

impl QueryInvoker {
    /// Construct a new value.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Register.
    pub fn register(&self, namespace: &str, engine: Arc<dyn DomainQueryEngine>) {
        self.engines
            .write()
            .expect("query invoker lock")
            .insert(namespace.to_string(), engine);
    }

    /// Execute.
    pub async fn execute(
        &self,
        ctx: &EngineContext,
        namespace: &str,
        resource: &str,
        access: QueryAccess,
        request: QueryRequest,
        item_id: Option<&str>,
    ) -> RiverbaseResult<Value> {
        let engine = self
            .engines
            .read()
            .expect("query invoker lock")
            .get(namespace)
            .cloned()
            .ok_or_else(|| crate::errors::FLR_013.with_data(namespace))?;

        let target_context = engine.context();
        let mut child = ctx.clone();
        child.namespace = engine.namespace().to_string();
        child.title = target_context.title.clone();
        engine
            .execute(&child, resource, access, request, item_id)
            .await
    }

    /// Execute a report resource with the given params object.
    pub async fn report(
        &self,
        ctx: &EngineContext,
        namespace: &str,
        resource: &str,
        params: Value,
    ) -> RiverbaseResult<Value> {
        let mut request = QueryRequest::default();
        request.params = Some(params);
        self.execute(ctx, namespace, resource, QueryAccess::Report, request, None)
            .await
    }
}

fn child_idempotency_key(
    parent_key: &str,
    namespace: &str,
    cmdkey: &str,
    target: &CommandTarget,
) -> String {
    let mut digest = Sha1::new();
    digest.update(parent_key.as_bytes());
    digest.update([0]);
    digest.update(namespace.as_bytes());
    digest.update([0]);
    digest.update(cmdkey.as_bytes());
    digest.update([0]);
    digest.update(serde_json::to_vec(target).unwrap_or_default());
    format!("child-{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::AggregateRoot;
    use crate::command::CommandMeta;
    use std::sync::Mutex;

    #[test]
    fn child_retry_key_is_target_qualified_and_stable() {
        let target = CommandTarget::Object(AggregateRoot::new("order", "order-1"));
        let first = child_idempotency_key("request-1", "exp.order", "confirm", &target);
        let again = child_idempotency_key("request-1", "exp.order", "confirm", &target);
        let other_domain = child_idempotency_key("request-1", "exp.payment", "confirm", &target);
        assert_eq!(first, again);
        assert_ne!(first, other_domain);
    }

    struct CapturingEngine {
        context: EngineContext,
        seen: Arc<Mutex<Option<EngineContext>>>,
    }

    #[async_trait::async_trait]
    impl DomainCommandEngine for CapturingEngine {
        fn context(&self) -> &EngineContext {
            &self.context
        }

        async fn commands(&self) -> RiverbaseResult<Vec<String>> {
            Ok(vec!["confirm".to_string()])
        }

        fn command_meta(&self, cmdkey: &str) -> Option<CommandMeta> {
            (cmdkey == "confirm")
                .then(|| CommandMeta::object("confirm", "Confirm").with_resources(["order"]))
        }

        async fn execute(
            &self,
            ctx: &EngineContext,
            _cmdkey: &str,
            _payload: Value,
            _target: CommandTarget,
        ) -> RiverbaseResult<Value> {
            *self.seen.lock().expect("capture lock") = Some(ctx.clone());
            Ok(serde_json::json!({ "ok": true }))
        }
    }

    #[tokio::test]
    async fn invocation_rebinds_target_identity_and_preserves_actor() {
        let invoker = CommandInvoker::new();
        let seen = Arc::new(Mutex::new(None));
        invoker.register(
            "exp.order",
            Arc::new(CapturingEngine {
                context: EngineContext::new("exp.order").with_title("Orders"),
                seen: seen.clone(),
            }),
        );
        let profile_id = uuid::Uuid::new_v4();
        let parent = EngineContext::new("exp.checkout")
            .with_title("Checkout")
            .with_profile_id(profile_id)
            .with_parent_command_id("parent-command")
            .with_idempotency_key("request-key");
        invoker
            .execute(
                &parent,
                "exp.order",
                "confirm",
                serde_json::json!({}),
                CommandTarget::Object(AggregateRoot::new("order", "order-1")),
            )
            .await
            .expect("invoke");

        let child = seen
            .lock()
            .expect("capture lock")
            .clone()
            .expect("captured context");
        assert_eq!(child.namespace, "exp.order");
        assert_eq!(child.title.as_deref(), Some("Orders"));
        assert_eq!(child.actor.profile_id, Some(profile_id));
        assert_eq!(child.causation_id.as_deref(), Some("parent-command"));
        assert_eq!(child.parent_command_id.as_deref(), Some("parent-command"));
        assert_eq!(child.invocation_depth, 1);
        assert_ne!(child.idempotency_key.as_deref(), Some("request-key"));
    }

    struct DenyUnlessCapability;

    #[async_trait::async_trait]
    impl CommandActivityGate for DenyUnlessCapability {
        async fn authorize(
            &self,
            ctx: &EngineContext,
            namespace: &str,
            cmdkey: &str,
            _meta: Option<&CommandMeta>,
        ) -> RiverbaseResult<()> {
            if ctx.invocation_mode == InvocationMode::Service {
                if let Some(cap) = &ctx.service_capability {
                    if cap == "supplier.register" || cap == "exp.supplier.register" {
                        return Ok(());
                    }
                }
            }
            if ctx.roles.iter().any(|role| role == "supplier-admin") {
                return Ok(());
            }
            Err(crate::errors::CAS_010
                .with_data(serde_json::json!({ "namespace": namespace, "command": cmdkey })))
        }
    }

    #[tokio::test]
    async fn invoker_denies_target_command_without_activity() {
        let invoker = CommandInvoker::new();
        invoker.set_activity_gate(Arc::new(DenyUnlessCapability));
        invoker.register(
            "exp.supplier",
            Arc::new(CapturingEngine {
                context: EngineContext::new("exp.supplier").with_title("Supplier"),
                seen: Arc::new(Mutex::new(None)),
            }),
        );
        let mut parent = EngineContext::new("exp.sourcing");
        parent.roles = vec!["sourcing-buyer".into()];
        let err = invoker
            .execute(
                &parent,
                "exp.supplier",
                "confirm",
                serde_json::json!({}),
                CommandTarget::Object(AggregateRoot::new("supplier", "s-1")),
            )
            .await
            .expect_err("denied");
        assert_eq!(err.errcode.as_str(), "CAS-010");
    }

    #[tokio::test]
    async fn invoker_allows_explicit_service_capability() {
        let invoker = CommandInvoker::new();
        invoker.set_activity_gate(Arc::new(DenyUnlessCapability));
        let seen = Arc::new(Mutex::new(None));
        invoker.register(
            "exp.supplier",
            Arc::new(CapturingEngine {
                context: EngineContext::new("exp.supplier").with_title("Supplier"),
                seen: seen.clone(),
            }),
        );
        let mut parent = EngineContext::new("exp.sourcing");
        parent.roles = vec!["sourcing-buyer".into()];
        invoker
            .execute_as_service(
                &parent,
                "supplier.register",
                "exp.supplier",
                "confirm",
                serde_json::json!({}),
                CommandTarget::Object(AggregateRoot::new("supplier", "s-1")),
            )
            .await
            .expect("service capability grants target");
        assert!(seen.lock().expect("capture lock").is_some());
    }
}
