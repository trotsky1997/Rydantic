use rydantic::{AsyncCheck, Context, Input, JsonType, Model, RawValue, SchemaContext, Value};
use std::collections::BTreeMap;

// This type fails immediately if parsing or collection constraints round-trip through a DOM.
#[derive(Debug, PartialEq)]
struct RawOnly(String);
impl AsyncCheck for RawOnly {}
impl JsonType for RawOnly {
    fn decode(_: Value, _: &mut Context, _: bool) -> Option<Self> {
        panic!("unexpected Value decoding")
    }
    fn decode_input(input: Input<'_>, _: &mut Context, _: bool) -> Option<Self> {
        let Input::Borrowed(raw) = input else {
            panic!("input subtree lost its raw slice")
        };
        Some(Self(serde_json::from_str(raw.get()).unwrap()))
    }
    fn prepare_default(self, _: &mut Context) -> Option<Self> {
        Some(self)
    }
    fn encode(&self, _: &mut Context) -> Value {
        panic!("unexpected encode while parsing")
    }
    fn schema(_: &mut SchemaContext, _: bool) -> Value {
        rydantic::json!({"type":"string"})
    }
}
#[derive(Debug, PartialEq, Model)]
struct Child {
    value: RawOnly,
}
#[derive(Debug, PartialEq, Model)]
struct Root {
    child: Box<Child>,
    #[model(min_length = 1)]
    items: Vec<RawOnly>,
    #[model(min_length = 1)]
    map: BTreeMap<String, RawOnly>,
    #[model(min_length = 1)]
    array: [RawOnly; 1],
    optional: Option<RawOnly>,
    #[model(default_factory = default_child)]
    defaulted: Child,
}
fn default_child() -> Child {
    Child {
        value: RawOnly("default".into()),
    }
}
const INPUT: &str = r#"{"child":{"value":"nested"},"items":["item"],"map":{"key":"mapped"},"array":["fixed"],"optional":"yes"}"#;
#[test]
fn owned_entry_points_and_constraints_preserve_raw_subtrees() {
    let parsed = Root::parse_json(INPUT).unwrap();
    assert_eq!(parsed.child.value.0, "nested");
    assert_eq!(parsed.defaulted.value.0, "default");
    assert_eq!(Root::parse_slice(INPUT.as_bytes()).unwrap(), parsed);
    assert_eq!(Root::parse_reader(INPUT.as_bytes()).unwrap(), parsed);
    assert_eq!(serde_json::from_str::<Root>(INPUT).unwrap(), parsed);
    assert_eq!(
        serde_json::from_reader::<_, Root>(INPUT.as_bytes()).unwrap(),
        parsed
    );
    assert_eq!(
        rydantic::stream::ndjson::<_, Root>(std::io::Cursor::new(INPUT))
            .next()
            .unwrap()
            .unwrap(),
        parsed
    );
}
#[tokio::test]
async fn async_parsing_preserves_raw_subtrees() {
    assert_eq!(
        Root::parse_json_async(INPUT).await.unwrap(),
        Root::parse_json(INPUT).unwrap()
    );
}
#[test]
fn opaque_raw_values_preserve_large_payloads_and_original_numeric_text() {
    #[derive(Debug, Model)]
    #[model(extra = "allow")]
    struct Envelope {
        payload: Box<RawValue>,
        #[model(extra)]
        extra: BTreeMap<String, Box<RawValue>>,
    }
    let data = r#"{"payload":{ "n":1e+100, "items":[1,2,3] },"unknown":[ {"x":true} ]}"#;
    let parsed = Envelope::parse_json(data).unwrap();
    assert_eq!(parsed.payload.get(), r#"{ "n":1e+100, "items":[1,2,3] }"#);
    assert_eq!(parsed.extra["unknown"].get(), r#"[ {"x":true} ]"#);
    assert!(parsed.dump_json().unwrap().contains("1e+100"));
}
