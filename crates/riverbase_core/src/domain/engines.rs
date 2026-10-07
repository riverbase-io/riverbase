use std::sync::Arc;

use crate::base::{Engine, EngineContext, RiverbaseResult, ScopeMeta};
use crate::command::{BatchExecuteResult, CommandMeta, CommandTarget, PreparedCommand};
use crate::domain::DomainMeta;
use crate::query::{QueryAccess, QueryRequest};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::marker::PhantomData;

/// Domain command surface: one engine per domain command slot.
#[async_trait]
pub trait DomainCommandEngine: Send + Sync {
    /// Execution context identifier.
    fn context(&self) -> &EngineContext;

    /// Namespace.
    fn namespace(&self) -> &str {
        self.context().namespace()
    }

    /// Ensure namespace.
    fn ensure_namespace(&self, expected: &str) -> RiverbaseResult<()> {
        self.context().ensure_namespace(expected)
    }

    /// Domain identity for discovery and OpenAPI (`GET …/domain.meta`).
    fn domain_meta(&self) -> DomainMeta {
        let title = self
            .context()
            .title()
            .map(str::to_string)
            .unwrap_or_else(|| self.namespace().to_string());
        DomainMeta::new(self.namespace(), title)
    }

    /// Commands.
    async fn commands(&self) -> RiverbaseResult<Vec<String>>;

    /// Command meta.
    fn command_meta(&self, cmdkey: &str) -> Option<CommandMeta>;

    /// Per-command info for `GET …/{cmdkey}.meta` (`command:meta`).
    fn command_info(&self, cmdkey: &str) -> Option<Value> {
        let meta = self.command_meta(cmdkey)?;
        Some(meta.command_info_document(
            json!({
                "type": "object",
                "additionalProperties": true
            }),
            json!({
                "type": "object",
                "additionalProperties": true
            }),
        ))
    }

    /// Execute.
    async fn execute(
        &self,
        ctx: &EngineContext,
        cmdkey: &str,
        payload: Value,
        target: CommandTarget,
    ) -> RiverbaseResult<Value>;

    /// Run several prepared commands in one host command transaction (internal; no HTTP batch API).
    ///
    /// Default: [`crate::errors::CMD_029`]. [`crate::command::CommandEngine`] implements this for
    /// `river_culvert` `execute-command-batch` and similar in-process callers.
    async fn execute_prepared_batch(
        &self,
        _ctx: &EngineContext,
        _items: Vec<PreparedCommand>,
    ) -> RiverbaseResult<BatchExecuteResult> {
        Err(crate::errors::CMD_029.with_data(""))
    }
}

/// Domain query surface: one engine per domain query slot.
#[async_trait]
pub trait DomainQueryEngine: Send + Sync {
    /// Execution context identifier.
    fn context(&self) -> &EngineContext;

    /// Namespace.
    fn namespace(&self) -> &str {
        self.context().namespace()
    }

    /// Ensure namespace.
    fn ensure_namespace(&self, expected: &str) -> RiverbaseResult<()> {
        self.context().ensure_namespace(expected)
    }

    /// Queries.
    async fn queries(&self) -> RiverbaseResult<Vec<String>>;

    /// Query scope metas.
    async fn query_scope_metas(&self) -> RiverbaseResult<Vec<crate::query::QueryRouteMeta>>;

    /// Back-compat alias for [`Self::query_scope_metas`] (name + scope only).
    async fn query_http_metas(&self) -> RiverbaseResult<Vec<(String, ScopeMeta)>> {
        Ok(self
            .query_scope_metas()
            .await?
            .into_iter()
            .map(|m| (m.resource, m.scope))
            .collect())
    }

    /// Execute.
    async fn execute(
        &self,
        ctx: &EngineContext,
        resource: &str,
        access: QueryAccess,
        request: QueryRequest,
        item_id: Option<&str>,
    ) -> RiverbaseResult<Value>;
}

/// Optional domain service actor / message surface.
///
/// Requires the low-level [`Engine`] trait, so a service engine reports
/// `kind() == EngineKind::Service` and exposes its capability keys via `items()`.
#[async_trait]
pub trait DomainServiceEngine: Engine {
    /// Execution context identifier.
    fn context(&self) -> &EngineContext;

    /// Namespace.
    fn namespace(&self) -> &str {
        self.context().namespace()
    }

    /// Opaque JSON dispatch for transport extensions.
    async fn dispatch(&self, item_key: &str, payload: Value) -> RiverbaseResult<Value>;
}

/// Zero-actor command slot for a domain whose command capability is disabled.
pub struct DisabledCommandEngine<S> {
    ctx: EngineContext,
    _store: PhantomData<fn() -> S>,
}

