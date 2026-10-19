use rydantic::{Model,SqlModel};
#[derive(Model,SqlModel)]
struct Invalid { #[sql(auto_increment)] id:Option<i64> }
fn main(){}
