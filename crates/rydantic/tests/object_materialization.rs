use rydantic::{BorrowModel, BorrowType, Context, Input, Model, ValidationErrors};
use serde_json::value::RawValue;
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Model)]
struct Child<'a> {
    #[model(validation_alias = "wire/name", min_length = 2)]
    name: &'a str,
}
#[derive(Debug, PartialEq, Model)]
struct Outer<'a> {
    #[model(flatten)]
    child: Child<'a>,
    count: i64,
    items: Vec<Child<'a>>,
}

fn previous<'a>(input: &'a str) -> Result<Outer<'a>, ValidationErrors> {
    let old: BTreeMap<String, &'a RawValue> = serde_json::from_str(input).unwrap();
    let mut ctx = Context::new();
    let value = Outer::decode(
        Input::Object(
            old.into_iter()
                .map(|(k, v)| (k, Input::Borrowed(v)))
                .collect(),
        ),
        &mut ctx,
        false,
    );
    ctx.finish(value)
}

#[test]
fn aliases_flatten_nested_errors_and_duplicates_match_previous_object_path() {
    for text in [
        r#"{"wire/name":"borrowed","count":2,"items":[{"wire/name":"child"}]}"#,
        r#"{"wire/name":"bad","wire/name":"borrowed","count":1,"count":2,"items":[]}"#,
        r#"{"wire/name":"x","count":"bad","items":[{"wire/name":1},{"wire/name":"x"}]}"#,
        r#"{"wire/name":"escaped\nname","count":1.5,"items":[{}]}"#,
        r#"{"wire/name":"ok","count":9223372036854775808,"items":null}"#,
        r#"{"wire/name":"ok","count":2.0,"items":[],"$serde_json::private::Number":"42"}"#,
    ] {
        match (previous(text), Outer::parse_json(text)) {
            (Ok(old), Ok(new)) => {
                assert_eq!(old, new);
                assert_eq!(old.child.name.as_ptr(), new.child.name.as_ptr());
                assert_eq!(old.dump().unwrap(), new.dump().unwrap());
            }
            (Err(old), Err(new)) => assert_eq!(
                serde_json::to_value(old).unwrap(),
                serde_json::to_value(new).unwrap()
            ),
            _ => panic!("acceptance differs: {text}"),
        }
    }
}
