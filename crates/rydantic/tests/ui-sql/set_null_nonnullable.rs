use rydantic::{Model, SqlModel};
#[derive(Model, SqlModel)]
struct Invalid {
    #[sql(primary_key)]
    id: i64,
    #[sql(references = "parents.id", on_delete = "set_null")]
    parent_id: i64,
}
fn main() {}
