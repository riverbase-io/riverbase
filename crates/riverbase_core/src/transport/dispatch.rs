use std::sync::Arc;

use crate::command::CommandTarget;
use crate::domain::{DomainCommandEngine, DomainQueryEngine};
use crate::query::{QueryAccess, QueryRequest};
use ractor::{Actor, ActorProcessingErr, ActorRef};
use serde_json::Value;

#[derive(Debug)]
/// Transport Envelope enumeration.
pub enum TransportEnvelope {
    /// Command.
    Command {
        /// Cmdkey.
        cmdkey: String,
        /// Command or event payload.
        payload: Value,
        /// Target.
        target: CommandTarget,
    },
    /// Query.
    Query {
        /// Resource.
        resource: String,
        /// Access.
        access: QueryAccess,
        /// Request.
        request: QueryRequest,
        /// Item id.
        item_id: Option<String>,
    },
}

/// Command dispatch actor; structure.
pub struct CommandDispatchActor;

#[ractor::async_trait]
impl Actor for CommandDispatchActor {
    type Msg = TransportEnvelope;
    type State = Arc<dyn DomainCommandEngine>;
    type Arguments = Arc<dyn DomainCommandEngine>;

    async fn pre_start(
        &self,
        _selfref: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(args)
    }

    async fn handle(
        &self,
        _selfref: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let TransportEnvelope::Command {
            cmdkey,
            payload,
            target,
        } = message
        {
            state
                .execute(state.context(), &cmdkey, payload, target)
                .await
                .map_err(|e| ActorProcessingErr::from(e.to_string()))?;
        }
        Ok(())
    }
}

/// Query dispatch actor; structure.
pub struct QueryDispatchActor;

#[ractor::async_trait]
impl Actor for QueryDispatchActor {
    type Msg = TransportEnvelope;
    type State = Arc<dyn DomainQueryEngine>;
    type Arguments = Arc<dyn DomainQueryEngine>;

    async fn pre_start(
        &self,
        _selfref: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(args)
    }

    async fn handle(
        &self,
        _selfref: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let TransportEnvelope::Query {
            resource,
            access,
            request,
            item_id,
        } = message
        {
            state
                .execute(
                    state.context(),
                    &resource,
                    access,
                    request,
                    item_id.as_deref(),
                )
                .await
                .map_err(|e| ActorProcessingErr::from(e.to_string()))?;
        }
        Ok(())
    }
}