impl<S> DisabledCommandEngine<S> {
    /// Construct a new value.
    pub fn new(ctx: EngineContext) -> Self {
        Self {
            ctx,
            _store: PhantomData,
        }
    }
}

#[async_trait]
impl<S: Send + Sync + 'static> DomainCommandEngine for DisabledCommandEngine<S> {
    fn context(&self) -> &EngineContext {
        &self.ctx
    }

    async fn commands(&self) -> RiverbaseResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn command_meta(&self, _cmdkey: &str) -> Option<CommandMeta> {
        None
    }

    async fn execute(
        &self,
        _ctx: &EngineContext,
        cmdkey: &str,
        _payload: Value,
        _target: CommandTarget,
    ) -> RiverbaseResult<Value> {
        Err(crate::errors::DOM_015.with_data(cmdkey.to_string()))
    }

    async fn execute_prepared_batch(
        &self,
        _ctx: &EngineContext,
        _items: Vec<PreparedCommand>,
    ) -> RiverbaseResult<BatchExecuteResult> {
        Err(crate::errors::DOM_015.with_data("prepared_batch"))
    }
}

/// Merges several [`DomainCommandEngine`] implementations into one command slot.
pub struct CompositeCommandEngine {
    ctx: EngineContext,
    engines: Vec<Arc<dyn DomainCommandEngine>>,
    key_index: Vec<(String, usize)>,
}

impl CompositeCommandEngine {
    /// Construct a new value.
    pub async fn new(
        ctx: EngineContext,
        engines: Vec<Arc<dyn DomainCommandEngine>>,
    ) -> RiverbaseResult<Self> {
        let mut key_index = Vec::new();
        for engine in &engines {
            if engine.namespace() != ctx.namespace() {
                return Err(crate::errors::DOM_002.with_data(format!(
                    "command engine namespace mismatch: expected {}, got {}",
                    ctx.namespace(),
                    engine.namespace()
                )));
            }
        }
        for (idx, engine) in engines.iter().enumerate() {
            let keys = engine.commands().await?;
            for key in keys {
                if key_index.iter().any(|(k, _)| k == &key) {
                    return Err(crate::errors::DOM_003.with_data(format!(
                        "duplicate command key in CompositeCommandEngine: {key}"
                    )));
                }
                key_index.push((key, idx));
            }
        }
        Ok(Self {
            ctx,
            engines,
            key_index,
        })
    }

    fn child_index(&self, cmdkey: &str) -> Option<usize> {
        self.key_index
            .iter()
            .find(|(k, _)| k == cmdkey)
            .map(|(_, idx)| *idx)
    }
}

#[async_trait]
impl DomainCommandEngine for CompositeCommandEngine {
    fn context(&self) -> &EngineContext {
        &self.ctx
    }

    fn domain_meta(&self) -> DomainMeta {
        self.engines
            .first()
            .map(|engine| engine.domain_meta())
            .unwrap_or_else(|| DomainMeta::new(self.namespace(), self.namespace()))
    }

    async fn commands(&self) -> RiverbaseResult<Vec<String>> {
        let mut keys: Vec<String> = self.key_index.iter().map(|(k, _)| k.clone()).collect();
        keys.sort();
        Ok(keys)
    }

    fn command_meta(&self, cmdkey: &str) -> Option<CommandMeta> {
        self.child_index(cmdkey)
            .and_then(|idx| self.engines[idx].command_meta(cmdkey))
    }

    fn command_info(&self, cmdkey: &str) -> Option<Value> {
        self.child_index(cmdkey)
            .and_then(|idx| self.engines[idx].command_info(cmdkey))
    }

    async fn execute(
        &self,
        ctx: &EngineContext,
        cmdkey: &str,
        payload: Value,
        target: CommandTarget,
    ) -> RiverbaseResult<Value> {
        let idx = self
            .child_index(cmdkey)
            .ok_or_else(|| crate::errors::DOM_004.with_data(format!("command {cmdkey}")))?;
        self.engines[idx]
            .execute(ctx, cmdkey, payload, target)
            .await
    }

    async fn execute_prepared_batch(
        &self,
        ctx: &EngineContext,
        items: Vec<PreparedCommand>,
    ) -> RiverbaseResult<BatchExecuteResult> {
        if items.is_empty() {
            return Ok(BatchExecuteResult {
                ok: true,
                items: Vec::new(),
            });
        }
        let first_idx = self.child_index(&items[0].cmdkey).ok_or_else(|| {
            crate::errors::DOM_004.with_data(format!("command {}", items[0].cmdkey))
        })?;
        for item in items.iter().skip(1) {
            let idx = self.child_index(&item.cmdkey).ok_or_else(|| {
                crate::errors::DOM_004.with_data(format!("command {}", item.cmdkey))
            })?;
            if idx != first_idx {
                return Err(crate::errors::CMD_032.with_data(format!(
                    "command {} is not on the same engine partition as {}",
                    item.cmdkey, items[0].cmdkey
                )));
            }
        }
        self.engines[first_idx]
            .execute_prepared_batch(ctx, items)
            .await
    }
}

