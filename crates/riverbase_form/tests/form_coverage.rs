//! Full-coverage form validation tests using large sample elements.

use riverbase_form::config::{load_element_from_path, load_elements_from_dir, load_form_from_path};
use riverbase_form::registry::ElementRegistry;
use riverbase_form::spec::FormSpec;
use riverbase_form::validation::{
    compile_element_schema, compile_form_schema, validate_element_data, validate_form_submission,
    X_CONSTRAINT, X_MESSAGE,
};
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

fn comprehensive_form() -> FormSpec {
    load_form_from_path(&form_app_dir().join("forms/comprehensive_application.hcl")).unwrap()
}

fn valid_personal_data() -> serde_json::Value {
    json!({
        "legal_first_name": "Jordan",
        "legal_last_name": "Lee",
        "date_of_birth": "1990-01-15",
        "ssn_last4": "1234",
        "citizenship": "US",
        "email": "jordan@example.com",
        "phone_mobile": "5551234567",
        "address_line1": "100 Main St",
        "city": "Austin",
        "state": "TX",
        "postal_code": "78701",
        "country": "US",
        "employment_status": "employed",
    })
}

fn valid_business_data() -> serde_json::Value {
    json!({
        "legal_business_name": "Acme LLC",
        "entity_type": "LLC",
        "tax_id": "12-3456789",
        "incorporation_state": "TX",
        "industry_code": "541511",
        "employee_count": 12,
        "annual_revenue": 2500000,
        "business_phone": "5559876543",
        "business_email": "info@acme.example",
        "hq_address_line1": "200 Commerce Dr",
        "hq_city": "Austin",
        "hq_state": "TX",
        "hq_postal_code": "78702",
        "hq_country": "US",
        "primary_contact_name": "Jordan Lee",
        "primary_contact_email": "jordan@acme.example",
        "primary_contact_phone": "5551234567",
    })
}

#[test]
fn large_personal_profile_has_dozens_of_fields() {
    let spec =
        load_element_from_path(&form_app_dir().join("elements/personal_profile.hcl")).unwrap();
    assert_eq!(spec.key, "PER-0001");
    assert!(
        spec.field.len() >= 30,
        "expected 30+ fields, got {}",
        spec.field.len()
    );
    assert!(spec.validation.is_some());
    assert!(spec.constraint.contains_key("postal_when_country"));
    assert!(spec.field["email"].validation.is_some());
    assert!(spec.field["email"].constraint.contains_key("not_blank"));
}

#[test]
fn large_business_profile_has_dozens_of_fields() {
    let spec =
        load_element_from_path(&form_app_dir().join("elements/business_profile.hcl")).unwrap();
    assert_eq!(spec.key, "BUS-0001");
    assert!(
        spec.field.len() >= 25,
        "expected 25+ fields, got {}",
        spec.field.len()
    );
    assert!(spec.constraint.contains_key("revenue_when_employees"));
}

#[test]
fn compiles_large_element_schemas_with_extensions() {
    let personal =
        load_element_from_path(&form_app_dir().join("elements/personal_profile.hcl")).unwrap();
    let schema = compile_element_schema(&personal);
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"].is_object());
    assert!(schema.get(X_CONSTRAINT).is_some());
    let email = &schema["properties"]["email"];
    assert!(email.get("pattern").is_some());
    assert!(email.get(X_MESSAGE).is_some());
    assert!(email.get(X_CONSTRAINT).is_some());
}

#[test]
fn personal_profile_validates_good_payload() {
    let spec =
        load_element_from_path(&form_app_dir().join("elements/personal_profile.hcl")).unwrap();
    validate_element_data(&spec, &valid_personal_data()).unwrap();
}

#[test]
fn personal_profile_rejects_invalid_email() {
    let spec =
        load_element_from_path(&form_app_dir().join("elements/personal_profile.hcl")).unwrap();
    let mut data = valid_personal_data();
    data["email"] = json!("not-an-email");
    let err = validate_element_data(&spec, &data).unwrap_err();
    assert!(!err.is_empty());
}

