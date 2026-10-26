//! Compatibility oracle for fixed named-model raw slots. The map route below
//! freezes the pre-slot materialization boundary; the printed records also allow
//! the entire test to be run unchanged on an independently built baseline,
//! covering nested decoders that the root map route alone cannot freeze.
use rydantic::{
    json, AsyncCheck, BorrowModel, BorrowType, Context, Input, JsonType, Model, RawValue,
    SchemaContext, Stage, ValidationError, ValidationErrors, Value,
};
use std::{borrow::Cow, cell::RefCell, collections::BTreeMap, fmt::Debug};

thread_local! {
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}
fn event(value: String) {
    EVENTS.with(|events| events.borrow_mut().push(value));
}
fn take_events() -> Vec<String> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}
fn before(label: &str, value: Value, ctx: &Context) -> Result<Value, ValidationError> {
    event(format!("before:{label}:{}:{value}", ctx.error_count()));
    if value == json!("reject-before") {
        Err(
            ValidationError::new("before_rejected", "before rejected input")
                .with_param("label", label),
        )
    } else {
        Ok(value)
    }
}
fn before_z(value: Value, ctx: &Context) -> Result<Value, ValidationError> {
    before("z", value, ctx)
}
fn before_a(value: Value, ctx: &Context) -> Result<Value, ValidationError> {
    before("a", value, ctx)
}
fn before_d(value: Value, ctx: &Context) -> Result<Value, ValidationError> {
    before("d", value, ctx)
}
fn check(label: &str, value: i64, ctx: &Context) -> Result<(), ValidationError> {
    event(format!("check:{label}:{}:{value}", ctx.error_count()));
    if value == 13 {
        Err(ValidationError::new("unlucky", "thirteen rejected").with_param("label", label))
    } else {
        Ok(())
    }
}
fn check_z(value: &i64, ctx: &Context) -> Result<(), ValidationError> {
    check("z", *value, ctx)
}
fn check_d(value: &i64, ctx: &Context) -> Result<(), ValidationError> {
    check("d", *value, ctx)
}
fn default_d() -> i64 {
    event("default:d".into());
    7
}
fn check_model(value: &Traced, ctx: &Context) -> Result<(), ValidationErrors> {
    event(format!(
        "model:{}:{}:{}",
        ctx.error_count(),
        value.z,
        value.a
    ));
    if value.z == value.a {
        Err(ValidationError::new("equal_fields", "z and a must differ").into())
    } else {
        Ok(())
    }
}
#[derive(Debug, PartialEq, Model)]
#[model(validate_with_context = check_model)]
struct Traced {
    #[model(
        validation_alias = "z/~",
        serialization_alias = "output",
        before_with_context = before_z,
        validate_with_context = check_z,
        ge = -5
    )]
    z: i64,
    #[model(before_with_context = before_a, coerce)]
    a: i64,
    #[model(default_factory = default_d, before_with_context = before_d, validate_with_context = check_d)]
    d: i64,
    #[model(required)]
    maybe: Option<i64>,
}
#[derive(Debug, PartialEq, Model)]
struct Nested {
    #[model(validation_alias = "items/~")]
    items: Vec<Traced>,
    #[model(default)]
    flag: bool,
}

// The exact typed BTreeMap<String, &RawValue> boundary used by the previous
// implementation, including type-error mapping and decoded-key last-wins.
fn frozen_map<'de>(raw: &'de RawValue, ctx: &mut Context) -> Option<Input<'de>> {
    serde_json::from_str::<BTreeMap<String, &'de RawValue>>(raw.get())
        .map(|map| {
            Input::Object(
                map.into_iter()
                    .map(|(key, value)| (key, Input::Borrowed(value)))
                    .collect(),
            )
        })
        .map_err(|_| {
            ctx.push(
                ValidationError::new("type_error", "expected object").with_stage(Stage::Parse),
            );
        })
        .ok()
}
fn frozen_json_error(error: serde_json::Error) -> ValidationErrors {
    ValidationError::new("json_invalid", error.to_string())
        .with_stage(Stage::Parse)
        .with_param("line", error.line())
        .with_param("column", error.column())
        .into()
}
fn parse<'de, T: BorrowType<'de>>(text: &'de str, map: bool) -> Result<T, ValidationErrors> {
    let raw = serde_json::from_str::<&'de RawValue>(text).map_err(frozen_json_error)?;
    let mut ctx = Context::new();
    let result = if map {
        frozen_map(raw, &mut ctx).and_then(|input| T::decode(input, &mut ctx, false))
    } else {
        T::decode(Input::Borrowed(raw), &mut ctx, false)
    };
    ctx.finish(result)
}
fn snapshot<T: Debug>(result: &Result<T, ValidationErrors>) -> Value {
    match result {
        Ok(value) => json!({"ok":format!("{value:?}")}),
        Err(errors) => {
            json!({"errors":serde_json::to_string(errors).unwrap(), "display":errors.to_string()})
        }
    }
}
fn parity<'de, T: BorrowType<'de> + PartialEq + Debug>(text: &'de str) -> Value {
    take_events();
    let old = parse::<T>(text, true);
    let old_events = take_events();
    let actual = parse::<T>(text, false);
    let actual_events = take_events();
    assert_eq!(old, actual, "input: {text}");
    assert_eq!(snapshot(&old), snapshot(&actual), "input: {text}");
    assert_eq!(old_events, actual_events, "input: {text}");
    json!({"input":text,"result":snapshot(&actual),"events":actual_events})
}

