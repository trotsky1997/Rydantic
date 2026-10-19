#[rydantic::capnp_models]
mod models {
    pub struct Duplicate {
        #[model(capnp_ordinal = 0)] pub first: u32,
        #[model(capnp_ordinal = 0)] pub second: u32,
    }
}
fn main() {}
