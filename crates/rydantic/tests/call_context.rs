use rydantic::{
    json, AsyncCheck, BoxFuture, Context, JsonType, Model, SchemaContext, ValidationError, Value,
};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

struct Policy {
    allowed: String,
    checks: AtomicUsize,
}
fn policy(allowed: &str) -> Arc<Policy> {
    Arc::new(Policy {
        allowed: allowed.into(),
        checks: AtomicUsize::new(0),
    })
}
#[derive(Debug)]
struct Scoped(String);
impl Scoped {
    fn check(&self, ctx: &Context) -> Result<(), ValidationError> {
        let p = ctx.require_data::<Policy>()?;
        p.checks.fetch_add(1, Ordering::Relaxed);
        if self.0 == p.allowed {
            Ok(())
        } else {
            Err(ValidationError::new(
                "not_allowed",
                "not allowed by this request",
            ))
        }
    }
}
impl JsonType for Scoped {
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let value = Self(String::decode(value, ctx, coerce)?);
        value.validate_into(ctx);
        Some(value)
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        // Also prove the encoding phase carries the same data.
        if let Err(e) = ctx.require_data::<Policy>() {
            ctx.push(e);
        }
        self.0.encode(ctx)
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        String::schema(ctx, coerce)
    }
    fn validate_into(&self, ctx: &mut Context) {
        if let Err(e) = self.check(ctx) {
            ctx.push(e);
        }
    }
}
impl AsyncCheck for Scoped {}
#[derive(Debug, Model)]
struct Nested {
    values: BTreeMap<String, Vec<Option<Box<Scoped>>>>,
}
fn input(value: &str) -> Value {
    json!({"values":{"tenant":[value]}})
}

#[test]
fn context_propagates_through_nested_containers_and_all_sync_entrypoints() {
    let p = policy("ok");
    let text = input("ok").to_string();
    let raw = serde_json::value::RawValue::from_string(text.clone()).unwrap();
    let models = [
        Nested::parse_json_with_context(&text, p.clone()).unwrap(),
        Nested::parse_slice_with_context(text.as_bytes(), p.clone()).unwrap(),
        Nested::parse_reader_with_context(text.as_bytes(), p.clone()).unwrap(),
        Nested::parse_raw_with_context(&raw, p.clone()).unwrap(),
        Nested::parse_value_with_context(input("ok"), p.clone()).unwrap(),
    ];
    for model in models {
        model.validate_with_context(p.clone()).unwrap();
        assert_eq!(model.dump_with_context(p.clone()).unwrap(), input("ok"));
        assert_eq!(model.dump_json_with_context(p.clone()).unwrap(), text);
    }
    assert_eq!(p.checks.load(Ordering::Relaxed), 20);
}
#[test]
fn missing_wrong_context_and_errors_keep_precise_paths() {
    for (result, code) in [
        (Nested::parse_value(input("ok")), "context_missing"),
        (
            Nested::parse_value_with_context(input("ok"), Arc::new(42u32)),
            "context_type_mismatch",
        ),
        (
            Nested::parse_value_with_context(input("bad"), policy("ok")),
            "not_allowed",
        ),
    ] {
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.errors()[0].code, code);
        assert_eq!(errors.errors()[0].pointer(), "/values/tenant/0");
    }
}
#[test]
fn context_debug_redacts_data_and_child_shares_identity() {
    let p = policy("SECRET");
    let ctx = Context::with_data(p.clone());
    let child = ctx.child(Some("nested".into()));
    assert!(std::ptr::eq(
        ctx.require_data::<Policy>().unwrap(),
        child.require_data::<Policy>().unwrap()
    ));
    assert!(!format!("{ctx:?}").contains("SECRET"));
    assert!(Context::new().require_data::<Policy>().is_err());
}

