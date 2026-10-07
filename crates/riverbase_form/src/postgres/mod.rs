pub mod entity;
pub mod migrate;
pub mod schema;

use riverbase_core::datastore::{PgDataStore, PgPool, ResourceRegistry};
use riverbase_core::domain::DomainStoreSpec;

pub use migrate::{establish_form_dbpool, run_form_migrations, FORM_MIGRATIONS};
pub use schema::RIVERBASE_FORM_SCHEMA;

pub type FormPostgresDataStore = PgDataStore;

pub fn new_form_store(dbpool: PgPool) -> FormPostgresDataStore {
    new_form_store_with_registry(dbpool, form_resource_registry())
}

pub fn new_form_store_with_registry(
    dbpool: PgPool,
    registry: ResourceRegistry,
) -> FormPostgresDataStore {
    let mut store = PgDataStore::with_registry(dbpool, registry);
    for entity in entity::all_entities() {
        store = store.register_entity(entity);
    }
    store
}

pub fn form_resource_registry() -> ResourceRegistry {
    ResourceRegistry::new([
        "collection",
        "document",
        "document_collection",
        "document_node",
        "form_submission",
        "form_element",
        "text_input_data",
        "TXT-0001",
        "element_registry",
        "form_registry",
        "template_registry",
    ])
}

pub fn form_store_spec() -> DomainStoreSpec {
    DomainStoreSpec::new(form_resource_registry())
        .with_entities(entity::all_entities())
        .with_migrate(run_form_migrations)
        .with_migration_name("riverbase_form")
}
