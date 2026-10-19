use rydantic::Model;

#[derive(Model)]
struct Broken {
    #[model(rename = "value")]
    first: String,
    #[model(rename = "value")]
    second: String,
}

fn main() {}
