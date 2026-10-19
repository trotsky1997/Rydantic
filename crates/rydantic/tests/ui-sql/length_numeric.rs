use rydantic::{Model, SqlModel};
#[derive(Model, SqlModel)]
struct Invalid {
    #[sql(primary_key)] id: i64,
    #[sql(length = 32)] value: i64,
}
fn main() {}
