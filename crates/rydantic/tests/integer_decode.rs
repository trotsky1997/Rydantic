use rydantic::{Context, Input, JsonType, RawValue, Stage, ValidationError, Value};
use serde_json::Number;
use std::{fmt::Debug, str::FromStr};

fn check<T: JsonType + FromStr + PartialEq + Debug>(text: &str, name: &str) {
    let number: Number = text.parse().unwrap();
    // Original numeric decode algorithm; deliberately no as_i128/as_u128 oracle.
    let expected = rydantic::rules::integer_text(&number).and_then(|s| s.parse::<T>().ok());
    let mut expected_context = Context::new();
    if expected.is_none() {
        expected_context.at("value", |ctx| {
            ctx.push(
                ValidationError::new(
                    if number.is_f64() {
                        "int_type"
                    } else {
                        "int_out_of_range"
                    },
                    format!("value must be an integer representable as {name}"),
                )
                .with_stage(Stage::Parse),
            );
        });
    }
    let expected_errors = serde_json::to_value(expected_context.result()).unwrap();
    let raw: &RawValue = serde_json::from_str(text).unwrap();
    for input in [Input::Owned(Value::Number(number)), Input::Borrowed(raw)] {
        for coerce in [false, true] {
            let mut context = Context::new();
            let actual = context.at("value", |ctx| T::decode_input(input.clone(), ctx, coerce));
            assert_eq!(actual, expected, "{name}: {text}, coerce={coerce}");
            assert_eq!(
                serde_json::to_value(context.result()).unwrap(),
                expected_errors,
                "{name}: {text}, coerce={coerce}"
            );
        }
    }
}

fn corpus() -> Vec<String> {
    let mut values: Vec<String> = [
        "0",
        "-0",
        "0.0",
        "-0.0",
        "42.0",
        "4.2e1",
        "420e-1",
        "0.42e2",
        "0e999999999999999999999999",
        "-0.000e-999999999999999999999999",
        "1e999999999999999999999999",
        "1e-999999999999999999999999",
        "1e9223372036854775807",
        "1e-9223372036854775808",
        "42.000000000000000000000000001",
        "9007199254740991",
        "9007199254740992",
        "9007199254740993",
        "-170141183460469231731687303715884105729",
        "-170141183460469231731687303715884105728",
        "170141183460469231731687303715884105727",
        "170141183460469231731687303715884105728",
        "340282366920938463463374607431768211455",
        "340282366920938463463374607431768211456",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for power in [7, 8, 15, 16, 31, 32, 63, 64, 127] {
        let n = 1u128 << power;
        for magnitude in [n - 1, n, n + 1] {
            for sign in ["", "-"] {
                for suffix in ["", ".0", "e0"] {
                    values.push(format!("{sign}{magnitude}{suffix}"));
                }
            }
        }
    }
    let mut state = 0x6a09e667f3bcc908bb67ae8584caa73bu128;
    for _ in 0..512 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        values.push(state.to_string());
        values.push((state as i128).to_string());
    }
    values
}

#[test]
fn all_primitive_integer_decoders_match_original_values_and_full_errors() {
    for text in corpus() {
        macro_rules! check_types { ($($ty:ty),*) => {$(check::<$ty>(&text,stringify!($ty));)*}; }
        check_types!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);
    }
}

#[test]
fn integer_string_coercion_and_non_number_errors_are_unchanged() {
    macro_rules! check_types { ($($ty:ty),*) => {$(
        for text in ["42", "-0", "42.0", "4.2e1", "340282366920938463463374607431768211455", "not a number"] {
            let mut ctx=Context::new();
            let actual=<$ty>::decode(Value::String(text.into()), &mut ctx,true);
            assert_eq!(actual,text.parse::<$ty>().ok());
            let mut expected=Context::new();
            if actual.is_none() { expected.push(ValidationError::new("int_parsing",concat!("string must contain a valid ",stringify!($ty))).with_stage(Stage::Parse)); }
            assert_eq!(serde_json::to_value(ctx.result()).unwrap(),serde_json::to_value(expected.result()).unwrap());
        }
        for value in [Value::Null, Value::Bool(true), Value::String("42".into()),serde_json::json!([]),serde_json::json!({})] {
            let mut ctx=Context::new();assert!(<$ty>::decode(value,&mut ctx,false).is_none());
            assert_eq!(ctx.result().unwrap_err().errors()[0].code,"type_error");
        }
    )*}; }
    check_types!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);
}
