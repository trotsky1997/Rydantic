use rydantic::Model;
mod generated {
    include!("fixtures/integer.txt");
}
#[test]
fn generated_integer_source_is_current() {
    let schema = serde_json::from_str(include_str!("fixtures/integer.json")).unwrap();
    assert_eq!(
        rydantic_codegen::generate(&schema).unwrap(),
        include_str!("fixtures/integer.txt")
    );
}
#[test]
fn exact_numeric_expectations_do_not_depend_on_float_rounded_oracle() {
    // DynamicModel's underlying jsonschema 0.33 is not an arbitrary-precision
    // oracle: long decimals near integers may be rounded in its integer check.
    // These acceptance expectations come from exact decimal mathematics instead.
    for (input, expected) in [
        ("1.0", true),
        ("1e0", true),
        ("100e-2", true),
        ("-0.0", true),
        ("0e999999999999999999999999999999", true),
        ("-0.00e-999999999999999999999999999999", true),
        ("9223372036854775807", true),
        ("-9223372036854775808", true),
        ("9223372036854775807.0", true),
        ("-9223372036854775808e0", true),
        ("9223372036854775808", false),
        ("-9223372036854775809", false),
        ("1e999999999999999999999999999999", false),
        ("1e-999999999999999999999999999999", false),
        ("10.00000000000000000000000000001", false),
        ("-10.00000000000000000000000000001", false),
        ("9.99999999999999999999999999999", false),
        ("\"1\"", false),
        ("true", false),
        ("null", false),
        ("1.5", false),
    ] {
        let value = serde_json::from_str(input).unwrap();
        assert_eq!(
            generated::RootModel::parse_value(value).is_ok(),
            expected,
            "{input}"
        );
    }
}
