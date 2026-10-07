//! JSON Schema validation for form specs.

use std::sync::OnceLock;

use riverbase_core::base::{ErrorSpec, RiverbaseResult};
use riverbase_core::cfgfmt::{
    compile_validator, load_json_schema_from_hcl, parse_to_json, ConfigFormat,
};
use jsonschema::Validator;
use serde_json::{Map, Value};

use crate::spec::{DocumentSpec, ElementSpec, FormSpec};

pub const ELEMENT_SCHEMA_HCL: &str = include_str!("../schemas/element.hcl");
pub const FORM_SCHEMA_HCL: &str = include_str!("../schemas/form.hcl");
pub const DOCUMENT_SCHEMA_HCL: &str = include_str!("../schemas/document.hcl");

const NODE_TYPES: &[&str] = &["content", "section", "form"];

static ELEMENT_VALIDATOR: OnceLock<Validator> = OnceLock::new();
static FORM_VALIDATOR: OnceLock<Validator> = OnceLock::new();
static DOCUMENT_VALIDATOR: OnceLock<Validator> = OnceLock::new();

fn element_validator() -> RiverbaseResult<&'static Validator> {
    if let Some(v) = ELEMENT_VALIDATOR.get() {
        return Ok(v);
    }
    let schema = load_json_schema_from_hcl(ELEMENT_SCHEMA_HCL)?;
    let validator = compile_validator(&schema)?;
    let _ = ELEMENT_VALIDATOR.set(validator);
    Ok(ELEMENT_VALIDATOR.get().expect("validator set"))
}

fn form_validator() -> RiverbaseResult<&'static Validator> {
    if let Some(v) = FORM_VALIDATOR.get() {
        return Ok(v);
    }
    let schema = load_json_schema_from_hcl(FORM_SCHEMA_HCL)?;
    let validator = compile_validator(&schema)?;
    let _ = FORM_VALIDATOR.set(validator);
    Ok(FORM_VALIDATOR.get().expect("validator set"))
}

fn document_validator() -> RiverbaseResult<&'static Validator> {
    if let Some(v) = DOCUMENT_VALIDATOR.get() {
        return Ok(v);
    }
    let schema = load_json_schema_from_hcl(DOCUMENT_SCHEMA_HCL)?;
    let validator = compile_validator(&schema)?;
    let _ = DOCUMENT_VALIDATOR.set(validator);
    Ok(DOCUMENT_VALIDATOR.get().expect("validator set"))
}

fn validate(
    instance: &Value,
    validator_fn: fn() -> RiverbaseResult<&'static Validator>,
    spec: ErrorSpec,
) -> RiverbaseResult<()> {
    let v = validator_fn()?;
    if v.is_valid(instance) {
        return Ok(());
    }
    let messages: Vec<String> = v.iter_errors(instance).map(|e| e.to_string()).collect();
    Err(spec.with_data(messages.join("; ")))
}

pub fn validate_element_spec(instance: &Value) -> RiverbaseResult<()> {
    validate(instance, element_validator, crate::errors::FRM_010)
}

pub fn validate_form_spec(instance: &Value) -> RiverbaseResult<()> {
    validate(instance, form_validator, crate::errors::FRM_016)
}

pub fn validate_document_spec(instance: &Value) -> RiverbaseResult<()> {
    validate(instance, document_validator, crate::errors::FRM_017)
}

/// Convert labeled `content`/`section`/`form` blocks into a canonical `nodes` array.
pub fn normalize_document_blocks(mut value: Value) -> Value {
    let Some(obj) = value.as_object_mut() else {
        return value;
    };
    if obj.contains_key("nodes") {
        return value;
    }

    let mut nodes = Vec::new();
    collect_document_nodes(obj, &mut nodes);
    obj.retain(|k, _| !NODE_TYPES.contains(&k.as_str()));
    obj.insert("nodes".to_string(), Value::Array(nodes));
    value
}

fn collect_document_nodes(body: &Map<String, Value>, out: &mut Vec<Value>) {
    let mut order = 0_i32;
    for key in body.keys() {
        if !NODE_TYPES.contains(&key.as_str()) {
            continue;
        }
        let Some(bucket) = body.get(key).and_then(Value::as_object) else {
            continue;
        };
        for (_label, node_body) in bucket {
            let mut node = match node_body.as_object() {
                Some(map) => map.clone(),
                None => continue,
            };
            node.insert("node_type".to_string(), Value::String(key.clone()));
            node.insert("order".to_string(), Value::Number(order.into()));
            order += 1;

            if key == "section" {
                let mut children = Vec::new();
                collect_document_nodes(&node, &mut children);
                for node_type in NODE_TYPES {
                    node.remove(*node_type);
                }
                if !children.is_empty() {
                    node.insert("children".to_string(), Value::Array(children));
                }
            }

            out.push(Value::Object(node));
        }
    }
}

