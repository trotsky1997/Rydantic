use rydantic::Model;
#[derive(Model)]
#[model(extra = "allow")]
struct Broken { #[model(extra)] values: String }
fn main() {}
