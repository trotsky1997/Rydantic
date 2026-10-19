use rydantic::{BorrowModel, Model};
#[derive(Model)]
struct View<'a> { name: &'a str }
fn escaped() -> View<'static> {
    let input = String::from(r#"{"name":"Alice"}"#);
    View::parse_json(&input).unwrap()
}
fn main() { let _ = escaped(); }
