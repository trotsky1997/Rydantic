#[rydantic::capnp_models]
mod models {
    pub struct Gap {
        #[model(capnp_ordinal = 1)] pub value: u32,
    }
}
fn main() {}
