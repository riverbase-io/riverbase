//! Runtime validation: JSON Schema + expression constraints.

use riverbase_core::cfgfmt::compile_validator;
use serde_json::Value;

use crate::registry::ElementRegistry;
use crate::spec::ElementSpec;

use super::compile::{
    compile_element_schema, compile_form_schema, compile_inline_element_schema, strip_extensions,
};
use super::expr::{
    collect_form_paths, element_scope_vars, eval_constraint_expr, field_scope_vars,
    form_scope_vars, rewrite_expr_paths,
};
use super::{X_CONSTRAINT, X_MESSAGE};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub rule: String,
    pub message: String,
}

struct CompiledConstraintEntry {
    id: String,
    path: Option<String>,
    expr: String,
    message: Option<String>,
}

fn parse_constraints(value: &Value) -> Vec<CompiledConstraintEntry> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            Some(CompiledConstraintEntry {
                id: v.get("id")?.as_str()?.to_string(),
                path: v.get("path").and_then(|p| p.as_str()).map(str::to_string),
                expr: v.get("expr")?.as_str()?.to_string(),
                message: v
                    .get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string),
            })
        })
        .collect()
}

fn compile_and_validate(schema: &Value, data: &Value) -> Vec<ValidationError> {
    let mut clean = schema.clone();
    strip_extensions(&mut clean);
    let Ok(validator) = compile_validator(&clean) else {
        return vec![ValidationError {
            path: "/".to_string(),
            rule: "schema".to_string(),
            message: "Failed to compile validation schema.".to_string(),
        }];
    };
    if validator.is_valid(data) {
        return Vec::new();
    }
    validator
        .iter_errors(data)
        .map(|e| {
            let path = e.instance_path().to_string();
            ValidationError {
                path: if path.is_empty() {
                    "/".to_string()
                } else {
                    path
                },
                rule: "validation".to_string(),
                message: lookup_message(schema, &e.instance_path().to_string())
                    .unwrap_or_else(|| e.to_string()),
            }
        })
        .collect()
}

fn lookup_message(schema: &Value, _instance_path: &str) -> Option<String> {
    // Walk schema for x-riverbase:message on failing subschema (best-effort: root message)
    schema
        .pointer("/x-riverbase:message")
        .or_else(|| schema.get(X_MESSAGE))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn run_constraints(schema: &Value, data: &Value, field_names: &[String]) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    if let Some(root) = schema.get(X_CONSTRAINT) {
        let entries = parse_constraints(root);
        let vars = element_scope_vars(data, field_names);
        for entry in entries {
            match eval_constraint_expr(&entry.expr, &vars) {
                Ok(true) => {}
                Ok(false) => errors.push(ValidationError {
                    path: entry
                        .path
                        .as_ref()
                        .map(|p| format!("/{p}"))
                        .unwrap_or_else(|| "/".to_string()),
                    rule: entry.id.clone(),
                    message: entry
                        .message
                        .unwrap_or_else(|| format!("Constraint '{}' failed.", entry.id)),
                }),
                Err(e) => errors.push(ValidationError {
                    path: entry
                        .path
                        .as_ref()
                        .map(|p| format!("/{p}"))
                        .unwrap_or_else(|| "/".to_string()),
                    rule: entry.id.clone(),
                    message: entry.message.unwrap_or_else(|| e.to_string()),
                }),
            }
        }
    }

    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        for (field_name, prop_schema) in props {
            if data.get(field_name).is_some() {
                if let Some(constraints) = prop_schema.get(X_CONSTRAINT) {
                    let entries = parse_constraints(constraints);
                    let vars = field_scope_vars(field_name, data, field_names);
                    for entry in entries {
                        match eval_constraint_expr(&entry.expr, &vars) {
                            Ok(true) => {}
                            Ok(false) => errors.push(ValidationError {
                                path: format!("/{field_name}"),
                                rule: entry.id.clone(),
                                message: entry.message.unwrap_or_else(|| {
                                    format!("Constraint '{}' failed.", entry.id)
                                }),
                            }),
                            Err(e) => errors.push(ValidationError {
                                path: format!("/{field_name}"),
                                rule: entry.id.clone(),
                                message: entry.message.unwrap_or_else(|| e.to_string()),
                            }),
                        }
                    }
                }
            }
        }
    }

    errors
}

