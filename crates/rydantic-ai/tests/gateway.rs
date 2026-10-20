mod common;
use common::*;
use rydantic_ai::{
    gateway::{Budget, BudgetSnapshot, ClientPolicy, Gateway, Price, Route},
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
    gateway_with_budgets(routes, budget(), global)
}
fn gateway_with_budgets(routes: Vec<Route>, client: Budget, global: Budget) -> Gateway {
    Gateway::new(
        BTreeMap::from([("chat".into(), routes)]),
        vec![ClientPolicy {
            id: "app".into(),
            key: KEY.into(),
            models: HashSet::from(["chat".into()]),
            budget: client,
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

fn priced_route(provider: Arc<dyn ModelProvider>) -> Route {
    Route {
        price: Some(Price {
            input_microusd_per_million: 1_000_000,
            output_microusd_per_million: 2_000_000,
        }),
        ..route(provider)
    }
}

fn amounts(snapshot: BudgetSnapshot) -> (u64, u64, u64, u64, u64) {
    (
        snapshot.requests,
        snapshot.tokens,
        snapshot.reserved_tokens,
        snapshot.known_cost_microusd,
        snapshot.reserved_cost_microusd,
    )
}

#[tokio::test]
async fn asymmetric_token_reservations_use_each_ledgers_limit() {
    let small = Budget {
        max_tokens: 1000,
        reservation_tokens: 100,
        ..budget()
    };
    let large = Budget::default();
    for (client, global) in [(small.clone(), large.clone()), (large, small)] {
        let provider = Arc::new(Scripted::new(vec![Ok(response("hello"))]));
        let gateway = gateway_with_budgets(vec![route(provider.clone())], client, global);
        gateway
            .client(KEY)
            .unwrap()
            .complete(ModelRequest::new("chat", "x"), RunContext::default())
            .await
            .expect("each ledger has capacity for its own reservation");
        assert_eq!(provider.requests.lock().unwrap().len(), 1);
        assert_eq!(amounts(gateway.snapshot("app").unwrap()), (1, 5, 0, 0, 0));
        assert_eq!(amounts(gateway.global_snapshot().unwrap()), (1, 5, 0, 0, 0));
    }
}

#[tokio::test]
async fn asymmetric_cost_reservations_use_each_ledgers_limit() {
    let small = Budget {
        max_cost_microusd: Some(30),
        ..budget()
    };
    let large = Budget {
        reservation_tokens: 100,
        max_cost_microusd: Some(300),
        ..budget()
    };
    for (client, global) in [(small.clone(), large.clone()), (large, small)] {
        let provider = Arc::new(Scripted::new(vec![Ok(response("hello"))]));
        let gateway = gateway_with_budgets(vec![priced_route(provider)], client, global);
        gateway
            .client(KEY)
            .unwrap()
            .complete(ModelRequest::new("chat", "x"), RunContext::default())
            .await
            .expect("each ledger has cost capacity for its own reservation");
        assert_eq!(amounts(gateway.snapshot("app").unwrap()), (1, 5, 0, 8, 0));
        assert_eq!(amounts(gateway.global_snapshot().unwrap()), (1, 5, 0, 8, 0));
    }
}

#[tokio::test]
async fn asymmetric_pending_reservations_and_cancellation_keep_separate_charges() {
    for (client_tokens, global_tokens) in [(10, 100), (100, 10)] {
        let started = Arc::new(Notify::new());
        let gateway = gateway_with_budgets(
            vec![priced_route(Arc::new(Waiting {
                started: started.clone(),
            }))],
            Budget {
                reservation_tokens: client_tokens,
                ..budget()
            },
            Budget {
                reservation_tokens: global_tokens,
                ..budget()
            },
        );
        let client = gateway.client(KEY).unwrap();
        let context = RunContext::with_timeout(Duration::from_secs(2));
        let cancellation = context.cancellation.clone();
        let pending = tokio::spawn(async move {
            client
                .complete(ModelRequest::new("chat", "x"), context)
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), started.notified())
            .await
            .unwrap();
        assert_eq!(
            amounts(gateway.snapshot("app").unwrap()),
            (1, 0, client_tokens, 0, client_tokens * 3)
        );
        assert_eq!(
            amounts(gateway.global_snapshot().unwrap()),
            (1, 0, global_tokens, 0, global_tokens * 3)
        );
        cancellation.cancel();
        assert!(matches!(pending.await.unwrap(), Err(Error::Cancelled)));
        assert_eq!(
            amounts(gateway.snapshot("app").unwrap()),
            (1, client_tokens, 0, client_tokens * 3, 0)
        );
        assert_eq!(
            amounts(gateway.global_snapshot().unwrap()),
            (1, global_tokens, 0, global_tokens * 3, 0)
        );
    }
}

#[tokio::test]
async fn asymmetric_failover_reconciles_each_attempt_with_its_own_price() {
    let first = Arc::new(Scripted::new(vec![Err(Error::Provider { status: 503 })]));
    let second = Arc::new(Scripted::new(vec![Ok(response("hello"))]));
    let mut second_route = priced_route(second.clone());
    second_route.price = Some(Price {
        input_microusd_per_million: 3_000_000,
        output_microusd_per_million: 4_000_000,
    });
    let gateway = gateway_with_budgets(
        vec![priced_route(first.clone()), second_route],
        budget(),
        Budget {
            reservation_tokens: 100,
            ..budget()
        },
    );
    gateway
        .client(KEY)
        .unwrap()
        .complete(ModelRequest::new("chat", "x"), RunContext::default())
        .await
        .unwrap();
    assert_eq!(first.requests.lock().unwrap().len(), 1);
    assert_eq!(second.requests.lock().unwrap().len(), 1);
    assert_eq!(amounts(gateway.snapshot("app").unwrap()), (2, 15, 0, 48, 0));
    assert_eq!(
        amounts(gateway.global_snapshot().unwrap()),
        (2, 105, 0, 318, 0)
    );
}

#[tokio::test]
async fn asymmetric_rejected_admission_leaves_both_ledgers_unchanged() {
    for client_is_limited in [false, true] {
        let started = Arc::new(Notify::new());
        let gateway = gateway_with_budgets(
            vec![priced_route(Arc::new(Waiting {
                started: started.clone(),
            }))],
            Budget {
                max_tokens: if client_is_limited { 10 } else { 1000 },
                ..budget()
            },
            Budget {
                reservation_tokens: 100,
                max_tokens: if client_is_limited { 1000 } else { 100 },
                ..budget()
            },
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
        let before_client = amounts(gateway.snapshot("app").unwrap());
        let before_global = amounts(gateway.global_snapshot().unwrap());
        assert!(matches!(
            gateway
                .client(KEY)
                .unwrap()
                .complete(ModelRequest::new("chat", "two"), RunContext::default())
                .await,
            Err(Error::Budget)
        ));
        assert_eq!(amounts(gateway.snapshot("app").unwrap()), before_client);
        assert_eq!(amounts(gateway.global_snapshot().unwrap()), before_global);
        cancellation.cancel();
        assert!(matches!(pending.await.unwrap(), Err(Error::Cancelled)));
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
