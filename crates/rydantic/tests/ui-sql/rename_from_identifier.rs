use rydantic::{Model, SqlModel};
#[derive(Debug, Model, SqlModel)]
struct Bad {
    #[sql(primary_key)]
    id: i64,
    #[sql(rename_from = "name; DROP TABLE users")]
    value: String,
}
fn main() {}
