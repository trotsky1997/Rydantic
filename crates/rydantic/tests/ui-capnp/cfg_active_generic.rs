#[rydantic::capnp_models]
mod models {
    #[cfg_attr(all(), cfg(all()))]
    pub struct Active<T> { pub value: T }
}
fn main() {}
