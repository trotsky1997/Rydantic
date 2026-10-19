use rydantic::{json, BorrowModel, Model, SchemaMode, Value};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
};

#[derive(Debug, PartialEq, Model)]
struct Borrowed<'a> {
    #[model(trim, min_length = 2)]
    name: &'a str,
    #[model(trim, lowercase)]
    note: Cow<'a, str>,
    tags: Vec<&'a str>,
    maybe: Option<Cow<'a, str>>,
}
#[test]
fn borrowed_strings_keep_input_addresses_and_cow_allocates_when_needed() {
    let input = String::from(r#"{"name":" Alice ","note":" hello ","tags":["one","two"]}"#);
    let user = Borrowed::parse_json(&input).unwrap();
    assert!(
        user.name.as_ptr() >= input.as_ptr()
            && user.name.as_ptr() < (input.as_ptr() as usize + input.len()) as *const u8
    );
    assert_eq!(user.name, "Alice");
    assert!(matches!(user.note, Cow::Borrowed("hello")));
    assert_eq!(user.tags, ["one", "two"]);
    let user: Borrowed<'_> =
        serde_json::from_str(r#"{"name":"ok","note":"HE\nLLO","tags":[]}"#).unwrap();
    assert!(matches!(user.note, Cow::Owned(_)));
    assert_eq!(user.note, "he\nllo");
    let dump = user.dump().unwrap();
    assert_eq!(dump["name"], "ok");
    assert!(
        jsonschema::validator_for(&Borrowed::json_schema(SchemaMode::Output))
            .unwrap()
            .is_valid(&dump)
    );
    assert!(Borrowed::parse_json(r#"{"name":"a\nb","note":"x","tags":[]}"#).is_err());
}
#[derive(Debug, Model)]
struct BorrowDefaults<'a> {
    #[model(default = " guest ", trim)]
    name: &'a str,
    #[model(default = " LOUD ", trim, lowercase)]
    note: Cow<'a, str>,
}
#[test]
fn borrowed_defaults_are_checked_and_normalized() {
    let x = BorrowDefaults::parse_slice(b"{}").unwrap();
    assert_eq!(x.name, "guest");
    assert_eq!(x.note, "loud");
}
#[derive(Debug, PartialEq, Model)]
struct Newtype(#[model(trim, min_length = 2)] String);
#[derive(Debug, PartialEq, Model)]
struct Point(#[model(ge = 0)] i32, String);
#[derive(Debug, PartialEq, Model)]
struct BorrowTuple<'a>(&'a str, Cow<'a, str>);
#[derive(Debug, PartialEq, Model)]
struct Unit;
#[derive(Debug, PartialEq, Model)]
struct Empty();
#[test]
fn tuple_and_newtype_models_round_trip() {
    assert_eq!(
        Newtype::parse_json(r#"" hi ""#).unwrap(),
        Newtype("hi".into())
    );
    assert_eq!(Newtype("hi".into()).dump().unwrap(), json!("hi"));
    assert_eq!(
        Point::parse_json(r#"[2,"a"]"#).unwrap(),
        Point(2, "a".into())
    );
    assert_eq!(Point::parse_json(r#"[-1,4]"#).unwrap_err().len(), 2);
    assert_eq!(
        Point::parse_json(r#"[-1,"a"]"#).unwrap_err().errors()[0].pointer(),
        "/0"
    );
    assert!(Point::parse_json(r#"[2,"a",3]"#).is_err());
    let p = BorrowTuple::parse_json(r#"["a","b\nc"]"#).unwrap();
    assert_eq!(p.0, "a");
    assert_eq!(p.1, "b\nc");
    assert_eq!(
        Unit::parse_json("null").unwrap().dump().unwrap(),
        Value::Null
    );
    assert_eq!(Empty::parse_json("[]").unwrap().dump().unwrap(), json!([]));
    for value in [json!([2, "a"]), json!([-1, "a"]), json!([2]), json!(2)] {
        assert_eq!(
            Point::parse_value(value.clone()).is_ok(),
            jsonschema::validator_for(&Point::json_schema(SchemaMode::Input))
                .unwrap()
                .is_valid(&value)
        );
    }
}
#[derive(Debug, PartialEq, Model)]
#[model(untagged)]
enum Flexible {
    Count(#[model(ge = 1)] u32),
    Name {
        #[model(min_length = 2)]
        name: String,
    },
    Pair(String, u8),
    Null,
}
#[derive(Debug, PartialEq, Model)]
#[model(untagged)]
enum BorrowUnion<'a> {
    Name(&'a str),
    Object { value: Cow<'a, str> },
}
#[test]
fn untagged_trials_respect_validation_and_return_branch_errors() {
    assert_eq!(Flexible::parse_json("5").unwrap(), Flexible::Count(5));
    assert_eq!(Flexible::parse_json("null").unwrap(), Flexible::Null);
    assert_eq!(
        Flexible::parse_json(r#"["x",2]"#).unwrap(),
        Flexible::Pair("x".into(), 2)
    );
    assert!(matches!(
        BorrowUnion::parse_json(r#""raw""#).unwrap(),
        BorrowUnion::Name("raw")
    ));
    assert!(matches!(
        BorrowUnion::parse_json(r#"{"value":"a\nb"}"#).unwrap(),
        BorrowUnion::Object { .. }
    ));
    let errors = Flexible::parse_json("0").unwrap_err();
    assert_eq!(errors.errors()[0].code, "union_no_match");
    assert_eq!(
        errors.errors()[0].params["branches"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let schema = jsonschema::validator_for(&Flexible::json_schema(SchemaMode::Input)).unwrap();
    for v in [
        json!(0),
        json!(3),
        json!("a"),
        json!({"name":"ab"}),
        json!({"name":"a"}),
        json!(["a", 3]),
        json!(null),
        json!({"extra":1}),
    ] {
        assert_eq!(
            Flexible::parse_value(v.clone()).is_ok(),
            schema.is_valid(&v),
            "{v}"
        );
    }
}
#[derive(Debug, PartialEq, Model)]
struct Address {
    #[model(
        validation_alias = "town",
        serialization_alias = "city",
        min_length = 2
    )]
    city: String,
    zip: Option<String>,
}
#[derive(Debug, PartialEq, Model)]
#[model(extra = "allow")]
struct Flattened {
    #[model(extra)]
    extra: BTreeMap<String, Value>,
    id: u32,
    #[model(flatten)]
    address: Address,
}
#[test]
fn flatten_preserves_aliases_extra_and_wire_error_paths() {
    let x = Flattened::parse_json(r#"{"id":2,"town":"Paris","x":[1]}"#).unwrap();
    assert_eq!(x.address.city, "Paris");
    assert_eq!(x.extra["x"], json!([1]));
    let dump = x.dump().unwrap();
    assert_eq!(dump, json!({"id":2,"city":"Paris","zip":null,"x":[1]}));
    let original = json!({"id":2,"town":"Paris","x":[1]});
    for mode in [SchemaMode::Input, SchemaMode::Output] {
        let schema = Flattened::json_schema(mode);
        assert!(jsonschema::validator_for(&schema).unwrap().is_valid(
            if mode == SchemaMode::Input {
                &original
            } else {
                &dump
            }
        ));
    }
    assert_eq!(
        Flattened::parse_json(r#"{"id":2,"town":"x"}"#)
            .unwrap_err()
            .errors()[0]
            .pointer(),
        "/town"
    );
    let mut x = x;
    x.extra.insert("id".into(), json!(9));
    assert_eq!(x.dump().unwrap_err().errors()[0].code, "field_collision");
}
#[derive(Debug, Model)]
struct MaybeAddress {
    #[model(flatten)]
    address: Option<Address>,
    id: u32,
}
#[derive(Debug, Model)]
struct MapFlatten {
    #[model(flatten)]
    values: HashMap<String, u32>,
    id: u32,
}
#[derive(Debug, Model)]
struct Collision {
    town: u8,
    #[model(flatten)]
    address: Address,
}
#[test]
fn optional_flatten_uses_conditional_required_and_typed_maps() {
    assert!(MaybeAddress::parse_json(r#"{"id":1}"#)
        .unwrap()
        .address
        .is_none());
    assert!(MaybeAddress::parse_json(r#"{"id":1,"zip":"100"}"#).is_err());
    let schema = jsonschema::validator_for(&MaybeAddress::json_schema(SchemaMode::Input)).unwrap();
    for v in [
        json!({"id":1}),
        json!({"id":1,"zip":"100"}),
        json!({"id":1,"town":"Paris"}),
        json!({"id":1,"town":"Paris","zip":"100"}),
    ] {
        assert_eq!(
            MaybeAddress::parse_value(v.clone()).is_ok(),
            schema.is_valid(&v),
            "{v}"
        );
    }
    let x = MapFlatten::parse_json(r#"{"id":1,"x":3}"#).unwrap();
    assert_eq!(x.values["x"], 3);
    assert_eq!(x.dump().unwrap(), json!({"id":1,"x":3}));
    assert!(MapFlatten::parse_json(r#"{"id":1,"x":"bad"}"#).is_err());
    assert_eq!(
        Collision::parse_json(r#"{"town":1,"id":2}"#)
            .unwrap_err()
            .errors()[0]
            .code,
        "field_collision"
    );
}
#[derive(Debug, Model)]
struct BorrowAddress<'a> {
    city: &'a str,
}
#[derive(Debug, Model)]
#[model(extra = "allow")]
struct BorrowFlatten<'a> {
    #[model(flatten)]
    address: BorrowAddress<'a>,
    #[model(extra)]
    extra: BTreeMap<String, Cow<'a, str>>,
}
#[test]
fn borrowed_flatten_and_extra_keep_original_buffer() {
    let input = String::from(r#"{"city":"Paris","a":"hello","b":"esc\nape"}"#);
    let x = BorrowFlatten::parse_json(&input).unwrap();
    assert_eq!(x.address.city, "Paris");
    assert!(matches!(x.extra["a"], Cow::Borrowed(_)));
    assert!(matches!(x.extra["b"], Cow::Owned(_)));
    assert_eq!(x.dump().unwrap()["city"], "Paris");
}
#[derive(Debug, PartialEq, Model)]
struct Wide {
    signed: i128,
    unsigned: u128,
}
#[test]
fn exact_128_bit_integers_and_extreme_exponents() {
    let input = format!(r#"{{"signed":{},"unsigned":{}}}"#, i128::MIN, u128::MAX);
    let x = Wide::parse_json(&input).unwrap();
    assert_eq!(x.signed, i128::MIN);
    assert_eq!(x.unsigned, u128::MAX);
    assert_eq!(x.dump_json().unwrap(), input);
    assert_eq!(
        Wide::parse_json(r#"{"signed":100.0,"unsigned":1e3}"#).unwrap(),
        Wide {
            signed: 100,
            unsigned: 1000
        }
    );
    for bad in [
        "340282366920938463463374607431768211456",
        "1.00000000000000000000001",
        "1e9999999999999999999999999999999999999",
    ] {
        let input = format!(r#"{{"signed":0,"unsigned":{bad}}}"#);
        assert!(Wide::parse_json(&input).is_err());
    }
}

#[test]
fn empty_and_generic_tuple_schemas_are_valid_uris_and_valid_schemas() {
    #[derive(Debug, Model)]
    struct Generic<T>(T);
    let schema = Generic::<Vec<String>>::json_schema(SchemaMode::Input);
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&json!(["a", "b"])));
    assert!(!validator.is_valid(&json!([1])));
    let empty = jsonschema::validator_for(&Empty::json_schema(SchemaMode::Input)).unwrap();
    assert!(empty.is_valid(&json!([])));
    assert!(!empty.is_valid(&json!([1])));
}
#[test]
fn optional_flatten_output_matches_none_and_some() {
    for input in [json!({"id":1}), json!({"id":1,"town":"Paris"})] {
        let x = MaybeAddress::parse_value(input).unwrap();
        let dump = x.dump().unwrap();
        assert!(
            jsonschema::validator_for(&MaybeAddress::json_schema(SchemaMode::Output))
                .unwrap()
                .is_valid(&dump),
            "{dump}"
        );
    }
}
#[test]
fn defaults_normalize_borrowed_models_recursively_in_collections() {
    #[derive(Debug, Model)]
    struct Inner<'a> {
        #[model(trim, lowercase)]
        name: Cow<'a, str>,
        #[model(trim)]
        raw: &'a str,
    }
    fn defaults() -> Vec<Inner<'static>> {
        vec![Inner {
            name: Cow::Borrowed(" LOUD "),
            raw: " raw ",
        }]
    }
    #[derive(Debug, Model)]
    struct Outer<'a> {
        #[model(default_factory=defaults)]
        items: Vec<Inner<'a>>,
    }
    let x = Outer::parse_json("{}").unwrap();
    assert_eq!(x.items[0].name, "loud");
    assert_eq!(x.items[0].raw, "raw");
}
#[test]
fn flatten_defaults_are_applied_to_an_empty_subset() {
    fn address() -> Address {
        Address {
            city: "Paris".into(),
            zip: None,
        }
    }
    #[derive(Debug, Model)]
    struct WithDefault {
        #[model(flatten,default_factory=address)]
        address: Address,
        id: u32,
    }
    let x = WithDefault::parse_json(r#"{"id":1}"#).unwrap();
    assert_eq!(x.address.city, "Paris");
    assert!(WithDefault::parse_json(r#"{"id":1,"zip":"x"}"#).is_err());
}
#[test]
fn inner_ignore_extra_does_not_relax_the_outer_extra_policy() {
    #[derive(Debug, Model)]
    #[model(extra = "ignore")]
    struct Inner {
        value: u32,
    }
    #[derive(Debug, Model)]
    struct Outer {
        #[model(flatten)]
        inner: Inner,
    }
    let bad = json!({"value":1,"unknown":2});
    assert!(Outer::parse_value(bad.clone()).is_err());
    assert!(
        !jsonschema::validator_for(&Outer::json_schema(SchemaMode::Input))
            .unwrap()
            .is_valid(&bad)
    );
}
#[test]
fn manually_inserted_extra_keys_are_checked_by_validate() {
    let mut x = Flattened::parse_json(r#"{"id":2,"town":"Paris"}"#).unwrap();
    x.extra.insert("id".into(), json!(9));
    assert!(x
        .validate()
        .unwrap_err()
        .errors()
        .iter()
        .any(|e| e.code == "field_collision"));
}
#[test]
fn multiple_catchall_flatten_fields_report_ambiguity() {
    #[derive(Debug, Model)]
    struct Ambiguous {
        #[model(flatten)]
        a: BTreeMap<String, Value>,
        #[model(flatten)]
        b: BTreeMap<String, Value>,
    }
    assert_eq!(
        Ambiguous::parse_json("{}").unwrap_err().errors()[0].code,
        "flatten_ambiguous"
    );
}
#[test]
fn borrowed_models_support_multiple_lifetimes_generics_and_recursion() {
    #[derive(Debug, Model)]
    struct Pair<'a, 'b, T> {
        a: &'a str,
        b: &'b str,
        value: T,
    }
    let x = Pair::<u8>::parse_json(r#"{"a":"one","b":"two","value":3}"#).unwrap();
    assert_eq!(x.a, "one");
    assert_eq!(x.b, "two");
    assert_eq!(x.dump().unwrap()["value"], 3);
    #[derive(Debug, Model)]
    struct Node<'a> {
        name: &'a str,
        child: Option<Box<Node<'a>>>,
    }
    let x = Node::parse_json(r#"{"name":"one","child":{"name":"two"}}"#).unwrap();
    assert_eq!(x.child.as_ref().unwrap().name, "two");
    assert!(
        jsonschema::validator_for(&Node::json_schema(SchemaMode::Output))
            .unwrap()
            .is_valid(&x.dump().unwrap())
    );
}

#[test]
fn extra_keys_cannot_shadow_serialization_aliases() {
    let err = Flattened::parse_json(r#"{"id":1,"town":"Paris","city":7}"#).unwrap_err();
    assert!(err.errors().iter().any(|e| e.code == "field_collision"));
    #[derive(Debug, Model)]
    #[model(extra = "allow")]
    struct OptionalExtra {
        #[model(flatten)]
        address: Option<Address>,
        #[model(extra)]
        extra: BTreeMap<String, Value>,
    }
    assert!(OptionalExtra::parse_json(r#"{"city":7}"#).is_err());
}
#[test]
fn flatten_defaults_and_invalid_layouts_export_matching_schemas() {
    fn address() -> Address {
        Address {
            city: "Paris".into(),
            zip: None,
        }
    }
    #[derive(Debug, Model)]
    struct WithDefault {
        #[model(flatten,default_factory=address)]
        address: Address,
        id: u32,
    }
    let validator =
        jsonschema::validator_for(&WithDefault::json_schema(SchemaMode::Input)).unwrap();
    assert!(validator.is_valid(&json!({"id":1})));
    assert!(!validator.is_valid(&json!({"id":1,"zip":"partial"})));
    #[derive(Debug, Model)]
    struct Ambiguous {
        #[model(flatten)]
        a: BTreeMap<String, Value>,
        #[model(flatten)]
        b: BTreeMap<String, Value>,
    }
    assert!(
        !jsonschema::validator_for(&Ambiguous::json_schema(SchemaMode::Input))
            .unwrap()
            .is_valid(&json!({}))
    );
    assert!(
        !jsonschema::validator_for(&Collision::json_schema(SchemaMode::Input))
            .unwrap()
            .is_valid(&json!({"town":1}))
    );
}

#[test]
fn borrowed_validated_wrapper_supports_serde() {
    use rydantic::BorrowValidated;
    let checked: BorrowValidated<'_, BorrowDefaults<'_>> = serde_json::from_str("{}").unwrap();
    assert_eq!(checked.name, "guest");
    assert_eq!(serde_json::to_value(&checked).unwrap()["note"], "loud");
}
