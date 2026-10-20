#![cfg(feature = "http")]
use rydantic_ai::{
    jev::{JevClient, Question, SystemOneRequest},
    RunContext,
};

#[tokio::test]
#[ignore = "requires TYPESAFE_BASEURL and TYPESAFE_APIKEY; performs a real billed request"]
async fn configured_systemone_roundtrip() {
    let client = JevClient::from_env().unwrap();
    let request = SystemOneRequest::new(
        "jev-1.14.0",
        "Payment failed for three days. Please help now.",
    )
    .question(
        "urgency",
        Question::noul("Does the customer request urgent help?"),
    )
    .unwrap()
    .question(
        "department",
        Question::choice(
            "Which team should handle this?",
            [
                ("billing", "Payment issues"),
                ("technical", "Bugs or integrations"),
            ],
        ),
    )
    .unwrap()
    .question(
        "frustration",
        Question::score(
            "How frustrated is the customer?",
            ["Calm", "Frustrated", "Very angry"],
        ),
    )
    .unwrap();
    let response = client
        .system_one(&request, RunContext::default())
        .await
        .unwrap();
    assert_eq!(response.model, "jev-1.14.0");
    assert!(response.usage.total().unwrap() > 0);
    response.validate_for(&request).unwrap();
    println!(
        "JEV_LIVE_RECEIPT={}",
        serde_json::to_string(&response).unwrap()
    );
}
