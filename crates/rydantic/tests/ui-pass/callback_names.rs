//! Every bare callback path must resolve in the caller's scope, not to a
//! generated local. Include the generated prefix so renaming is not a fix.
#![allow(dead_code)]
use rydantic::{Model, ValidationError, ValidationErrors, Value};

macro_rules! callback_names {
    ($($name:ident),* $(,)?) => {$(
        mod $name {
            use super::*;
            mod before {
                use super::*;
                fn $name(v: Value) -> Result<Value, ValidationError> { Ok(v) }
                #[derive(Model)]
                struct Named { #[model(before = $name)] n: u32 }
                #[derive(Model)]
                struct Tuple(#[model(before = $name)] u32, bool);
                #[derive(Model)]
                #[model(untagged)]
                enum Untagged { Item(#[model(before = $name)] u32) }
            }
            mod validate {
                use super::*;
                fn $name(_: &u32) -> Result<(), ValidationError> { Ok(()) }
                #[derive(Model)]
                struct Named { #[model(validate = $name)] n: u32 }
                #[derive(Model)]
                struct Newtype(#[model(validate = $name)] u32);
                #[derive(Model)]
                #[model(tag = "kind")]
                enum Tagged { Item { #[model(validate = $name)] n: u32 } }
            }
            mod asynchronous {
                use super::*;
                async fn $name(_: &u32) -> Result<(), ValidationError> { Ok(()) }
                #[derive(Model)]
                struct Named { #[model(async_validate = $name)] n: u32 }
                #[derive(Model)]
                #[model(untagged)]
                enum Untagged { Item(#[model(async_validate = $name)] u32) }
            }
            mod model_validate {
                use super::*;
                fn $name(_: &Named) -> Result<(), ValidationErrors> { Ok(()) }
                #[derive(Model)]
                #[model(validate = $name)]
                struct Named { n: u32 }
            }
            mod model_async {
                use super::*;
                async fn $name(_: &Named) -> Result<(), ValidationErrors> { Ok(()) }
                #[derive(Model)]
                #[model(async_validate = $name)]
                struct Named { n: u32 }
            }
            mod defaults {
                use super::*;
                fn $name() -> u32 { 7 }
                #[derive(Model)]
                struct Named { #[model(default_factory = $name)] n: u32 }
            }
        }
    )*};
}
callback_names!(
    raw, value, child, context, ctx, start, rules, trial,
    __rydantic_ctx, __rydantic_raw, __rydantic_value, __rydantic_start,
    __rydantic_field_0,
);

fn main() {}
