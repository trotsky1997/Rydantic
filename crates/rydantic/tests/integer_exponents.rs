use rydantic::{Model, Value};

#[derive(Debug, PartialEq, Model)]
struct Bounded(#[model(ge = -10, le = 10)] i64);

#[test]
fn zero_with_arbitrarily_large_exponents_is_an_integer() {
    for text in [
        "0e999999999999999999999999999999",
        "0e+999999999999999999999999999999",
        "0e-999999999999999999999999999999",
        "-0.000E+999999999999999999999999999999",
        "-0.000E-999999999999999999999999999999",
        "0e9223372036854775807",
        "0.0e-9223372036854775808",
        "-0.0",
    ] {
        let value: Value = serde_json::from_str(text).unwrap();
        assert_eq!(Bounded::parse_value(value).unwrap(), Bounded(0), "{text}");
        assert_eq!(Bounded::parse_json(text).unwrap(), Bounded(0), "{text}");
    }
}

#[test]
fn nonzero_numbers_preserve_exact_integer_and_range_checks() {
    for text in ["1.0", "1e0", "10e-1", "0.01e2"] {
        assert_eq!(Bounded::parse_json(text).unwrap(), Bounded(1), "{text}");
    }
    for text in [
        "1e999999999999999999999999999999",
        "1e-999999999999999999999999999999",
        "-1e999999999999999999999999999999",
        "0.00001e-999999999999999999999999999999",
        "11",
        "-11",
        "10.00000000000000000000000000001",
        "-10.00000000000000000000000000001",
        "9.99999999999999999999999999999",
        "9223372036854775808",
        "-9223372036854775809",
    ] {
        let value: Value = serde_json::from_str(text).unwrap();
        assert!(Bounded::parse_value(value).is_err(), "{text}");
        assert!(Bounded::parse_json(text).is_err(), "{text}");
    }
}

#[test]
fn malformed_numbers_and_numeric_strings_remain_invalid() {
    for text in [
        "0e",
        "0e+",
        "0e-",
        "0e++1",
        "0e--1",
        "0e+-1",
        "0e1.0",
        "0ee1",
        "00e1",
        "0.e1",
        ".0e1",
        "+0e1",
        "0e1x",
        "0e 1",
        "\"0e999999999999999999999999999999\"",
    ] {
        assert!(Bounded::parse_json(text).is_err(), "{text}");
    }
}
