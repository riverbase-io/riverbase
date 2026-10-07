use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::aggregate::Aggregate;
use super::message::CommandMessage;
use super::meta::CommandMeta;
use super::outcome::CommandDispatchResult;
use super::typed::{erase_handler, TypedCommandHandler};
use crate::base::{AggregateContext, RiverbaseResult};
use crate::datastore::DataStore;
use crate::logstore::DomainLogStore;
use serde_json::Value;

#[async_trait]
/// Erased command handler trait.
pub trait ErasedCommandHandler<S, A>: Send + Sync
where
    S: DataStore,
    A: Aggregate<S>,
{
    /// Handle.
    async fn handle(
        &self,
        statemgr: Arc<S>,
        logstore: DomainLogStore,
        message: CommandMessage,
        context: &AggregateContext,
    ) -> RiverbaseResult<CommandDispatchResult>;

    /// Command meta.
    fn command_meta(&self) -> CommandMeta;

    /// Per-command info (`key`, `name`, `description`, `schema`, `resources`).
    fn command_info(&self) -> Value;
}

/// Command registry structure.
pub struct CommandRegistry<S, A>
where
    S: DataStore,
    A: Aggregate<S>,
{
    handlers: HashMap<String, Arc<dyn ErasedCommandHandler<S, A>>>,
}

impl<S, A> Clone for CommandRegistry<S, A>
where
    S: DataStore + 'static,
    A: Aggregate<S>,
{
    fn clone(&self) -> Self {
        Self {
            handlers: self.handlers.clone(),
        }
    }
}

impl<S, A> Default for CommandRegistry<S, A>
where
    S: DataStore + 'static,
    A: Aggregate<S>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S, A> CommandRegistry<S, A>
where
    S: DataStore + 'static,
    A: Aggregate<S>,
{
    /// Construct a new value.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register.
    pub fn register(
        &mut self,
        cmdkey: impl Into<String>,
        handler: Arc<dyn ErasedCommandHandler<S, A>>,
    ) {
        self.handlers.insert(cmdkey.into(), handler);
    }

    /// Register using [`TypedCommandHandler::meta`] key (single source of truth).
    pub fn register_typed<H>(&mut self, handler: H)
    where
        H: TypedCommandHandler<A> + 'static,
    {
        let key = handler.meta().key.clone();
        self.register(key, erase_handler::<S, A, H>(handler));
    }

    /// Merge.
    pub fn merge(&mut self, other: CommandRegistry<S, A>) {
        self.handlers.extend(other.handlers);
    }

    /// Dispatch.
    pub async fn dispatch(
        &self,
        statemgr: Arc<S>,
        logstore: DomainLogStore,
        message: CommandMessage,
        context: &AggregateContext,
    ) -> RiverbaseResult<CommandDispatchResult> {
        let handler = self.handlers.get(&message.cmdkey).ok_or_else(|| {
            crate::errors::CMD_018.with_data(format!("command {}", message.cmdkey))
        })?;
        handler.handle(statemgr, logstore, message, context).await
    }

    /// Cmdkeys.
    pub fn cmdkeys(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }

    /// Command meta.
    pub fn command_meta(&self, cmdkey: &str) -> Option<CommandMeta> {
        self.handlers.get(cmdkey).map(|h| h.command_meta())
    }

    /// Command info.
    pub fn command_info(&self, cmdkey: &str) -> Option<Value> {
        self.handlers.get(cmdkey).map(|h| h.command_info())
    }
}
