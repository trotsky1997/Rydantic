//! Run with: cargo run -p rydantic --example jsonrpc --features jsonrpc
use rydantic::{
    json,
    jsonrpc::{Params, Router},
    Model,
};
#[derive(Model)]
struct Hello {
    #[model(trim, min_length = 2)]
    name: String,
}
#[derive(Model)]
struct Greeting {
    message: String,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut router = Router::new();
    router.register("hello", Params::Named, |input: Hello| {
        Ok(Greeting {
            message: format!("Hello, {}!", input.name),
        })
    })?;
    let reply = router
        .handle_json(r#"{"jsonrpc":"2.0","method":"hello","params":{"name":" Alice "},"id":1}"#)
        .unwrap();
    assert_eq!(
        reply,
        json!({"jsonrpc":"2.0","id":1,"result":{"message":"Hello, Alice!"}})
    );
    println!("{reply}");
    assert!(router
        .handle_json(r#"{"jsonrpc":"2.0","method":"hello","params":{"name":"Alice"}}"#)
        .is_none());
    Ok(())
}
