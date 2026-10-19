use rydantic::Model;

#[derive(Model)]
struct Broken {
    #[model(pattern = "[")]
    name: String,
}

fn main() {}
