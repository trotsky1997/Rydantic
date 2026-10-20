use rydantic_ai::{rydantic::Model, Agent, Error, ResponsesProvider, RunContext, TypedTool};
use std::sync::Arc;

#[derive(Debug, Model)]
struct Answer {
    answer: String,
}
#[derive(Model)]
struct Add {
    a: i64,
    b: i64,
}
#[derive(Model)]
struct Sum {
    sum: i64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base =
        std::env::var("RYDANTIC_PI_URL").unwrap_or_else(|_| "http://127.0.0.1:8787/v1".into());
    let model = std::env::var("RYDANTIC_AI_MODEL").unwrap_or_else(|_| "gpt-6-luna".into());
    let provider = Arc::new(ResponsesProvider::pi_codex(&base)?);
    let add = TypedTool::<(), Add, Sum, _>::new(
        "add",
        "Add two integers exactly",
        |args: Add, _, _| async move {
            Ok(Sum {
                sum: args
                    .a
                    .checked_add(args.b)
                    .ok_or_else(|| Error::Tool("integer overflow".into()))?,
            })
        },
    )?;
    let agent = Agent::<(), Answer>::new(provider, model)
        .instructions("Use the add tool for arithmetic.")
        .tool(add)?;
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Use the add tool to compute 19 + 23 and report the answer.".into());
    let result = agent
        .run(prompt, Arc::new(()), RunContext::default())
        .await?;
    println!("{}", result.output.dump_json()?);
    println!(
        "model_calls={} tool_calls={} input_tokens={} output_tokens={}",
        result.model_calls,
        result.tool_calls,
        result.usage.input_tokens,
        result.usage.output_tokens
    );
    Ok(())
}
