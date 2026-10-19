use rydantic::Model;
#[derive(Model)]
struct Broken(#[model(exclude)] String);
fn main() {}
