use regex::Regex;
use rydantic::{
    json, rules::Rules, BorrowModel, BorrowType, Context, JsonType, Model, ValidationError,
    ValidationErrors, Value,
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

fn errors_with_prior_and_nested_path(validate: impl FnOnce(&mut Context)) -> Value {
    let mut ctx = Context::new();
    ctx.push(ValidationError::new("prior", "keep the earlier error").with_param("sentinel", 7));
    ctx.at_static("outer/~", |ctx| {
        ctx.at(String::from("dynamic/值"), |ctx| ctx.at(2, validate));
    });
    ctx.push(ValidationError::new("after", "path was restored"));
    ctx.result().unwrap_err().to_json()
}

#[test]
fn borrowed_string_rules_preserve_prior_errors_nested_paths_and_wrapper_forwarding() {
    let rules = Rules {
        min_length: Some(2),
        max_length: Some(3),
        pattern: Some(Regex::new("@").unwrap()),
        ..Rules::default()
    };
    for (text, code, message, actual, limit) in [
        ("😀", "too_short", "length must be >= 2", 1, 2),
        ("😀好é@", "too_long", "length must be <= 3", 4, 3),
    ] {
        let mut expected = vec![
            json!({"loc":[],"code":"prior","message":"keep the earlier error",
                   "params":{"sentinel":7},"stage":"validate"}),
            json!({"loc":["outer/~","dynamic/值",2],"code":code,"message":message,
                   "params":{"actual":actual,"limit":limit},"stage":"validate"}),
        ];
        if !text.contains('@') {
            expected.push(
                json!({"loc":["outer/~","dynamic/值",2],"code":"pattern_mismatch",
                "message":"string does not match the pattern","params":{"pattern":"@"},
                "stage":"validate"}),
            );
        }
        expected.push(
            json!({"loc":[],"code":"after","message":"path was restored",
                            "stage":"validate"}),
        );
        let expected = Value::Array(expected);
        assert_eq!(
            errors_with_prior_and_nested_path(|ctx| rules.validate(&json!(text), ctx)),
            expected
        );
        assert_eq!(
            errors_with_prior_and_nested_path(|ctx| {
                BorrowType::validate_rules(&Some(Box::new(text)), &rules, ctx)
            }),
            expected
        );
        for cow in [Cow::Borrowed(text), Cow::Owned(text.to_owned())] {
            assert_eq!(
                errors_with_prior_and_nested_path(|ctx| {
                    BorrowType::validate_rules(&Box::new(Some(cow)), &rules, ctx)
                }),
                expected
            );
        }
    }
    assert_eq!(
        errors_with_prior_and_nested_path(|ctx| {
            BorrowType::validate_rules(&None::<Box<&str>>, &rules, ctx);
            BorrowType::validate_rules(&Box::new(None::<Cow<'_, str>>), &rules, ctx);
        }),
        json!([
            {"loc":[],"code":"prior","message":"keep the earlier error",
             "params":{"sentinel":7},"stage":"validate"},
            {"loc":[],"code":"after","message":"path was restored","stage":"validate"}
        ])
    );
}

#[derive(Debug, Model)]
struct WrappedStrings<'a> {
    #[model(
        validation_alias = "borrow/~",
        serialization_alias = "borrowed",
        trim,
        min_length = 2,
        max_length = 3
    )]
    borrowed: Box<Option<&'a str>>,
    #[model(
        validation_alias = "cow/值",
        serialization_alias = "cow",
        trim,
        lowercase,
        min_length = 2,
        max_length = 3
    )]
    cow: Box<Option<Cow<'a, str>>>,
}

#[test]
fn wrapped_strings_keep_alias_errors_unicode_boundaries_and_borrowed_ownership() {
    let plain = String::from(r#"{"borrow/~":"  好😀  ","cow/值":"  好😀é  "}"#);
    let parsed = WrappedStrings::parse_json(&plain).unwrap();
    let borrowed = *parsed.borrowed.as_ref().as_ref().unwrap();
    let cow = parsed.cow.as_ref().as_ref().unwrap();
    assert!(std::ptr::eq(
        borrowed.as_ptr(),
        plain[plain.find("好😀").unwrap()..].as_ptr()
    ));
    assert!(matches!(cow, Cow::Borrowed(_)));
    assert!(std::ptr::eq(
        cow.as_ptr(),
        plain[plain.find("好😀é").unwrap()..].as_ptr()
    ));
    assert_eq!(
        parsed.dump().unwrap(),
        json!({"borrowed":"好😀","cow":"好😀é"})
    );

    let escaped = r#"{"borrow/~":null,"cow/值":"  A\u0042  "}"#;
    let parsed = WrappedStrings::parse_json(escaped).unwrap();
    assert!(parsed.borrowed.is_none());
    assert!(matches!(parsed.cow.as_ref(), Some(Cow::Owned(text)) if text == "ab"));
    let absent = WrappedStrings::parse_json("{}").unwrap();
    assert!(absent.borrowed.is_none() && absent.cow.is_none());

    let invalid = r#"{"borrow/~":"  😀  ","cow/值":"  😀😀😀😀  "}"#;
    assert_eq!(
        WrappedStrings::parse_json(invalid).unwrap_err().to_json(),
        json!([
            {"loc":["borrow/~"],"code":"too_short","message":"length must be >= 2",
             "params":{"actual":1,"limit":2},"stage":"validate"},
            {"loc":["cow/值"],"code":"too_long","message":"length must be <= 3",
             "params":{"actual":4,"limit":3},"stage":"validate"}
        ])
    );
}

thread_local! {
    static STRING_CALLBACKS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn check_normalized_text(value: &str, ctx: &Context) -> Result<(), ValidationError> {
    STRING_CALLBACKS.with(|events| {
        events
            .borrow_mut()
            .push(format!("{value}:{}", ctx.error_count()))
    });
    if value == "no" {
        Err(ValidationError::new("reserved", "reserved text").with_param("value", value))
    } else {
        Ok(())
    }
}

#[derive(Debug, Model)]
struct CallbackStrings<'a> {
    #[model(validation_alias = "borrow/~", trim, min_length = 2,
            validate_with_context = check_normalized_text)]
    borrowed: &'a str,
    #[model(validation_alias = "cow/值", trim, lowercase, min_length = 2,
            validate_with_context = check_normalized_text)]
    cow: Cow<'a, str>,
}

#[test]
fn string_callbacks_follow_normalization_and_only_skip_the_failing_field() {
    STRING_CALLBACKS.with(|events| events.borrow_mut().clear());
    let parsed = CallbackStrings::parse_json(r#"{"borrow/~":" ok ","cow/值":" OK "}"#).unwrap();
    assert_eq!(parsed.borrowed, "ok");
    assert_eq!(parsed.cow, "ok");
    STRING_CALLBACKS.with(|events| assert_eq!(*events.borrow(), ["ok:0", "ok:0"]));

    STRING_CALLBACKS.with(|events| events.borrow_mut().clear());
    let errors = CallbackStrings::parse_json(r#"{"borrow/~":" x ","cow/值":" NO "}"#).unwrap_err();
    STRING_CALLBACKS.with(|events| assert_eq!(*events.borrow(), ["no:1"]));
    assert_eq!(
        errors.to_json(),
        json!([
            {"loc":["borrow/~"],"code":"too_short","message":"length must be >= 2",
             "params":{"actual":1,"limit":2},"stage":"validate"},
            {"loc":["cow/值"],"code":"reserved","message":"reserved text",
             "params":{"value":"no"},"stage":"validate"}
        ])
    );
}
