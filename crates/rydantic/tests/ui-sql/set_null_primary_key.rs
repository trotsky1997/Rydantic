use rydantic::{Model, SqlModel};
#[derive(Model, SqlModel)]
struct Invalid {
    #[sql(primary_key, references = "parents.id", on_delete = "set_null")]
    id: Option<i64>,
}
fn main() {}
