#[rydantic::capnp_models(id = 42)]
mod models {
    pub struct User { pub name: String }
}
fn main() {}
