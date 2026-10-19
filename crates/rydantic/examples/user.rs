use rydantic::{json, Model, SchemaMode};

#[derive(Debug, Model)]
struct User {
    #[model(trim, min_length = 3, max_length = 30)]
    name: String,
    #[model(coerce, ge = 18, le = 120)]
    age: u8,
    #[model(trim, lowercase, email)]
    email: String,
    #[model(default)]
    tags: Vec<String>,
    note: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let user = User::parse_json(r#"{"name":" Alice ","age":"25","email":" ALICE@EXAMPLE.COM "}"#)?;
    println!("{}", user.dump_json()?);

    let errors =
        User::parse_value(json!({"name":"李", "age":17, "email":"invalid", "unknown":true}))
            .unwrap_err();
    println!("{}", serde_json::to_string_pretty(&errors)?);

    let report = User::schema_report(SchemaMode::Input);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
