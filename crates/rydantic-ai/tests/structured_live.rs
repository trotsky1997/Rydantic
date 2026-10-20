#![cfg(feature = "http")]
use rydantic_ai::{rydantic::Model, Agent, OutputEvent, ResponsesProvider, RunContext};
use std::sync::Arc;

#[derive(Debug, Model)]
struct Person {
    name: String,
    age: u8,
}

#[tokio::test]
#[ignore = "requires an authorized pi bridge and RYDANTIC_PI_URL; performs real inference"]
async fn pi_codex_partial_and_iterable_roundtrip() {
    let base = std::env::var("RYDANTIC_PI_URL").expect("set RYDANTIC_PI_URL after authorization");
    let model = std::env::var("RYDANTIC_AI_MODEL").unwrap_or_else(|_| "gpt-6-luna".into());
    let provider = Arc::new(ResponsesProvider::pi_codex(&base).unwrap());
    let agent = Agent::<(), Person>::new(provider, model);
    let mut partial = agent
        .stream_partial(
            "Extract exactly: Ada is 36 years old.",
            Arc::new(()),
            RunContext::default(),
        )
        .unwrap();
    let mut snapshots = 0;
    let mut partial_tokens = 0;
    while let Some(event) = partial.recv().await {
        match event.unwrap() {
            OutputEvent::Delta { .. } => snapshots += 1,
            OutputEvent::Complete(result) => {
                assert_eq!(result.output.name, "Ada");
                assert_eq!(result.output.age, 36);
                partial_tokens = result.usage.total().unwrap();
            }
            _ => (),
        }
    }
    assert!(snapshots > 0 && partial_tokens > 0);
    let result = agent
        .stream_iterable(
            "Extract exactly two people, preserving order: Ada is 36; Grace is 40.",
            Arc::new(()),
            RunContext::default(),
        )
        .unwrap()
        .collect()
        .await
        .unwrap();
    assert_eq!(
        result
            .output
            .iter()
            .map(|p| (p.name.as_str(), p.age))
            .collect::<Vec<_>>(),
        [("Ada", 36), ("Grace", 40)]
    );
    assert!(result.usage.total().unwrap() > 0);
    println!(
        "STRUCTURED_LIVE_RECEIPT={}",
        serde_json::json!({"partial_snapshots":snapshots,"partial_tokens":partial_tokens,"iterable_items":result.output.len(),"iterable_tokens":result.usage.total().unwrap(),"iterable_model_calls":result.model_calls})
    );
}
