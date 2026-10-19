use rydantic::{
    chrono::{DateTime, Utc},
    rust_decimal::Decimal,
    Model,
};
#[derive(Debug, Model)]
struct Invoice {
    created_at: DateTime<Utc>,
    #[model(gt = 0)]
    amount: Decimal,
}
fn main() -> Result<(), rydantic::ValidationErrors> {
    let invoice = Invoice::parse_json(
        r#"{"created_at":"2026-10-19T12:30:00+08:00","amount":"12345678901234567890.123456789"}"#,
    )?;
    assert_eq!(invoice.amount.to_string(), "12345678901234567890.123456789");
    println!("{}", invoice.dump_json()?);
    Ok(())
}