/// Merges several [`DomainQueryEngine`] implementations into one query slot.
pub struct CompositeQueryEngine {
    ctx: EngineContext,
    engines: Vec<Arc<dyn DomainQueryEngine>>,
    name_index: Vec<(String, usize)>,
}

impl CompositeQueryEngine {
    /// Construct a new value.
    pub async fn new(
        ctx: EngineContext,
        engines: Vec<Arc<dyn DomainQueryEngine>>,
    ) -> RiverbaseResult<Self> {
        for engine in &engines {
            if engine.namespace() != ctx.namespace() {
                return Err(crate::errors::DOM_005.with_data(format!(
                    "query engine namespace mismatch: expected {}, got {}",
                    ctx.namespace(),
                    engine.namespace()
                )));
            }
        }
        let mut name_index = Vec::new();
        for (idx, engine) in engines.iter().enumerate() {
            let names = engine.queries().await?;
            for name in names {
                if name_index.iter().any(|(n, _)| n == &name) {
                    return Err(crate::errors::DOM_006.with_data(format!(
                        "duplicate query resource in CompositeQueryEngine: {name}"
                    )));
                }
                name_index.push((name, idx));
            }
        }
        Ok(Self {
            ctx,
            engines,
            name_index,
        })
    }

    fn child_index(&self, query_resource: &str) -> Option<usize> {
        self.name_index
            .iter()
            .find(|(n, _)| n == query_resource)
            .map(|(_, idx)| *idx)
    }
}

#[async_trait]
impl DomainQueryEngine for CompositeQueryEngine {
    fn context(&self) -> &EngineContext {
        &self.ctx
    }

    async fn queries(&self) -> RiverbaseResult<Vec<String>> {
        let mut names: Vec<String> = self.name_index.iter().map(|(n, _)| n.clone()).collect();
        names.sort();
        Ok(names)
    }

    async fn query_scope_metas(&self) -> RiverbaseResult<Vec<crate::query::QueryRouteMeta>> {
        let mut metas = Vec::new();
        for engine in &self.engines {
            metas.extend(engine.query_scope_metas().await?);
        }
        Ok(metas)
    }

    async fn execute(
        &self,
        ctx: &EngineContext,
        resource: &str,
        access: QueryAccess,
        request: QueryRequest,
        item_id: Option<&str>,
    ) -> RiverbaseResult<Value> {
        let idx = self
            .child_index(resource)
            .ok_or_else(|| crate::errors::DOM_007.with_data(resource.to_string()))?;
        self.engines[idx]
            .execute(ctx, resource, access, request, item_id)
            .await
    }
}

#[async_trait]
impl<T: DomainCommandEngine + ?Sized> DomainCommandEngine for Arc<T> {
    fn context(&self) -> &EngineContext {
        (**self).context()
    }

    async fn commands(&self) -> RiverbaseResult<Vec<String>> {
        (**self).commands().await
    }

    fn command_meta(&self, cmdkey: &str) -> Option<CommandMeta> {
        (**self).command_meta(cmdkey)
    }

    fn command_info(&self, cmdkey: &str) -> Option<Value> {
        (**self).command_info(cmdkey)
    }

    async fn execute(
        &self,
        ctx: &EngineContext,
        cmdkey: &str,
        payload: Value,
        target: CommandTarget,
    ) -> RiverbaseResult<Value> {
        (**self).execute(ctx, cmdkey, payload, target).await
    }

    async fn execute_prepared_batch(
        &self,
        ctx: &EngineContext,
        items: Vec<PreparedCommand>,
    ) -> RiverbaseResult<BatchExecuteResult> {
        (**self).execute_prepared_batch(ctx, items).await
    }
}

#[async_trait]
impl<T: DomainQueryEngine + ?Sized> DomainQueryEngine for Arc<T> {
    fn context(&self) -> &EngineContext {
        (**self).context()
    }

    async fn queries(&self) -> RiverbaseResult<Vec<String>> {
        (**self).queries().await
    }

    async fn query_scope_metas(&self) -> RiverbaseResult<Vec<crate::query::QueryRouteMeta>> {
        (**self).query_scope_metas().await
    }

    async fn execute(
        &self,
        ctx: &EngineContext,
        resource: &str,
        access: QueryAccess,
        request: QueryRequest,
        item_id: Option<&str>,
    ) -> RiverbaseResult<Value> {
        (**self)
            .execute(ctx, resource, access, request, item_id)
            .await
    }
}
