use rydantic::Model;
#[derive(Model)]
struct Empty { #[model(validate=[])] value: i32 }
#[derive(Model)]
struct NotPath { #[model(before=[42])] value: i32 }
#[derive(Model)]
struct Call { #[model(async_validate=[check()])] value: i32 }
#[derive(Model)]
struct Mixed { #[model(validate=check, validate_with_context=check)] value: i32 }
fn main() {}
