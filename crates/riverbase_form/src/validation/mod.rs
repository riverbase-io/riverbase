//! JSON Schema compilation and runtime validation for form data.

pub mod compile;
pub mod expr;
pub mod runtime;

pub use compile::{compile_element_schema, compile_form_schema, compile_inline_element_schema};
pub use runtime::{validate_element_data, validate_form_submission, ValidationError};

pub const X_MESSAGE: &str = "x-riverbase:message";
pub const X_CONSTRAINT: &str = "x-riverbase:constraint";
