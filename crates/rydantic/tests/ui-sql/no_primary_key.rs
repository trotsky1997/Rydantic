use rydantic::{Model,SqlModel};
#[derive(Model,SqlModel)]
struct Missing { name:String }
fn main(){}
