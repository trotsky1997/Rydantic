use rydantic::{Model, SchemaMode};

#[derive(Debug, Model)]
#[model(tag = "kind")]
enum ToolCall {
    #[model(rename = "search")]
    Search {
        /// Text to search for in the local example corpus.
        #[model(min_length = 1, max_length = 100)]
        query: String,
        #[model(default = 3, ge = 1, le = 10)]
        limit: usize,
    },
    #[model(rename = "ping")]
    Ping,
}

#[derive(Model)]
struct ToolResult {
    results: Vec<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r#"{"kind":"search","query":"Rust","limit":2}"#.into());
    if input == "--schema" {
        println!(
            "{}",
            serde_json::to_string_pretty(&ToolCall::json_schema(SchemaMode::Input))?
        );
        return Ok(());
    }
    let call = match ToolCall::parse_json(&input) {
        Ok(call) => call,
        Err(errors) => {
            eprintln!("{}", serde_json::to_string_pretty(&errors)?);
            std::process::exit(2);
        }
    };
    let results = match call {
        ToolCall::Search { query, limit } => {
            ["Rust data modeling", "Rust JSON Schema", "Python Pydantic"]
                .into_iter()
                .filter(|text| text.to_lowercase().contains(&query.to_lowercase()))
                .take(limit)
                .map(str::to_owned)
                .collect()
        }
        ToolCall::Ping => vec!["pong".into()],
    };
    println!("{}", (ToolResult { results }).dump_json()?);
    Ok(())
}