#[test]
fn duplicate_survivors_and_callbacks_follow_declaration_order() {
    let text = r#"{"a":"8","z/~":"reject-before","maybe":null,"z\u002f~":3,"d":13,"d":7}"#;
    let record = parity::<Traced>(text);
    assert_eq!(
        record["events"],
        json!([
            "before:z:0:3",
            "check:z:0:3",
            "before:a:0:\"8\"",
            "before:d:0:7",
            "check:d:0:7",
            "model:0:3:8"
        ])
    );
    let record = parity::<Traced>(r#"{"a":"reject-before","z/~":-6,"maybe":null}"#);
    assert_eq!(
        record["events"],
        json!([
            "before:z:0:-6",
            "before:a:1:\"reject-before\"",
            "default:d",
            "before:d:2:7",
            "check:d:2:7"
        ])
    );
}

#[test]
fn full_errors_keep_declared_fields_before_sorted_deduplicated_unknowns() {
    let text = r#"{"z/~":13,"a":false,"z":0,"\u007a":1,"a/b~c":0,"é":1,"$serde_json::private::Number":"9","$serde_json::private::RawValue":"[]"}"#;
    parity::<Traced>(text);
    let errors = parse::<Traced>(text, false).unwrap_err();
    assert_eq!(
        errors
            .errors()
            .iter()
            .map(ValidationError::pointer)
            .collect::<Vec<_>>(),
        [
            "/z~1~0",
            "/a",
            "/maybe",
            "/$serde_json::private::Number",
            "/$serde_json::private::RawValue",
            "/a~1b~0c",
            "/z",
            "/é"
        ]
    );
    take_events();
}

#[derive(Debug, PartialEq, Model)]
#[model(extra = "ignore")]
struct Ignored {
    #[model(before_with_context = before_a, coerce)]
    a: i64,
}
#[test]
fn ignored_fields_and_discarded_duplicates_still_require_complete_valid_json() {
    for text in [
        r#"{"a":1,"ignored":[0,{"deep":[true,null,1e+99999]}]}"#,
        r#"{"a":"reject-before","a":2,"ignored":{"$serde_json::private::Number":"1"}}"#,
        r#"{"a":1,"ignored":[1,]}"#,
        r#"{"a":1,"ignored":{"x":1} trailing}"#,
        r#"{"a":1,"ignored":"\uD800"}"#,
        r#"{"a":1,"a":[1,],"a":2}"#,
        r#"{"a":1} false"#,
        r#"{"a":1,"\uD800":0}"#,
        r#"{"a":1,"\uDC00":0}"#,
    ] {
        parity::<Ignored>(text);
        take_events();
        let result = Ignored::parse_json(text);
        let events = take_events();
        if serde_json::from_str::<&RawValue>(text).is_err() {
            assert_eq!(result.unwrap_err().errors()[0].code, "json_invalid");
            assert!(
                events.is_empty(),
                "syntax must fail before callbacks: {text}"
            );
        } else if let Err(errors) = result {
            // RawValue may accept a raw escaped key that fails decoded-key validation.
            assert_eq!(errors.errors()[0].code, "type_error");
            assert!(
                events.is_empty(),
                "keys must decode before callbacks: {text}"
            );
        }
    }
}

#[derive(Debug)]
struct RawRef<'a>(&'a RawValue);
impl PartialEq for RawRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.get() == other.0.get()
    }
}
impl AsyncCheck for RawRef<'_> {}
impl<'de: 'a, 'a> BorrowType<'de> for RawRef<'a> {
    fn decode(input: Input<'de>, _: &mut Context, _: bool) -> Option<Self> {
        let Input::Borrowed(raw) = input else {
            panic!("raw field lost its original slice")
        };
        Some(Self(raw))
    }
    fn encode(&self, _: &mut Context) -> Value {
        serde_json::from_str(self.0.get()).unwrap()
    }
    fn schema(_: &mut SchemaContext, _: bool) -> Value {
        json!({})
    }
}
#[derive(Debug, PartialEq, Model)]
struct Borrowed<'a> {
    #[model(validation_alias = "wire/name", trim, min_length = 2)]
    name: &'a str,
    cow: Cow<'a, str>,
    raw: RawRef<'a>,
    integer: u128,
    #[model(validation_alias = "$serde_json::private::Number")]
    marker: i64,
    #[model(validation_alias = "$serde_json::private::RawValue")]
    raw_marker: bool,
}
#[test]
fn borrows_exact_numeric_text_and_private_marker_keys_are_preserved() {
    let text = r#"{"wire/name":"discarded","wire\u002fname":" final ","cow":"plain","raw":{ "n":1e+100, "big":340282366920938463463374607431768211455 },"integer":340282366920938463463374607431768211455,"$serde_json::private::Number":42,"$serde_json::private::RawValue":true}"#;
    let old = parse::<Borrowed<'_>>(text, true).unwrap();
    let actual = Borrowed::parse_json(text).unwrap();
    assert_eq!(old, actual);
    assert!(std::ptr::eq(old.name.as_ptr(), actual.name.as_ptr()));
    assert!(std::ptr::eq(
        old.raw.0.get().as_ptr(),
        actual.raw.0.get().as_ptr()
    ));
    assert!(std::ptr::eq(
        actual.name.as_ptr(),
        text[text.find("final").unwrap()..].as_ptr()
    ));
    assert!(matches!(actual.cow, Cow::Borrowed("plain")));
    assert!(std::ptr::eq(
        actual.raw.0.get().as_ptr(),
        text[text.find("{ \"n\"").unwrap()..].as_ptr()
    ));
    assert_eq!(actual.integer, u128::MAX);
    assert_eq!(
        actual.raw.0.get(),
        r#"{ "n":1e+100, "big":340282366920938463463374607431768211455 }"#
    );
    parity::<Borrowed<'_>>(&text.replace("\"plain\"", "\"esca\\nped\""));
    parity::<Borrowed<'_>>(&text.replace(" final ", " esca\\nped "));
}

