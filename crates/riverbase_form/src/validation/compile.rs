//! Compile element and form specs into JSON Schema (draft 2020-12) with Riverbase extensions.

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::registry::ElementRegistry;
use crate::spec::{ConstraintRule, ElementSpec, FieldDef, FormSpec, InlineElement};

use super::{X_CONSTRAINT, X_MESSAGE};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CompiledConstraint {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    expr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

/// Compile an element spec into a JSON Schema for its data object.
pub fn compile_element_schema(spec: &ElementSpec) -> Value {
    compile_fields_schema(&spec.field, spec.validation.as_ref(), &spec.constraint)
}

/// Compile an inline element schema (within a form element block).
pub fn compile_inline_element_schema(inline: &InlineElement) -> Value {
    compile_fields_schema(
        &inline.field,
        inline.validation.as_ref(),
        &inline.constraint,
    )
}

fn compile_fields_schema(
    fields: &std::collections::BTreeMap<String, FieldDef>,
    root_validation: Option<&Value>,
    root_constraints: &IndexMap<String, ConstraintRule>,
) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    let mut constraints = Vec::new();

    for (name, field) in fields {
        let mut prop = compile_field_property(field);
        if !field.constraint.is_empty() {
            let mut field_constraints = Vec::new();
            collect_constraints(
                &field.constraint,
                Some(name.clone()),
                &mut field_constraints,
            );
            if let Some(obj) = prop.as_object_mut() {
                obj.insert(X_CONSTRAINT.to_string(), Value::Array(field_constraints));
            }
        }
        if field.required {
            required.push(Value::String(name.clone()));
        }
        properties.insert(name.clone(), prop);
    }

    collect_constraints(root_constraints, None, &mut constraints);

    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    if let Some(obj) = schema.as_object_mut() {
        merge_validation_block(obj, root_validation);
        if !constraints.is_empty() {
            obj.insert(X_CONSTRAINT.to_string(), Value::Array(constraints));
        }
    }
    Value::Object(schema.as_object().cloned().unwrap_or_default())
}

fn compile_field_property(field: &FieldDef) -> Value {
    let mut prop = Map::new();
    prop.insert("type".to_string(), Value::String(field.field_type.clone()));
    if let Some(format) = &field.format {
        prop.insert("format".to_string(), Value::String(format.clone()));
    }
    merge_validation_block(&mut prop, field.validation.as_ref());
    Value::Object(prop)
}

fn merge_validation_block(target: &mut Map<String, Value>, validation: Option<&Value>) {
    let Some(obj) = validation.and_then(Value::as_object) else {
        return;
    };
    for (key, value) in obj {
        if key == "message" {
            target.insert(X_MESSAGE.to_string(), value.clone());
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn collect_constraints(
    rules: &IndexMap<String, ConstraintRule>,
    path: Option<String>,
    out: &mut Vec<Value>,
) {
    for (id, rule) in rules {
        let entry = CompiledConstraint {
            id: id.clone(),
            path: path.clone(),
            expr: rule.expr.clone(),
            message: rule.message.clone(),
        };
        out.push(serde_json::to_value(entry).unwrap_or(Value::Null));
    }
}

/// Compile a form spec into a JSON Schema for submission payloads.
pub fn compile_form_schema(form: &FormSpec, elements: &ElementRegistry) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    let mut all_of = Vec::new();

    for (name, _group, elem) in form.iter_elements() {
        let elem_schema = if let Some(ref inline) = elem.schema {
            compile_inline_element_schema(inline)
        } else if let Some(ref key) = elem.key {
            elements
                .get(key)
                .as_ref()
                .map(compile_element_schema)
                .unwrap_or_else(|| json!({ "type": "object" }))
        } else {
            json!({ "type": "object" })
        };
        properties.insert(name.to_string(), elem_schema);
        if elem.required {
            required.push(Value::String(name.to_string()));
        }
    }

    for (id, rule) in &form.constraint {
        if rule.has_expr() {
            let mut entry = Map::new();
            entry.insert(
                "x-riverbase:expr".to_string(),
                Value::String(rule.expr.clone().unwrap_or_default()),
            );
            entry.insert(
                "x-riverbase:constraint-id".to_string(),
                Value::String(id.clone()),
            );
            if let Some(msg) = &rule.message {
                entry.insert(X_MESSAGE.to_string(), Value::String(msg.clone()));
            }
            all_of.push(Value::Object(entry));
        } else if rule.has_schema() {
            let mut entry = rule.schema.clone();
            if let Some(msg) = &rule.message {
                entry.insert(X_MESSAGE.to_string(), Value::String(msg.clone()));
            }
            entry.insert(
                "x-riverbase:constraint-id".to_string(),
                Value::String(id.clone()),
            );
            all_of.push(Value::Object(entry.into_iter().collect()));
        }
    }

    let mut schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    let obj = schema.as_object_mut().expect("schema object");
    if !required.is_empty() {
        obj.insert("required".to_string(), Value::Array(required));
    }
    if !all_of.is_empty() {
        obj.insert("allOf".to_string(), Value::Array(all_of));
    }
    schema
}

/// Remove Riverbase extension keys before passing a schema to the jsonschema crate.
pub fn strip_extensions(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove(X_CONSTRAINT);
            map.remove(X_MESSAGE);
            map.remove("x-riverbase:expr");
            map.remove("x-riverbase:constraint-id");
            for v in map.values_mut() {
                strip_extensions(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_extensions(v);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::FieldDef;
    use std::collections::BTreeMap;

    #[test]
    fn compiles_field_validation_and_constraints() {
        let mut field = BTreeMap::new();
        field.insert(
            "value".to_string(),
            FieldDef {
                field_type: "string".to_string(),
                format: None,
                required: true,
                validation: Some(json!({
                    "pattern": "^[a-z]+$",
                    "minLength": 2,
                    "message": "bad value"
                })),
                constraint: IndexMap::from([(
                    "not_blank".to_string(),
                    ConstraintRule {
                        expr: "trim(value) != \"\"".to_string(),
                        message: Some("required".to_string()),
                    },
                )]),
            },
        );
        let spec = ElementSpec {
            key: "TXT-0001".to_string(),
            title: "Text".to_string(),
            desc: None,
            table_name: "text_input_data".to_string(),
            validation: None,
            constraint: IndexMap::new(),
            field,
        };
        let schema = compile_element_schema(&spec);
        let prop = &schema["properties"]["value"];
        assert_eq!(prop["pattern"], "^[a-z]+$");
        assert_eq!(prop["minLength"], 2);
        assert_eq!(prop[X_MESSAGE], "bad value");
        assert!(prop[X_CONSTRAINT].is_array());
    }
}
