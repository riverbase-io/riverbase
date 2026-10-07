//! JSON Schema validation; schemas are authored in HCL and compiled at load time.

use jsonschema::{Draft, Validator};
use serde_json::Value;

use crate::base::RiverbaseResult;

use super::hcl::parse_hcl_to_json;

/// Extract the `schema` attribute from an HCL-encoded JSON Schema file.
pub fn load_json_schema_from_hcl(text: &str) -> RiverbaseResult<Value> {
    let root = parse_hcl_to_json(text)?;
    let obj = root
        .as_object()
        .ok_or_else(|| crate::errors::CFG_030.with_data(root.to_string()))?;
    if let Some(schema) = obj.get("schema") {
        return Ok(schema.clone());
    }
    if obj.contains_key("$schema") || obj.contains_key("type") {
        return Ok(root);
    }
    Err(crate::errors::CFG_031.with_data(""))
}

/// Compile a JSON Schema value into a validator.
pub fn compile_validator(schema: &Value) -> RiverbaseResult<Validator> {
    Validator::options()
        .with_draft(Draft::Draft202012)
        .build(schema)
        .map_err(|e| crate::errors::CFG_032.with_data(e.to_string()))
}

/// Validate `instance` against `schema`.
pub fn validate_instance(schema: &Value, instance: &Value) -> RiverbaseResult<()> {
    let validator = compile_validator(schema)?;
    if validator.is_valid(instance) {
        return Ok(());
    }
    let messages: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| e.to_string())
        .collect();
    Err(crate::errors::CFG_033.with_data(messages.join("; ")))
}
