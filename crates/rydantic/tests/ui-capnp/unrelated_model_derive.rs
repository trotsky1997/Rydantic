mod other {
    pub use serde::Serialize as Model;
}
#[rydantic::capnp_models]
mod models {
    #[derive(super::other::Model)]
    pub struct Unrelated { pub value: u64 }
}
fn main() {}
