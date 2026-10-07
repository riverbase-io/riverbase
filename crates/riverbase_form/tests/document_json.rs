use riverbase_form::config::{load_documents_from_dir, load_forms_from_dir};
use riverbase_form::registry::{document_registry, form_registry};
use riverbase_form::template::generate;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn document_to_json_has_nested_structure() {
    form_registry().clear();
    document_registry().clear();

    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/form-app");
    for spec in load_forms_from_dir(&base.join("forms")).unwrap() {
        form_registry().register(spec.key.clone(), spec).unwrap();
    }
    for spec in load_documents_from_dir(&base.join("documents")).unwrap() {
        document_registry()
            .register(spec.key.clone(), spec)
            .unwrap();
    }

    let template = document_registry().get("DOC-0001").unwrap();
    let doc = generate(&template, &json!({ "applicant_name": "Alice" })).unwrap();
    let dumped = doc.to_json();

    assert_eq!(dumped["node_type"], "Document");
    assert_eq!(dumped["document_key"], "DOC-0001");
    assert!(dumped["children"].is_array());
    assert_eq!(dumped["children"][0]["node_type"], "Content");
    assert_eq!(dumped["children"][0]["content"], "Application for Alice");
}
