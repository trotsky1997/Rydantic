use rydantic::{
    json,
    pipeline::{AsyncPipeline, Pipeline},
    Model, ValidationError, ValidationErrors, Value,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Waker},
};
fn err() -> ValidationError {
    ValidationError::new("policy", "rejected")
}

#[test]
fn typed_transforms_checks_context_and_short_circuit() {
    let count = Arc::new(AtomicUsize::new(0));
    let later = count.clone();
    let pipeline = Pipeline::plain(|s: &str, _: &usize| s.parse::<usize>().map_err(|_| err()))
        .check(|n, limit| if n <= limit { Ok(()) } else { Err(err()) })
        .then(move |n, _: &usize| {
            later.fetch_add(1, Ordering::SeqCst);
            Ok(n.to_string())
        });
    assert_eq!(pipeline.run("4", &5).unwrap(), "4");
    assert!(pipeline.run("6", &5).is_err());
    assert!(pipeline.run("bad", &5).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
#[test]
fn sync_pipeline_can_borrow_configuration() {
    let prefix = String::from("hello");
    let pipeline =
        Pipeline::plain(|s: &str, _: &()| Ok::<_, ValidationError>(format!("{prefix} {s}")));
    assert_eq!(pipeline.run("world", &()).unwrap(), "hello world");
}
#[test]
fn wrap_can_skip_or_retry_and_does_not_hide_side_effects() {
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let base = Pipeline::plain(move |n: u32, _: &()| {
        calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Err(err())
        } else {
            Ok(n)
        }
    });
    let skip = base.clone().wrap(|_, _, _| Ok(99));
    assert_eq!(skip.run(0, &()).unwrap(), 99);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let retry = base.wrap(|n, c, next| next(n, c).or_else(|_| next(1, c)));
    assert_eq!(retry.run(0, &()).unwrap(), 1);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}
fn normalize(value: Value) -> Result<Value, ValidationError> {
    Pipeline::plain(|v: Value, _: &()| v.as_str().map(str::to_owned).ok_or_else(err))
        .then(|s, _| s.parse::<u32>().map_err(|_| err()))
        .check(|n, _| if *n > 0 { Ok(()) } else { Err(err()) })
        .then(|n, _| Ok(json!(n)))
        .run(value, &())
}
fn positive(n: &u32) -> Result<(), ValidationError> {
    Pipeline::plain(|n: &u32, _: &()| Ok(n))
        .check(|n, _| if **n > 0 { Ok(()) } else { Err(err()) })
        .then(|_, _| Ok(()))
        .run(n, &())
}
#[derive(Debug, Model)]
struct Field {
    #[model(before = normalize, validate = positive, le = 10)]
    n: u32,
}
#[derive(Debug, Model)]
struct Parent {
    child: Field,
}
#[test]
fn real_field_callbacks_keep_paths_builtins_and_revalidation() {
    assert_eq!(Field::parse_value(json!({"n":"3"})).unwrap().n, 3);
    let errors = Parent::parse_value(json!({"child":{"n":"0"}})).unwrap_err();
    assert_eq!(errors.0[0].pointer(), "/child/n");
    // A plain callback cannot bypass constraints outside its own pipeline.
    assert!(Field::parse_value(json!({"n":"11"})).is_err());
    assert!(Field { n: 0 }.validate().is_err());
}
fn model_check(value: &Record) -> Result<(), ValidationErrors> {
    Pipeline::plain(|v: &Record, _: &()| Ok(v))
        .check(|v, _| {
            if v.low <= v.high {
                Ok(())
            } else {
                Err(err().into())
            }
        })
        .then(|_, _| Ok(()))
        .run(value, &())
}
#[derive(Debug, Model)]
#[model(validate = model_check)]
struct Record {
    low: u32,
    high: u32,
}
#[test]
fn model_callbacks_and_full_model_parse_stage_preserve_error_collection() {
    let pipeline = Pipeline::plain(|v: Value, _: &()| Record::parse_value(v))
        .then(|record, _| Ok(record.high - record.low));
    assert_eq!(pipeline.run(json!({"low":2,"high":5}), &()).unwrap(), 3);
    assert!(pipeline.run(json!({"low":5,"high":2}), &()).is_err());
    assert_eq!(
        pipeline
            .run(json!({"low":"x","high":"x"}), &())
            .unwrap_err()
            .0
            .len(),
        2
    );
}
async fn required(_: &u32) -> Result<(), ValidationError> {
    Ok(())
}
#[derive(Debug, Model)]
struct AsyncRecord {
    #[model(async_validate = required)]
    n: u32,
}
#[test]
fn model_parse_stage_cannot_silently_skip_required_async_validation() {
    let pipeline = Pipeline::plain(|v: Value, _: &()| AsyncRecord::parse_value(v));
    assert_eq!(
        pipeline.run(json!({"n":1}), &()).unwrap_err().0[0].code,
        "async_validation_required"
    );
}
#[tokio::test]
async fn async_pipeline_transforms_checks_and_wraps_with_borrowed_context() {
    let pipeline = AsyncPipeline::plain(|n: u32, limit: &u32| {
        Box::pin(async move {
            if n <= *limit {
                Ok(n)
            } else {
                Err(err())
            }
        })
    })
    .check(|n, _| {
        Box::pin(async move {
            if *n > 0 {
                Ok(())
            } else {
                Err(err())
            }
        })
    })
    .then(|n, _| Box::pin(async move { Ok(n.to_string()) }))
    .wrap(|n, c, next| Box::pin(async move { next.run(n, c).await }));
    assert_eq!(pipeline.run(3, &5).await.unwrap(), "3");
    assert!(pipeline.run(0, &5).await.is_err());
    assert!(pipeline.run(6, &5).await.is_err());
}
#[tokio::test]
async fn async_model_stage_runs_required_checks() {
    let pipeline = AsyncPipeline::plain(|v: Value, _: &()| {
        Box::pin(async move { AsyncRecord::parse_value_async(v).await })
    })
    .then(|v, _| Box::pin(async move { Ok(v.n) }));
    assert_eq!(pipeline.run(json!({"n":7}), &()).await.unwrap(), 7);
}
#[test]
fn dropping_pending_pipeline_never_starts_later_stages() {
    let count = Arc::new(AtomicUsize::new(0));
    let later = count.clone();
    let pipeline = AsyncPipeline::plain(|_: (), _: &()| {
        Box::pin(std::future::pending::<Result<(), ValidationError>>())
    })
    .then(move |_, _| {
        later.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    });
    let mut future = pipeline.run((), &());
    assert!(matches!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    drop(future);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

struct Policy {
    limit: u32,
}
fn contextual_field(value: &u32, context: &rydantic::Context) -> Result<(), ValidationError> {
    let policy = context.require_data::<Policy>()?;
    Pipeline::plain(|n: u32, _: &Policy| Ok(n))
        .check(|n, policy| {
            if *n <= policy.limit {
                Ok(())
            } else {
                Err(err())
            }
        })
        .then(|_, _| Ok(()))
        .run(*value, policy)
}
#[derive(Debug, Model)]
struct ContextualField {
    #[model(validate_with_context = contextual_field)]
    n: u32,
}
#[derive(Debug, Model)]
struct ContextualParent {
    children: Vec<ContextualField>,
}
#[test]
fn pipeline_uses_typed_business_context_in_nested_derived_hook() {
    let policy = Arc::new(Policy { limit: 2 });
    assert!(ContextualParent::parse_value_with_context(
        json!({"children":[{"n":2}]}),
        policy.clone()
    )
    .is_ok());
    let errors = ContextualParent::parse_value_with_context(json!({"children":[{"n":3}]}), policy)
        .unwrap_err();
    assert_eq!(errors.0[0].pointer(), "/children/0/n");
    let missing = ContextualParent::parse_value(json!({"children":[{"n":1}]})).unwrap_err();
    assert_eq!(missing.0[0].code, "context_missing");
    assert_eq!(missing.0[0].pointer(), "/children/0/n");
}

#[tokio::test]
async fn async_wrap_controls_retries_and_failure_skips_later_stage() {
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let base = AsyncPipeline::plain(move |n: u32, _: &()| {
        calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if n == 0 {
                Err(err())
            } else {
                Ok(n)
            }
        })
    });
    let skip = base.clone().wrap(|_, _, _| Box::pin(async { Ok(9) }));
    assert_eq!(skip.run(0, &()).await.unwrap(), 9);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let retry = base.clone().wrap(|n, context, next| {
        Box::pin(async move {
            match next.run(n, context).await {
                Ok(v) => Ok(v),
                Err(_) => next.run(1, context).await,
            }
        })
    });
    assert_eq!(retry.run(0, &()).await.unwrap(), 1);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let stop = base.then(
        |_, _| -> rydantic::BoxFuture<'_, Result<(), ValidationError>> {
            panic!("later stage must not start")
        },
    );
    assert!(stop.run(0, &()).await.is_err());
}

#[test]
fn async_run_is_lazy_even_before_stage_future_construction() {
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let pipeline = AsyncPipeline::plain(move |_: (), _: &()| {
        calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok::<_, ValidationError>(()) })
    });
    let future = pipeline.run((), &());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    drop(future);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}
