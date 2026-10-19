use rydantic::{Model,SqlModel};
#[derive(Model,SqlModel)]
struct Duplicate { #[sql(primary_key)] first:i64, #[sql(primary_key)] second:i64 }
fn main(){}
