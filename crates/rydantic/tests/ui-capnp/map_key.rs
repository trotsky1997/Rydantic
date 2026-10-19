#[rydantic::capnp_models]
mod models {
    pub struct Map { pub values: std::collections::BTreeMap<u32, String> }
}
fn main() {}
