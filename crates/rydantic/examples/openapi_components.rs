use rydantic::{json, schema::openapi::OpenApiComponents, Model, SchemaMode};

#[derive(Model)]
struct User {
    #[model(min_length = 2)]
    name: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut components = OpenApiComponents::new();
    let input = components.insert::<User>("UserInput", SchemaMode::Input)?;
    let output = components.insert::<User>("UserOutput", SchemaMode::Output)?;
    let document = json!({
        "openapi": "3.1.1",
        "info": {"title": "Users", "version": "1.0.0"},
        "paths": {"/users": {"post": {
            "requestBody": {"required": true, "content": {"application/json": {"schema": input.reference}}},
            "responses": {"200": {"description": "Created user", "content": {"application/json": {"schema": output.reference}}}}
        }}},
        "components": {"schemas": components.into_schemas()}
    });
    println!("{}", serde_json::to_string_pretty(&document)?);
    Ok(())
}
