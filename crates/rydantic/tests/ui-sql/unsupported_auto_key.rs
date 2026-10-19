use rydantic::{Model,SqlModel};
#[derive(Model,SqlModel)]
struct Invalid { #[sql(primary_key,auto_increment)] id:Option<u64> }
fn main(){}
