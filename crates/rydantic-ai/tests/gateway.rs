mod common;
use common::*;
use rydantic_ai::{
    gateway::{Budget, ClientPolicy, Gateway, Price, Route},
    BoxFuture, Error, ModelProvider, ModelRequest, ModelResponse, Result, RunContext, Usage,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Notify;

const KEY: &str = "private-test-key-123456";
fn budget() -> Budget {
    Budget {
        max_requests: 100,
        max_tokens: 1000,
        max_cost_microusd: None,
        reservation_tokens: 10,
    }
}
fn gateway(routes: Vec<Route>, global: Budget) -> Gateway {
    Gateway::new(
        BTreeMap::from([("chat".into(), routes)]),
        vec![ClientPolicy {
            id: "app".into(),
            key: KEY.into(),
            models: HashSet::from(["chat".into()]),
            budget: budget(),
        }],
        global,
    )
    .unwrap()
}
fn route(provider: Arc<dyn ModelProvider>) -> Route {
    Route {
        provider,
        model: "upstream".into(),
        price: None,
    }
}

#[tokio::test]
async fn transient_failover_is_metered_and_unauthorized_routes_are_blocked() {
    let first = Arc::new(Scripted::new(vec![Err(Error::Provider { status: 503 })]));
    let second = Arc::new(Scripted::new(vec![Ok(response("hello"))]));
    let gateway = gateway(vec![route(first.clone()), route(second.clone())], budget());
    assert!(matches!(
        gateway.client("invalid"),
        Err(Error::Unauthorized)
    ));
    let client = gateway.client(KEY).unwrap();
    assert!(matches!(
        client
            .complete(ModelRequest::new("secret", "x"), RunContext::default())
            .await,
        Err(Error::UnknownRoute)
    ));
    client
        .complete(ModelRequest::new("chat", "x"), RunContext::default())
        .await
        .unwrap();
    assert_eq!(first.requests.lock().unwrap()[0].model, "upstream");
    assert_eq!(second.requests.lock().unwrap().len(), 1);
    let snapshot = gateway.snapshot("app").unwrap();
    assert_eq!(
        (snapshot.requests, snapshot.tokens, snapshot.reserved_tokens),
        (2, 15, 0)
    );
    assert_eq!(gateway.global_snapshot().unwrap().tokens, 15);
}

struct Partial;
impl ModelProvider for Partial {
    fn complete(
        &self,
        _: ModelRequest,
        context: RunContext,
    ) -> BoxFuture<'_, Result<ModelResponse>> {
        Box::pin(async move {
            context
                .emit(json!({"type":"response.output_text.delta","delta":"started"}))
                .await?;
            Err(Error::Provider { status: 503 })
        })
    }
}
#[tokio::test]
async fn partial_streams_and_permanent_errors_never_fail_over() {
    let unused = Arc::new(Scripted::new(vec![]));
    let gateway = gateway(
        vec![route(Arc::new(Partial)), route(unused.clone())],
        budget(),
    );
    assert!(gateway
        .client(KEY)
        .unwrap()
        .complete(ModelRequest::new("chat", "x"), RunContext::default())
        .await
        .is_err());
    assert!(unused.requests.lock().unwrap().is_empty());
    let permanent = Arc::new(Scripted::new(vec![Err(Error::Provider { status: 401 })]));
    let gateway = self::gateway(vec![route(permanent), route(unused.clone())], budget());
    assert!(gateway
        .client(KEY)
        .unwrap()
        .complete(ModelRequest::new("chat", "x"), RunContext::default())
        .await
        .is_err());
    assert!(unused.requests.lock().unwrap().is_empty());
}

struct Waiting {
    started: Arc<Notify>,
}
impl ModelProvider for Waiting {
    fn complete(&self, _: ModelRequest, _: RunContext) -> BoxFuture<'_, Result<ModelResponse>> {
        Box::pin(async {
            self.started.notify_one();
            std::future::pending().await
        })
    }
}
#[tokio::test]
async fn global_admission_is_atomic_and_cancellation_retains_a_charge() {
    let started = Arc::new(Notify::new());
    let global = Budget {
        max_requests: 1,
        ..budget()
    };
    let gateway = gateway(
        vec![route(Arc::new(Waiting {
            started: started.clone(),
        }))],
        global,
    );
    let client = gateway.client(KEY).unwrap();
    let context = RunContext::with_timeout(Duration::from_secs(2));
    let cancellation = context.cancellation.clone();
    let pending = tokio::spawn(async move {
        client
            .complete(ModelRequest::new("chat", "one"), context)
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), started.notified())
        .await
        .unwrap();
    assert_eq!(gateway.snapshot("app").unwrap().reserved_tokens, 10);
    assert!(matches!(
        gateway
            .client(KEY)
            .unwrap()
            .complete(ModelRequest::new("chat", "two"), RunContext::default())
            .await,
        Err(Error::Budget)
    ));
    cancellation.cancel();
    assert!(matches!(pending.await.unwrap(), Err(Error::Cancelled)));
    let snapshot = gateway.snapshot("app").unwrap();
    assert_eq!(
        (snapshot.tokens, snapshot.reserved_tokens, snapshot.requests),
        (10, 0, 1)
    );
    assert_eq!(gateway.global_snapshot().unwrap().tokens, 10);
}

#[tokio::test]
async fn actual_usage_reconciles_reservations_and_overdraw_blocks_later_calls() {
    let provider = Arc::new(Scripted::new(vec![
        Ok(response("hello")),
        Ok(response("world")),
    ]));
    let global = Budget {
        max_tokens: 14,
        ..budget()
    };
    let gateway = gateway(vec![route(provider.clone())], global);
    gateway
        .client(KEY)
        .unwrap()
        .complete(ModelRequest::new("chat", "x"), RunContext::default())
        .await
        .unwrap();
    assert_eq!(gateway.global_snapshot().unwrap().tokens, 5);
    assert!(matches!(
        gateway
            .client(KEY)
            .unwrap()
            .complete(ModelRequest::new("chat", "x"), RunContext::default())
            .await,
        Err(Error::Budget)
    ));
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
}

#[test]
fn price_math_is_integer_checked_and_missing_prices_reject_cost_limits() {
    assert_eq!(
        Price {
            input_microusd_per_million: 1,
            output_microusd_per_million: 1
        }
        .cost(Usage {
            input_tokens: 1,
            output_tokens: 1
        })
        .unwrap(),
        1
    );
    assert!(Price {
        input_microusd_per_million: u64::MAX,
        output_microusd_per_million: u64::MAX
    }
    .cost(Usage {
        input_tokens: u64::MAX,
        output_tokens: u64::MAX
    })
    .is_err());
    let config = Gateway::new(
        BTreeMap::from([("chat".into(), vec![route(Arc::new(Scripted::new(vec![])))])]),
        vec![ClientPolicy {
            id: "app".into(),
            key: KEY.into(),
            models: HashSet::from(["chat".into()]),
            budget: Budget {
                max_cost_microusd: Some(100),
                ..budget()
            },
        }],
        budget(),
    );
    assert!(config.is_err());
}
