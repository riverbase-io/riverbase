use std::sync::Arc;

use super::config::AuditLogConfig;
use super::idempotency::IdempotencyStore;
use super::model::{
    ActivityLogRecord, ContextLogRecord, EventLogRecord, MessageLogRecord, QueryLogRecord,
};
use super::noop::{NoOpLogStore, NoOpResponseLogStore};
use super::outbox::OutboxStore;
use super::response::ResponseLogStore;
use super::store::{CommandStatusLogStore, LogStore};

/// Typed log stores used by the command pipeline.
#[derive(Clone)]
pub struct DomainLogStore {
    /// Contexts.
    pub contexts: Arc<dyn LogStore<ContextLogRecord>>,
    /// Commands.
    pub commands: Arc<dyn CommandStatusLogStore>,
    /// Events.
    pub events: Arc<dyn LogStore<EventLogRecord>>,
    /// Messages.
    pub messages: Arc<dyn LogStore<MessageLogRecord>>,
    /// Activities.
    pub activities: Arc<dyn LogStore<ActivityLogRecord>>,
    /// Queries.
    pub queries: Arc<dyn LogStore<QueryLogRecord>>,
    /// Responses.
    pub responses: Arc<dyn ResponseLogStore>,
    /// Idempotency.
    pub idempotency: Arc<dyn IdempotencyStore>,
    /// Outbox.
    pub outbox: Arc<dyn OutboxStore>,
}

impl DomainLogStore {
    /// Construct a new value.
    pub fn new(
        contexts: Arc<dyn LogStore<ContextLogRecord>>,
        commands: Arc<dyn CommandStatusLogStore>,
        events: Arc<dyn LogStore<EventLogRecord>>,
        messages: Arc<dyn LogStore<MessageLogRecord>>,
        activities: Arc<dyn LogStore<ActivityLogRecord>>,
        queries: Arc<dyn LogStore<QueryLogRecord>>,
        responses: Arc<dyn ResponseLogStore>,
        idempotency: Arc<dyn IdempotencyStore>,
        outbox: Arc<dyn OutboxStore>,
    ) -> Self {
        Self {
            contexts,
            commands,
            events,
            messages,
            activities,
            queries,
            responses,
            idempotency,
            outbox,
        }
    }

    /// Enable real backends or no-op stores per [`AuditLogConfig`] channel switch.
    pub fn from_config(
        config: &AuditLogConfig,
        context: Arc<dyn LogStore<ContextLogRecord>>,
        command: Arc<dyn CommandStatusLogStore>,
        event: Arc<dyn LogStore<EventLogRecord>>,
        message: Arc<dyn LogStore<MessageLogRecord>>,
        activity: Arc<dyn LogStore<ActivityLogRecord>>,
        query: Arc<dyn LogStore<QueryLogRecord>>,
        response: Arc<dyn ResponseLogStore>,
        idempotency: Arc<dyn IdempotencyStore>,
        outbox: Arc<dyn OutboxStore>,
    ) -> Self {
        Self {
            contexts: if config.context {
                context
            } else {
                Arc::new(NoOpLogStore::new())
            },
            commands: if config.command {
                command
            } else {
                Arc::new(NoOpLogStore::new())
            },
            events: if config.event {
                event
            } else {
                Arc::new(NoOpLogStore::new())
            },
            messages: if config.message {
                message
            } else {
                Arc::new(NoOpLogStore::new())
            },
            activities: if config.activity {
                activity
            } else {
                Arc::new(NoOpLogStore::new())
            },
            queries: if config.query {
                query
            } else {
                Arc::new(NoOpLogStore::new())
            },
            responses: if config.response {
                response
            } else {
                Arc::new(NoOpResponseLogStore)
            },
            // Idempotency always uses the provided Postgres-backed store.
            idempotency,
            outbox,
        }
    }
}
