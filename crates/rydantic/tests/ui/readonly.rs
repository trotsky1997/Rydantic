use rydantic::Model;

#[derive(Model)]
struct User {
    age: u8,
}

fn main() {
    let user = User::parse_validated_json(r#"{"age":25}"#).unwrap();
    user.age = 1;
}
