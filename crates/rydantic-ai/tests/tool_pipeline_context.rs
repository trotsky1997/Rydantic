use rydantic_ai::{
    rydantic::{json, pipeline::AsyncPipeline, Context, Model, ValidationError},
    Error, RunContext, Tool, TypedTool,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::Poll,
};

struct Policy {
    suspend: AtomicBool,
    effects: AtomicUsize,
    continuations: AtomicUsize,
}
async fn business_check(_: &u32, context: &Context) -> Result<(), ValidationError> {
    let policy = context.require_data::<Policy>()?;
    policy.effects.fetch_add(1, Ordering::SeqCst);
    if policy.suspend.load(Ordering::SeqCst) {
        std::future::pending::<()>().await;
    }
    Ok(())
}
#[derive(Model)]
struct Arguments {
    value: u32,
}
#[derive(Model)]
struct Checked {
    #[model(async_validate_with_context = business_check)]
    value: u32,
}
#[derive(Model)]
struct Business {
    children: Vec<Checked>,
}
#[derive(Model)]
struct Output {
    value: u32,
}

#[tokio::test]
async fn typed_tool_cancellation_drops_explicit_context_pipeline_without_rollback() {
    // TypedTool's argument/output validation is context-free. The handler opts
    // into business context explicitly; RunContext owns cancellation policy.
    let tool = TypedTool::<Policy, Arguments, Output, _>::new(
        "checked",
        "Synthetic context pipeline",
        |args: Arguments, policy, _| async move {
            let pipeline = AsyncPipeline::plain(|args: Arguments, policy: &Arc<Policy>| {
                Box::pin(async move {
                    Business::parse_value_async_with_context(
                        json!({"children":[{"value":args.value}]}),
                        policy.clone(),
                    )
                    .await
                    .map_err(Error::Validation)
                })
            })
            .then(|business, policy| {
                policy.continuations.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    Ok(Output {
                        value: business.children[0].value,
                    })
                })
            });
            pipeline.run(args, &policy).await
        },
    )
    .unwrap();
    let policy = Arc::new(Policy {
        suspend: AtomicBool::new(true),
        effects: AtomicUsize::new(0),
        continuations: AtomicUsize::new(0),
    });
    let context = RunContext::default();
    let mut call = tool.call(r#"{"value":7}"#.into(), policy.clone(), context.clone());
    std::future::poll_fn(|cx| {
        assert!(call.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(policy.effects.load(Ordering::SeqCst), 1);
    assert_eq!(policy.continuations.load(Ordering::SeqCst), 0);
    context.cancellation.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), call)
        .await
        .expect("cancellation must not leave the handler pending");
    assert!(matches!(result, Err(Error::Cancelled)));
    // Cancellation drops the active validation future but cannot undo its effect.
    assert_eq!(policy.effects.load(Ordering::SeqCst), 1);
    assert_eq!(policy.continuations.load(Ordering::SeqCst), 0);
    assert_eq!(Arc::strong_count(&policy), 1);

    // A later call is independent and may complete with the same caller-owned data.
    policy.suspend.store(false, Ordering::SeqCst);
    let output = tool
        .call(
            r#"{"value":9}"#.into(),
            policy.clone(),
            RunContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(output, r#"{"value":9}"#);
    assert_eq!(policy.effects.load(Ordering::SeqCst), 2);
    assert_eq!(policy.continuations.load(Ordering::SeqCst), 1);
    assert_eq!(Arc::strong_count(&policy), 1);
}
