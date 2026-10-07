//! Kernel surface: command/query engines, datastore, domain wiring, and macros —
//! without the HTTP/portal stack.
//!
//! Prefer `riverbase_core` with `default-features = false, features = ["kernel"]` for
//! query/command-only domain crates. See
//! [architecture conformance](../../../../docs/02-design/09-architecture-conformance.md).

pub use crate::applog;
pub use crate::base;
pub use crate::command;
pub use crate::config;
pub use crate::datastore;
pub use crate::domain;
pub use crate::logstore;
pub use crate::pool;
pub use crate::query;
pub use crate::r#macro;
pub use crate::transport;
pub use crate::util;

// Common ergonomic re-exports for domain crates.
pub use crate::base::{
    AggregateRoot, Engine, EngineContext, RiverbaseError, RiverbaseResult, Namespace,
};
pub use crate::command::{Aggregate, AggregateCore, CommandEngine, TypedCommandHandler};
pub use crate::datastore::{DataStore, PgDataStore};
pub use crate::domain::{DataStoreInit, Domain, DomainStoreSpec};
pub use crate::query::{QueryEngine, QueryResource};
