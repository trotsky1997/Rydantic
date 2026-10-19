#[rydantic::capnp_models]
mod models {
    pub struct Generic<T> { pub value: T }
}
fn main() {}
