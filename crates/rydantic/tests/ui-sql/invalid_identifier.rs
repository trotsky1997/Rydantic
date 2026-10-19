use rydantic::{Model,SqlModel};
#[derive(Model,SqlModel)]
#[sql(table="users; DROP TABLE users")]
struct Invalid { #[sql(primary_key)] id:i64 }
fn main(){}
