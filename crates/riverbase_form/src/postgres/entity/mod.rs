//! Postgres entity wiring for form fixed tables.

mod collection;
mod document;
mod document_collection;
mod document_node;
mod element_registry;
mod form_element;
mod form_registry;
mod form_submission;
mod helpers;
mod template_registry;

pub use collection::CollectionEntity;
pub use document::DocumentEntity;
pub use document_collection::DocumentCollectionEntity;
pub use document_node::DocumentNodeEntity;
pub use element_registry::{
    element_registry_row_to_json, element_registry_upsert_from_json, ElementRegistryEntity,
    ElementRegistryRow,
};
pub use form_element::FormElementEntity;
pub use form_registry::{
    form_registry_row_to_json, form_registry_upsert_from_json, FormRegistryEntity, FormRegistryRow,
};
pub use form_submission::FormSubmissionEntity;
pub use template_registry::{
    template_registry_row_to_json, template_registry_upsert_from_json, TemplateRegistryEntity,
    TemplateRegistryRow,
};

use std::sync::Arc;

use riverbase_core::datastore::ErasedEntity;

use crate::generated::entities::register_element_entities;

pub fn register_fixed_entities() -> Vec<Arc<dyn ErasedEntity>> {
    vec![
        Arc::new(CollectionEntity) as Arc<dyn ErasedEntity>,
        Arc::new(DocumentEntity) as Arc<dyn ErasedEntity>,
        Arc::new(DocumentCollectionEntity) as Arc<dyn ErasedEntity>,
        Arc::new(DocumentNodeEntity) as Arc<dyn ErasedEntity>,
        Arc::new(FormSubmissionEntity) as Arc<dyn ErasedEntity>,
        Arc::new(FormElementEntity) as Arc<dyn ErasedEntity>,
        Arc::new(TemplateRegistryEntity) as Arc<dyn ErasedEntity>,
        Arc::new(FormRegistryEntity) as Arc<dyn ErasedEntity>,
        Arc::new(ElementRegistryEntity) as Arc<dyn ErasedEntity>,
    ]
}

pub fn all_entities() -> Vec<Arc<dyn ErasedEntity>> {
    let mut entities = register_fixed_entities();
    entities.extend(register_element_entities());
    entities
}
