use riverbase_form::config::{load_documents_from_dir, load_elements_from_dir, load_forms_from_dir};
use riverbase_form::registry::{document_registry, element_registry, form_registry};
use riverbase_form::template::generate;
use serde_json::json;
use std::path::PathBuf;

fn load_form_app() {
    element_registry().clear();
    form_registry().clear();
    document_registry().clear();

    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/form-app");
    for spec in load_elements_from_dir(&base.join("elements")).unwrap() {
        element_registry().register(spec.key.clone(), spec).unwrap();
    }
    for spec in load_forms_from_dir(&base.join("forms")).unwrap() {
        form_registry().register(spec.key.clone(), spec).unwrap();
    }
    for spec in load_documents_from_dir(&base.join("documents")).unwrap() {
        document_registry()
            .register(spec.key.clone(), spec)
            .unwrap();
    }
}

#[test]
fn generates_loan_application_from_template() {
    load_form_app();
    let template = document_registry()
        .get("DOC-0001")
        .expect("DOC-0001 template");
    let input = json!({ "applicant_name": "Jane Doe", "full_name": "Jane Doe" });
    let doc = generate(&template, &input).expect("generate");

    assert_eq!(doc.document_key, "DOC-0001");
    assert_eq!(doc.children.len(), 2);
    let intro = doc.children.first().expect("intro");
    if let riverbase_form::spec::RuntimeDocumentNode::Content { content, .. } = intro {
        assert_eq!(content.as_deref(), Some("Application for Jane Doe"));
    } else {
        panic!("expected content node");
    }
}
