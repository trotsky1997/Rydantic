use rydantic::{Model, SqlModel};
#[derive(Debug, Model, SqlModel)]
struct Bad {
    #[sql(primary_key)]
    id: i64,
    #[sql(default = "text")]
    value: i64,
}
fn main() {}
