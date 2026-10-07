//! Command service contracts (write side).

/// Aggregate; module.
pub mod aggregate;
/// Internal prepared-command batch UoW; module.
pub mod batch;
/// Engine; module.
pub mod engine;
/// Message; module.
pub mod message;
/// Meta; module.
pub mod meta;
/// Msgbus; module.
pub mod msgbus;
/// Outbox; module.
pub mod outbox;
/// Outcome; module.
pub mod outcome;
/// Payload; module.
pub mod payload;
/// Policy; module.
pub mod policy;
/// Process; module.
pub mod process;
/// Process worker; module.
pub mod process_worker;
/// Registry; module.
pub mod registry;
/// Target; module.
pub mod target;
/// Typed; module.
pub mod typed;

pub use crate::base::{AggregateContext, AuditActor, ScopeMap, ScopeMeta};
pub use crate::logstore::DomainLogStore;
pub use aggregate::{
    domain_action_event_keys, missing_action_event_keys, Aggregate, AggregateCore,
};
pub use batch::{BatchExecuteResult, BatchItemOutcome, BatchItemStatus, PreparedCommand};
pub use engine::{CommandEngine, CommandEngineArgs};
pub use message::CommandMessage;
pub use meta::{authorize_command_roles, CommandAuthz, CommandKind, CommandMeta};
pub use msgbus::MessageBus;
pub use outbox::{deliver_outbox_batch, spawn_outbox_publisher};
pub use outcome::CommandDispatchResult;
pub use payload::{CommandPayload, JsonObjectPayload};
pub use policy::{CommandPolicy, DefaultCommandPolicy};
pub use process::{process_step_idempotency_key, ProcessManagerStore, ProcessState, ProcessStatus};
pub use process_worker::{
    deliver_process_manager_batch, spawn_process_manager_worker, ProcessManagerRegistry,
    ProcessWorker,
};
pub use registry::{CommandRegistry, ErasedCommandHandler};
pub use target::CommandTarget;
pub use typed::{
    erase_handler, generic_object_schema, normalize_payload_schema, payload_json_schema,
    TypedCommandHandler,
};