pub fn parse_element_spec(text: &str, format: ConfigFormat) -> RiverbaseResult<ElementSpec> {
    let value = parse_to_json(text, format)?;
    validate_element_spec(&value)?;
    serde_json::from_value(value).map_err(|e| crate::errors::FRM_011.with_data(e.to_string()))
}

pub fn parse_form_spec(text: &str, format: ConfigFormat) -> RiverbaseResult<FormSpec> {
    let value = parse_to_json(text, format)?;
    validate_form_spec(&value)?;
    let spec: FormSpec = serde_json::from_value(value)
        .map_err(|e| crate::errors::FRM_012.with_data(e.to_string()))?;
    validate_form_constraints(&spec)?;
    Ok(spec)
}

fn validate_form_constraints(form: &FormSpec) -> RiverbaseResult<()> {
    for (id, rule) in &form.constraint {
        if rule.has_expr() && rule.has_schema() {
            return Err(crate::errors::FRM_014.with_data(id.clone()));
        }
        if !rule.has_expr() && !rule.has_schema() {
            return Err(crate::errors::FRM_015.with_data(id.clone()));
        }
    }
    Ok(())
}

pub fn parse_document_spec(text: &str, format: ConfigFormat) -> RiverbaseResult<DocumentSpec> {
    let value = parse_to_json(text, format)?;
    let value = normalize_document_blocks(value);
    validate_document_spec(&value)?;
    serde_json::from_value(value).map_err(|e| crate::errors::FRM_013.with_data(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use riverbase_core::cfgfmt::ConfigFormat;

    #[test]
    fn embedded_schemas_compile() {
        load_json_schema_from_hcl(ELEMENT_SCHEMA_HCL).unwrap();
        load_json_schema_from_hcl(FORM_SCHEMA_HCL).unwrap();
        load_json_schema_from_hcl(DOCUMENT_SCHEMA_HCL).unwrap();
    }

    #[test]
    fn parses_element_with_validation_and_constraint() {
        let text = r#"
key = "TXT-0001"
title = "Text Input"
table_name = "text_input_data"
field "value" {
  type = "string"
  required = true
  validation {
    pattern = "^[a-z]+$"
    minLength = 2
    message = "letters"
  }
  constraint "not_blank" {
    expr = "trim(value) != \"\""
    message = "required"
  }
}
"#;
        let spec = parse_element_spec(text, ConfigFormat::Hcl).unwrap();
        assert!(spec.field["value"].validation.is_some());
        assert!(spec.field["value"].constraint.contains_key("not_blank"));
    }

    #[test]
    fn parses_sample_element() {
        let text = r#"
key = "TXT-0001"
title = "Text Input"
table_name = "text_input_data"
field "value" {
  type = "string"
  required = true
}
"#;
        let spec = parse_element_spec(text, ConfigFormat::Hcl).unwrap();
        assert_eq!(spec.key, "TXT-0001");
        assert_eq!(spec.field.len(), 1);
        assert!(spec.field["value"].required);
    }

    #[test]
    fn rejects_nested_form_groups() {
        let text = r#"
key = "FRM-0001"
title = "Test"
group "outer" {
  title = "Outer"
  group "inner" {
    title = "Inner"
  }
}
"#;
        let value = parse_to_json(text, ConfigFormat::Hcl).unwrap();
        assert!(validate_form_spec(&value).is_err());
    }

    #[test]
    fn normalizes_document_blocks_with_order() {
        let text = r#"
key = "DOC-0001"
title = "Test"
content "intro" {
  title = "Intro"
}
section "details" {
  title = "Details"
  form "applicant" {
    form_key = "FRM-0001"
  }
}
"#;
        let value = parse_to_json(text, ConfigFormat::Hcl).unwrap();
        let normalized = normalize_document_blocks(value);
        let nodes = normalized["nodes"].as_array().expect("nodes array");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0]["node_type"], "content");
        assert_eq!(nodes[0]["order"], 0);
        assert_eq!(nodes[1]["node_type"], "section");
        assert_eq!(nodes[1]["order"], 1);
        let children = nodes[1]["children"].as_array().expect("children");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["node_type"], "form");
        assert_eq!(children[0]["order"], 0);
    }
}
