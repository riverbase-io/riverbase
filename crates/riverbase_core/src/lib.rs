//! Riverbase domain platform for Rust — command/query engines, persistence, and transport.

#![warn(missing_docs)]

/// Structured application logging.
pub mod applog;
/// Shared kernel types: errors, identifiers, aggregates, and engine context.
pub mod base;
#[cfg(feature = "cfgfetch")]
/// Remote configuration fetch adapters (S3 / OCI).
pub mod cfgfetch;
/// Configuration file formats and loaders.
pub mod cfgfmt;
/// Command engine, handlers, and message-bus types.
pub mod command;
/// Application configuration schema and loading.
pub mod config;
/// Persistence traits, Postgres store, and resource keys.
pub mod datastore;
/// Domain composition macros and runtime.
pub mod domain;
/// Crate error catalogue.
pub mod errors;
#[cfg(feature = "http")]
/// HTTP compatibility helpers used by `domain!` route expansion.
pub mod http_compat;
/// Kernel re-exports for domain crates ([ARC-03]).
pub mod kernel;
/// Application log persistence.
pub mod logstore;
/// OpenTelemetry spike stub ([OPS-05]).
pub mod otel;
/// Connection-pool helpers.
pub mod pool;
/// Common imports for domain crates (`use riverbase_core::prelude::*;`).
pub mod prelude;
/// Query engine, resources, and policy types.
pub mod query;
/// Message transport and outbox.
pub mod transport;
/// Shared utilities (API paths, OpenAPI metadata, identifiers).
pub mod util;

/// Re-exported for `domain! { routes { … } }` when the `http` feature is enabled.
#[cfg(feature = "http")]
#[doc(hidden)]
pub use axum;

/// Declarative macros (`diesel_table_with_domain_fields!`, …). Proc-macros: [`riverbase_proc`].
pub mod r#macro;

pub use r#macro::{DOMAIN_FIELDS_DDL, DOMAIN_FIELD_COLUMNS};

/// Re-exported for declarative macros (`query_resource!`, …) expanded in downstream crates.
#[doc(hidden)]
pub use paste;

/// Re-export for command payload `#[derive(Validate)]` in domain crates.
pub use garde;
/// Re-export for command payload `#[derive(JsonSchema)]` in domain crates.
pub use schemars;

