use rydantic::{
    json, BorrowModel, BorrowType, BorrowValidated, Context, Model, Stage, ValidationError,
    ValidationErrors, Value,
};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Model)]
struct Leaf<'a> {
    #[model(
        validation_alias = "source",
        serialization_alias = "text",
        trim,
        min_length = 2
    )]
    text: &'a str,
}

fn paths(errors: &ValidationErrors) -> Vec<String> {
    let mut paths = errors
        .errors()
        .iter()
        .map(ValidationError::pointer)
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
fn borrowed_validated_entry_points_preserve_borrows_and_reject_invalid_values() {
    let input = String::from(r#"{"source":" useful "}"#);
    let parsed = Leaf::parse_slice(input.as_bytes()).unwrap();
    assert_eq!(parsed.text, "useful");
    let start = input.find("useful").unwrap();
    assert!(std::ptr::eq(parsed.text.as_ptr(), input[start..].as_ptr()));
    assert_eq!(parsed.dump_json().unwrap(), r#"{"text":"useful"}"#);

    let checked = Leaf::parse_validated_json(&input).unwrap();
    assert_eq!(checked.get(), &parsed);
    assert_eq!(checked.text, "useful");
    assert_eq!(checked.clone().into_inner(), parsed);
    assert_eq!(
        serde_json::to_value(&checked).unwrap(),
        json!({"text": "useful"})
    );
    let decoded: BorrowValidated<'_, Leaf<'_>> = serde_json::from_str(&input).unwrap();
    assert!(std::ptr::eq(decoded.text.as_ptr(), parsed.text.as_ptr()));
    assert_eq!(
        BorrowValidated::new(parsed).unwrap().into_inner().text,
        "useful"
    );

    let errors = BorrowValidated::new(Leaf { text: "x" }).unwrap_err();
    assert_eq!(errors.errors()[0].code, "too_short");
    assert_eq!(errors.errors()[0].pointer(), "/source");
    let error =
        serde_json::from_str::<BorrowValidated<'_, Leaf<'_>>>(r#"{"source":"x"}"#).unwrap_err();
    assert!(error.to_string().contains("too_short"));
    assert!(error.to_string().contains("/source"));
}

#[derive(Debug, Model)]
struct Collections<'a> {
    maybe: Option<Box<Leaf<'a>>>,
    #[model(min_length = 1)]
    items: Vec<Leaf<'a>>,
    #[model(min_length = 1)]
    ordered: BTreeMap<String, Leaf<'a>>,
    #[model(min_length = 1)]
    unordered: HashMap<String, Leaf<'a>>,
    fixed: [Leaf<'a>; 2],
}

const COLLECTIONS: &str = r#"{
    "maybe":{"source":"optional"},
    "items":[{"source":"vector"}],
    "ordered":{"a/b~c":{"source":"ordered"}},
    "unordered":{"0":{"source":"unordered"}},
    "fixed":[{"source":"first"},{"source":"second"}]
}"#;

#[test]
fn borrowed_containers_keep_input_aliases_distinct_from_output_aliases() {
    let value = Collections::parse_slice(COLLECTIONS.as_bytes()).unwrap();
    assert_eq!(value.maybe.as_ref().unwrap().text, "optional");
    assert_eq!(value.items[0].text, "vector");
    assert_eq!(value.ordered["a/b~c"].text, "ordered");
    assert_eq!(value.unordered["0"].text, "unordered");
    assert_eq!(value.fixed[1].text, "second");

    let mut ctx = Context::new();
    let input = BorrowType::encode_input(&value, &mut ctx);
    assert!(ctx.result().is_ok());
    assert_eq!(input, serde_json::from_str::<Value>(COLLECTIONS).unwrap());
    let output = json!({
        "maybe":{"text":"optional"},
        "items":[{"text":"vector"}],
        "ordered":{"a/b~c":{"text":"ordered"}},
        "unordered":{"0":{"text":"unordered"}},
        "fixed":[{"text":"first"},{"text":"second"}]
    });
    assert_eq!(value.dump().unwrap(), output);
    assert_eq!(
        serde_json::from_str::<Value>(&value.dump_json().unwrap()).unwrap(),
        output
    );
    assert_eq!(serde_json::to_value(&value).unwrap(), output);
}

#[test]
fn borrowed_container_decoding_collects_all_nested_failures_with_wire_paths() {
    let errors = Collections::parse_json(
        r#"{
        "maybe":{"source":1},
        "items":[{"source":true},{"source":"x"}],
        "ordered":{"a/b~c":{"source":false}},
        "unordered":{"0":{"source":null}},
        "fixed":[{"source":"ok"},{"source":7}]
    }"#,
    )
    .unwrap_err();
    assert_eq!(
        paths(&errors),
        [
            "/fixed/1/source",
            "/items/0/source",
            "/items/1/source",
            "/maybe/source",
            "/ordered/a~1b~0c/source",
            "/unordered/0/source",
        ]
    );
    for error in errors.errors() {
        if error.pointer() == "/items/1/source" {
            assert_eq!(error.code, "too_short");
            assert_eq!(error.stage, Stage::Validate);
        } else {
            assert_eq!(error.code, "borrowed_string");
            assert_eq!(error.stage, Stage::Parse);
        }
    }
}

#[test]
fn borrowed_containers_revalidate_mutated_children_before_serialization() {
    let mut value = Collections::parse_json(COLLECTIONS).unwrap();
    value.maybe.as_mut().unwrap().text = "x";
    value.items[0].text = "x";
    value.ordered.get_mut("a/b~c").unwrap().text = "x";
    value.unordered.get_mut("0").unwrap().text = "x";
    value.fixed[1].text = "x";
    let expected = [
        "/fixed/1/source",
        "/items/0/source",
        "/maybe/source",
        "/ordered/a~1b~0c/source",
        "/unordered/0/source",
    ];
    for errors in [
        value.validate().unwrap_err(),
        value.dump().unwrap_err(),
        value.dump_json().unwrap_err(),
    ] {
        assert_eq!(paths(&errors), expected);
        assert!(errors
            .errors()
            .iter()
            .all(|error| error.code == "too_short" && error.stage == Stage::Validate));
    }
    assert!(serde_json::to_value(&value).is_err());
    assert_eq!(paths(&BorrowValidated::new(value).unwrap_err()), expected);
}

#[test]
fn borrowed_fixed_arrays_and_container_shapes_reject_invalid_inputs() {
    #[derive(Debug, Model)]
    struct Fixed<'a> {
        values: [&'a str; 2],
        empty: [&'a str; 0],
    }
    let parsed = Fixed::parse_json(r#"{"values":["first","second"],"empty":[]}"#).unwrap();
    assert!(parsed.empty.is_empty());
    assert_eq!(
        parsed.dump().unwrap(),
        json!({"values":["first","second"],"empty":[]})
    );
    for (input, length) in [
        (r#"{"values":[],"empty":[]}"#, 0),
        (r#"{"values":["one"],"empty":[]}"#, 1),
        (r#"{"values":["one","two","three"],"empty":[]}"#, 3),
    ] {
        let errors = Fixed::parse_json(input).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.errors()[0].code, "array_length");
        assert_eq!(errors.errors()[0].pointer(), "/values");
        assert_eq!(errors.errors()[0].params["actual"], length);
    }

    let errors = Collections::parse_json(
        r#"{
        "maybe":[], "items":{}, "ordered":[], "unordered":false, "fixed":{}
    }"#,
    )
    .unwrap_err();
    assert_eq!(
        paths(&errors),
        ["/fixed", "/items", "/maybe", "/ordered", "/unordered"]
    );
    assert!(errors
        .errors()
        .iter()
        .all(|error| error.code == "type_error" && error.stage == Stage::Parse));
}

#[test]
fn preparing_borrowed_container_defaults_normalizes_children_and_keeps_optional_absence() {
    let value = Collections {
        maybe: Some(Box::new(Leaf { text: " optional " })),
        items: vec![Leaf { text: " vector " }],
        ordered: BTreeMap::from([("key".into(), Leaf { text: " ordered " })]),
        unordered: HashMap::from([(
            "key".into(),
            Leaf {
                text: " unordered ",
            },
        )]),
        fixed: [Leaf { text: " first " }, Leaf { text: " second " }],
    };
    let mut ctx = Context::new();
    let prepared = BorrowType::prepare_default(value, &mut ctx);
    let mut prepared = ctx.finish(prepared).unwrap();
    assert_eq!(prepared.maybe.as_ref().unwrap().text, "optional");
    assert_eq!(prepared.items[0].text, "vector");
    assert_eq!(prepared.ordered["key"].text, "ordered");
    assert_eq!(prepared.unordered["key"].text, "unordered");
    assert_eq!(
        [prepared.fixed[0].text, prepared.fixed[1].text],
        ["first", "second"]
    );
    prepared.maybe = None;
    let mut ctx = Context::new();
    let prepared = BorrowType::prepare_default(prepared, &mut ctx);
    let prepared = ctx.finish(prepared).unwrap();
    assert_eq!(prepared.dump().unwrap()["maybe"], Value::Null);
    let mut ctx = Context::new();
    assert_eq!(
        BorrowType::encode_input(&prepared, &mut ctx)["maybe"],
        Value::Null
    );
    assert!(ctx.result().is_ok());

    #[derive(Debug, Model)]
    struct Optional<'a> {
        value: Box<Option<&'a str>>,
    }
    for input in ["{}", r#"{"value":null}"#] {
        assert_eq!(*Optional::parse_json(input).unwrap().value, None);
    }
}

#[test]
fn borrowed_case_conversion_reports_all_unrepresentable_default_values() {
    #[derive(Debug, Model)]
    struct Lowercase<'a> {
        #[model(trim, lowercase)]
        text: &'a str,
    }
    assert_eq!(
        Lowercase::parse_json(r#"{"text":" quiet "}"#).unwrap().text,
        "quiet"
    );
    let errors = Lowercase::parse_json(r#"{"text":" LOUD "}"#).unwrap_err();
    assert_eq!(errors.errors()[0].code, "borrowed_transform");
    assert_eq!(errors.errors()[0].pointer(), "/text");

    let values = HashMap::from([
        (
            "first".to_owned(),
            [Lowercase { text: " LOUD " }, Lowercase { text: " MORE " }],
        ),
        (
            "second".to_owned(),
            [Lowercase { text: " quiet " }, Lowercase { text: " AGAIN " }],
        ),
    ]);
    let mut ctx = Context::new();
    let prepared = BorrowType::prepare_default(values, &mut ctx);
    assert!(prepared.is_none());
    let errors = ctx.result().unwrap_err();
    assert_eq!(
        paths(&errors),
        ["/first/0/text", "/first/1/text", "/second/1/text"]
    );
    assert!(errors
        .errors()
        .iter()
        .all(|error| error.code == "borrowed_transform"));
}

#[tokio::test]
async fn borrowed_async_containers_enforce_sync_guards_and_recheck_before_dumping() {
    async fn available(value: &&str) -> Result<(), ValidationError> {
        if *value == "reserved" {
            Err(ValidationError::new("reserved", "name is unavailable"))
        } else {
            Ok(())
        }
    }
    #[derive(Debug, Model)]
    struct AsyncLeaf<'a> {
        #[model(async_validate = available)]
        name: &'a str,
    }
    #[derive(Debug, Model)]
    struct AsyncCollections<'a> {
        maybe: Option<Box<AsyncLeaf<'a>>>,
        entries: HashMap<String, AsyncLeaf<'a>>,
        fixed: [AsyncLeaf<'a>; 1],
    }
    let input = r#"{"maybe":{"name":"available"},"entries":{"key":{"name":"available"}},"fixed":[{"name":"available"}]}"#;
    assert_eq!(
        AsyncCollections::parse_slice(input.as_bytes())
            .unwrap_err()
            .errors()[0]
            .code,
        "async_validation_required"
    );
    let mut value = AsyncCollections::parse_json_async(input).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&value.dump_json_async().await.unwrap()).unwrap(),
        serde_json::from_str::<Value>(input).unwrap()
    );
    for errors in [
        value.validate().unwrap_err(),
        value.dump().unwrap_err(),
        value.dump_json().unwrap_err(),
    ] {
        assert_eq!(errors.errors()[0].code, "async_validation_required");
    }
    assert!(serde_json::to_value(&value).is_err());

    value.maybe.as_mut().unwrap().name = "reserved";
    value.entries.get_mut("key").unwrap().name = "reserved";
    value.fixed[0].name = "reserved";
    for errors in [
        value.validate_async().await.unwrap_err(),
        value.dump_async().await.unwrap_err(),
        value.dump_json_async().await.unwrap_err(),
    ] {
        assert_eq!(
            paths(&errors),
            ["/entries/key/name", "/fixed/0/name", "/maybe/name"]
        );
        assert!(errors.errors().iter().all(|error| error.code == "reserved"));
    }
}
