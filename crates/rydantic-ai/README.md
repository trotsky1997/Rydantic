# RydanticAI

Typed Rust agents, explicit workflow graphs and a Responses HTTP gateway, built on [Rydantic](https://github.com/trotsky1997/Rydantic).

```toml
[dependencies]
rydantic-ai = "0.1.0"
```

`rydantic-ai` has its own version and release lifecycle inside the Rydantic workspace. The core `rydantic` crate does not depend on it. Disable default features for graphs and custom providers without HTTP dependencies.

## pi Codex login and Responses bridge

The included Node bridge uses pi's public `ModelRuntime` SDK for OAuth storage, refresh, model lookup and inference. It forwards native Responses SSE events through `onProviderStreamEvent`; OAuth tokens stay in pi's local credential storage.

From this package's `bridge/` directory (also included in the crate source archive):

```sh
npm ci
npm run login
npm start
```

Complete the browser authorization yourself. The bridge listens on `127.0.0.1:8787`, rejects browser-origin requests and exposes `GET /health`, `GET /v1/models` and `POST /v1/responses`. It is a local single-user sidecar. Put the authenticated Rust gateway in front of it for network access.

The supported request fields are `model`, `input`, `instructions`, `tools`, `text`, `reasoning`, `tool_choice`, `parallel_tool_calls`, `stream`, `store` and `include`. Requests use `store=false`. Unknown fields fail explicitly. The pi Codex transport has no `max_output_tokens` request parameter; `ResponsesProvider::pi_codex` uses schema instructions plus runtime validation. A native Responses endpoint can use provider JSON Schema and output limits through `ResponsesProvider::new`.

## Typed agents and tools

`Agent<Dependencies, Output>` validates its final `Output: Model + AsyncCheck`. `TypedTool<Dependencies, Arguments, Output, Handler>` derives tool schemas from its argument model, validates arguments before execution, and validates the handler's result. Handlers receive `Arc<Dependencies>` and a cancellation/deadline context.

The provider schema adapter resolves document-root references and gives definitions stable ASCII names. Numeric constraints outside JavaScript's safe integer range are moved into textual descriptions, with every move recorded by `schema::prepare_model_schema::<T>()` in `runtime_constraints`. Full integer bounds remain enforced by Rydantic at runtime. Literal values and field names are preserved. This projection is necessary for pi's JSON transport and Codex tool schema handling.

Unsafe numeric literals in `const`, `enum`, `default` or `examples` fail preparation explicitly. Tool argument schemas and native structured-output schemas require an object root; instruction-based final outputs can use other model shapes.

```rust,no_run
use rydantic_ai::{rydantic::Model, Agent, ResponsesProvider, RunContext};
use std::sync::Arc;

#[derive(Model)]
struct Answer { answer: String }

# async fn example() -> Result<(), rydantic_ai::Error> {
let provider = Arc::new(ResponsesProvider::pi_codex("http://127.0.0.1:8787/v1")?);
let agent = Agent::<(), Answer>::new(provider, "gpt-6-luna");
let result = agent.run("What is 19 + 23?", Arc::new(()), RunContext::default()).await?;
println!("{}", result.output.answer);
# Ok(()) }
```

Run `cargo run -p rydantic-ai --example pi_codex` for a real typed tool-call round trip. Set `RYDANTIC_AI_MODEL` to a model returned by the bridge's `/v1/models` endpoint. Agent limits bound model calls, tool calls, validation retries and observed token usage. Cancellation and the absolute deadline cover model calls, validation and tool handlers. Tools execute sequentially; execution errors stop the run and are never automatically replayed. Handlers own transactional/idempotency behavior for external side effects. Prompt content is not written to tracing spans; operations, tool names and usage are recorded with standard `tracing`.

## Graphs

`Graph<State, Dependencies, NodeKey, Output>` accepts `Node` implementations with typed state, dependencies and transitions. Node keys can be application enums. Declare each node's allowed outgoing edges when building the graph. Unknown nodes, undeclared transitions, duplicate nodes and exceeded step budgets fail explicitly.

Return `Transition::Next(key)`, `Transition::End(output)` or `Transition::Pause(next_key)`. A pause yields a Serde-serializable `Checkpoint { state, next, steps }`. Persist it in your own storage, then call `Graph::resume` with fresh dependencies and execution context. The step count survives resume. Checkpoints occur between nodes; storage durability, checkpoint authenticity and exactly-once external side effects belong to the application. See the `graph` example.

## Gateway

`Gateway` implements ordered model routing and per-client/global admission budgets. A gateway client implements `ModelProvider`, so the same agent can use a direct provider or a gateway. Only explicit HTTP 429/502/503/504 failures permit ordered failover. Transport/protocol failures and partially delivered streams are not replayed.

The `rydantic-ai-gateway` executable exposes authenticated `GET /v1/models` and `POST /v1/responses` endpoints, including SSE. Configure gateway keys and upstream credentials by environment-variable **names** in a JSON config; credential values are excluded from the config file and error responses. See `examples/gateway.json`.

```sh
cargo run -p rydantic-ai --bin rydantic-ai-gateway -- crates/rydantic-ai/examples/gateway.json
```

Set `RYDANTIC_GATEWAY_KEY` to a private random value of at least 16 bytes before starting the example. Each client has an explicit model allowlist. Incoming gateway bearer keys are not forwarded upstream. Disconnecting an SSE consumer cancels the provider request.

Budgets track attempts, observed tokens and optional operator-configured prices in integer micro-USD. Each attempt atomically reserves token/cost capacity against both client and global ledgers before dispatch. Completion reconciles actual usage; failed, aborted or unknown outcomes retain the reservation conservatively. An upstream can exceed the token reservation, producing an overdrawn ledger that blocks later admission. These are admission controls and observed usage accounting, not a provider-side billing ceiling. Dollar limits require pricing on every permitted upstream. Codex subscription usage can use request/token controls without claiming a dollar price. Ledgers are in memory and reset on restart; durable/shared accounting requires an application-provided service in a later deployment.

## Validation

```sh
cargo test -p rydantic-ai --all-features
cargo check -p rydantic-ai --no-default-features
cd crates/rydantic-ai/bridge && npm ci && npm test
```

Live provider tests require explicit `RYDANTIC_PI_URL` and are ignored by default. No credentials or live provider traffic are needed for ordinary tests.
