#[test]
fn invalid_declarations_and_mutation_are_compile_errors() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/*.rs");
    #[cfg(feature = "capnp")]
    tests.compile_fail("tests/ui-capnp/*.rs");
}
