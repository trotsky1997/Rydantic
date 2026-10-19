# Rydantic derive

Procedural macros for [Rydantic](https://crates.io/crates/rydantic), a Rust library for data modeling, validation, JSON Schema and native Cap'n Proto.

Use the `rydantic` crate, which enables and re-exports `#[derive(Model)]` by default:

```toml
[dependencies]
rydantic = "0.2.0"
```

```rust
use rydantic::Model;

#[derive(Debug, Model)]
struct User {
    #[model(trim, min_length = 2)]
    name: String,
    #[model(ge = 18, le = 120)]
    age: u8,
}

let user = User::parse_json(r#"{"name":" Alice ","age":25}"#)?;
assert_eq!(user.name, "Alice");
# Ok::<(), rydantic::ValidationErrors>(())
```

See the [API documentation](https://docs.rs/rydantic) and [complete guide](https://github.com/trotsky1997/Rydantic) for borrowing, validators, model attributes, schemas and Cap'n Proto mappings.

License: MIT.