#[derive(Debug)]
struct AsyncScoped(Scoped);
impl JsonType for AsyncScoped {
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        Scoped::decode(value, ctx, coerce).map(Self)
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        self.0.encode(ctx)
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        Scoped::schema(ctx, coerce)
    }
    fn validate_into(&self, ctx: &mut Context) {
        self.0.validate_into(ctx)
    }
    fn async_required(_: &mut std::collections::HashSet<&'static str>) -> bool {
        true
    }
}
impl AsyncCheck for AsyncScoped {
    fn check_async<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            tokio::task::yield_now().await;
            if self.0 .0 == "deny_async" {
                ctx.push(ValidationError::new(
                    "async_denied",
                    "async policy rejected this value",
                ));
            }
            if let Err(e) = self.0.check(ctx) {
                ctx.push(e);
            }
        })
    }
}
#[derive(Debug, Model)]
struct AsyncNested {
    values: BTreeMap<String, Vec<Option<Box<AsyncScoped>>>>,
}
#[tokio::test]
async fn async_phases_keep_context_and_concurrent_requests_are_isolated() {
    let a = policy("a");
    let b = policy("b");
    let spawn = |p: Arc<Policy>, text: &'static str| {
        tokio::spawn(async move {
            let text = input(text).to_string();
            let raw = serde_json::value::RawValue::from_string(text.clone()).unwrap();
            let model = AsyncNested::parse_json_async_with_context(&text, p.clone())
                .await
                .unwrap();
            AsyncNested::parse_raw_async_with_context(&raw, p.clone())
                .await
                .unwrap();
            AsyncNested::parse_value_async_with_context(
                serde_json::from_str(&text).unwrap(),
                p.clone(),
            )
            .await
            .unwrap();
            model.validate_async_with_context(p.clone()).await.unwrap();
            assert_eq!(
                model
                    .dump_async_with_context(p.clone())
                    .await
                    .unwrap()
                    .to_string(),
                text
            );
            assert_eq!(
                model.dump_json_async_with_context(p.clone()).await.unwrap(),
                text
            );
            assert_eq!(p.checks.load(Ordering::Relaxed), 12);
        })
    };
    let (x, y) = tokio::join!(spawn(a, "a"), spawn(b, "b"));
    x.unwrap();
    y.unwrap();
}
#[tokio::test]
async fn sync_guard_is_not_bypassed_and_sync_failure_skips_async() {
    let p = policy("ok");
    let errors = AsyncNested::parse_value_with_context(input("ok"), p.clone()).unwrap_err();
    assert_eq!(errors.errors()[0].code, "async_validation_required");
    assert_eq!(p.checks.load(Ordering::Relaxed), 0);
    let errors = AsyncNested::parse_value_async_with_context(input("bad"), p.clone())
        .await
        .unwrap_err();
    assert_eq!(errors.errors()[0].pointer(), "/values/tenant/0");
    assert_eq!(p.checks.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn async_only_failure_keeps_nested_path_and_drops_call_data() {
    let p = policy("deny_async");
    let errors = AsyncNested::parse_value_async_with_context(input("deny_async"), p.clone())
        .await
        .unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors()[0].code, "async_denied");
    assert_eq!(errors.errors()[0].pointer(), "/values/tenant/0");
    assert_eq!(p.checks.load(Ordering::Relaxed), 2);
    assert_eq!(Arc::strong_count(&p), 1);
}

#[tokio::test]
async fn cancelled_future_releases_its_business_context() {
    let p = policy("ok");
    let mut future = Box::pin(AsyncNested::parse_value_async_with_context(
        input("ok"),
        p.clone(),
    ));
    // Poll until the async callback yields, after sync decoding already succeeded.
    std::future::poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert_eq!(p.checks.load(Ordering::Relaxed), 1);
    drop(future);
    assert_eq!(Arc::strong_count(&p), 1);
    AsyncNested::parse_value_async_with_context(input("ok"), p.clone())
        .await
        .unwrap();
    assert_eq!(p.checks.load(Ordering::Relaxed), 3);
}
