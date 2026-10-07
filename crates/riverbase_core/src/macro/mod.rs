//! Declarative macros for `riverbase_core` (Diesel domain-field helpers, query/command/domain engines).
//!
//! Proc-macros (`#[domain_action]`, `riverbase_namespace!`, …) live in the [`riverbase_proc`] crate.
//!
//! [`riverbase_proc`]: ../../riverbase_proc

mod command;
mod domain;
mod domain_fields;
mod pg_entity;
mod pg_readonly_entity;
mod query;

pub use domain_fields::{DOMAIN_FIELDS_DDL, DOMAIN_FIELD_COLUMNS};
