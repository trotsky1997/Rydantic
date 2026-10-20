//! Offline structured streaming; replace Demo with a ResponsesProvider for inference.
use rydantic_ai::{
    rydantic::Model, Agent, BoxFuture, Capabilities, ModelProvider, ModelRequest, ModelResponse,
    OutputEvent, Result, RunContext,
};
use serde_json::json;
use std::sync::Arc;

struct Demo;
impl ModelProvider for Demo {
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn complete(
        &self,
        request: ModelRequest,
        context: RunContext,
    ) -> BoxFuture<'_, Result<ModelResponse>> {
        Box::pin(async move {
            let chunks = if request.model == "demo-list" {
                vec![
                    r#"{"items":[{"name":"Ada","age":36},"#,
                    r#"{"name":"Grace","age":40}]}"#,
                ]
            } else {
                vec![r#"{"name":"Ada","#, r#""age":36}"#]
            };
            let text = chunks.concat();
            for delta in chunks {
                context
                    .emit(json!({"type":"response.output_text.delta","delta":delta}))
                    .await?;
                tokio::task::yield_now().await;
            }
            ModelResponse::from_value(
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":5,"output_tokens":10}}),
            )
        })
    }
}
#[derive(Debug, Model)]
struct Person {
    name: String,
    #[model(ge = 18)]
    age: u8,
}

#[tokio::main]
async fn main() -> Result<()> {
    let provider = Arc::new(Demo);
    let mut partial = Agent::<(), Person>::new(provider.clone(), "demo-person").stream_partial(
        "Extract one person",
        Arc::new(()),
        RunContext::default(),
    )?;
    while let Some(event) = partial.recv().await {
        match event? {
            OutputEvent::Delta { value, .. } => println!("Preview: {}", value.value),
            OutputEvent::Complete(result) => {
                println!("Accepted: {} ({})", result.output.name, result.output.age)
            }
            OutputEvent::Reset { .. } => println!("Clear the previous preview"),
            OutputEvent::Started { .. } => (),
        }
    }
    let result = Agent::<(), Person>::new(provider, "demo-list")
        .stream_iterable("Extract all people", Arc::new(()), RunContext::default())?
        .collect()
        .await?;
    for person in result.output {
        println!("Person: {} ({})", person.name, person.age);
    }
    println!("Observed tokens: {}", result.usage.total()?);
    Ok(())
}
