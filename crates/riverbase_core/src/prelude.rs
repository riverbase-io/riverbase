//! Common imports for a domain crate.
//!
//! ```ignore
//! use riverbase_core::prelude::*;
//! ```
//!
//! Add crate-local types (`TodoCommandEngine`, row structs, …) beside this glob.

#![warn(missing_docs)]

pub use crate::base::{
    domain_fields_from_payload, AggregateContext, AggregateRoot, Engine, EngineContext,
    RiverbaseError, RiverbaseResult, ForbiddenError, InvalidArgumentError, NotFoundError,
};
pub use crate::command::{
    Aggregate, AggregateCore, CommandEngine, CommandTarget, MessageBus, TypedCommandHandler,
};
pub use crate::datastore::{DataStore, Expr, PgDataStore, ResourceKey, ResourceRegistry};
pub use crate::domain::{
    DataStoreInit, DisabledCommandEngine, Domain, DomainCommandEngine, DomainQueryEngine,
    DomainRuntime, DomainStoreSpec,
};
pub use crate::query::{PolicyRequirement, QueryEngine, QueryEngineArgs, QueryResource};

pub use crate::{
    command, command_engine, diesel_table_with_domain_fields, domain, domain_json_converters,
    domain_row, pg_domain_entity, query_engine, query_resource, report_resource, resource_name,
};
