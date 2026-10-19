use rydantic::Model;

#[derive(Model)]
struct Broken {
    #[model(trim)]
    count: u32,
}

fn main() {}
