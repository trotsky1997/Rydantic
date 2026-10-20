use rydantic::Model;
use rydantic_ai::{
    jev::{JevClient, Question, SystemOneRequest},
    Error, Result, RunContext,
};

#[derive(Model)]
struct TicketDecision {
    #[model(ge = 0.0, le = 1.0)]
    urgency: f64,
    department: String,
    #[model(ge = 0.0, le = 2.0)]
    frustration: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let client = JevClient::from_env()?;
    let request = SystemOneRequest::new(
        "jev-1.14.0",
        "Payment failed for three days. Please help now.",
    )
    .question(
        "urgency",
        Question::noul("Does the customer request urgent help?"),
    )?
    .question(
        "department",
        Question::choice(
            "Which team should handle this?",
            [
                ("billing", "Payment issues"),
                ("technical", "Bugs or integrations"),
            ],
        ),
    )?
    .question(
        "frustration",
        Question::score(
            "How frustrated is the customer?",
            ["Calm", "Frustrated", "Very angry"],
        ),
    )?;
    let context = RunContext::default();
    let response = client.system_one(&request, context.clone()).await?;
    let decision: TicketDecision = response.parse(&context).await?;
    println!("{}", decision.dump_json()?);
    println!(
        "{}",
        serde_json::to_string(&response)
            .map_err(|_| Error::Protocol("cannot encode Jev response".into()))?
    );
    Ok(())
}
