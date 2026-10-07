//! `riverbase_form` — declarative form and document templates for Riverbase.
//!
//! Core library: HCL spec parsing, registries, validation, template generation,
//! element codegen, and Postgres persistence primitives.

pub mod code;
pub mod config;
pub mod errors;
pub mod registry;
pub mod result;
pub mod schema;
pub mod spec;
pub mod template;
pub mod validation;

#[cfg(feature = "postgres")]
pub mod generated;
#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "cli")]
pub mod cli;

#[cfg(feature = "dgen")]
pub mod render;

pub use code::{validate_key, KEY_LEN, KEY_PATTERN};
pub use registry::{
    bind_registry_pool, document_registry, element_registry, form_registry, register_all_from_base,
    register_documents_from_dir, register_elements_from_dir, register_forms_from_dir,
    registry_pool, DocumentRegistry, ElementRegistry, FormRegistry,
};
#[cfg(feature = "postgres")]
pub use registry::{
    load_registries_from_postgres, reload_registries_from_postgres, seed_registries_from_dir,
};
pub use result::{FormError, FormResult};
pub use spec::{
    ConstraintRule, Document, DocumentNode, DocumentSpec, ElementSpec, FieldDef,
    FormConstraintRule, FormElement, FormGroup, FormSpec, InlineElement, NodeType,
};
pub use validation::{
    compile_element_schema, compile_form_schema, validate_element_data, validate_form_submission,
    ValidationError,
};
