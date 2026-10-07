//! Audit log stores for commands, domain events, messages, and activities.

/// Activity; module.
pub mod activity;
/// Bundle; module.
pub mod bundle;
/// Config; module.
pub mod config;
/// Context; module.
pub mod context;
pub mod idempotency;
/// Model; module.
pub mod model;
/// Noop; module.
pub mod noop;
/// Outbox; module.
pub mod outbox;
/// Response; module.
pub mod response;
pub mod retention;
/// Store; module.
pub mod store;
/// Types; module.
pub mod types;
/// Util; module.
pub mod util;

pub use crate::datastore::postgres::logstore::{
    new_id, PostgresDomainLogStore, PostgresIdempotencyStore,
};
pub use activity::{append_activity, emit_query_activity, ActivityEmitParams, ActivityEmitter};
pub use bundle::DomainLogStore;
pub use config::{parse_bool_env, AuditLogConfig};
pub use context::{append_command_context, append_query_context};
pub use idempotency::{
    idempotency_request_hash, ClaimOutcome, IdempotencyClaim, IdempotencyScope, IdempotencyStore,
};
pub use model::{
    ActivityLogRecord, CommandLogRecord, ContextLogRecord, EventLogRecord, LogRowMeta,
    MessageLogRecord, OutboxRecord, QueryLogRecord, ResponseRecord,
};
pub use noop::{NoOpLogStore, NoOpResponseLogStore};
pub use outbox::{NoOpOutboxStore, OutboxStore};
pub use response::ResponseLogStore;
pub use retention::{purge_audit_logs, AuditRetentionConfig};
pub use store::{append_many, CommandStatusLogStore, LogStore};
pub use types::{ActivityMsgType, CommandLogStatus, DomainTransport, QueryLogStatus};
pub use util::{log_uuid_from_command_id, new_log_id, parse_optional_uuid, scope_uuid};

/// Context / request audit log.
pub type ContextLogStore = dyn LogStore<ContextLogRecord>;
/// Command envelope audit log.
pub type CommandLogStore = dyn CommandStatusLogStore;
/// Domain event audit log.
pub type EventLogStore = dyn LogStore<EventLogRecord>;
/// Integration message audit log.
pub type MessageLogStore = dyn LogStore<MessageLogRecord>;
/// Activity audit log.
pub type ActivityLogStore = dyn LogStore<ActivityLogRecord>;
/// Query request audit log.
pub type QueryLogStore = dyn LogStore<QueryLogRecord>;