#[test]
fn personal_profile_rejects_blank_email_constraint() {
    let spec =
        load_element_from_path(&form_app_dir().join("elements/personal_profile.hcl")).unwrap();
    let mut data = valid_personal_data();
    data["email"] = json!("   ");
    let err = validate_element_data(&spec, &data).unwrap_err();
    assert!(err.iter().any(|e| e.rule == "not_blank"));
}

#[test]
fn personal_profile_element_constraint_postal_when_country() {
    let spec =
        load_element_from_path(&form_app_dir().join("elements/personal_profile.hcl")).unwrap();
    let mut data = valid_personal_data();
    data["postal_code"] = json!("");
    let err = validate_element_data(&spec, &data).unwrap_err();
    assert!(err.iter().any(|e| e.rule == "postal_when_country"));
}

#[test]
fn business_profile_rejects_negative_revenue_with_employees() {
    let spec =
        load_element_from_path(&form_app_dir().join("elements/business_profile.hcl")).unwrap();
    let mut data = valid_business_data();
    data["employee_count"] = json!(5);
    data["annual_revenue"] = json!(0);
    let err = validate_element_data(&spec, &data).unwrap_err();
    assert!(err.iter().any(|e| e.rule == "revenue_when_employees"));
}

#[test]
fn comprehensive_form_parses_with_groups_and_constraints() {
    let form =
        load_form_from_path(&form_app_dir().join("forms/comprehensive_application.hcl")).unwrap();
    assert_eq!(form.key, "FRM-0002");
    assert_eq!(form.group.len(), 3);
    assert_eq!(form.iter_elements().count(), 4);
    assert!(form
        .constraint
        .contains_key("business_revenue_with_personal_income"));
    assert!(form.constraint.contains_key("legal_name_alignment"));
}

#[test]
fn comprehensive_form_compiles_schema_for_all_slots() {
    let form = comprehensive_form();
    let schema = compile_form_schema(&form, &example_elements());
    let props = schema["properties"].as_object().unwrap();
    assert_eq!(props.len(), 4);
    assert!(props.contains_key("personal"));
    assert!(props.contains_key("business"));
    assert!(schema["allOf"].as_array().unwrap().len() >= 2);
}

#[test]
fn comprehensive_form_accepts_valid_submission() {
    let form = comprehensive_form();
    let elements = example_elements();
    let payload = json!({
        "personal": valid_personal_data(),
        "business": valid_business_data(),
        "prior_income": { "amount": 120000 },
        "legal_name_check": { "value": "Jordan Lee" }
    });
    validate_form_submission(&form, &elements, &payload).unwrap();
}

#[test]
fn comprehensive_form_rejects_low_business_revenue_constraint() {
    let form = comprehensive_form();
    let elements = example_elements();
    let mut business = valid_business_data();
    business["annual_revenue"] = json!(50000);
    let payload = json!({
        "personal": valid_personal_data(),
        "business": business,
        "prior_income": { "amount": 120000 },
        "legal_name_check": { "value": "Jordan Lee" }
    });
    let err = validate_form_submission(&form, &elements, &payload).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.rule == "business_revenue_with_personal_income"));
}

#[test]
fn comprehensive_form_rejects_schema_constraint_without_personal_name() {
    let form = comprehensive_form();
    let elements = example_elements();
    let mut personal = valid_personal_data();
    personal.as_object_mut().unwrap().remove("legal_first_name");
    personal.as_object_mut().unwrap().remove("legal_last_name");
    let payload = json!({
        "personal": personal,
        "business": valid_business_data(),
        "legal_name_check": { "value": "Jordan Lee" }
    });
    let err = validate_form_submission(&form, &elements, &payload).unwrap_err();
    assert!(err.iter().any(|e| e.rule == "legal_name_alignment"));
}

#[test]
fn comprehensive_form_rejects_invalid_tax_id_pattern() {
    let form = comprehensive_form();
    let elements = example_elements();
    let mut business = valid_business_data();
    business["tax_id"] = json!("123456789");
    let payload = json!({
        "personal": valid_personal_data(),
        "business": business,
        "legal_name_check": { "value": "Jordan Lee" }
    });
    let err = validate_form_submission(&form, &elements, &payload).unwrap_err();
    assert!(!err.is_empty());
}
