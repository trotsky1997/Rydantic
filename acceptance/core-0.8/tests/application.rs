use rydantic::{Model, Value};
use rydantic_core_080_acceptance::{App, Order, Policy};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn request(addr: std::net::SocketAddr, tenant: &str, body: &str) -> (u16, Value) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!("POST /orders HTTP/1.1\r\nHost: localhost\r\nX-Tenant: {tenant}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut bytes = Vec::new(); stream.read_to_end(&mut bytes).await.unwrap();
        let response = String::from_utf8(bytes).unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, serde_json::from_str(body).unwrap())
    }).await.expect("bounded loopback HTTP request")
}

#[tokio::test]
async fn synthetic_order_service_acceptance() {
    let app = App::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = app.router();
    let server = tokio::spawn(async move {
        rydantic::axum::framework::serve(listener, router)
            .await
            .unwrap()
    });
    let (status, value) = request(
        addr,
        "small",
        r#"{"name":"  ORDER: FIRST Order  ","amount":8}"#,
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(value["slug"], "first-order");
    #[cfg(feature = "openapi-acceptance")]
    assert_actual_response_schema(&value);
    assert_eq!(value["order"]["name"], "ORDER: FIRST Order");
    // Validation check errors accumulate; both policies and typed checks run.
    let (status, value) = request(addr, "small", r#"{"name":"Bad","amount":13}"#).await;
    assert_eq!(status, 422);
    let codes = value["codes"].as_array().unwrap();
    assert!(codes.contains(&Value::from("policy_limit")));
    assert!(codes.contains(&Value::from("even_amount")));
    let (small, large) = tokio::join!(
        request(addr, "small", r#"{"name":"Isolated","amount":20}"#),
        request(addr, "large", r#"{"name":"Isolated","amount":20}"#)
    );
    assert_eq!(small.0, 422);
    assert_eq!(large.0, 201);
    assert_eq!(large.1["tenant"], "large");
    assert_eq!(
        request(addr, "missing", r#"{"name":"x","amount":2}"#)
            .await
            .0,
        401
    );
    assert_eq!(request(addr, "small", "{").await.0, 422);
    let rejected = request(addr, "small", r#"{"name":"Reserved","amount":8}"#).await;
    assert_eq!(rejected.0, 422);
    assert_eq!(rejected.1["error"], "slug_policy");
    // Audit insert precedes duplicate insert: rollback must erase that intermediate write.
    assert_eq!(
        request(addr, "small", r#"{"name":"FIRST Order","amount":8}"#)
            .await
            .0,
        409
    );
    let orders: (i64,) = rydantic::sqlx::query_as("SELECT count(*) FROM orders")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let audit: (i64,) = rydantic::sqlx::query_as("SELECT count(*) FROM audit")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(orders.0, 2);
    assert_eq!(audit.0, 2);
    let rows: Vec<(String, String, i64)> =
        rydantic::sqlx::query_as("SELECT tenant,slug,amount FROM orders ORDER BY tenant")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            ("large".into(), "isolated".into(), 20),
            ("small".into(), "first-order".into(), 8)
        ]
    );
    server.abort();
    let _ = server.await;
    app.pool.close().await;
}

#[test]
fn explicit_context_ownership_and_repeated_lifecycle() {
    let policy = Arc::new(Policy::new("small", 10));
    let source = r#"{"name":"  Keeps Case  ","amount":8}"#;
    let order = Order::parse_json_with_context(source, policy.clone()).unwrap();
    assert_eq!(
        Arc::strong_count(&policy),
        1,
        "model must not retain request context"
    );
    for _ in 0..3 {
        order.validate_with_context(policy.clone()).unwrap();
        assert_eq!(
            order.dump_with_context(policy.clone()).unwrap()["name"],
            "Keeps Case"
        );
        assert_eq!(Arc::strong_count(&policy), 1);
    }
    assert_eq!(
        policy.transforms.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "validate/dump must not rerun construction callback"
    );
    assert!(
        Order::parse_json(source)
            .unwrap_err()
            .errors()
            .iter()
            .any(|e| e.code == "context_missing")
    );
    assert!(
        Order::parse_json_with_context(source, Arc::new(42_u32))
            .unwrap_err()
            .errors()
            .iter()
            .any(|e| e.code == "context_type_mismatch")
    );
    assert!(
        order
            .validate_with_context(Arc::new(Policy::new("restricted", 2)))
            .is_err()
    );
    assert!(
        order
            .dump_with_context(Arc::new(Policy::new("restricted", 2)))
            .is_err()
    );
}

#[test]
fn pipeline_rejection_skips_later_stage() {
    let policy = Arc::new(Policy::new("small", 10));
    let order = Order::parse_json_with_context(r#"{"name":"Reserved","amount":8}"#, policy.clone())
        .unwrap();
    assert_eq!(
        rydantic_core_080_acceptance::prepare(order, &policy).unwrap_err(),
        "slug_policy"
    );
    assert_eq!(policy.prepared.load(std::sync::atomic::Ordering::SeqCst), 0);
    let order = Order::parse_json_with_context(
        r#"{"name":"ORDER: Mixed Case","amount":8}"#,
        policy.clone(),
    )
    .unwrap();
    assert_eq!(
        rydantic_core_080_acceptance::prepare(order, &policy)
            .unwrap()
            .slug,
        "mixed-case"
    );
    assert_eq!(policy.prepared.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[cfg(feature = "openapi-acceptance")]
fn assert_actual_response_schema(response: &Value) {
    use rydantic_core_080_acceptance::{OrderResponse, application_openapi};
    let doc = application_openapi();
    fn resolve<'a>(doc: &'a Value, mut schema: &'a Value) -> &'a Value {
        for _ in 0..10 {
            let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
                return schema;
            };
            // URI fragment percent encoding is distinct from JSON Pointer escaping.
            let fragment = reference.strip_prefix('#').expect("local reference");
            let mut bytes = Vec::new();
            let mut chars = fragment.bytes();
            while let Some(byte) = chars.next() {
                if byte == b'%' {
                    let hex = [chars.next().unwrap(), chars.next().unwrap()];
                    bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).unwrap(), 16).unwrap());
                } else {
                    bytes.push(byte);
                }
            }
            let pointer = String::from_utf8(bytes).unwrap();
            schema = doc.pointer(&pointer).expect("resolvable component");
        }
        panic!("unexpected reference cycle");
    }
    let output = resolve(
        &doc,
        &doc["paths"]["/orders"]["post"]["responses"]["201"]["content"]["application/json"]["schema"],
    );
    let properties = output["properties"].as_object().unwrap();
    assert_eq!(
        properties.keys().collect::<Vec<_>>(),
        response.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    assert_eq!(properties.len(), 3);
    let nested = resolve(&doc, &properties["order"]);
    assert_eq!(
        nested["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        response["order"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>()
    );
    let input = resolve(
        &doc,
        &doc["paths"]["/orders"]["post"]["requestBody"]["content"]["application/json"]["schema"],
    );
    assert_eq!(input["properties"].as_object().unwrap().len(), 2);
    assert!(input["properties"].get("amount").is_some());
    assert!(input["properties"].get("name").is_some());
    let typed = OrderResponse::parse_value_with_context(
        response.clone(),
        Arc::new(Policy::new("small", 10)),
    )
    .unwrap();
    assert_eq!(
        typed
            .dump_with_context(Arc::new(Policy::new("small", 10)))
            .unwrap(),
        *response
    );
}
