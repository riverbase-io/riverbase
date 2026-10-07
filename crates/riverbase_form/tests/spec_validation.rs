use std::path::PathBuf;

use riverbase_form::config::{
    load_document_from_path, load_documents_from_dir, load_element_from_path,
    load_elements_from_dir, load_form_from_path, load_forms_from_dir,
};
use riverbase_form::registry::{document_registry, element_registry, form_registry};
use riverbase_form::spec::DocumentNode;
use riverbase_form::validate_key;

fn form_app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/form-app")
}

#[test]
fn validates_example_element_hcl() {
    let path = form_app_dir().join("elements/text_input.hcl");
    let spec = load_element_from_path(&path).expect("element parse");
    validate_key(&spec.key).expect("key format");
    assert_eq!(spec.key, "TXT-0001");
    assert_eq!(spec.table_name, "text_input_data");
    assert!(spec.field.contains_key("value"));
    assert!(spec.field["value"].required);
}

#[test]
fn validates_example_form_hcl() {
    let path = form_app_dir().join("forms/applicant_info.hcl");
    let spec = load_form_from_path(&path).expect("form parse");
    validate_key(&spec.key).expect("key format");
    assert_eq!(spec.key, "FRM-0001");
    assert!(spec.group.contains_key("basic_info"));
    assert!(spec.group["basic_info"].element.contains_key("full_name"));
    assert_eq!(spec.iter_elements().count(), 2);
}

#[test]
fn validates_example_document_hcl() {
    let path = form_app_dir().join("documents/loan_application.hcl");
    let spec = load_document_from_path(&path).expect("document parse");
    validate_key(&spec.key).expect("key format");
    assert_eq!(spec.key, "DOC-0001");
    assert_eq!(spec.nodes.len(), 2);
    assert!(matches!(
        spec.nodes[0],
        DocumentNode::Content { order: 0, .. }
    ));
    assert!(matches!(
        spec.nodes[1],
        DocumentNode::Section { order: 1, .. }
    ));
    if let DocumentNode::Section { children, .. } = &spec.nodes[1] {
        assert_eq!(children.len(), 1);
        assert!(matches!(children[0], DocumentNode::Form { order: 0, .. }));
    } else {
        panic!("expected section node");
    }
}

#[test]
fn registers_all_form_app_specs() {
    element_registry().clear();
    form_registry().clear();
    document_registry().clear();

    let base = form_app_dir();
    let elements = load_elements_from_dir(&base.join("elements")).expect("elements");
    let forms = load_forms_from_dir(&base.join("forms")).expect("forms");
    let documents = load_documents_from_dir(&base.join("documents")).expect("documents");

    for spec in elements {
        element_registry()
            .register(spec.key.clone(), spec)
            .expect("register element");
    }
    for spec in forms {
        form_registry()
            .register(spec.key.clone(), spec)
            .expect("register form");
    }
    for spec in documents {
        document_registry()
            .register(spec.key.clone(), spec)
            .expect("register document");
    }

    assert!(element_registry().get("TXT-0001").is_some());
    assert!(element_registry().get("PER-0001").is_some());
    assert!(element_registry().get("BUS-0001").is_some());
    assert!(form_registry().get("FRM-0001").is_some());
    assert!(form_registry().get("FRM-0002").is_some());
    assert!(document_registry().get("DOC-0001").is_some());
}
