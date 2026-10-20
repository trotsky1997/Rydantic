// Harness sanity fixture, never an agent trial or agent-visible input.
use rydantic::Model;
#[derive(Debug, Model)]
pub struct User {
    #[model(trim, min_length = 2, max_length = 8)]
    pub name: String,
    #[model(coerce, ge = 18, le = 120)]
    pub age: i64,
}
pub fn parse_user(input: serde_json::Value) -> Result<User, String> {
    User::parse_value(input).map_err(|error| error.to_string())
}
