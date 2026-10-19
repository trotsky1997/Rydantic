use rydantic::{Model,SqlModel};
#[derive(Model,SqlModel)]
struct Generic<T> { #[sql(primary_key)] id:i64, value:T }
fn main(){}
