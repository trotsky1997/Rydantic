use regex::Regex;
use rydantic::{
    json, rules::Rules, BorrowModel, BorrowType, Context, JsonType, Model, ValidationErrors, Value,
};
use std::borrow::Cow;

fn errors_at_value(validate: impl FnOnce(&mut Context)) -> Value {
    let mut ctx = Context::new();
    ctx.at("value", validate);
    ctx.result().unwrap_err().to_json()
}

fn short_errors() -> Value {
    json!([
        {"loc":["value"],"code":"too_short","message":"length must be >= 3",
         "params":{"actual":1,"limit":3},"stage":"validate"},
        {"loc":["value"],"code":"pattern_mismatch","message":"string does not match the pattern",
         "params":{"pattern":"@"},"stage":"validate"},
        {"loc":["value"],"code":"email","message":"string must be a valid email address",
         "stage":"validate"}
    ])
}

#[test]
fn direct_generic_and_borrowed_string_rules_have_the_same_complete_errors() {
    let rules = Rules {
        min_length: Some(3),
        max_length: Some(12),
        pattern: Some(Regex::new("@").unwrap()),
        email: true,
        ..Rules::default()
    };
    for text in ["x", "😀", "a-long-name@example.com"] {
        let expected = if text.chars().count() == 1 {
            short_errors()
        } else {
            json!([{"loc":["value"],"code":"too_long","message":"length must be <= 12",
                    "params":{"actual":23,"limit":12},"stage":"validate"}])
        };
        assert_eq!(
            errors_at_value(|ctx| rules.validate(&json!(text), ctx)),
            expected
        );
        assert_eq!(
            errors_at_value(|ctx| JsonType::validate_rules(&text.to_owned(), &rules, ctx)),
            expected
        );
        assert_eq!(
            errors_at_value(|ctx| BorrowType::validate_rules(&text, &rules, ctx)),
            expected
        );
        for cow in [Cow::Borrowed(text), Cow::Owned(text.to_owned())] {
            assert_eq!(
                errors_at_value(|ctx| BorrowType::validate_rules(&cow, &rules, ctx)),
                expected
            );
        }
    }
}

#[test]
fn generic_collection_length_rules_are_still_checked_once() {
    let rules = Rules {
        min_length: Some(1),
        ..Rules::default()
    };
    let expected = json!([{
        "loc":["value"],"code":"too_short","message":"length must be >= 1",
        "params":{"actual":0,"limit":1},"stage":"validate"
    }]);
    for value in [json!([]), json!({})] {
        assert_eq!(errors_at_value(|ctx| rules.validate(&value, ctx)), expected);
    }
    let mut ctx = Context::new();
    rules.validate(&Value::Null, &mut ctx);
    assert!(ctx.result().is_ok());
}

#[derive(Debug, Model)]
struct BorrowedText<'a> {
    #[model(trim, min_length = 3, max_length = 12, pattern = "@", email)]
    value: &'a str,
}

#[derive(Debug, Model)]
struct CowText<'a> {
    #[model(trim, lowercase, min_length = 3, max_length = 12, pattern = "@", email)]
    value: Cow<'a, str>,
}

#[derive(Debug, Model)]
struct OwnedText {
    #[model(trim, lowercase, min_length = 3, max_length = 12, pattern = "@", email)]
    value: String,
}

#[test]
fn model_strings_normalize_before_emitting_length_pattern_and_email_errors() {
    let input = r#"{"value":"  x  "}"#;
    assert_eq!(
        BorrowedText::parse_json(input).unwrap_err().to_json(),
        short_errors()
    );
    assert_eq!(
        CowText::parse_json(input).unwrap_err().to_json(),
        short_errors()
    );
    assert_eq!(
        OwnedText::parse_json(input).unwrap_err().to_json(),
        short_errors()
    );
}

fn assert_short(result: Result<(), ValidationErrors>) {
    assert_eq!(result.unwrap_err().to_json(), short_errors());
}

#[test]
fn model_revalidation_and_serialization_do_not_duplicate_string_errors() {
    let borrowed = BorrowedText { value: "x" };
    assert_short(borrowed.validate());
    assert_eq!(borrowed.dump().unwrap_err().to_json(), short_errors());
    for value in [Cow::Borrowed("x"), Cow::Owned("x".to_owned())] {
        let cow = CowText { value };
        assert_short(cow.validate());
        assert_eq!(cow.dump().unwrap_err().to_json(), short_errors());
    }
    let owned = OwnedText {
        value: "x".to_owned(),
    };
    assert_short(owned.validate());
    assert_eq!(owned.dump().unwrap_err().to_json(), short_errors());
}

#[test]
fn successful_normalization_preserves_borrowing_and_checks_transformed_strings() {
    let input = String::from(r#"{"value":"  a@b.co  "}"#);
    let borrowed = BorrowedText::parse_json(&input).unwrap();
    let start = input.find("a@b.co").unwrap();
    assert!(std::ptr::eq(
        borrowed.value.as_ptr(),
        input[start..].as_ptr()
    ));
    let cow = CowText::parse_json(&input).unwrap();
    assert!(matches!(cow.value, Cow::Borrowed(_)));
    assert!(std::ptr::eq(cow.value.as_ptr(), input[start..].as_ptr()));
    let uppercase = r#"{"value":"  A@B.CO  "}"#;
    let cow = CowText::parse_json(uppercase).unwrap();
    assert!(matches!(cow.value, Cow::Owned(_)));
    assert_eq!(cow.value, "a@b.co");
    assert_eq!(OwnedText::parse_json(uppercase).unwrap().value, "a@b.co");
    let escaped = r#"{"value":"  A@B.\u0043O  "}"#;
    assert_eq!(CowText::parse_json(escaped).unwrap().value, "a@b.co");
    assert_eq!(OwnedText::parse_json(escaped).unwrap().value, "a@b.co");
}
