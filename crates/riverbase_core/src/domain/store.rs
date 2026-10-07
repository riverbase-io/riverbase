//! Build a concrete [`DataStore`] for a domain from a backend-agnostic spec.
//!
//! A domain declares its persistence surface once via [`DomainStoreSpec`]; the chosen
//! store type (`PgDataStore`) implements [`DataStoreInit`] so the
//! generated `spawn` can construct `Arc<S>` generically from the [`DomainRuntime`]
//! connection.

use std::sync::Arc;

use crate::base::RiverbaseResult;
use crate::datastore::{DataStore, ErasedEntity, PgDataStore, ResourceRegistry};

use super::runtime::DbConnection;

/// Backend-agnostic description of a domain's persistence surface.
///
/// `entities` and `migrate` are used by the Postgres backend.
pub struct DomainStoreSpec {
    /// Registry.
    pub registry: ResourceRegistry,
    /// Entities.
    pub entities: Vec<Arc<dyn ErasedEntity>>,
    /// Migrate.
    pub migrate: Option<fn(&str) -> RiverbaseResult<()>>,
    /// Schema name recorded in `{name}.__diesel_schema_migrations` ([DAT-03]).
    pub migration_name: Option<&'static str>,
    /// Other migration names that must run before this crate's migrations.
    pub migration_after: &'static [&'static str],
}

impl DomainStoreSpec {
    /// Construct a new value.
    pub fn new(registry: ResourceRegistry) -> Self {
        Self {
            registry,
            entities: Vec::new(),
            migrate: None,
            migration_name: None,
            migration_after: &[],
        }
    }

    /// Set entities and return self.
    pub fn with_entities(mut self, entities: Vec<Arc<dyn ErasedEntity>>) -> Self {
        self.entities = entities;
        self
    }

    /// Set migrate and return self.
    pub fn with_migrate(mut self, migrate: fn(&str) -> RiverbaseResult<()>) -> Self {
        self.migrate = Some(migrate);
        self
    }

    /// Set migration name and return self.
    pub fn with_migration_name(mut self, name: &'static str) -> Self {
        self.migration_name = Some(name);
        self
    }

    /// Set migration after and return self.
    pub fn with_migration_after(mut self, after: &'static [&'static str]) -> Self {
        self.migration_after = after;
        self
    }
}

/// A [`DataStore`] that can be built for a domain from a [`DomainStoreSpec`].
pub trait DataStoreInit: DataStore + Sized + 'static {
    /// Init.
    fn init(conn: &DbConnection, spec: DomainStoreSpec) -> RiverbaseResult<Arc<Self>>;

    /// Initialize after applying this spec's migrations (isolated tests / single-domain spawn).
    fn init_migrated(conn: &DbConnection, spec: DomainStoreSpec) -> RiverbaseResult<Arc<Self>> {
        if let Some(dsn) = conn.db_dsn() {
            super::migrations::run_store_migrations(dsn, std::slice::from_ref(&spec))?;
        }
        Self::init(conn, spec)
    }
}

impl DataStoreInit for PgDataStore {
    fn init(conn: &DbConnection, spec: DomainStoreSpec) -> RiverbaseResult<Arc<Self>> {
        let DbConnection::Postgres(pgconn) = conn;
        // Migrations are applied by [`run_store_migrations`] before init ([DAT-02], [DAT-03]).
        let _ = spec.migrate;
        let mut store = PgDataStore::with_registry(pgconn.pgpool.as_ref().clone(), spec.registry);
        for entity in spec.entities {
            store = store.register_entity(entity);
        }
        Ok(Arc::new(store))
    }
}
