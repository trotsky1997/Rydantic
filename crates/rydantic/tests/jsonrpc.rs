#![cfg(all(feature = "jsonrpc", feature = "derive"))]
use rydantic::{
    json,
    jsonrpc::{Error, Params, RegistrationError, Router},
    Model, Value,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[derive(Model)]
#[model(extra = "forbid")]
struct Greeting {
    #[model(trim, min_length = 2)]
    name: String,
}
#[derive(Model)]
struct Answer {
    #[model(min_length = 2)]
    text: String,
}
#[derive(Model)]
struct Pair(i32, i32);
#[derive(Model)]
struct Sum(i32);
#[derive(Model)]
struct Empty {}
fn router() -> Router {
    let mut router = Router::new();
    router
        .register("greet", Params::Named, |p: Greeting| {
            Ok(Answer { text: p.name })
        })
        .unwrap();
    router
        .register("sum", Params::Positional, |p: Pair| Ok(Sum(p.0 + p.1)))
        .unwrap();
    router
        .register("empty", Params::Named, |_: Empty| Ok(Sum(0)))
        .unwrap();
    router
}
fn code(reply: Option<Value>) -> i64 {
    reply.unwrap()["error"]["code"].as_i64().unwrap()
}

#[test]
fn named_normalization_and_positional_models() {
    let router = router();
    assert_eq!(
        router.handle_json(
            r#"{"jsonrpc":"2.0","method":"greet","params":{"name":" Alice "},"id":"a"}"#
        ),
        Some(json!({"jsonrpc":"2.0","id":"a","result":{"text":"Alice"}}))
    );
    assert_eq!(
        router.handle_value(json!({"jsonrpc":"2.0","method":"sum","params":[3,4],"id":7})),
        Some(json!({"jsonrpc":"2.0","id":7,"result":7}))
    );
    assert_eq!(
        code(
            router.handle_value(
                json!({"jsonrpc":"2.0","method":"sum","params":{"0":3,"1":4},"id":1})
            )
        ),
        -32602
    );
    assert_eq!(
        code(
            router
                .handle_value(json!({"jsonrpc":"2.0","method":"greet","params":["Alice"],"id":1}))
        ),
        -32602
    );
}
#[test]
fn syntax_and_envelope_are_distinct_and_never_execute() {
    let router = router();
    for input in ["{", "", "null false", "[", "{\"jsonrpc\":NaN}"] {
        assert_eq!(code(router.handle_json(input)), -32700);
    }
    for input in [
        json!(null),
        json!(1),
        json!(true),
        json!("request"),
        json!([]),
        json!({}),
        json!({"jsonrpc":"1.0","method":"greet"}),
        json!({"jsonrpc":"2.0","method":1}),
        json!({"jsonrpc":"2.0","method":"greet","params":null}),
        json!({"jsonrpc":"2.0","method":"greet","params":"bad"}),
        json!({"jsonrpc":"2.0","method":"greet","id":false}),
        json!({"jsonrpc":"2.0","method":"greet","id":[]}),
        json!({"jsonrpc":"2.0","method":"greet","id":{}}),
    ] {
        let response = router.handle_value(input).unwrap();
        assert_eq!(
            response,
            json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}})
        );
    }
}
#[test]
fn exact_ids_including_null_and_arbitrary_precision() {
    let router = router();
    for id in [
        "null",
        "\"abc\"",
        "0",
        "-1",
        "1.25",
        "1844674407370955161600000001",
        "1e1000",
    ] {
        let raw = format!(r#"{{"jsonrpc":"2.0","method":"empty","id":{id}}}"#);
        assert_eq!(
            router.handle_json(&raw).unwrap()["id"],
            serde_json::from_str::<Value>(id).unwrap()
        );
    }
}
#[test]
fn valid_notifications_execute_without_any_response() {
    let count = Arc::new(AtomicUsize::new(0));
    let mut router = router();
    let counter = count.clone();
    router
        .register("count", Params::Named, move |_: Empty| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(Sum(1))
        })
        .unwrap();
    assert_eq!(
        router.handle_value(json!({"jsonrpc":"2.0","method":"count"})),
        None
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    for input in [
        json!({"jsonrpc":"2.0","method":"missing"}),
        json!({"jsonrpc":"2.0","method":"greet","params":{}}),
        json!({"jsonrpc":"2.0","method":"sum","params":{}}),
    ] {
        assert_eq!(router.handle_value(input), None);
    }
    // A malformed would-be notification is not a valid Notification.
    assert_eq!(
        code(router.handle_value(json!({"jsonrpc":"2.0","method":"count","params":false}))),
        -32600
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
#[test]
fn batch_shape_and_notifications() {
    let router = router();
    assert_eq!(
        router.handle_value(
            json!([{ "jsonrpc":"2.0", "method":"empty" }, {"jsonrpc":"2.0","method":"missing"}])
        ),
        None
    );
    assert_eq!(
        router.handle_value(json!([1])),
        Some(
            json!([{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}}])
        )
    );
    let response = router.handle_value(json!([{"jsonrpc":"2.0","method":"empty"}, {"jsonrpc":"2.0","method":"sum","params":[2,3],"id":1}, [], [{"jsonrpc":"2.0","method":"empty","id":9}], {"jsonrpc":"2.0","method":"missing","id":2}])).unwrap();
    let items = response.as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0]["result"], 5);
    assert_eq!(items[1]["error"]["code"], -32600);
    assert_eq!(items[2]["error"]["code"], -32600);
    assert_eq!(items[3]["error"]["code"], -32601);
    assert!(!router.handle_value(json!([])).unwrap().is_array());
}
#[test]
fn missing_params_and_case_sensitive_methods() {
    let router = router();
    assert_eq!(
        router
            .handle_value(json!({"jsonrpc":"2.0","method":"empty","id":1}))
            .unwrap()["result"],
        0
    );
    assert_eq!(
        code(router.handle_value(json!({"jsonrpc":"2.0","method":"sum","id":1}))),
        -32602
    );
    assert_eq!(
        code(router.handle_value(json!({"jsonrpc":"2.0","method":"Empty","id":1}))),
        -32601
    );
    assert_eq!(
        code(router.handle_value(json!({"jsonrpc":"2.0","method":"rpc.hidden","id":1}))),
        -32601
    );
    assert_eq!(
        router
            .handle_value(json!({"jsonrpc":"2.0","method":"empty","id":1,"extension":true}))
            .unwrap()["result"],
        0
    );
}
#[test]
fn param_validation_and_output_validation_are_redacted() {
    let mut router = router();
    router
        .register("broken", Params::Named, |_: Empty| {
            Ok(Answer { text: "x".into() })
        })
        .unwrap();
    for params in [
        json!({"name":"x"}),
        json!({"name":17}),
        json!({"name":"Alice","secret":"raw-secret"}),
    ] {
        assert_eq!(
            router.handle_value(json!({"jsonrpc":"2.0","method":"greet","params":params,"id":1})),
            Some(
                json!({"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Invalid params"}})
            )
        );
    }
    assert_eq!(
        router.handle_value(json!({"jsonrpc":"2.0","method":"broken","id":1})),
        Some(json!({"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"Internal error"}}))
    );
    assert_eq!(
        router.handle_value(json!({"jsonrpc":"2.0","method":"broken"})),
        None
    );
}
#[test]
fn registration_refuses_overwrite_and_reserved_names() {
    let mut router = router();
    assert_eq!(
        router.register("empty", Params::Named, |_: Empty| Ok(Sum(99))),
        Err(RegistrationError::Duplicate)
    );
    assert_eq!(
        router.register("rpc.test", Params::Named, |_: Empty| Ok(Sum(99))),
        Err(RegistrationError::ReservedName)
    );
    assert_eq!(
        router
            .handle_value(json!({"jsonrpc":"2.0","method":"empty","id":1}))
            .unwrap()["result"],
        0
    );
}
#[test]
fn application_errors_and_no_mutually_present_result() {
    let mut router = router();
    router
        .register("denied", Params::Named, |_: Empty| {
            Err::<Sum, _>(Error::application(42, "Denied").unwrap())
        })
        .unwrap();
    let reply = router
        .handle_value(json!({"jsonrpc":"2.0","method":"denied","id":1}))
        .unwrap();
    assert_eq!(
        reply,
        json!({"jsonrpc":"2.0","id":1,"error":{"code":42,"message":"Denied"}})
    );
    assert!(reply.get("result").is_none());
    assert_eq!(
        router.handle_value(json!({"jsonrpc":"2.0","method":"denied"})),
        None
    );
    for code in [-32768, -32700, -32600, -32099, -32000] {
        assert!(Error::application(code, "reserved").is_none());
    }
    assert_eq!(Error::internal().code(), -32603);
    assert_eq!(Error::invalid_params().message(), "Invalid params");
}

#[test]
fn output_serialization_errors_are_internal_not_success_or_details() {
    struct BrokenEncoding;
    impl rydantic::JsonType for BrokenEncoding {
        fn decode(_: Value, _: &mut rydantic::Context, _: bool) -> Option<Self> {
            Some(Self)
        }
        fn encode(&self, ctx: &mut rydantic::Context) -> Value {
            ctx.push(rydantic::ValidationError::new(
                "secret",
                "private-output-detail",
            ));
            json!("must-not-escape")
        }
        fn schema(_: &mut rydantic::SchemaContext, _: bool) -> Value {
            json!({})
        }
    }
    impl Model for BrokenEncoding {}
    let mut router = router();
    router
        .register("broken-encoding", Params::Named, |_: Empty| {
            Ok(BrokenEncoding)
        })
        .unwrap();
    assert_eq!(
        router.handle_value(json!({"jsonrpc":"2.0","method":"broken-encoding","id":1})),
        Some(json!({"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"Internal error"}}))
    );
    assert_eq!(
        router.handle_value(json!({"jsonrpc":"2.0","method":"broken-encoding"})),
        None
    );
}

#[test]
fn rejected_inputs_never_call_handler_and_order_is_sequential() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let capture = seen.clone();
    let mut router = Router::new();
    router
        .register("record", Params::Named, move |p: Greeting| {
            capture.lock().unwrap().push(p.name);
            Ok(Sum(0))
        })
        .unwrap();
    let reply = router
        .handle_value(json!([
            {"jsonrpc":"2.0","method":"record","params":{"name":"first"}},
            {"jsonrpc":"2.0","method":"record","params":{"name":"x"},"id":1},
            {"jsonrpc":"2.0","method":"record","params":{"name":"invalid-envelope"},"id":false},
            {"jsonrpc":"2.0","method":"record","params":{"name":"last"},"id":2}
        ]))
        .unwrap();
    assert_eq!(seen.lock().unwrap().as_slice(), ["first", "last"]);
    assert_eq!(reply.as_array().unwrap().len(), 3);
}

#[test]
fn serde_private_keys_never_change_client_json_types() {
    let router = router();
    for object in [
        r#"{"$serde_json::private::Number":"5"}"#,
        r#"{"$serde_json::private::RawValue":"null"}"#,
        r#"{"$serde_json::private::Number":"not-a-number"}"#,
    ] {
        let input = format!(r#"{{"jsonrpc":"2.0","method":"empty","id":{object}}}"#);
        assert_eq!(code(router.handle_json(&input)), -32600);
        assert_eq!(code(router.handle_json(object)), -32600);
        let input = format!(r#"{{"jsonrpc":"2.0","method":"greet","params":{object},"id":1}}"#);
        assert_eq!(code(router.handle_json(&input)), -32602);
    }
    // The same keys remain ordinary application data, including nested fields.
    #[derive(Model)]
    struct Payload {
        content: Value,
    }
    let mut router = Router::new();
    router
        .register("echo", Params::Named, |value: Payload| Ok(value))
        .unwrap();
    let reply = router.handle_json(r#"{"jsonrpc":"2.0","method":"echo","params":{"content":{"$serde_json::private::Number":"123"}},"id":1}"#).unwrap();
    let content = &reply["result"]["content"];
    assert!(content.is_object());
    assert_eq!(
        content
            .get("$serde_json::private::Number")
            .unwrap()
            .as_str(),
        Some("123")
    );
}

#[test]
fn hostile_marker_batches_cannot_execute_and_deep_text_fails_without_panic() {
    let seen = Arc::new(AtomicUsize::new(0));
    let counter = seen.clone();
    let mut router = Router::new();
    router
        .register("ok", Params::Named, move |_: Empty| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(Sum(0))
        })
        .unwrap();
    let reply = router.handle_json(r#"[{"$serde_json::private::RawValue":"{\"jsonrpc\":\"2.0\",\"method\":\"ok\",\"id\":1}"},{"jsonrpc":"2.0","method":"ok","id":{"$serde_json::private::Number":"5"}},{"jsonrpc":"2.0","method":"ok","id":{"$serde_json::private::RawValue":"null"}}]"#).unwrap();
    assert_eq!(reply.as_array().unwrap().len(), 3);
    assert!(reply
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["error"]["code"] == -32600));
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    for depth in [128, 512, 10000] {
        let text = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        // Any panic fails this test. The depth guard returns a protocol parse error.
        assert_eq!(code(router.handle_json(&text)), -32700);
    }
    assert_eq!(seen.load(Ordering::SeqCst), 0);
}