#[derive(Debug, PartialEq, Model)]
struct Empty {}
#[derive(Debug, PartialEq, Model)]
struct Generic<T> {
    value: T,
}
#[derive(Debug, PartialEq, Model)]
struct BorrowGeneric<'a, T> {
    label: &'a str,
    value: T,
}
macro_rules! wide {
    ($($field:ident),+ $(,)?) => {
        #[derive(Debug, PartialEq, Model)]
        struct Wide { $(#[model(default, ge = 0)] $field: i64,)+ }
        #[derive(Debug, PartialEq, Model)]
        struct RequiredWide { $(#[model(ge = 0)] $field: i64,)+ }
    };
}
wide!(
    f00, f01, f02, f03, f04, f05, f06, f07, f08, f09, f10, f11, f12, f13, f14, f15, f16, f17, f18,
    f19, f20, f21, f22, f23, f24, f25, f26, f27, f28, f29, f30, f31, f32, f33, f34, f35, f36, f37,
    f38, f39, f40, f41, f42, f43, f44, f45, f46, f47, f48, f49, f50, f51, f52, f53, f54, f55, f56,
    f57, f58, f59, f60, f61, f62, f63, f64, f65, f66, f67, f68, f69, f70, f71, f72, f73, f74, f75,
    f76, f77, f78, f79,
);
#[test]
fn empty_generic_and_wide_models_keep_shape_and_default_rules() {
    for text in ["{}", "[]", "null", "42", r#"{"z":1,"a":2,"z":3}"#] {
        parity::<Empty>(text);
    }
    parity::<Generic<Traced>>(r#"{"value":{"z/~":2,"a":3,"maybe":null}}"#);
    parity::<BorrowGeneric<'_, Borrowed<'_>>>(r#"{"label":"ok","value":{}}"#);
    let values = (0..80)
        .rev()
        .map(|i| format!("\"f{i:02}\":{i}"))
        .collect::<Vec<_>>()
        .join(",");
    parity::<Wide>(&format!("{{{values}}}"));
    parity::<Wide>("{}");
    parity::<Wide>(r#"{"f79":-1,"f00":false,"f40":13,"f40":-3,"unknown":null}"#);
    for text in ["{}", r#"{"f79":-1,"f00":false,"unknown":null}"#] {
        println!("SLOT_ORACLE {}", parity::<RequiredWide>(text));
    }
    let errors = parse::<RequiredWide>("{}", false).unwrap_err();
    assert_eq!(errors.len(), 80);
    assert_eq!(errors.errors()[0].pointer(), "/f00");
    assert_eq!(errors.errors()[79].pointer(), "/f79");
}

#[derive(Debug, PartialEq, Model)]
struct Recursive<'a> {
    label: &'a str,
    wide: Wide,
    next: Option<Box<Recursive<'a>>>,
}
#[test]
fn bounded_recursive_fixed_layouts_preserve_borrows_and_error_order() {
    for depth in [0, 1, 8, 24] {
        for bad in [false, true] {
            let mut text = if bad {
                r#"{"label":"bad\nlabel","wide":{"f79":-1},"z":0}"#.to_owned()
            } else {
                r#"{"label":"leaf","wide":{}}"#.to_owned()
            };
            for _ in 0..depth {
                text = format!(r#"{{"label":"parent","wide":{{"f40":3}},"next":{text}}}"#);
            }
            println!("SLOT_ORACLE {}", parity::<Recursive<'_>>(&text));
            if !bad {
                let mut value = Recursive::parse_json(&text).unwrap();
                for _ in 0..depth {
                    value = *value.next.unwrap();
                }
                assert!(std::ptr::eq(
                    value.label.as_ptr(),
                    text[text.find("leaf").unwrap()..].as_ptr()
                ));
            }
        }
    }
}

#[test]
fn deterministic_full_program_oracle_records() {
    // Run this exact source against main and the candidate with --nocapture;
    // compare SLOT_ORACLE lines byte for byte, not just acceptance or counts.
    let atoms = [
        "null",
        "true",
        "0",
        "-6",
        "13",
        "2.0",
        "2e0",
        "9223372036854775808",
        "\"8\"",
        "\"reject-before\"",
        "[]",
        "{}",
    ];
    let mut count = 0;
    for (i, z) in atoms.iter().enumerate() {
        for (j, a) in atoms.iter().enumerate() {
            for variant in 0..4 {
                let tail = match variant {
                    0 => r#", "maybe":null"#,
                    1 => r#", "z\u002f~":2, "maybe":null, "z":"ignored", "\u007a":0, "a/b~c":true"#,
                    2 => r#", "d":13, "maybe":false"#,
                    _ => r#", "$serde_json::private::Number":"12", "é":[], "\u0000":null"#,
                };
                let text = format!("{{\"a\":{a},\"z/~\":{z}{tail}}}");
                let record = parity::<Traced>(&text);
                println!(
                    "SLOT_ORACLE {}",
                    json!({"case":[i,j,variant],"direct":record})
                );
                let nested =
                    format!("{{\"items/~\":[{text},{{\"z/~\":3,\"a\":4,\"maybe\":null}}]}}");
                let record = parity::<Nested>(&nested);
                println!(
                    "SLOT_ORACLE {}",
                    json!({"case":[i,j,variant],"nested":record})
                );
                count += 2;
            }
        }
    }
    assert_eq!(count, 1152);
}

#[test]
fn owned_object_fallbacks_preserve_full_results_and_prior_context() {
    for text in [
        r#"{"z/~":2,"a":3,"maybe":null}"#,
        r#"{"z/~":13,"a":false,"unknown":true}"#,
        "{}",
        "[]",
        "null",
    ] {
        let expected = parity::<Traced>(text);
        take_events();
        let actual = Traced::parse_value(serde_json::from_str(text).unwrap());
        let events = take_events();
        assert_eq!(snapshot(&actual), expected["result"]);
        assert_eq!(json!(events), expected["events"]);
        for map in [true, false] {
            let raw = serde_json::from_str::<&RawValue>(text).unwrap();
            let mut ctx = Context::new();
            ctx.push(ValidationError::custom("existing"));
            let value = ctx.at("outer/~", |ctx| {
                let input = if map {
                    frozen_map(raw, ctx)?
                } else {
                    Input::Borrowed(raw)
                };
                <Traced as JsonType>::decode_input(input, ctx, false)
            });
            let errors = ctx.finish(value).unwrap_err();
            assert_eq!(errors.errors()[0].message, "existing");
            println!(
                "SLOT_ORACLE {}",
                json!({"prior":text,"map":map,"errors":serde_json::to_string(&errors).unwrap(),"events":take_events()})
            );
        }
    }
}
