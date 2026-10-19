use rydantic::Model;

#[derive(Model)]
struct Broken {
    #[model(min_len = 3)]
    name: String,
}

fn main() {}
