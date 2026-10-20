mod common;
use common::*;
use rydantic_ai::{rydantic::Model, Agent, AgentLimits, Error, RunContext, TypedTool};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

#[derive(Debug, Model)]
struct Answer {
    #[model(ge = 0)]
    value: i64,
}
#[derive(Model)]
struct Add {
    a: i64,
    b: i64,
}

#[tokio::test]
async fn invalid_final_output_is_corrected_with_bounded_feedback() {
    let provider = Arc::new(Scripted::new(vec![
        Ok(response(r#"{"value":-1}"#)),
        Ok(response(r#"{"value":42}"#)),
    ]));
    let result = Agent::<(), Answer>::new(provider.clone(), "mock")
        .run("compute", Arc::new(()), RunContext::default())
        .await
        .unwrap();
    assert_eq!(result.output.value, 42);
    assert_eq!(result.model_calls, 2);
    assert_eq!(result.usage.total().unwrap(), 10);
    let requests = provider.requests.lock().unwrap();
    assert!(requests[1].input.last().unwrap()["content"]
        .as_str()
        .unwrap()
        .contains("Validation errors"));
    assert!(requests[0].max_output_tokens.is_none());
    assert!(requests[0].text.is_none());
}

#[tokio::test]
async fn tool_arguments_validate_before_execution_and_reasoning_survives() {
    let provider = Arc::new(Scripted::new(vec![
        Ok(response_items(vec![
            json!({"type":"reasoning","encrypted_content":"opaque"}),
            call("bad", r#"{"a":"bad","b":2}"#),
        ])),
        Ok(response_items(vec![call("good", r#"{"a":19,"b":23}"#)])),
        Ok(response(r#"{"value":42}"#)),
    ]));
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    let tool = TypedTool::<(), Add, Answer, _>::new("add", "Add", move |args: Add, _, _| {
        handler_calls.fetch_add(1, Ordering::SeqCst);
        async move {
            Ok(Answer {
                value: args.a + args.b,
            })
        }
    })
    .unwrap();
    let result = Agent::<(), Answer>::new(provider.clone(), "mock")
        .tool(tool)
        .unwrap()
        .run("compute", Arc::new(()), RunContext::default())
        .await
        .unwrap();
    assert_eq!(result.output.value, 42);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.tool_calls, 2);
    let requests = provider.requests.lock().unwrap();
    assert!(requests[1]
        .input
        .iter()
        .any(|i| i["encrypted_content"] == "opaque"));
    assert!(requests[2]
        .input
        .iter()
        .any(|i| i["type"] == "function_call_output" && i["output"] == r#"{"value":42}"#));
}

#[tokio::test]
async fn handler_errors_are_fatal_even_if_the_error_looks_retryable() {
    let provider = Arc::new(Scripted::new(vec![Ok(response_items(vec![call(
        "one",
        r#"{"a":1,"b":2}"#,
    )]))]));
    let tool = TypedTool::<(), Add, Answer, _>::new("add", "Add", |_: Add, _, _| async {
        Err(Error::Configuration("failure after effect".into()))
    })
    .unwrap();
    let result = Agent::<(), Answer>::new(provider.clone(), "mock")
        .tool(tool)
        .unwrap()
        .run("compute", Arc::new(()), RunContext::default())
        .await;
    assert!(matches!(result, Err(Error::Tool(_))));
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn duplicate_tool_ids_never_execute_twice() {
    let provider = Arc::new(Scripted::new(vec![Ok(response_items(vec![
        call("same", r#"{"a":1,"b":2}"#),
        call("same", r#"{"a":1,"b":2}"#),
    ]))]));
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let tool = TypedTool::<(), Add, Answer, _>::new("add", "Add", move |_: Add, _, _| {
        counted.fetch_add(1, Ordering::SeqCst);
        async { Ok(Answer { value: 3 }) }
    })
    .unwrap();
    assert!(matches!(
        Agent::<(), Answer>::new(provider, "mock")
            .tool(tool)
            .unwrap()
            .run("x", Arc::new(()), RunContext::default())
            .await,
        Err(Error::Protocol(_))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_and_limits_stop_work() {
    let provider = Arc::new(Scripted::new(vec![]));
    let context = RunContext::default();
    context.cancellation.cancel();
    assert!(matches!(
        Agent::<(), Answer>::new(provider.clone(), "mock")
            .run("x", Arc::new(()), context)
            .await,
        Err(Error::Cancelled)
    ));
    assert!(provider.requests.lock().unwrap().is_empty());
    let provider = Arc::new(Scripted::new(vec![Ok(response(r#"{"value":42}"#))]));
    let limits = AgentLimits {
        max_total_tokens: 4,
        ..AgentLimits::default()
    };
    assert!(matches!(
        Agent::<(), Answer>::new(provider, "mock")
            .limits(limits)
            .run("x", Arc::new(()), RunContext::default())
            .await,
        Err(Error::Limit(_))
    ));
}

#[tokio::test]
async fn tool_execution_obeys_the_run_deadline() {
    let provider = Arc::new(Scripted::new(vec![Ok(response_items(vec![call(
        "one",
        r#"{"a":1,"b":2}"#,
    )]))]));
    let tool = TypedTool::<(), Add, Answer, _>::new("add", "Add", |_: Add, _, _| async {
        std::future::pending().await
    })
    .unwrap();
    assert!(matches!(
        Agent::<(), Answer>::new(provider, "mock")
            .tool(tool)
            .unwrap()
            .run(
                "x",
                Arc::new(()),
                RunContext::with_timeout(Duration::from_millis(30))
            )
            .await,
        Err(Error::Timeout)
    ));
}

#[test]
fn incomplete_refused_and_missing_usage_responses_fail_explicitly() {
    assert!(
        rydantic_ai::ModelResponse::from_value(json!({"status":"incomplete","output":[]})).is_err()
    );
    assert!(
        rydantic_ai::ModelResponse::from_value(json!({"status":"completed","output":[]})).is_err()
    );
    let refused = response_items(vec![
        json!({"type":"message","content":[{"type":"refusal","refusal":"no"}]}),
    ]);
    assert!(matches!(refused.text(), Err(Error::Refusal)));
}
