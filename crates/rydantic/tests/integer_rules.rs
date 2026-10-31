// Semantic regression coverage retained from PR #127 at 7f4c86bcaddf943f279df9fcf9fcdfc265148c49.
// Importing these tests does not import the draft optimization or its timing claims.
use rydantic::{
    json,
    rules::{compare_numbers, Rules},
    BorrowModel, Context, Input, JsonType, Model, RawValue, Stage, ValidationError, Value,
};
use serde_json::Number;
use std::{cmp::Ordering, fmt::Debug, str::FromStr};

// Freeze the pre-optimization numeric error loop as well as checking the public
// generic route, so a shared helper cannot silently change both sides' errors.
fn original_numeric_rules(number: &Number, rules: &Rules, ctx: &mut Context) {
    for (bound, operator, code) in [
        (&rules.ge, ">=", "greater_than_equal"),
        (&rules.gt, ">", "greater_than"),
        (&rules.le, "<=", "less_than_equal"),
        (&rules.lt, "<", "less_than"),
    ] {
        if let Some(bound) = bound {
            let order = compare_numbers(number, bound);
            let valid = match operator {
                ">=" => order != Ordering::Less,
                ">" => order == Ordering::Greater,
                "<=" => order != Ordering::Greater,
                _ => order == Ordering::Less,
            };
            if !valid {
                ctx.push(
                    ValidationError::new(code, format!("value must be {operator} {bound}"))
                        .with_param("limit", bound.clone()),
                );
            }
        }
    }
}

fn at_value(ctx: &mut Context, action: impl FnOnce(&mut Context)) {
    ctx.at("outer/~", |ctx| ctx.at(2usize, action));
}

fn check<T: JsonType + Debug>(value: &T, rules: &Rules) {
    let mut encode_ctx = Context::new();
    let encoded = value.encode(&mut encode_ctx);
    assert!(encode_ctx.result().is_ok());
    let mut actual = Context::new();
    at_value(&mut actual, |ctx| value.validate_rules(rules, ctx));
    let mut generic = Context::new();
    at_value(&mut generic, |ctx| rules.validate(&encoded, ctx));
    let mut frozen = Context::new();
    at_value(&mut frozen, |ctx| {
        original_numeric_rules(encoded.as_number().unwrap(), rules, ctx)
    });
    let actual = serde_json::to_value(actual.result()).unwrap();
    assert_eq!(
        actual,
        serde_json::to_value(generic.result()).unwrap(),
        "native vs generic: value={value:?}, rules={rules:?}"
    );
    assert_eq!(
        actual,
        serde_json::to_value(frozen.result()).unwrap(),
        "native vs frozen: value={value:?}, rules={rules:?}"
    );
}

fn bounds() -> Vec<Number> {
    [
        "0",
        "-0",
        "1",
        "-1",
        "41",
        "42",
        "43",
        "42.0",
        "4.2e1",
        "420e-1",
        "-0.0",
        "0e999999999999999999999999",
        "-0e-999999999999999999999999",
        "42.00000000000000000000000000000000001",
        "41.99999999999999999999999999999999999",
        "9007199254740991",
        "9007199254740992",
        "9007199254740993",
        "-9223372036854775809",
        "-9223372036854775808",
        "9223372036854775807",
        "9223372036854775808",
        "18446744073709551615",
        "18446744073709551616",
        "-170141183460469231731687303715884105729",
        "-170141183460469231731687303715884105728",
        "170141183460469231731687303715884105727",
        "170141183460469231731687303715884105728",
        "340282366920938463463374607431768211455",
        "340282366920938463463374607431768211456",
        "1e9223372036854775807",
        "1e9223372036854775808",
        "1e-9223372036854775808",
        "1e-9223372036854775809",
        "-1e999999999999999999999999",
        "1e999999999999999999999999",
        "1e-999999999999999999999999",
        "-1e-999999999999999999999999",
    ]
    .into_iter()
    .map(|text| text.parse().unwrap())
    .collect()
}

