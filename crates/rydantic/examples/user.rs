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
        User::parse_value(json!({"name":"A", "age":17, "email":"invalid", "unknown":true}))
            .unwrap_err();
    println!("{}", errors.to_json());

    let report = User::schema_report(SchemaMode::Input);
    let local_rules = report
        .local_rules
        .iter()
        .map(|rule| json!({"schema_path": rule.schema_path, "rule": rule.rule}))
        .collect::<Vec<_>>();
    println!(
        "{}",
        json!({"schema": report.schema, "local_rules": local_rules})
    );
    Ok(())
}
