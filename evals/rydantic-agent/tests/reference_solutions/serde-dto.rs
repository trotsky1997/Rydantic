// Harness sanity fixture, never an agent trial or agent-visible input.
use serde::{Deserialize, Serialize};
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub label: String,
    pub count: u32,
}
pub fn decode(input: &str) -> Result<Message, serde_json::Error> {
    serde_json::from_str(input)
}
pub fn encode(message: &Message) -> Result<String, serde_json::Error> {
    serde_json::to_string(message)
}
