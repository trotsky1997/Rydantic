use rydantic::Model;

#[derive(Model)]
struct Broken {
    #[model(min_length = 5, max_length = 2)]
    name: String,
}

fn main() {}
