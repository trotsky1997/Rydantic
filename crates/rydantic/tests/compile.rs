#[test]
fn invalid_declarations_and_mutation_are_compile_errors() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/*.rs");
    #[cfg(feature = "capnp")]
    tests.compile_fail("tests/ui-capnp/*.rs");
    #[cfg(feature = "sqlx")]
    tests.compile_fail("tests/ui-sql/*.rs");
}

#[test]
fn callback_paths_are_not_shadowed_by_generated_locals() {
    let tests = trybuild::TestCases::new();
    tests.pass("tests/ui-pass/*.rs");
}
