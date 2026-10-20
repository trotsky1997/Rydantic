#![cfg(feature = "http")]
use rydantic_ai::{
    gateway::{self, Budget, ClientPolicy, Gateway, Route},
    graph::{Graph, GraphResult, Node, Transition},
    rydantic::Model,
    Agent, BoxFuture, ResponsesProvider, Result, RunContext, TypedTool,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

#[derive(Model)]
struct Answer {
    answer: String,
}
#[derive(Model)]
struct Add {
    a: i64,
    b: i64,
}
#[derive(Model)]
struct Sum {
    sum: i64,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum Key {
    Agent,
    Finish,
}
#[derive(Serialize, Deserialize)]
struct State {
    answer: String,
    model_calls: u32,
    tool_calls: u32,
    tokens: u64,
}
struct CallAgent;
struct Finish;
impl Node<State, Agent<(), Answer>, Key, String> for CallAgent {
    fn run<'a>(
        &'a self,
        state: &'a mut State,
        agent: &'a Agent<(), Answer>,
        context: &'a RunContext,
    ) -> BoxFuture<'a, Result<Transition<Key, String>>> {
        Box::pin(async move {
            let result = agent
                .run(
                    "Call the add tool to calculate 19 + 23. Report the answer as a string.",
                    Arc::new(()),
                    context.clone(),
                )
                .await?;
            *state = State {
                answer: result.output.answer,
                model_calls: result.model_calls,
                tool_calls: result.tool_calls,
                tokens: result.usage.total()?,
            };
            Ok(Transition::Pause(Key::Finish))
        })
    }
}
impl Node<State, Agent<(), Answer>, Key, String> for Finish {
    fn run<'a>(
        &'a self,
        state: &'a mut State,
        _: &'a Agent<(), Answer>,
        _: &'a RunContext,
    ) -> BoxFuture<'a, Result<Transition<Key, String>>> {
        Box::pin(async move { Ok(Transition::End(state.answer.clone())) })
    }
}

#[tokio::test]
#[ignore = "requires BASEURL and MODEL for a live Responses endpoint"]
async fn responses_agent_tool_gateway_and_graph_checkpoint() {
    let base = std::env::var("BASEURL").expect("set BASEURL for the live endpoint");
    let model = std::env::var("MODEL").expect("set MODEL");
    let provider = Arc::new(
        ResponsesProvider::new(&base, std::env::var("APIKEY").ok())
            .unwrap()
            .with_capabilities(rydantic_ai::Capabilities {
                native_json_schema: std::env::var("NATIVE_JSON_SCHEMA")
                    .map(|v| v == "true")
                    .unwrap_or(true),
                output_token_limit: std::env::var("OUTPUT_TOKEN_LIMIT")
                    .map(|v| v == "true")
                    .unwrap_or(true),
            }),
    );
    let budget = Budget {
        max_requests: 8,
        max_tokens: 100_000,
        reservation_tokens: 2048,
        max_cost_microusd: None,
    };
    let key = "live-test-gateway-key-12345";
    let gateway = Gateway::new(
        BTreeMap::from([(
            "model".into(),
            vec![Route {
                provider,
                model,
                price: None,
            }],
        )]),
        vec![ClientPolicy {
            id: "live".into(),
            key: key.into(),
            models: HashSet::from(["model".into()]),
            budget: budget.clone(),
        }],
        budget,
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let app = gateway::router(gateway.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let http = Arc::new(
        ResponsesProvider::gateway(&url, Some(key.into()))
            .unwrap()
            .with_capabilities(rydantic_ai::Capabilities {
                native_json_schema: false,
                output_token_limit: false,
            }),
    );
    let tool = TypedTool::<(), Add, Sum, _>::new(
        "add",
        "Add two integers",
        |args: Add, _, _| async move {
            Ok(Sum {
                sum: args.a + args.b,
            })
        },
    )
    .unwrap();
    let agent = Agent::<(), Answer>::new(http, "model")
        .instructions("Use add for arithmetic and return the exact numeric result in answer.")
        .tool(tool)
        .unwrap();
    let graph = Graph::new(4)
        .unwrap()
        .node(Key::Agent, [Key::Finish], CallAgent)
        .unwrap()
        .node(Key::Finish, [], Finish)
        .unwrap();
    let state = State {
        answer: String::new(),
        model_calls: 0,
        tool_calls: 0,
        tokens: 0,
    };
    let result = graph
        .run(state, &agent, Key::Agent, RunContext::default())
        .await
        .unwrap();
    let GraphResult::Paused(checkpoint) = result else {
        panic!("expected paused checkpoint")
    };
    let saved = serde_json::to_string(&checkpoint).unwrap();
    let resumed = graph
        .resume(
            serde_json::from_str(&saved).unwrap(),
            &agent,
            RunContext::default(),
        )
        .await
        .unwrap();
    let GraphResult::Completed {
        state,
        output,
        steps,
    } = resumed
    else {
        panic!("expected completion")
    };
    assert!(output.contains("42"));
    assert!(state.tool_calls >= 1);
    assert!(state.model_calls >= 2);
    assert!(state.tokens > 0);
    assert_eq!(steps, 2);
    let ledger = gateway.snapshot("live").unwrap();
    assert_eq!(ledger.requests, u64::from(state.model_calls));
    assert_eq!(ledger.tokens, state.tokens);
    assert_eq!(ledger.reserved_tokens, 0);
    println!(
        "LIVE_RECEIPT={}",
        serde_json::json!({"answer":output,"model_calls":state.model_calls,"tool_calls":state.tool_calls,"tokens":state.tokens,"graph_steps":steps,"gateway":ledger})
    );
    server.abort();
}
