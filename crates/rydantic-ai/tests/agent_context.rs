mod common;
use common::*;
use rydantic_ai::{
    rydantic::{AsyncCheck, Model, ValidationError},
    Agent, AgentLimits, Error, Result, RunContext, RunResult,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

async fn final_output<T>(
    agent: &Agent<(), T>,
    partial: bool,
    context: RunContext,
) -> Result<RunResult<T>>
where
    T: Model + AsyncCheck + Send + Sync + 'static,
{
    if partial {
        agent
            .stream_partial("extract", Arc::new(()), context)?
            .finish()
            .await
    } else {
        agent.run("extract", Arc::new(()), context).await
    }
}

#[tokio::test]
async fn ready_final_validator_cancellation_is_not_accepted() {
    static TOKEN: Mutex<Option<CancellationToken>> = Mutex::new(None);
    async fn cancel(_: &u8) -> std::result::Result<(), ValidationError> {
        TOKEN.lock().unwrap().as_ref().unwrap().cancel();
        Ok(())
    }
    #[derive(Debug, Model)]
    struct Output {
        #[model(async_validate = cancel)]
        value: u8,
    }
    let mut rejected = Vec::new();
    for partial in [false, true] {
        let context = RunContext::default();
        *TOKEN.lock().unwrap() = Some(context.cancellation.clone());
        let provider = Arc::new(Scripted::new(vec![Ok(response(r#"{"value":3}"#))]));
        let agent = Agent::<(), Output>::new(provider.clone(), "mock");
        let result = final_output(&agent, partial, context.clone()).await;
        assert!(matches!(context.check(), Err(Error::Cancelled)));
        assert_eq!(provider.requests.lock().unwrap().len(), 1);
        rejected.push(matches!(result, Err(Error::Cancelled)));
    }
    assert_eq!(rejected, [true, true], "ordinary and partial Agent results");
}

#[tokio::test]
async fn synchronous_final_validator_deadline_overrun_is_not_accepted() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn slow(_: &u8) -> std::result::Result<(), ValidationError> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(300));
        Ok(())
    }
    #[derive(Debug, Model)]
    struct Output {
        #[model(validate = slow)]
        value: u8,
    }
    let mut rejected = Vec::new();
    for partial in [false, true] {
        CALLS.store(0, Ordering::SeqCst);
        let context = RunContext::with_timeout(Duration::from_millis(200));
        let provider = Arc::new(Scripted::new(vec![Ok(response(r#"{"value":3}"#))]));
        let agent = Agent::<(), Output>::new(provider.clone(), "mock");
        let result = final_output(&agent, partial, context.clone()).await;
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        assert!(matches!(context.check(), Err(Error::Timeout)));
        assert_eq!(provider.requests.lock().unwrap().len(), 1);
        rejected.push(matches!(result, Err(Error::Timeout)));
    }
    assert_eq!(rejected, [true, true], "ordinary and partial Agent results");
}

#[derive(Debug, Model)]
struct Checked {
    #[model(ge = 0)]
    value: i64,
}

#[tokio::test]
async fn active_final_validation_preserves_output_and_usage() {
    for partial in [false, true] {
        let context = RunContext::default();
        let provider = Arc::new(Scripted::new(vec![Ok(response(r#"{"value":3}"#))]));
        let agent = Agent::<(), Checked>::new(provider.clone(), "mock");
        let result = final_output(&agent, partial, context.clone())
            .await
            .unwrap();
        assert_eq!(result.output.value, 3);
        assert_eq!(result.model_calls, 1);
        assert_eq!(result.tool_calls, 0);
        assert_eq!(result.usage.total().unwrap(), 5);
        context.check().unwrap();
    }
}

#[tokio::test]
async fn active_final_validation_errors_remain_validation_errors() {
    for partial in [false, true] {
        let context = RunContext::default();
        let provider = Arc::new(Scripted::new(vec![Ok(response(r#"{"value":-1}"#))]));
        let agent = Agent::<(), Checked>::new(provider.clone(), "mock").limits(AgentLimits {
            max_validation_retries: 0,
            ..Default::default()
        });
        assert!(matches!(
            final_output(&agent, partial, context.clone()).await,
            Err(Error::Validation(_))
        ));
        assert_eq!(provider.requests.lock().unwrap().len(), 1);
        context.check().unwrap();
    }
}
