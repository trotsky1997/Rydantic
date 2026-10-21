use rydantic_ai::{rydantic::Model, Agent, Api, HttpProvider, Pdf, Prompt, RunContext};
use std::{io::Read, sync::Arc};

#[derive(Debug, Model)]
struct Summary {
    summary: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("Usage: pdf DOCUMENT_PATH")?;
    let filename = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("invalid PDF filename")?;
    // Read one byte past the maximum decoded size so oversized inputs fail.
    let mut bytes = Vec::new();
    std::fs::File::open(&path)?
        .take((10 * 1024 * 1024 / 4 * 3 + 1) as u64)
        .read_to_end(&mut bytes)?;
    let api = std::env::var("API")
        .unwrap_or("responses".into())
        .parse::<Api>()?;
    let provider = Arc::new(HttpProvider::from_env(api)?);
    let prompt = Prompt::new("Summarize this PDF, including its important figures and tables.")
        .pdf(Pdf::from_bytes(filename, &bytes)?);
    let result = Agent::<(), Summary>::new(provider, std::env::var("MODEL")?)
        .run(prompt, Arc::new(()), RunContext::default())
        .await?;
    println!("{}", result.output.summary);
    Ok(())
}