// Ergonomic re-exports matching the former `flrs-*` crate roots.
pub use applog::{init_logging, init_logging_from_config};
pub use base::{
    apply_tenant_stamp, authorize_tenant_transfer, carry_row_tenant,
    domain_fields_for_entity_insert, domain_fields_from_json, domain_fields_from_payload, fq,
    parse_fq, realm_uuid, tenant_id_from_config, tenant_uuid, uuid_from_text,
    with_json_domain_meta, with_json_etag, with_json_tenant, AggregateContext, AggregateRoot,
    AuditActor, BadRequestError, CommandId, ConfigError, ConflictError, DomainFields,
    DomainTenantLookup, Engine, EngineContext, EngineKind, EntityId, ErrorSpec, RiverbaseError,
    RiverbaseErrorCode, RiverbaseResult, ForbiddenError, FqName, IntoErrorData, InvalidArgumentError,
    InvocationMode, JsonMap, Namespace, NotFoundError, ProblemDetails, ScopeMap, ScopeMeta,
    ServiceMode, StorageError, TenantAccess, TenantAccessContext, TenantAccessKind,
    TenantPolicyResolver, TenantTransfer, TrackerId, ACCESS_DOMAIN_TENANT, ACCESS_PROFILE,
    ACCESS_PROFILE_ORGANIZATION, ACCESS_SYSTEM, ACCESS_USER, API_CONTRACT_VERSION, BASE_NAMESPACE,
    DEFAULT_TENANT_ACCESS_POLICY, DEFAULT_TENANT_STAMP_POLICY, NAME_UUID_NAMESPACE,
    STAMP_DOMAIN_TENANT, STAMP_PROFILE, STAMP_PROFILE_ORGANIZATION, STAMP_USER,
};
#[cfg(feature = "cfgfetch")]
pub use cfgfetch::{
    fetch_and_decrypt, resolve_source, ConfigSource, RemoteConfigSpec, SopsMode, ENV_CONFIG_FORMAT,
    ENV_CONFIG_SOPS, ENV_CONFIG_URI,
};
pub use cfgfmt::{
    parse_hcl_process_documents, parse_hcl_to_json, parse_to_json, read_config_text,
    validate_instance, ConfigFormat,
};
pub use command::{
    deliver_outbox_batch, deliver_process_manager_batch, domain_action_event_keys, erase_handler,
    missing_action_event_keys, process_step_idempotency_key, spawn_outbox_publisher,
    spawn_process_manager_worker, Aggregate, AggregateCore, BatchExecuteResult, BatchItemOutcome,
    BatchItemStatus, CommandAuthz, CommandDispatchResult, CommandEngine, CommandEngineArgs,
    CommandKind, CommandMessage, CommandMeta, CommandPayload, CommandPolicy, CommandRegistry,
    CommandTarget, DefaultCommandPolicy, ErasedCommandHandler, JsonObjectPayload, MessageBus,
    PreparedCommand, ProcessManagerRegistry, ProcessManagerStore, ProcessState, ProcessStatus,
    ProcessWorker, TypedCommandHandler,
};
pub use config::{
    discover_config_path, AuditLogConfig, AuthConfig, AuthProvider, CasbinConfig,
    ConfigPayloadFormat, RiverbaseConfig, LinkTokenConfig, LogFormat, MockUser, CONFIG_SECTION,
    ENV_CONFIG_PATH, ENV_PREFIX,
};
pub use datastore::{
    entity, establish_dbpool, exec, merge_json, CommandTransaction, CommandUnitOfWork, DataError,
    DataQuery, DataResult, DataStore, ErasedEntity, Expr, FieldPath, OrderDirection, OrderSpec,
    PageSpec, PgDataStore, PgPool, PostgresProcessManagerStore, PredicateOp, Projection,
    ResourceKey, ResourceName, ResourceRegistry,
};
pub use domain::{
    build_domain_manifest, CommandActivityGate, CommandContract, CommandInvoker,
    CompositeCommandEngine, CompositeQueryEngine, DataStoreInit, DbConnection, DecisionReadPort,
    DisabledCommandEngine, Domain, DomainCommandEngine, DomainManifest, DomainMeta,
    DomainQueryEngine, DomainRuntime, DomainServiceEngine, DomainStoreSpec, QueryContract,
    QueryInvoker,
};
pub use logstore::{
    append_activity, append_many, log_uuid_from_command_id, new_log_id, parse_bool_env,
    parse_optional_uuid, scope_uuid, ActivityEmitParams, ActivityEmitter, ActivityLogRecord,
    ActivityLogStore, ActivityMsgType, CommandLogRecord, CommandLogStatus, CommandLogStore,
    CommandStatusLogStore, ContextLogRecord, ContextLogStore, DomainLogStore, DomainTransport,
    EventLogRecord, EventLogStore, LogRowMeta, MessageLogRecord, MessageLogStore, NoOpLogStore,
    NoOpOutboxStore, NoOpResponseLogStore, OutboxRecord, OutboxStore, PostgresDomainLogStore,
    QueryLogRecord, QueryLogStatus, QueryLogStore, ResponseLogStore, ResponseRecord,
};
pub use otel::init_otel;
pub use pool::{
    clamp_actor_pool_size, clamp_command_pool_size, clamp_query_pool_size, DEFAULT_ACTOR_POOL_SIZE,
    DEFAULT_COMMAND_POOL_SIZE, DEFAULT_QUERY_POOL_SIZE, MAX_ACTOR_POOL_SIZE, MAX_COMMAND_POOL_SIZE,
    MAX_QUERY_POOL_SIZE,
};
pub use query::{
    append_query_log, PolicyDecision, PolicyRequirement, QueryEngine, QueryEngineArgs,
    QueryResource, QuerySession, ReportOutput,
};
pub use util::api_path;
pub use util::openapi_meta;
pub use util::OpenApiMeta;
