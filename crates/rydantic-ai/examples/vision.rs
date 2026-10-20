use rydantic_ai::{rydantic::Model, Agent, Api, HttpProvider, Image, Prompt, RunContext};
use std::{io::Read, sync::Arc};

#[derive(Debug, Model)]
struct Description {
    description: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("Usage: vision IMAGE_PATH [image/png|image/jpeg|image/gif|image/webp]")?;
    let mime = args.next().unwrap_or("image/png".into());
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(8 * 1024 * 1024)
        .read_to_end(&mut bytes)?;
    let api = std::env::var("API")
        .unwrap_or("responses".into())
        .parse::<Api>()?;
    let provider = Arc::new(HttpProvider::from_env(api)?);
    let model = std::env::var("MODEL")?;
    let prompt = Prompt::new("Describe the contents of this image.")
        .image(Image::from_bytes(&mime, &bytes)?);
    let result = Agent::<(), Description>::new(provider, model)
        .run(prompt, Arc::new(()), RunContext::default())
        .await?;
    println!("{}", result.output.description);
    Ok(())
}
