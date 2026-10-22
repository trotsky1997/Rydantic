// Evaluator-only harness fixture, never an agent trial or agent-visible input.
use rydantic::{Context, Model, ValidationError};

pub struct Policy {
    pub maximum: i64,
}

fn within_policy(amount: &i64, context: &Context) -> Result<(), ValidationError> {
    let policy = context.require_data::<Policy>()?;
    if *amount > policy.maximum {
        Err(ValidationError::new(
            "policy_limit",
            "amount exceeds the per-call limit",
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Model)]
pub struct Request {
    #[model(ge = 0, validate_with_context = within_policy)]
    pub amount: i64,
}
