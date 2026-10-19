use rydantic::{Model,SqlModel};
#[derive(Model,SqlModel)]
struct Duplicate { #[sql(primary_key)] id:i64, #[sql(rename="Name")] first:String, #[sql(rename="name")] second:String }
fn main(){}
