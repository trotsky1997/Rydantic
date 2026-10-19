#[rydantic::capnp_models]
mod models {
    pub struct Duplicate {
        #[model(capnp_name = "first")] pub value: u32,
        pub first: u32,
    }
}
fn main() {}
