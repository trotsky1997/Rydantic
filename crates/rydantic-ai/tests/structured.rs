mod common;
use common::*;
use rydantic_ai::{
    rydantic::{Model, ValidationError},
    Agent, AgentLimits, BoxFuture, Capabilities, Error, ModelProvider, ModelRequest, ModelResponse,
    OutputEvent, ResetReason, Result, RunContext, StreamOptions,
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::Notify;

struct Plan {
    chunks: Vec<String>,
    response: Result<ModelResponse>,
    gate: Option<Arc<Notify>>,
}
impl Plan {
    fn text(text: &str) -> Self {
        Self {
            chunks: text.chars().map(|c| c.to_string()).collect(),
            response: Ok(response(text)),
            gate: None,
        }
    }
}
struct Streaming {
    plans: Mutex<VecDeque<Plan>>,
    requests: Mutex<Vec<ModelRequest>>,
    native: bool,
}
impl Streaming {
    fn new(plans: Vec<Plan>) -> Self {
        Self {
            plans: Mutex::new(plans.into()),
            requests: Mutex::new(vec![]),
            native: false,
        }
    }
}
impl ModelProvider for Streaming {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            native_json_schema: self.native,
            output_token_limit: true,
        }
    }
    fn complete(
        &self,
        request: ModelRequest,
        context: RunContext,
    ) -> BoxFuture<'_, Result<ModelResponse>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request);
            let plan = self.plans.lock().unwrap().pop_front().unwrap_or_else(|| {
                panic!(
                    "unexpected mock request: {:?}",
                    self.requests.lock().unwrap().last()
                )
            });
            for chunk in plan.chunks {
                context.emit(json!({"type":"response.output_text.delta", "delta":chunk, "item_id":"text", "output_index":0, "content_index":0})).await?;
            }
            if let Some(gate) = plan.gate {
                context
                    .wait(async {
                        gate.notified().await;
                        Ok(())
                    })
                    .await?;
            }
            plan.response
        })
    }
}
#[derive(Debug, Model)]
struct Answer {
    #[model(ge = 0)]
    value: i64,
}