#[test]
fn all_integer_types_match_generic_and_frozen_rules_with_complete_errors() {
    let bounds = bounds();
    let mut cases = 0;
    let pattern = regex::Regex::new("never matches").unwrap();
    macro_rules! check_types {
        ($($ty:ty),*) => {$(
            for value in [<$ty>::MIN, <$ty>::MAX, 0, 1, 42, -1i128 as $ty] {
                check(&value, &Rules::default());
                cases += 1;
                for bound in &bounds {
                    for shape in 0..5 {
                        let rules = Rules {
                            ge: (shape == 0 || shape == 4).then(|| bound.clone()),
                            gt: (shape == 1 || shape == 4).then(|| bound.clone()),
                            le: (shape == 2 || shape == 4).then(|| bound.clone()),
                            lt: (shape == 3 || shape == 4).then(|| bound.clone()),
                            // These unrelated public rule fields must remain ignored
                            // for integer values, even when manually constructed.
                            min_length: Some(100), max_length: Some(0),
                            trim: true, lowercase: true, uppercase: true, email: true,
                            pattern: Some(pattern.clone()),
                        };
                        check(&value, &rules);
                        cases += 1;
                    }
                }
            }
        )*};
    }
    check_types!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);
    assert_eq!(bounds.len(), 38);
    assert_eq!(cases, 13_752);
}

#[test]
fn generated_values_and_mixed_native_fallback_bounds_preserve_every_error() {
    let bounds = bounds();
    let mut state = 0x6a09e667f3bcc908bb67ae8584caa73bu128;
    let mut cases = 0;
    macro_rules! check_types {
        ($($ty:ty),*) => {$(
            for index in 0..64 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let value = state as $ty;
                let rules = Rules {
                    ge: Some(Number::from_i128(state as i128).unwrap()),
                    gt: Some(bounds[index % bounds.len()].clone()),
                    le: Some(bounds[(index + 11) % bounds.len()].clone()),
                    lt: Some(Number::from_u128(state).unwrap()),
                    ..Rules::default()
                };
                check(&value, &rules);
                cases += 1;
            }
        )*};
    }
    check_types!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);
    assert_eq!(cases, 768);

    // Failures alternate direct and fallback comparisons; all four must be
    // emitted once, in the original order, without losing existing errors/path.
    let rules = Rules {
        ge: Some("43".parse().unwrap()),
        gt: Some("42.0".parse().unwrap()),
        le: Some("41".parse().unwrap()),
        lt: Some("4.2e1".parse().unwrap()),
        ..Rules::default()
    };
    let sentinel = ValidationError::new("existing", "retain earlier diagnostics")
        .with_stage(Stage::Serialize)
        .at("before")
        .with_param("detail", json!({"x": [1, 2]}));
    let mut actual = Context::new();
    actual.push(sentinel.clone());
    at_value(&mut actual, |ctx| 42i128.validate_rules(&rules, ctx));
    actual.push(ValidationError::new("after", "path restored"));
    let errors = actual.result().unwrap_err();
    let mut expected = Context::new();
    expected.push(sentinel);
    at_value(&mut expected, |ctx| {
        original_numeric_rules(&Number::from(42), &rules, ctx)
    });
    expected.push(ValidationError::new("after", "path restored"));
    assert_eq!(errors.to_json(), expected.result().unwrap_err().to_json());
    assert_eq!(errors.len(), 6);
    assert_eq!(errors.errors()[1].pointer(), "/outer~1~0/2");
    assert!(errors.errors()[5].loc.is_empty());
}

fn check_decode<T: JsonType + Debug + PartialEq + FromStr>(text: &str, expected: &str) {
    let expected = expected.parse::<T>().ok().unwrap();
    let raw: &RawValue = serde_json::from_str(text).unwrap();
    let owned: Value = serde_json::from_str(text).unwrap();
    for input in [Input::Owned(owned), Input::Borrowed(raw)] {
        for coerce in [false, true] {
            let mut ctx = Context::new();
            let actual = T::decode_input(input.clone(), &mut ctx, coerce).unwrap();
            assert_eq!(actual, expected);
            assert!(ctx.result().is_ok());
            for bound in ["0", "-0", "0.0", "42", "42.0", "4.2e1"] {
                let rules = Rules {
                    ge: Some(bound.parse().unwrap()),
                    gt: Some(bound.parse().unwrap()),
                    le: Some(bound.parse().unwrap()),
                    lt: Some(bound.parse().unwrap()),
                    ..Rules::default()
                };
                check(&actual, &rules);
            }
        }
    }
    let mut ctx = Context::new();
    let coerced = T::decode(
        Value::String(expected.encode(&mut ctx).to_string()),
        &mut ctx,
        true,
    )
    .unwrap();
    assert!(ctx.result().is_ok());
    assert_eq!(coerced, expected);
    check(
        &coerced,
        &Rules {
            gt: Some("42.0".parse().unwrap()),
            ..Rules::default()
        },
    );
}

