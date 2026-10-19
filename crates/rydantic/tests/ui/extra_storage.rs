use rydantic::Model;
#[derive(Model)]
#[model(extra = "allow")]
struct Broken { value: String }
fn main() {}
