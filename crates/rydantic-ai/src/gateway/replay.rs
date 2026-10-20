//! Bounded process-local replay, scoped to the authenticated conversation prefix.
use crate::{ModelRequest, ModelResponse};
use serde_json::{json, Value};
use std::{
    collections::{hash_map::DefaultHasher, VecDeque},
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 128;
const TTL: Duration = Duration::from_secs(30 * 60);
struct Entry {
    scope: (String, String, Option<String>),
    visible: Vec<Value>,
    digest: u64,
    output: Vec<Value>,
    bytes: usize,
    at: Instant,
}
#[derive(Default)]
pub(super) struct Replay {
    entries: VecDeque<Entry>,
    bytes: usize,
}
pub(super) struct Group {
    pub start: usize,
    pub end: usize,
    pub visible: Value,
}
fn arguments(value: &Value) -> Value {
    value
        .as_str()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or_else(|| value.clone())
}
fn plain(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.into();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["text"].as_str())
        .collect()
}
pub(super) fn groups(input: &[Value]) -> Vec<Group> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < input.len() {
        let start = i;
        let item = &input[i];
        if item["type"] == "reasoning"
            || item["type"] == "function_call"
            || item["role"] == "assistant"
        {
            let mut text = String::new();
            let mut calls = Vec::new();
            while i < input.len() {
                let item = &input[i];
                if item["type"] == "reasoning" {
                    i += 1;
                    continue;
                }
                if item["type"] == "function_call" {
                    calls.push(json!({"id":item["call_id"],"name":item["name"],"arguments":arguments(&item["arguments"])}));
                } else if item["role"] == "assistant" {
                    if i > start && input[i - 1]["type"] != "reasoning" {
                        break;
                    }
                    text.push_str(&plain(&item["content"]));
                } else {
                    break;
                }
                i += 1;
            }
            out.push(Group {
                start,
                end: i,
                visible: json!({"role":"assistant","text":text,"calls":calls}),
            });
        } else {
            let visible = if item["type"] == "function_call_output" {
                json!({"role":"tool","id":item["call_id"],"output":arguments(&item["output"])})
            } else {
                json!({"role":item["role"],"text":plain(&item["content"])})
            };
            i += 1;
            out.push(Group {
                start,
                end: i,
                visible,
            });
        }
    }
    out
}
impl Replay {
    fn expire(&mut self) {
        while self.entries.front().is_some_and(|e| e.at.elapsed() >= TTL) {
            if let Some(entry) = self.entries.pop_front() {
                self.bytes -= entry.bytes;
            }
        }
    }
    pub fn restore(&mut self, client: &str, request: &mut ModelRequest) {
        self.expire();
        let mut visible = Vec::new();
        let mut input = Vec::new();
        let mut hasher = DefaultHasher::new();
        for group in groups(&request.input) {
            group.visible.to_string().hash(&mut hasher);
            visible.push(group.visible);
            let found = if visible.last().is_some_and(|v| v["role"] == "assistant") {
                let digest = hasher.finish();
                self.entries.iter().rev().find(|e| {
                    e.digest == digest
                        && e.scope.0 == client
                        && e.scope.1 == request.model
                        && e.scope.2 == request.instructions
                        && e.visible == visible
                })
            } else {
                None
            };
            if let Some(entry) = found {
                input.extend_from_slice(&entry.output);
            } else {
                input.extend_from_slice(&request.input[group.start..group.end]);
            }
        }
        request.input = input;
    }
    pub fn record(&mut self, client: &str, request: &ModelRequest, response: &ModelResponse) {
        self.expire();
        if !response.output().iter().any(|v| {
            v.get("_rydantic_native").is_some()
                || matches!(v["type"].as_str(), Some("reasoning" | "function_call"))
        }) {
            return;
        }
        let mut visible: Vec<_> = groups(&request.input)
            .into_iter()
            .map(|g| g.visible)
            .collect();
        // Key the same assistant projection that public frontends return, not
        // the upstream item boundaries/order hidden from those clients.
        let (Ok(text), Ok(calls)) = (response.text(), response.tool_calls()) else {
            return;
        };
        let calls: Vec<_> = calls
            .into_iter()
            .map(|call| json!({"id":call.call_id,"name":call.name,"arguments":arguments(&json!(call.arguments))}))
            .collect();
        visible.push(json!({"role":"assistant","text":text,"calls":calls}));
        let scope = (
            client.to_owned(),
            request.model.clone(),
            request.instructions.clone(),
        );
        let output = response.output().to_vec();
        let mut hasher = DefaultHasher::new();
        for group in &visible {
            group.to_string().hash(&mut hasher);
        }
        let bytes = json!([scope, visible, output]).to_string().len();
        if bytes > MAX_BYTES {
            return;
        }
        while self.entries.len() >= MAX_ENTRIES || self.bytes + bytes > MAX_BYTES {
            let Some(entry) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= entry.bytes;
        }
        self.bytes += bytes;
        self.entries.push_back(Entry {
            scope,
            visible,
            digest: hasher.finish(),
            output,
            bytes,
            at: Instant::now(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(padding: usize) -> ModelResponse {
        ModelResponse::from_value(json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"working"}],"_rydantic_native":{"api":"chat_completions","message":{"role":"assistant","content":"working","reasoning_content":"x".repeat(padding)}}}],"usage":{"input_tokens":1,"output_tokens":1}})).unwrap()
    }
    fn continuation(request: &ModelRequest) -> ModelRequest {
        let mut next = request.clone();
        next.input.push(json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"working","annotations":[]}]}));
        next
    }
    #[test]
    fn projected_multi_turn_replay_keeps_scope_and_prefix_isolation() {
        let turn = |index| {
            ModelResponse::from_value(json!({"status":"completed","output":[
                {"type":"reasoning","id":format!("opaque-{index}"),"encrypted_content":"opaque"},
                {"type":"message","role":"assistant","content":[{"type":"output_text","text":"first"}]},
                {"type":"function_call","id":format!("original-{index}"),"call_id":format!("call-{index}"),"name":"add","arguments":"{}"},
                {"type":"message","role":"assistant","content":[{"type":"output_text","text":"second"}]}
            ],"usage":{"input_tokens":1,"output_tokens":1}})).unwrap()
        };
        let append = |request: &mut ModelRequest, index| {
            request.input.extend([
                json!({"role":"assistant","content":"firstsecond"}),
                json!({"type":"function_call","call_id":format!("call-{index}"),"name":"add","arguments":"{}"}),
                json!({"type":"function_call_output","call_id":format!("call-{index}"),"output":"42"}),
            ]);
        };
        let mut replay = Replay::default();
        let mut visible = ModelRequest::new("model", "compute");
        replay.record("app", &visible, &turn(1));
        append(&mut visible, 1);
        replay.record("app", &visible, &turn(2));
        append(&mut visible, 2);
        for case in [
            "control",
            "client",
            "model",
            "instructions",
            "prompt",
            "first_call",
            "second_call",
            "tool_result",
        ] {
            let mut next = visible.clone();
            match case {
                "model" => next.model = "other".into(),
                "instructions" => next.instructions = Some("other".into()),
                "prompt" => next.input[0]["content"] = json!("other"),
                "first_call" => next.input[2]["call_id"] = json!("other"),
                "second_call" => next.input[5]["call_id"] = json!("other"),
                "tool_result" => next.input[3]["output"] = json!("other"),
                _ => (),
            }
            replay.restore(if case == "client" { "other" } else { "app" }, &mut next);
            for index in [1, 2] {
                let expected = case == "control"
                    || (index == 1 && matches!(case, "second_call" | "tool_result"));
                let original = turn(index);
                assert_eq!(
                    next.input
                        .windows(original.output().len())
                        .any(|window| window == original.output()),
                    expected,
                    "{case}: turn {index}"
                );
            }
        }
    }
    #[test]
    fn replay_limits_entries_and_expires_native_history() {
        let mut replay = Replay::default();
        let response = response(0);
        for index in 0..MAX_ENTRIES + 1 {
            replay.record(
                "app",
                &ModelRequest::new("model", format!("prompt {index}")),
                &response,
            );
        }
        assert_eq!(replay.entries.len(), MAX_ENTRIES);
        let mut old = continuation(&ModelRequest::new("model", "prompt 0"));
        replay.restore("app", &mut old);
        assert!(old.input[1].get("_rydantic_native").is_none());
        let mut recent = continuation(&ModelRequest::new("model", format!("prompt {MAX_ENTRIES}")));
        replay.restore("app", &mut recent);
        assert!(recent.input[1].get("_rydantic_native").is_some());
        for entry in &mut replay.entries {
            entry.at = Instant::now() - TTL - Duration::from_secs(1);
        }
        let mut expired =
            continuation(&ModelRequest::new("model", format!("prompt {MAX_ENTRIES}")));
        replay.restore("app", &mut expired);
        assert!(expired.input[1].get("_rydantic_native").is_none());
        assert_eq!(replay.bytes, 0);
    }
    #[test]
    fn replay_evicts_payloads_before_the_serialized_byte_budget_is_exceeded() {
        let mut replay = Replay::default();
        let response = response(1024 * 1024);
        for index in 0..12 {
            replay.record(
                "app",
                &ModelRequest::new("model", format!("prompt {index}")),
                &response,
            );
            assert!(replay.bytes <= MAX_BYTES);
        }
        assert!(replay.entries.len() < 12);
        let mut old = continuation(&ModelRequest::new("model", "prompt 0"));
        replay.restore("app", &mut old);
        assert!(old.input[1].get("_rydantic_native").is_none());
        let mut recent = continuation(&ModelRequest::new("model", "prompt 11"));
        replay.restore("app", &mut recent);
        assert!(recent.input[1].get("_rydantic_native").is_some());
    }
}
