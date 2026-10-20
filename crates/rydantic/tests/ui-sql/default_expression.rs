use rydantic::{Model, SqlModel};
#[derive(Debug, Model, SqlModel)]
struct Bad {
    #[sql(primary_key)]
    id: i64,
    #[sql(default = String::new())]
    value: String,
}
fn main() {}
