//! Data access traits, query DSL, and store backends.

/// Dsl; module.
pub mod dsl;
/// Error; module.
pub mod error;
/// Postgres; module.
pub mod postgres;
/// Store; module.
pub mod store;
/// Transaction; module.
pub mod transaction;

pub use dsl::{
    DataQuery, Expr, FieldPath, OrderDirection, OrderSpec, PageSpec, PredicateOp, Projection,
};
pub use error::{is_not_found, DataError, DataResult};
pub use postgres::{
    entity, establish_dbpool, establish_dbpool_with_size, establish_sync_dbpool_with_size, exec,
    new_id, run_embedded_migrations, ErasedEntity, PgDataStore, PgPool, PostgresDomainLogStore,
    PostgresProcessManagerStore, SyncPgPool, DOMAIN_FIELDS_DDL,
};
pub use store::{merge_json, DataStore, ResourceKey, ResourceName, ResourceRegistry};
pub use transaction::{CommandTransaction, CommandUnitOfWork};
