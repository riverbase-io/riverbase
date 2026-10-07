use std::fs;

use riverbase_form::cli::codegen::codegen_elements;
use tempfile::TempDir;

#[test]
fn codegen_emits_schema_entities_and_migration() {
    let elements_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/form-app/elements");
    let tmp = TempDir::new().expect("tempdir");
    let out_rs = tmp.path().join("generated");
    let out_migrations = tmp.path().join("migrations");

    codegen_elements(&elements_dir, &out_rs, &out_migrations).expect("codegen");

    let schema = fs::read_to_string(out_rs.join("schema.rs")).expect("schema.rs");
    assert!(schema.contains("text_input_data"));
    assert!(schema.contains("value -> Text"));

    let entities = fs::read_to_string(out_rs.join("entities.rs")).expect("entities.rs");
    assert!(entities.contains("TextInputDataEntity"));
    assert!(entities.contains("register_element_entities"));

    let mig_dirs: Vec<_> = fs::read_dir(&out_migrations)
        .expect("migrations dir")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(mig_dirs.len(), 1);
    let mig_dir = mig_dirs[0].path();
    let up = fs::read_to_string(mig_dir.join("up.sql")).expect("up.sql");
    assert!(up.contains("CREATE SCHEMA IF NOT EXISTS riverbase_form"));
    assert!(up.contains("text_input_data"));
}
