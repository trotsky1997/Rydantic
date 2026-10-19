use rydantic::{Model, ValidationError};

async fn available(name: &str) -> Result<(), ValidationError> {
    tokio::task::yield_now().await;
    if name == "taken" {
        Err(ValidationError::new("taken", "name is already registered"))
    } else {
        Ok(())
    }
}
#[derive(Debug, Model)]
struct Account {
    #[model(trim, min_length = 2, async_validate = available)]
    name: String,
}
#[tokio::main]
async fn main() -> Result<(), rydantic::ValidationErrors> {
    let account = Account::parse_validated_json_async(r#"{"name":" Alice "}"#).await?;
    println!(
        "{}",
        serde_json::to_string(&account).expect("validated account")
    );
    let errors = Account::parse_json_async(r#"{"name":"taken"}"#)
        .await
        .unwrap_err();
    assert_eq!(errors.errors()[0].code, "taken");
    println!("{}", errors.to_json());
    Ok(())
}
