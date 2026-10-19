use rd::{Model, SchemaMode};

#[derive(Debug, PartialEq, Model)]
struct Message {
    #[model(trim, min_length = 1)]
    text: String,
}

fn main() {
    let message = Message::parse_json(r#"{"text":" hello "}"#).unwrap();
    assert_eq!(message.text, "hello");
    assert!(Message::json_schema(SchemaMode::Output).is_object());
    println!("{}", message.dump_json().unwrap());
}
