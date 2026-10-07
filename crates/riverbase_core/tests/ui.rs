//! Compile-fail diagnostics for `query_resource!` ([DX-02]).

#[test]
fn query_resource_diagnostics() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/query_unknown_attr.rs");
    t.compile_fail("tests/ui/query_unknown_preset.rs");
    t.compile_fail("tests/ui/sort_column_not_orderable.rs");
}