#[tokio::test]
async fn partial_snapshots_arrive_before_transport_completion() {
    #[derive(Debug, Model)]
    struct Person {
        name: String,
        age: u8,
    }
    let gate = Arc::new(Notify::new());
    let provider = Arc::new(Streaming::new(vec![Plan {
        chunks: vec![r#"{"name":"Ada","#.into(), r#""age":36}"#.into()],
        response: Ok(response(r#"{"name":"Ada","age":36}"#)),
        gate: Some(gate.clone()),
    }]));
    let mut stream = Agent::<(), Person>::new(provider.clone(), "mock")
        .stream_partial("extract", Arc::new(()), RunContext::default())
        .unwrap();
    assert!(matches!(
        stream.recv().await.unwrap().unwrap(),
        OutputEvent::Started { attempt: 1 }
    ));
    let event = tokio::time::timeout(Duration::from_secs(1), stream.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    match event {
        OutputEvent::Delta { attempt: 1, value } => {
            assert_eq!(value.value["name"], "Ada");
            assert!(value.value.get("age").is_none());
            assert!(!value.complete_json);
        }
        other => panic!("unexpected event {other:?}"),
    }
    gate.notify_one();
    let result = stream.finish().await.unwrap();
    assert_eq!(result.output.name, "Ada");
    assert_eq!(result.output.age, 36);
    assert_eq!(result.usage.total().unwrap(), 5);
    assert!(provider.requests.lock().unwrap()[0].stream);
}

#[tokio::test]
async fn partial_json_completeness_waits_for_final_model_validation_and_correction() {
    let provider = Arc::new(Streaming::new(vec![
        Plan::text(r#"{"value":-1}"#),
        Plan::text(r#"{"value":42}"#),
    ]));
    let mut stream = Agent::<(), Answer>::new(provider.clone(), "mock")
        .stream_partial("extract", Arc::new(()), RunContext::default())
        .unwrap();
    let mut reset = false;
    let mut saw_invalid_preview = false;
    let mut complete = false;
    while let Some(event) = stream.recv().await {
        match event.unwrap() {
            OutputEvent::Delta { attempt: 1, value } => {
                saw_invalid_preview |= value.complete_json && value.value["value"] == -1;
            }
            OutputEvent::Reset {
                attempt: 1,
                reason: ResetReason::Validation(errors),
            } => {
                assert!(!errors.is_empty());
                reset = true;
            }
            OutputEvent::Complete(result) => {
                assert_eq!(result.output.value, 42);
                assert_eq!(result.model_calls, 2);
                complete = true;
            }
            _ => (),
        }
    }
    assert!(reset && saw_invalid_preview && complete);
    assert!(
        provider.requests.lock().unwrap()[1].input.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("Validation errors")
    );
}

#[tokio::test]
async fn iterable_items_arrive_before_completion_without_a_clone_bound() {
    let gate = Arc::new(Notify::new());
    let provider = Arc::new(Streaming::new(vec![Plan {
        chunks: vec![
            r#"{"items":[{"value":1},"#.into(),
            r#"{"value":2}]}"#.into(),
        ],
        response: Ok(response(r#"{"items":[{"value":1},{"value":2}]}"#)),
        gate: Some(gate.clone()),
    }]));
    let mut stream = Agent::<(), Answer>::new(provider, "mock")
        .stream_iterable("list", Arc::new(()), RunContext::default())
        .unwrap();
    assert!(matches!(
        stream.recv().await.unwrap().unwrap(),
        OutputEvent::Started { attempt: 1 }
    ));
    match tokio::time::timeout(Duration::from_secs(1), stream.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
    {
        OutputEvent::Delta { attempt: 1, value } => {
            assert_eq!(value.index, 0);
            assert_eq!(value.value.value, 1);
        }
        event => panic!("unexpected {event:?}"),
    }
    gate.notify_one();
    let mut count = 1;
    while let Some(event) = stream.recv().await {
        match event.unwrap() {
            OutputEvent::Delta { value, .. } => {
                assert_eq!(value.index, count);
                count += 1;
            }
            OutputEvent::Complete(result) => assert_eq!(result.output, 2),
            _ => (),
        }
    }
    assert_eq!(count, 2);
}

#[tokio::test]
async fn iterable_collect_discards_failed_attempts_and_keeps_usage() {
    let provider = Arc::new(Streaming::new(vec![
        Plan::text(r#"{"items":[{"value":1},{"value":-1}]}"#),
        Plan::text(r#"{"items":[{"value":2},{"value":3}]}"#),
    ]));
    let result = Agent::<(), Answer>::new(provider.clone(), "mock")
        .stream_iterable("list", Arc::new(()), RunContext::default())
        .unwrap()
        .collect()
        .await
        .unwrap();
    assert_eq!(
        result.output.iter().map(|x| x.value).collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(result.model_calls, 2);
    assert_eq!(result.usage.total().unwrap(), 10);
    let requests = provider.requests.lock().unwrap();
    assert!(
        requests[1].input.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("/items/1/value"),
        "{}",
        requests[1].input.last().unwrap()
    );
}

#[tokio::test]
async fn iterable_async_checks_run_once_per_emitted_item() {
    static CHECKS: AtomicUsize = AtomicUsize::new(0);
    async fn check(_: &String) -> std::result::Result<(), ValidationError> {
        CHECKS.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        Ok(())
    }
    #[derive(Debug, Model)]
    struct Item {
        #[model(validation_alias="displayName", trim, async_validate=check)]
        name: String,
    }
    let provider = Arc::new(Streaming::new(vec![Plan::text(
        r#"{"items":[{"displayName":" Ada "},{"displayName":" Bob "}]}"#,
    )]));
    let result = Agent::<(), Item>::new(provider, "mock")
        .stream_iterable("list", Arc::new(()), RunContext::default())
        .unwrap()
        .collect()
        .await
        .unwrap();
    assert_eq!(result.output[0].name, "Ada");
    assert_eq!(result.output[1].name, "Bob");
    assert_eq!(CHECKS.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn iterable_preserves_large_numbers_nested_values_and_escaped_unicode() {
    #[derive(Debug, Model)]
    struct Record {
        number: u128,
        value: Value,
    }
    let text = r#"{"items":[{"number":340282366920938463463374607431768211455,"value":{"text":"你好😊 \\\" [] {}","decimal":1.2345678901234567890123456789}}]}"#;
    let provider = Arc::new(Streaming::new(vec![Plan::text(text)]));
    let result = Agent::<(), Record>::new(provider, "mock")
        .stream_iterable("list", Arc::new(()), RunContext::default())
        .unwrap()
        .collect()
        .await
        .unwrap();
    assert_eq!(result.output[0].number, u128::MAX);
    assert_eq!(
        result.output[0].value["decimal"].to_string(),
        "1.2345678901234567890123456789"
    );
    assert!(result.output[0].value["text"]
        .as_str()
        .unwrap()
        .contains("你好😊"));
}

#[tokio::test]
async fn scalar_number_is_held_until_its_right_delimiter() {
    #[derive(Debug, Model)]
    struct Number(u64);
    let gate = Arc::new(Notify::new());
    let provider = Arc::new(Streaming::new(vec![Plan {
        chunks: vec![r#"{"items":[1"#.into(), "2,3]}".into()],
        response: Ok(response(r#"{"items":[12,3]}"#)),
        gate: Some(gate.clone()),
    }]));
    let mut stream = Agent::<(), Number>::new(provider, "mock")
        .stream_iterable("list", Arc::new(()), RunContext::default())
        .unwrap();
    let mut values = Vec::new();
    while values.len() < 2 {
        if let OutputEvent::Delta { value, .. } = stream.recv().await.unwrap().unwrap() {
            values.push(value.value.0);
        }
    }
    assert_eq!(values, [12, 3]);
    gate.notify_one();
    assert!(matches!(
        stream.recv().await.unwrap().unwrap(),
        OutputEvent::Complete(_)
    ));
}

#[tokio::test]
async fn buffered_providers_empty_lists_and_native_recursive_schema_work() {
    #[derive(Debug, Model)]
    struct Node {
        value: u8,
        next: Option<Box<Node>>,
    }
    let provider = Arc::new(Streaming {
        native: true,
        ..Streaming::new(vec![Plan {
            chunks: vec![],
            response: Ok(response(r#"{"items":[{"value":1,"next":{"value":2}}]}"#)),
            gate: None,
        }])
    });
    let result = Agent::<(), Node>::new(provider.clone(), "native")
        .stream_iterable("list", Arc::new(()), RunContext::default())
        .unwrap()
        .collect()
        .await
        .unwrap();
    assert_eq!(result.output[0].next.as_ref().unwrap().value, 2);
    let schema =
        provider.requests.lock().unwrap()[0].text.as_ref().unwrap()["format"]["schema"].clone();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["items"]["type"], "array");
    fn refs(value: &Value, root: &Value) {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(reference)) = map.get("$ref") {
                    assert!(root.pointer(reference.strip_prefix('#').unwrap()).is_some());
                }
                for child in map.values() {
                    refs(child, root);
                }
            }
            Value::Array(list) => {
                for child in list {
                    refs(child, root);
                }
            }
            _ => (),
        }
    }
    refs(&schema, &schema);
    let provider = Arc::new(Scripted::new(vec![Ok(response(r#"{"items":[]}"#))]));
    assert!(Agent::<(), Answer>::new(provider, "mock")
        .stream_iterable("list", Arc::new(()), RunContext::default())
        .unwrap()
        .collect()
        .await
        .unwrap()
        .output
        .is_empty());
    let provider = Arc::new(Scripted::new(vec![Ok(response(r#"{"value":7}"#))]));
    assert_eq!(
        Agent::<(), Answer>::new(provider, "mock")
            .stream_partial("one", Arc::new(()), RunContext::default())
            .unwrap()
            .finish()
            .await
            .unwrap()
            .output
            .value,
        7
    );
}

#[tokio::test]
async fn malformed_envelopes_never_complete_and_have_bounded_retries() {
    for text in [
        r#"{"items":[{"value":1}],"items":[]}"#,
        r#"{"items":[{"value":1}],"extra":true}"#,
        r#"{"items":[{"value":1},]}"#,
        r#"{"items":[{"value":1}]} garbage"#,
        r#"[{"value":1}]"#,
        r#"{"items":[{"value":1}"#,
    ] {
        let provider = Arc::new(Streaming::new(vec![Plan::text(text)]));
        let result = Agent::<(), Answer>::new(provider.clone(), "mock")
            .limits(AgentLimits {
                max_validation_retries: 0,
                ..Default::default()
            })
            .stream_iterable("list", Arc::new(()), RunContext::default())
            .unwrap()
            .collect()
            .await;
        assert!(
            matches!(result, Err(Error::Validation(_))),
            "unexpected result for {text}: {result:?}"
        );
        assert_eq!(provider.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn tool_continuation_resets_items_before_the_next_model_call() {
    #[derive(Model)]
    struct Args {
        a: i64,
        b: i64,
    }
    let provider = Arc::new(Streaming::new(vec![
        Plan {
            chunks: vec![r#"{"items":[{"value":99}]}"#.into()],
            response: Ok(response_items(vec![
                json!({"type":"message","content":[{"type":"output_text","text":r#"{"items":[{"value":99}]}"#}]}),
                call("add", r#"{"a":1,"b":2}"#),
            ])),
            gate: None,
        },
        Plan::text(r#"{"items":[{"value":3}]}"#),
    ]));
    let tool = rydantic_ai::TypedTool::<(), Args, Answer, _>::new(
        "add",
        "add",
        |args: Args, _, _| async move {
            Ok(Answer {
                value: args.a + args.b,
            })
        },
    )
    .unwrap();
    let result = Agent::<(), Answer>::new(provider, "mock")
        .tool(tool)
        .unwrap()
        .stream_iterable("list", Arc::new(()), RunContext::default())
        .unwrap()
        .collect()
        .await
        .unwrap();
    assert_eq!(result.output.len(), 1);
    assert_eq!(result.output[0].value, 3);
    assert_eq!(result.tool_calls, 1);
}

fn counted_tool_agent(provider: Arc<Streaming>, calls: Arc<AtomicUsize>) -> Agent<(), Answer> {
    #[derive(Model)]
    struct Args {
        a: i64,
        b: i64,
    }
    let tool = rydantic_ai::TypedTool::<(), Args, Answer, _>::new(
        "add",
        "add",
        move |args: Args, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move {
                Ok(Answer {
                    value: args.a + args.b,
                })
            }
        },
    )
    .unwrap();
    Agent::new(provider, "mock").tool(tool).unwrap()
}

fn text_and_tool(text: &str) -> ModelResponse {
    response_items(vec![
        json!({"type":"message","content":[{"type":"output_text","text":text}]}),
        call("add", r#"{"a":1,"b":2}"#),
    ])
}

async fn finish_test_stream(
    agent: &Agent<(), Answer>,
    iterable: bool,
    options: StreamOptions,
) -> Result<()> {
    if iterable {
        agent
            .stream_iterable_with_options("list", Arc::new(()), RunContext::default(), options)?
            .collect()
            .await
            .map(|_| ())
    } else {
        agent
            .stream_partial_with_options("one", Arc::new(()), RunContext::default(), options)?
            .finish()
            .await
            .map(|_| ())
    }
}

#[tokio::test]
async fn mismatched_tool_turns_fail_before_tool_execution() {
    for iterable in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let final_text = if iterable {
            r#"{"items":[{"value":3}]}"#
        } else {
            r#"{"value":3}"#
        };
        let provider = Arc::new(Streaming::new(vec![
            Plan {
                chunks: vec![r#"{"value":111}"#.into()],
                response: Ok(text_and_tool(r#"{"value":222}"#)),
                gate: None,
            },
            Plan::text(final_text),
        ]));
        let agent = counted_tool_agent(provider.clone(), calls.clone());
        assert!(matches!(
            finish_test_stream(&agent, iterable, StreamOptions::default()).await,
            Err(Error::Protocol(_))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn buffered_tool_turns_obey_byte_and_depth_limits_before_execution() {
    let cases = [
        (
            format!(r#"{{"value":"{}"}}"#, "x".repeat(1024)),
            StreamOptions {
                max_output_bytes: 64,
                ..Default::default()
            },
        ),
        (
            format!("{}0{}", "[".repeat(65), "]".repeat(65)),
            StreamOptions::default(),
        ),
    ];
    for iterable in [false, true] {
        for (text, options) in &cases {
            let calls = Arc::new(AtomicUsize::new(0));
            let final_text = if iterable {
                r#"{"items":[{"value":3}]}"#
            } else {
                r#"{"value":3}"#
            };
            let provider = Arc::new(Streaming::new(vec![
                Plan {
                    chunks: vec![],
                    response: Ok(text_and_tool(text)),
                    gate: None,
                },
                Plan::text(final_text),
            ]));
            let agent = counted_tool_agent(provider.clone(), calls.clone());
            assert!(matches!(
                finish_test_stream(&agent, iterable, *options).await,
                Err(Error::Limit(_))
            ));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(provider.requests.lock().unwrap().len(), 1);
        }
    }
}

#[tokio::test]
async fn consistent_tool_turn_text_need_not_validate_as_the_final_model() {
    for iterable in [false, true] {
        for buffered in [false, true] {
            let calls = Arc::new(AtomicUsize::new(0));
            let provisional = "I will call add.";
            let final_text = if iterable {
                r#"{"items":[{"value":3}]}"#
            } else {
                r#"{"value":3}"#
            };
            let provider = Arc::new(Streaming::new(vec![
                Plan {
                    chunks: if buffered {
                        vec![]
                    } else {
                        vec![provisional.into()]
                    },
                    response: Ok(text_and_tool(provisional)),
                    gate: None,
                },
                Plan::text(final_text),
            ]));
            let agent = counted_tool_agent(provider.clone(), calls.clone());
            finish_test_stream(&agent, iterable, StreamOptions::default())
                .await
                .unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(provider.requests.lock().unwrap().len(), 2);
        }
    }
}

#[tokio::test]
async fn resource_limits_and_stream_final_mismatches_fail_explicitly() {
    let options = StreamOptions {
        max_items: 1,
        ..Default::default()
    };
    let provider = Arc::new(Streaming::new(vec![Plan::text(
        r#"{"items":[{"value":1},{"value":2}]}"#,
    )]));
    assert!(matches!(
        Agent::<(), Answer>::new(provider, "mock")
            .stream_iterable_with_options("list", Arc::new(()), RunContext::default(), options)
            .unwrap()
            .collect()
            .await,
        Err(Error::Limit(_))
    ));
    let provider = Arc::new(Streaming::new(vec![Plan::text(r#"{"value":123}"#)]));
    assert!(matches!(
        Agent::<(), Answer>::new(provider, "mock")
            .stream_partial_with_options(
                "one",
                Arc::new(()),
                RunContext::default(),
                StreamOptions {
                    max_output_bytes: 8,
                    ..Default::default()
                }
            )
            .unwrap()
            .finish()
            .await,
        Err(Error::Limit(_))
    ));
    let provider = Arc::new(Streaming::new(vec![Plan {
        chunks: vec![r#"{"value":1}"#.into()],
        response: Ok(response(r#"{"value":2}"#)),
        gate: None,
    }]));
    assert!(matches!(
        Agent::<(), Answer>::new(provider, "mock")
            .stream_partial("one", Arc::new(()), RunContext::default())
            .unwrap()
            .finish()
            .await,
        Err(Error::Protocol(_))
    ));
}

struct Pending {
    dropped: Arc<AtomicBool>,
}
struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
impl ModelProvider for Pending {
    fn complete(&self, _: ModelRequest, _: RunContext) -> BoxFuture<'_, Result<ModelResponse>> {
        Box::pin(async move {
            let _guard = Dropped(self.dropped.clone());
            std::future::pending().await
        })
    }
}
#[tokio::test]
async fn dropping_a_stream_cancels_work_without_cancelling_its_parent() {
    let dropped = Arc::new(AtomicBool::new(false));
    let provider = Arc::new(Pending {
        dropped: dropped.clone(),
    });
    let context = RunContext::default();
    let parent = context.cancellation.clone();
    let mut stream = Agent::<(), Answer>::new(provider, "mock")
        .stream_partial("one", Arc::new(()), context)
        .unwrap();
    stream.recv().await.unwrap().unwrap();
    tokio::task::yield_now().await;
    drop(stream);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !dropped.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!parent.is_cancelled());
}

#[tokio::test]
async fn timeouts_and_cancellation_are_terminal_even_with_a_full_progress_queue() {
    let provider = Arc::new(Streaming::new(vec![Plan::text(r#"{"value":1}"#)]));
    let stream = Agent::<(), Answer>::new(provider, "mock")
        .stream_partial_with_options(
            "one",
            Arc::new(()),
            RunContext::with_timeout(Duration::from_millis(20)),
            StreamOptions {
                capacity: 1,
                ..Default::default()
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert!(matches!(stream.finish().await, Err(Error::Timeout)));
    let dropped = Arc::new(AtomicBool::new(false));
    let provider = Arc::new(Pending { dropped });
    let mut stream = Agent::<(), Answer>::new(provider, "mock")
        .stream_iterable("list", Arc::new(()), RunContext::default())
        .unwrap();
    stream.recv().await.unwrap().unwrap();
    stream.cancel();
    assert!(matches!(stream.collect().await, Err(Error::Cancelled)));
}

#[tokio::test]
async fn exhausted_retries_refusal_and_transport_failures_never_commit() {
    for result in [
        Ok(response(r#"{"value":-1}"#)),
        Err(Error::Provider { status: 503 }),
        Ok(response_items(vec![
            json!({"type":"message","content":[{"type":"refusal","refusal":"no"}]}),
        ])),
    ] {
        let provider = Arc::new(Streaming::new(vec![Plan {
            chunks: vec![],
            response: result,
            gate: None,
        }]));
        let result = Agent::<(), Answer>::new(provider, "mock")
            .limits(AgentLimits {
                max_validation_retries: 0,
                ..Default::default()
            })
            .stream_partial("one", Arc::new(()), RunContext::default())
            .unwrap()
            .finish()
            .await;
        assert!(matches!(
            result,
            Err(Error::Validation(_) | Error::Provider { .. } | Error::Refusal)
        ));
    }
}

#[tokio::test]
async fn streaming_retains_the_native_event_sink() {
    let provider = Arc::new(Streaming::new(vec![Plan::text(r#"{"value":1}"#)]));
    let (sender, mut events) = tokio::sync::mpsc::channel(64);
    let result = Agent::<(), Answer>::new(provider, "mock")
        .stream_partial(
            "one",
            Arc::new(()),
            RunContext::default().with_events(sender),
        )
        .unwrap()
        .finish()
        .await
        .unwrap();
    assert_eq!(result.output.value, 1);
    let mut count = 0;
    while events.recv().await.is_some() {
        count += 1;
    }
    assert!(count > 0);
}

#[test]
fn invalid_options_and_missing_runtime_return_configuration_errors() {
    let agent = Agent::<(), Answer>::new(Arc::new(Scripted::new(vec![])), "mock");
    assert!(matches!(
        agent.stream_partial("one", Arc::new(()), RunContext::default()),
        Err(Error::Configuration(_))
    ));
    assert!(matches!(
        agent.stream_iterable_with_options(
            "list",
            Arc::new(()),
            RunContext::default(),
            StreamOptions {
                capacity: 0,
                ..Default::default()
            }
        ),
        Err(Error::Configuration(_))
    ));
}

#[tokio::test]
async fn every_character_cut_preserves_scalar_and_nested_item_boundaries() {
    #[derive(Debug, Model)]
    struct Any(Value);
    let text = r#" { "items" : [null, true, false, -12, 1e20, 1.234567890123456789, "你好😊\\\"", [], {"items":[1,2]}] } "#;
    let expected: Value = serde_json::from_str(text).unwrap();
    for cut in text
        .char_indices()
        .map(|(index, _)| index)
        .chain([text.len()])
    {
        let plan = Plan {
            chunks: vec![text[..cut].into(), text[cut..].into()],
            response: Ok(response(text)),
            gate: None,
        };
        let result = Agent::<(), Any>::new(Arc::new(Streaming::new(vec![plan])), "mock")
            .stream_iterable("list", Arc::new(()), RunContext::default())
            .unwrap()
            .collect()
            .await
            .unwrap();
        assert_eq!(
            result.output.into_iter().map(|x| x.0).collect::<Vec<_>>(),
            *expected["items"].as_array().unwrap(),
            "cut {cut}"
        );
    }
}

#[tokio::test]
async fn pending_async_item_validation_obeys_the_deadline() {
    async fn pending(_: &String) -> std::result::Result<(), ValidationError> {
        std::future::pending().await
    }
    #[derive(Model)]
    struct Wait {
        #[model(async_validate=pending)]
        name: String,
    }
    let provider = Arc::new(Streaming::new(vec![Plan::text(
        r#"{"items":[{"name":"wait"}]}"#,
    )]));
    let result = Agent::<(), Wait>::new(provider, "mock")
        .stream_iterable(
            "list",
            Arc::new(()),
            RunContext::with_timeout(Duration::from_millis(20)),
        )
        .unwrap()
        .collect()
        .await;
    assert!(matches!(result, Err(Error::Timeout)));
}

#[tokio::test]
async fn streamed_attempts_obey_token_call_and_depth_limits() {
    for limits in [
        AgentLimits {
            max_total_tokens: 1,
            ..Default::default()
        },
        AgentLimits {
            max_model_calls: 1,
            ..Default::default()
        },
    ] {
        let provider = Arc::new(Streaming::new(vec![Plan::text(r#"{"value":-1}"#)]));
        assert!(matches!(
            Agent::<(), Answer>::new(provider, "mock")
                .limits(limits)
                .stream_partial("one", Arc::new(()), RunContext::default())
                .unwrap()
                .finish()
                .await,
            Err(Error::Limit(_))
        ));
    }
    let text = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    let provider = Arc::new(Streaming::new(vec![Plan::text(&text)]));
    assert!(matches!(
        Agent::<(), Answer>::new(provider, "mock")
            .stream_partial("one", Arc::new(()), RunContext::default())
            .unwrap()
            .finish()
            .await,
        Err(Error::Limit(_))
    ));
}
