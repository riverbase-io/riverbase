//! Bundle command services and query resources into a coherent domain namespace.

/// Bundle; module.
pub mod bundle;
/// Engines; module.
pub mod engines;
pub mod invoker;
/// Manifest; module.
pub mod manifest;
/// Meta; module.
pub mod meta;
pub mod migrations;
/// Repository; module.
pub mod repository;
pub mod runtime;
pub mod store;

pub use crate::base::EngineContext;
pub use bundle::Domain;
pub use engines::{
    CompositeCommandEngine, CompositeQueryEngine, DisabledCommandEngine, DomainCommandEngine,
    DomainQueryEngine, DomainServiceEngine,
};
pub use invoker::{CommandActivityGate, CommandInvoker, QueryInvoker};
pub use manifest::{build_domain_manifest, CommandContract, DomainManifest, QueryContract};
pub use meta::DomainMeta;
pub use migrations::{
    migration_specs_from_store_specs, run_ordered_migrations, run_store_migrations, MigrationSpec,
};
pub use repository::DecisionReadPort;
pub use runtime::{DbConnection, DomainRuntime, PgConnection, TEST_TENANT};
pub use store::{DataStoreInit, DomainStoreSpec};