/// Validate element data against an element spec.
pub fn validate_element_data(spec: &ElementSpec, data: &Value) -> Result<(), Vec<ValidationError>> {
    let schema = compile_element_schema(spec);
    let field_names: Vec<String> = spec.field.keys().cloned().collect();
    let mut errors = compile_and_validate(&schema, data);
    errors.extend(run_constraints(&schema, data, &field_names));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate a form submission payload against a form spec.
pub fn validate_form_submission(
    form: &crate::spec::FormSpec,
    elements: &ElementRegistry,
    payload: &Value,
) -> Result<(), Vec<ValidationError>> {
    let schema = compile_form_schema(form, elements);
    let mut errors = compile_and_validate(&schema, payload);

    let paths = collect_form_paths(payload);
    let form_vars = form_scope_vars(payload, &paths);

    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
        for entry in all_of {
            if let Some(expr) = entry.get("x-riverbase:expr").and_then(Value::as_str) {
                let id = entry
                    .get("x-riverbase:constraint-id")
                    .and_then(Value::as_str)
                    .unwrap_or("constraint")
                    .to_string();
                let msg = entry
                    .get(X_MESSAGE)
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let rewritten = rewrite_expr_paths(expr, &paths);
                match eval_constraint_expr(&rewritten, &form_vars) {
                    Ok(true) => {}
                    Ok(false) => errors.push(ValidationError {
                        path: "/".to_string(),
                        rule: id.clone(),
                        message: msg.unwrap_or_else(|| format!("Constraint '{id}' failed.")),
                    }),
                    Err(e) => errors.push(ValidationError {
                        path: "/".to_string(),
                        rule: id,
                        message: msg.unwrap_or_else(|| e.to_string()),
                    }),
                }
            } else {
                let id = entry
                    .get("x-riverbase:constraint-id")
                    .and_then(Value::as_str)
                    .unwrap_or("constraint")
                    .to_string();
                let mut sub = entry.clone();
                strip_extensions(&mut sub);
                if let Some(m) = sub.as_object_mut() {
                    m.remove("x-riverbase:constraint-id");
                }
                if let Ok(v) = compile_validator(&sub) {
                    if !v.is_valid(payload) {
                        let msg = entry
                            .get(X_MESSAGE)
                            .and_then(Value::as_str)
                            .unwrap_or("Form constraint failed.");
                        errors.push(ValidationError {
                            path: "/".to_string(),
                            rule: id,
                            message: msg.to_string(),
                        });
                    }
                }
            }
        }
    }

    for (name, _group, elem) in form.iter_elements() {
        if let Some(elem_data) = payload.get(name) {
            if let Some(ref key) = elem.key {
                if let Some(spec) = elements.get(key) {
                    if let Err(mut elem_errors) = validate_element_data(&spec, elem_data) {
                        for e in &mut elem_errors {
                            if !e.path.starts_with('/') {
                                e.path = format!("/{name}{}", e.path);
                            } else if e.path == "/" {
                                e.path = format!("/{name}");
                            } else {
                                e.path = format!("/{name}{}", e.path);
                            }
                        }
                        errors.extend(elem_errors);
                    }
                }
            } else if let Some(ref inline) = elem.schema {
                let inline_schema = compile_inline_element_schema(inline);
                let field_names: Vec<String> = inline.field.keys().cloned().collect();
                errors.extend(compile_and_validate(&inline_schema, elem_data));
                errors.extend(run_constraints(&inline_schema, elem_data, &field_names));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{ConstraintRule, FieldDef};
    use indexmap::IndexMap;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn rejects_invalid_pattern() {
        let mut field = BTreeMap::new();
        field.insert(
            "value".to_string(),
            FieldDef {
                field_type: "string".to_string(),
                format: None,
                required: true,
                validation: Some(json!({ "pattern": "^[a-z]+$", "message": "letters only" })),
                constraint: IndexMap::new(),
            },
        );
        let spec = ElementSpec {
            key: "TXT-0001".to_string(),
            title: "T".to_string(),
            desc: None,
            table_name: "t".to_string(),
            validation: None,
            constraint: IndexMap::new(),
            field,
        };
        let err = validate_element_data(&spec, &json!({ "value": "123" })).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn rejects_constraint_expr() {
        let mut field = BTreeMap::new();
        field.insert(
            "value".to_string(),
            FieldDef {
                field_type: "string".to_string(),
                format: None,
                required: false,
                validation: None,
                constraint: IndexMap::from([(
                    "not_blank".to_string(),
                    ConstraintRule {
                        expr: "trim(value) != \"\"".to_string(),
                        message: Some("blank".to_string()),
                    },
                )]),
            },
        );
        let spec = ElementSpec {
            key: "TXT-0001".to_string(),
            title: "T".to_string(),
            desc: None,
            table_name: "t".to_string(),
            validation: None,
            constraint: IndexMap::new(),
            field,
        };
        let err = validate_element_data(&spec, &json!({ "value": "   " })).unwrap_err();
        assert_eq!(err[0].message, "blank");
    }
}