#[test]
fn normalized_and_coerced_integer_inputs_retain_rule_semantics() {
    for (text, expected) in [
        ("0", "0"),
        ("-0", "0"),
        ("42", "42"),
        ("42.0", "42"),
        ("4.2e1", "42"),
        ("420e-1", "42"),
        ("0e999999999999999999999999", "0"),
        ("-0.000e-999999999999999999999999", "0"),
    ] {
        macro_rules! check_types { ($($ty:ty),*) => {$(check_decode::<$ty>(text, expected);)*}; }
        check_types!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);
    }
}

#[derive(Debug, Model)]
struct Entry {
    #[model(validation_alias = "wire/~id", serialization_alias = "id", ge = 0, gt = -1, le = 42, lt = 43)]
    value: i128,
    #[model(ge = 0)]
    wide: u128,
}

#[derive(Debug, Model)]
struct Nested {
    rows: Vec<Entry>,
}

#[derive(Debug, Model)]
struct Borrowed<'a> {
    name: &'a str,
    #[model(ge = 0, le = 42)]
    value: i64,
    #[model(gt = 0)]
    optional: Option<u128>,
}

#[test]
fn derived_owned_borrowed_nested_and_dump_paths_preserve_rules() {
    let input = r#"{"rows":[{"wire/~id":4.2e1,"wide":340282366920938463463374607431768211455}]}"#;
    let expected = json!({"rows": [{"id": 42, "wide": u128::MAX}]});
    for parsed in [
        Nested::parse_json(input).unwrap(),
        Nested::parse_value(serde_json::from_str(input).unwrap()).unwrap(),
    ] {
        assert!(parsed.validate().is_ok());
        assert_eq!(parsed.dump().unwrap(), expected);
    }
    let invalid = r#"{"rows":[{"wire/~id":-2,"wide":0},{"wire/~id":44,"wide":0}]}"#;
    let mut expected_ctx = Context::new();
    let rules = Rules {
        ge: Some(0.into()),
        gt: Some((-1).into()),
        le: Some(42.into()),
        lt: Some(43.into()),
        ..Rules::default()
    };
    expected_ctx.at("rows", |ctx| {
        for (index, value) in [(0usize, -2), (1, 44)] {
            ctx.at(index, |ctx| {
                ctx.at("wire/~id", |ctx| {
                    original_numeric_rules(&Number::from(value), &rules, ctx)
                })
            });
        }
    });
    let expected_errors = expected_ctx.result().unwrap_err().to_json();
    assert_eq!(
        Nested::parse_json(invalid).unwrap_err().to_json(),
        expected_errors
    );
    assert_eq!(
        Nested::parse_value(serde_json::from_str(invalid).unwrap())
            .unwrap_err()
            .to_json(),
        expected_errors
    );
    let constructed = Nested {
        rows: vec![Entry { value: -2, wide: 0 }, Entry { value: 44, wide: 0 }],
    };
    assert_eq!(
        constructed.validate().unwrap_err().to_json(),
        expected_errors
    );
    assert_eq!(constructed.dump().unwrap_err().to_json(), expected_errors);

    let input = String::from(
        r#"{"name":"borrowed","value":42.0,"optional":340282366920938463463374607431768211455}"#,
    );
    let borrowed = Borrowed::parse_json(&input).unwrap();
    assert!(std::ptr::eq(
        borrowed.name.as_ptr(),
        input[input.find("borrowed").unwrap()..].as_ptr()
    ));
    assert_eq!(borrowed.optional, Some(u128::MAX));
    assert!(borrowed.validate().is_ok());
    assert_eq!(
        borrowed.dump().unwrap(),
        json!({"name":"borrowed", "value":42, "optional":u128::MAX})
    );
    assert!(Borrowed::parse_json(r#"{"name":"borrowed","value":0,"optional":null}"#).is_ok());
    let invalid = Borrowed {
        name: "borrowed",
        value: -1,
        optional: Some(0),
    };
    let errors = invalid.validate().unwrap_err().to_json();
    assert_eq!(invalid.dump().unwrap_err().to_json(), errors);
    assert_eq!(
        Borrowed::parse_json(r#"{"name":"borrowed","value":-1,"optional":0}"#)
            .unwrap_err()
            .to_json(),
        errors
    );
}
