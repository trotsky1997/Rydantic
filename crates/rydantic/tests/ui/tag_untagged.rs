use rydantic::Model;
#[derive(Model)]
#[model(tag = "kind", untagged)]
enum Broken { A }
fn main() {}
