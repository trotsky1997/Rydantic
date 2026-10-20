# RydanticAI

Typed Rust agents, explicit workflow graphs and a three-protocol HTTP gateway, built on [Rydantic](https://github.com/trotsky1997/Rydantic).

```toml
[dependencies]
rydantic = "0.7.0-beta.3"
rydantic-ai = "0.6.0"
```

`rydantic-ai` has its own version and release lifecycle inside the Rydantic workspace. The core `rydantic` crate does not depend on it. Disable default features for graphs and custom providers without HTTP dependencies.

Declare `rydantic` directly when deriving application models so its derive macro can resolve the model dependency, including renamed dependencies. The requirement above permits compatible published 0.7.0 prereleases.

## API providers

Connect using `BASEURL` and an optional `APIKEY`. `BASEURL` includes the API prefix, such as `https://api.openai.com/v1` or `https://api.anthropic.com/v1`. Choose `MODEL` from the configured endpoint.

| Protocol | Provider | Appended endpoint | Credential header |
| --- | --- | --- | --- |
| Responses | `ResponsesProvider` | `/responses` | `Authorization: Bearer APIKEY` |
| Chat Completions | `ChatCompletionsProvider` | `/chat/completions` | `Authorization: Bearer APIKEY` |
| Anthropic Messages | `AnthropicMessagesProvider` | `/messages` | `x-api-key: APIKEY` |

Each provider supports URL/Base64 image input, streamed text, typed client tools, validation correction, `Partial<T>`, `Iterable<T>`, usage accounting, cancellation and deadlines. Messages requests also include `anthropic-version: 2023-06-01`.

Use `HttpProvider::from_env(Api::ChatCompletions)?` for protocol selection at runtime, or the named provider constructors for a fixed protocol. Constructors take `(base_url, Option<String>)`; `from_env()` reads `BASEURL` and `APIKEY` for named providers. No login flow or credential storage is part of the SDK.

Responses and Chat Completions request native JSON Schema by default. Messages uses schema instructions and runtime validation by default; supported models and schemas can explicitly enable `Capabilities { native_json_schema: true, output_token_limit: true }` to send `output_config.format`. All three retain Rydantic runtime validation. An endpoint with fewer features can override capabilities explicitly.

Requests are sent once with redirects disabled. HTTPS is required for network upstreams; loopback HTTP is supported for local endpoints. Credentials and BASEURL are redacted in Debug and transport errors, and upstream error bodies are omitted. See the [API provider guide](https://rydantic.mintlify.site/integrations/api-providers) for protocol mappings and limits.

## Typed agents and tools

`Agent<Dependencies, Output>` validates its final `Output: Model + AsyncCheck`. `TypedTool<Dependencies, Arguments, Output, Handler>` derives tool schemas from its argument model, validates arguments before execution, and validates the handler's result. Handlers receive `Arc<Dependencies>` and a cancellation/deadline context.

The provider schema adapter resolves document-root references and gives definitions stable ASCII names. Numeric constraints outside JavaScript's safe integer range are moved into textual descriptions, with every move recorded by `schema::prepare_model_schema::<T>()` in `runtime_constraints`. Full integer bounds remain enforced by Rydantic at runtime. Literal values and field names are preserved. This projection preserves numeric constraints across JSON-based provider protocols.

Unsafe numeric literals in `const`, `enum`, `default` or `examples` fail preparation explicitly. Tool argument schemas and native structured-output schemas require an object root; instruction-based final outputs can use other model shapes.

```rust,no_run
use rydantic::Model;
use rydantic_ai::{Agent, ResponsesProvider, RunContext};
use std::sync::Arc;

#[derive(Model)]
struct Answer { answer: String }

# async fn example() -> Result<(), rydantic_ai::Error> {
let provider = Arc::new(ResponsesProvider::from_env()?);
let model = std::env::var("MODEL").expect("set MODEL locally");
let agent = Agent::<(), Answer>::new(provider, model);
let result = agent.run("What is 19 + 23?", Arc::new(()), RunContext::default()).await?;
println!("{}", result.output.answer);
# Ok(()) }
```

Run `cargo run -p rydantic-ai --example provider` for a typed tool-call round trip. Set `BASEURL`, `MODEL`, optional `APIKEY`, and `API` to `responses`, `chat_completions` or `anthropic_messages` (default: `responses`). Agent limits bound model calls, tool calls, validation retries and observed token usage. Cancellation and the absolute deadline cover model calls, validation and tool handlers. Tools execute sequentially; execution errors stop the run and are never automatically replayed. Handlers own transactional/idempotency behavior for external side effects. Prompt content is not written to tracing spans; operations, tool names and usage are recorded with standard `tracing`.

Synchronous validators cannot be interrupted within a poll. Ordinary runs and partial streams recheck cancellation and the deadline after successful final validation, rejecting a late result instead of accepting it.

## Image inputs

All three providers and gateway frontends support ordered user text/image input, HTTP(S) image URLs and Base64 PNG/JPEG/GIF/WebP images. `Agent::run`, `stream_partial`, `stream_iterable` and `ModelRequest::new` accept a `Prompt` or a string:

```rust
use rydantic_ai::{Image, Prompt};
let prompt = Prompt::new("Describe this image.")
    .image(Image::url("https://example.com/photo.png")?);
let result = agent.run(prompt, dependencies, context).await?;
```

Use `Image::from_bytes("image/png", &bytes)?` or `Image::base64("image/png", encoded_data)?` for embedded images. Serialized HTTP requests are limited to 16 MiB, Base64 data to 10 MiB per image and images to 64 per request. `ImageDetail::Auto` is portable; non-auto OpenAI detail controls are preserved for Responses/Chat and rejected for Messages. Image identity and part order are included in native history replay. See the [image input guide](https://rydantic.mintlify.site/integrations/images) and the `vision` example.

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

Noul returns a probability in `[0, 1]`. Choice returns a selected label and probabilities. Score returns a possibly fractional value with confidence, level probabilities and the original legend. `response.parse::<YourModel>(&context).await` projects primary answers into a validated Rydantic model while retaining the full result in `response.answers`. It can be called from a typed tool or graph node using the same `RunContext`. Jev usage is available on `response.usage`; Agent usage counts its model provider requests.

Requests are sent once, redirects are disabled, and cancellation/deadlines cover HTTP transfer. The client validates question/answer names, variants, numeric ranges and option/level domains. Request and response limits are 1 MiB and 8 MiB. Debug output redacts the credential; upstream error bodies are omitted. Use HTTPS for network endpoints and keep credentials server-side.

See [the Jev guide](https://rydantic.mintlify.site/integrations/jev) and `cargo run -p rydantic-ai --example jev`. Live tests require explicit local configuration and are ignored by default.

## Graphs

`Graph<State, Dependencies, NodeKey, Output>` accepts `Node` implementations with typed state, dependencies and transitions. Node keys can be application enums. Declare each node's allowed outgoing edges when building the graph. Unknown nodes, undeclared transitions, duplicate nodes and exceeded step budgets fail explicitly.

Return `Transition::Next(key)`, `Transition::End(output)` or `Transition::Pause(next_key)`. A pause yields a Serde-serializable `Checkpoint { state, next, steps }`. Persist it in your own storage, then call `Graph::resume` with fresh dependencies and execution context. The step count survives resume. Checkpoints occur between nodes; storage durability, checkpoint authenticity and exactly-once external side effects belong to the application. See the `graph` example.

## Gateway

`Gateway` implements ordered model routing and per-client/global admission budgets. A gateway client implements `ModelProvider`, so the same agent can use a direct provider or a gateway. Only explicit HTTP 429/502/503/504 failures permit ordered failover. Successful stream terminals are emitted after usage and budget settlement. Transport/protocol failures and partially delivered streams are not replayed.

The `rydantic-ai-gateway` executable exposes authenticated `GET /v1/models`, `POST /v1/responses`, `POST /v1/chat/completions` and `POST /v1/messages`. All nine combinations of frontend and upstream protocols support JSON and SSE. Configure gateway keys and upstream credentials by environment-variable **names** in a JSON config; credential values are excluded from the config file and error responses. See `examples/gateway.json`. Each upstream selects `api`: `responses` (default), `chat_completions` or `anthropic_messages`. Clients choose their standard wire protocol independently of the route's upstream. Messages accepts `x-api-key`; all frontends accept bearer authentication. Opaque native tool history is restored from a bounded process-local replay cache, scoped to the authenticated client, model alias and full conversation prefix. It holds up to 128 entries and 8 MiB of serialized data for 30 minutes, with FIFO eviction. Keep tool sessions on one instance; restart, expiry or eviction removes opaque replay metadata. Ordinary text and function history remains translatable. See the [gateway protocol guide](https://rydantic.mintlify.site/integrations/gateway-protocols).

```sh
cargo run -p rydantic-ai --bin rydantic-ai-gateway -- crates/rydantic-ai/examples/gateway.json
```

Set `RYDANTIC_GATEWAY_KEY` to a private random value of at least 16 bytes before starting the example. Each client has an explicit model allowlist. Incoming gateway bearer keys are not forwarded upstream. Disconnecting an SSE consumer cancels the provider request.

Budgets track attempts, observed tokens and optional operator-configured prices in integer micro-USD. Each attempt atomically reserves token/cost capacity against both client and global ledgers before dispatch. Completion reconciles actual usage; failed, aborted or unknown outcomes retain the reservation conservatively. An upstream can exceed the token reservation, producing an overdrawn ledger that blocks later admission. These are admission controls and observed usage accounting, not a provider-side billing ceiling. Dollar limits require pricing on every permitted upstream. Routes without configured prices can use request and token controls. Ledgers are in memory and reset on restart; durable/shared accounting requires an application-provided service in a later deployment.

Client and global policies each use their own reservation size and cost estimate. They are admitted together atomically and reconciled separately, including conservative charges for unknown outcomes.

## Independent releases

RydanticAI uses tags such as `rydantic-ai-v0.6.0`. Its version is read from this package manifest and is independent of the core workspace version. A maintainer with the repository's GitHub identity and a local Cargo publishing credential can run from a clean repository checkout:

```sh
python3 scripts/release_ai.py preflight --tag rydantic-ai-v0.6.0
python3 scripts/release_ai.py publish --tag rydantic-ai-v0.6.0
```

The release script requires an existing remote tag on tested main history, successful exact-commit main-push CI, an unpublished registry version and a package containing no installed Node dependencies or credential files. It performs one upload attempt and verifies the registry checksum and sparse index, including after an uncertain upload exit status. It publishes only `rydantic-ai`. Use Python 3.11 or newer.

## Validation

```sh
cargo test -p rydantic-ai --all-features
cargo check -p rydantic-ai --no-default-features
```

Live Responses tests require explicit `BASEURL` and `MODEL`, with optional `APIKEY`, and are ignored by default. Set `NATIVE_JSON_SCHEMA=false` or `OUTPUT_TOKEN_LIMIT=false` when the configured endpoint requires instruction-based output or lacks a request token limit. Native Chat Completions and Messages tests use HTTP fixtures, including fragmented SSE and tool-history replay. No credentials or live provider traffic are needed for ordinary tests.
