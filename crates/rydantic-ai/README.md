# RydanticAI

Typed Rust agents, explicit workflow graphs and a Responses HTTP gateway, built on [Rydantic](https://github.com/trotsky1997/Rydantic).

```toml
[dependencies]
rydantic = "0.7.0-beta.3"
rydantic-ai = "0.3.0"
```

`rydantic-ai` has its own version and release lifecycle inside the Rydantic workspace. The core `rydantic` crate does not depend on it. Disable default features for graphs and custom providers without HTTP dependencies.

Declare `rydantic` directly when deriving application models so its derive macro can resolve the model dependency, including renamed dependencies. The requirement above permits compatible published 0.7.0 prereleases.

## pi Codex login and Responses bridge

The included Node bridge uses pi's public `ModelRuntime` SDK for OAuth storage, refresh, model lookup and inference. It forwards native Responses SSE events through `onProviderStreamEvent`; OAuth tokens stay in pi's local credential storage.

From this package's `bridge/` directory (also included in the crate source archive):

```sh
npm ci
npm run login
npm start
```

Complete the browser authorization yourself. The bridge listens on `127.0.0.1:8787`, rejects browser-origin requests and exposes `GET /health`, `GET /v1/models` and `POST /v1/responses`. It is a local single-user sidecar. Put the authenticated Rust gateway in front of it for network access.

The gateway executable serves HTTP. For network clients, keep it on loopback or a trusted private interface behind an HTTPS reverse proxy with TLS termination. HTTPS protects bearer keys, prompts and responses in transit; bearer authentication handles client identity. Keep the unauthenticated pi bridge loopback-only. Local processes with access to the sidecar can use the authorized Codex session.

TLS termination protects the client-to-proxy connection, not an off-host proxy-to-gateway HTTP connection. Prefer a same-host proxy with the gateway bound to loopback. For an off-host backend, restrict the gateway listener to the proxy and protect that hop with encrypted transport; a private IP alone does not encrypt traffic. Do not expose the plaintext gateway listener directly.

The supported request fields are `model`, `input`, `instructions`, `tools`, `text`, `reasoning`, `tool_choice`, `parallel_tool_calls`, `stream`, `store` and `include`. Requests use `store=false`. Unknown fields fail explicitly. The pi Codex transport has no `max_output_tokens` request parameter; `ResponsesProvider::pi_codex` uses schema instructions plus runtime validation. A native Responses endpoint can use provider JSON Schema and output limits through `ResponsesProvider::new`.

## Typed agents and tools

`Agent<Dependencies, Output>` validates its final `Output: Model + AsyncCheck`. `TypedTool<Dependencies, Arguments, Output, Handler>` derives tool schemas from its argument model, validates arguments before execution, and validates the handler's result. Handlers receive `Arc<Dependencies>` and a cancellation/deadline context.

The provider schema adapter resolves document-root references and gives definitions stable ASCII names. Numeric constraints outside JavaScript's safe integer range are moved into textual descriptions, with every move recorded by `schema::prepare_model_schema::<T>()` in `runtime_constraints`. Full integer bounds remain enforced by Rydantic at runtime. Literal values and field names are preserved. This projection is necessary for pi's JSON transport and Codex tool schema handling.

Unsafe numeric literals in `const`, `enum`, `default` or `examples` fail preparation explicitly. Tool argument schemas and native structured-output schemas require an object root; instruction-based final outputs can use other model shapes.

```rust,no_run
use rydantic::Model;
use rydantic_ai::{Agent, ResponsesProvider, RunContext};
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

## Structured output streaming

`Agent::stream_partial()` returns `Partial<T>`: a typed stream of provisional JSON snapshots followed by a fully validated final model. `Agent::stream_iterable()` returns `Iterable<T>`: complete, runtime-validated elements from an object with an `items` array. The object envelope supports native JSON Schema providers. Neither streaming mode requires `T: Clone` or HTTP transport.

```rust,no_run
use rydantic_ai::{rydantic::Model, Agent, ModelProvider, OutputEvent, RunContext};
use std::sync::Arc;

#[derive(Debug, Model)]
struct Person { name: String, age: u8 }

# async fn example(provider: Arc<dyn ModelProvider>) -> rydantic_ai::Result<()> {
let agent = Agent::<(), Person>::new(provider, "your-model");
let mut partial = agent.stream_partial("Extract one person", Arc::new(()), RunContext::default())?;
while let Some(event) = partial.recv().await {
    match event? {
        OutputEvent::Delta { value, .. } => println!("{}", value.value),
        OutputEvent::Complete(result) => println!("{}", result.output.name),
        OutputEvent::Reset { .. } => { /* Clear previews from this attempt. */ }
        OutputEvent::Started { .. } => {}
    }
}
let people = agent.stream_iterable("Extract all people", Arc::new(()), RunContext::default())?
    .collect().await?;
for person in people.output { println!("{}", person.name); }
# Ok(()) }
```

Consume `OutputEvent::Delta` from `Iterable::recv()` for individual `StreamItem<T>` values and zero-based indexes. `Complete` on an iterable reports the accepted item count and usage. `Partial::finish()` drains previews and returns `RunResult<T>`; `Iterable::collect()` consumes a fresh stream, discards invalidated attempts and returns `RunResult<Vec<T>>`.

Each `Started`, `Delta` and `Reset` includes the one-based model-call number. All progress belongs to that attempt until final completion. `ResetReason::Validation` carries structured errors; `ToolContinuation` invalidates intermediate output before tool execution. A terminal error also invalidates unfinished progress. A syntactically complete preview may still fail final model validation. Missing required fields stay missing in previews; no defaults are invented.

Partial snapshots use the existing bounded Rydantic JSON parser. Unfinished strings/keys wait for more input; whole-buffer snapshots can reparse prefixes and incur quadratic total work. Iterable parsing advances past emitted elements and waits for a right delimiter before committing scalar numbers. Async item validators run once per emitted item per attempt; collection finalization checks the strict envelope and syntax without rerunning those validators.

`StreamOptions` controls progress queue capacity (default 16), JSON byte limit (default/hard maximum 1 MiB) and item limit (default 4096, hard maximum 100,000). Both modes cap nesting at 64 containers. Use `stream_partial_with_options` or `stream_iterable_with_options` for lower limits. Slow consumers apply backpressure. Run deadlines cover queue waits and async checks. Dropping or cancelling the stream stops its child task while preserving the parent's cancellation token.

Providers send `response.output_text.delta` through `RunContext::emit`; original native-event sinks remain available. A provider without deltas produces events from its completed response. Deltas must belong to one text part per model call and exactly match the completed response. Refusals, premature transport termination and mismatches fail explicitly.

Run `cargo run -p rydantic-ai --example structured` for an offline example. See the [structured output guide](https://rydantic.mintlify.site/integrations/structured-output) for event handling and correction behavior.

## TypeSafe Jev SDK

`rydantic_ai::jev` supports native `noul`, `choice` and `score` questions, typed answers, probability distributions, confidence, score legends and usage. Configure only `TYPESAFE_BASEURL` (the `/v1` API base) and `TYPESAFE_APIKEY` locally, then construct `JevClient::from_env()`. The credential is sent unchanged as the complete bearer value. Explicit configuration also accepts `JevClient::new(base_url, api_key)`.

```rust,no_run
use rydantic_ai::{jev::{JevClient, Question, SystemOneRequest}, RunContext};

# async fn example() -> rydantic_ai::Result<()> {
let client = JevClient::from_env()?;
let request = SystemOneRequest::new("jev-1.14.0", "Payment failed. Please help now.")
    .question("urgency", Question::noul("Does the customer request urgent help?"))?;
let response = client.system_one(&request, RunContext::default()).await?;
println!("{}", response.noul("urgency")?.noul);
# Ok(()) }
```

Noul returns a probability in `[0, 1]`. Choice returns a selected label and probabilities. Score returns a possibly fractional value with confidence, level probabilities and the original legend. `response.parse::<YourModel>(&context).await` projects primary answers into a validated Rydantic model while retaining the full result in `response.answers`. It can be called from a typed tool or graph node using the same `RunContext`. Jev usage is available on `response.usage`; Agent usage counts its Responses model requests.

Requests are sent once, redirects are disabled, and cancellation/deadlines cover HTTP transfer. The client validates question/answer names, variants, numeric ranges and option/level domains. Request and response limits are 1 MiB and 8 MiB. Debug output redacts the credential; upstream error bodies are omitted. Use HTTPS for network endpoints and keep credentials server-side.

See [the Jev guide](https://rydantic.mintlify.site/integrations/jev) and `cargo run -p rydantic-ai --example jev`. Live tests require explicit local configuration and are ignored by default.

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

Client and global policies each use their own reservation size and cost estimate. They are admitted together atomically and reconciled separately, including conservative charges for unknown outcomes.

## Independent releases

RydanticAI uses tags such as `rydantic-ai-v0.3.0`. Its version is read from this package manifest and is independent of the core workspace version. A maintainer with the repository's GitHub identity and a local Cargo publishing credential can run from a clean repository checkout:

```sh
python3 scripts/release_ai.py preflight --tag rydantic-ai-v0.3.0
python3 scripts/release_ai.py publish --tag rydantic-ai-v0.3.0
```

The release script requires an existing remote tag on tested main history, successful exact-commit main-push CI, an unpublished registry version and a package containing no installed Node dependencies or credential files. It performs one upload attempt and verifies the registry checksum and sparse index, including after an uncertain upload exit status. It publishes only `rydantic-ai`. Use Python 3.11 or newer.

## Validation

```sh
cargo test -p rydantic-ai --all-features
cargo check -p rydantic-ai --no-default-features
cd crates/rydantic-ai/bridge && npm ci && npm test
```

Live provider tests require explicit `RYDANTIC_PI_URL` and are ignored by default. No credentials or live provider traffic are needed for ordinary tests.
