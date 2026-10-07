//! Integration tests for validation compile and runtime.

use riverbase_form::config::{load_element_from_path, load_elements_from_dir, load_form_from_path};
use riverbase_form::registry::ElementRegistry;
use riverbase_form::spec::FormSpec;
use riverbase_form::validation::{validate_element_data, validate_form_submission};
use serde_json::json;
use std::path::PathBuf;

fn form_app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/form-app")
}

fn example_elements() -> ElementRegistry {
    let registry = ElementRegistry::new("element");
    for spec in load_elements_from_dir(&form_app_dir().join("elements")).unwrap() {
        registry.register(spec.key.clone(), spec).unwrap();
    }
    registry
}

fn applicant_form() -> FormSpec {
    load_form_from_path(&form_app_dir().join("forms/applicant_info.hcl")).unwrap()
}

#[test]
fn text_input_validation_passes() {
    let path = form_app_dir().join("elements/text_input.hcl");
    let spec = load_element_from_path(&path).unwrap();
    validate_element_data(&spec, &json!({ "value": "Jane Doe" })).unwrap();
}

#[test]
fn text_input_validation_rejects_pattern() {
    let path = form_app_dir().join("elements/text_input.hcl");
    let spec = load_element_from_path(&path).unwrap();
    let err = validate_element_data(&spec, &json!({ "value": "!!!" })).unwrap_err();
    assert!(!err.is_empty());
}

#[test]
fn text_input_constraint_rejects_blank() {
    let path = form_app_dir().join("elements/text_input.hcl");
    let spec = load_element_from_path(&path).unwrap();
    let err = validate_element_data(&spec, &json!({ "value": "   " })).unwrap_err();
    assert!(err.iter().any(|e| e.rule == "not_blank"));
}

#[test]
fn form_constraint_expr_and_schema() {
    let form = applicant_form();
    let elements = example_elements();
    let payload = json!({
        "full_name": { "value": "AB" },
        "annual_income": { "amount": 600000 }
    });
    validate_form_submission(&form, &elements, &payload).unwrap();

    let bad = json!({
        "full_name": { "value": "A" },
        "annual_income": { "amount": 600000 }
    });
    let err = validate_form_submission(&form, &elements, &bad).unwrap_err();
    assert!(!err.is_empty());
}

#[test]
fn rejects_empty_form_constraint_at_parse() {
    let text = r#"
key = "FRM-9999"
title = "Bad"
constraint "empty" {
  message = "nothing"
}
"#;
    assert!(
        riverbase_form::schema::parse_form_spec(text, riverbase_core::cfgfmt::ConfigFormat::Hcl)
            .is_err()
    );
}
