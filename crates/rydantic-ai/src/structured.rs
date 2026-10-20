//! Typed structured-output streams with explicit attempt boundaries.
//!
//! [`Partial`] emits provisional JSON snapshots and ends with a fully validated
//! model. [`Iterable`] validates each complete element once before emission.
//! Items and snapshots belong to an attempt until its `Complete` event; a reset
//! invalidates all results from that attempt. Dropping a stream cancels its task.
use crate::{Agent, BoxFuture, Error, Result, RunContext, RunResult};
use rydantic::{partial_json, AsyncCheck, Model, ValidationError, ValidationErrors};
use serde::Deserialize;
use serde_json::{value::RawValue, Value};
use std::{marker::PhantomData, ops::Range, sync::Arc};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

/// Resource bounds shared by both stream modes.
#[derive(Debug, Clone, Copy)]
pub struct StreamOptions {
    /// Bounded progress queue, between 1 and 1024 entries.
    pub capacity: usize,
    /// Accumulated JSON bytes per model call, at most 1 MiB.
    pub max_output_bytes: usize,
    /// Maximum iterable elements per model call, at most 100,000.
    pub max_items: usize,
}
impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            capacity: 16,
            max_output_bytes: 1024 * 1024,
            max_items: 4096,
        }
    }
}
impl StreamOptions {
    fn validate(self) -> Result<Self> {
        if !(1..=1024).contains(&self.capacity)
            || !(1..=1024 * 1024).contains(&self.max_output_bytes)
            || !(1..=100_000).contains(&self.max_items)
        {
            return Err(Error::Configuration(
                "invalid structured stream limits".into(),
            ));
        }
        Ok(self)
    }
}

/// A preview of available JSON fields. Model validators run on the final output.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialSnapshot {
    pub value: Value,
    /// Syntactic completeness of this buffer; generation may still continue.
    pub complete_json: bool,
}

/// A fully validated element, indexed from zero within its attempt.
#[derive(Debug)]
pub struct StreamItem<T> {
    pub index: usize,
    pub value: T,
}

#[derive(Debug, Clone)]
pub enum ResetReason {
    Validation(ValidationErrors),
    ToolContinuation,
}

/// `attempt` is the one-based model-call number, including tool turns.
#[derive(Debug)]
pub enum OutputEvent<P, F> {
    Started { attempt: u32 },
    Delta { attempt: u32, value: P },
    Reset { attempt: u32, reason: ResetReason },
    Complete(RunResult<F>),
}
pub type PartialEvent<T> = OutputEvent<PartialSnapshot, T>;
/// Completion contains the accepted attempt's element count and run metadata.
pub type IterableEvent<T> = OutputEvent<StreamItem<T>, usize>;

/// Provisional snapshots followed by a validated final `T`.
///
/// Required fields are allowed to be absent from snapshots. Access previews
/// through `PartialSnapshot::value`; only `Complete` contains a validated `T`.
pub struct Partial<T> {
    inner: StreamTask<PartialSnapshot, T>,
}
impl<T> Partial<T> {
    pub async fn recv(&mut self) -> Option<Result<PartialEvent<T>>> {
        self.inner.recv().await
    }
    pub fn cancel(&self) {
        self.inner.cancellation.cancel();
    }
    /// Drain previews and return the accepted final model and usage.
    pub async fn finish(mut self) -> Result<RunResult<T>> {
        while let Some(event) = self.recv().await {
            if let OutputEvent::Complete(result) = event? {
                return Ok(result);
            }
        }
        Err(Error::Protocol(
            "partial stream completion was already consumed".into(),
        ))
    }
}

/// Validated elements from a JSON object of the form `{"items": [T, ...]}`.
///
/// The envelope works with object-root native JSON Schema providers. Elements
/// require no `Clone` bound. A later reset invalidates earlier emitted elements.
pub struct Iterable<T> {
    inner: StreamTask<StreamItem<T>, usize>,
}
impl<T> Iterable<T> {
    pub async fn recv(&mut self) -> Option<Result<IterableEvent<T>>> {
        self.inner.recv().await
    }
    pub fn cancel(&self) {
        self.inner.cancellation.cancel();
    }
    /// Collect a fresh stream, discarding attempts invalidated by a reset.
    /// Previously consumed item events cannot be recovered by this method.
    pub async fn collect(mut self) -> Result<RunResult<Vec<T>>> {
        let mut items = Vec::new();
        while let Some(event) = self.recv().await {
            match event? {
                OutputEvent::Started { .. } | OutputEvent::Reset { .. } => items.clear(),
                OutputEvent::Delta { value, .. } => {
                    if value.index != items.len() {
                        return Err(Error::Protocol(
                            "iterable items were already consumed".into(),
                        ));
                    }
                    items.push(value.value);
                }
                OutputEvent::Complete(result) => {
                    if result.output != items.len() {
                        return Err(Error::Protocol("iterable item count mismatch".into()));
                    }
                    return Ok(RunResult {
                        output: items,
                        usage: result.usage,
                        history: result.history,
                        model_calls: result.model_calls,
                        tool_calls: result.tool_calls,
                    });
                }
            }
        }
        Err(Error::Protocol(
            "iterable completion was already consumed".into(),
        ))
    }
}

// Terminal results live on the JoinHandle rather than the bounded queue. This
// preserves timeout/error delivery even when the progress queue is full.
struct StreamTask<P, F> {
    receiver: mpsc::Receiver<OutputEvent<P, F>>,
    task: Option<JoinHandle<Result<RunResult<F>>>>,
    cancellation: CancellationToken,
}
impl<P, F> StreamTask<P, F> {
    async fn recv(&mut self) -> Option<Result<OutputEvent<P, F>>> {
        if let Some(event) = self.receiver.recv().await {
            return Some(Ok(event));
        }
        let result = self.task.as_mut()?.await;
        self.task.take();
        Some(match result {
            Ok(result) => result.map(OutputEvent::Complete),
            Err(_) => Err(Error::Protocol("structured output task stopped".into())),
        })
    }
}
impl<P, F> Drop for StreamTask<P, F> {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

impl<D, T> Agent<D, T>
where
    D: Send + Sync + 'static,
    T: Model + AsyncCheck + Send + Sync + 'static,
{
    pub fn stream_partial(
        &self,
        prompt: impl Into<String>,
        deps: Arc<D>,
        context: RunContext,
    ) -> Result<Partial<T>> {
        self.stream_partial_with_options(prompt, deps, context, StreamOptions::default())
    }
    pub fn stream_partial_with_options(
        &self,
        prompt: impl Into<String>,
        deps: Arc<D>,
        context: RunContext,
        options: StreamOptions,
    ) -> Result<Partial<T>> {
        let options = options.validate()?;
        let (sender, receiver) = mpsc::channel(options.capacity);
        let mode = PartialMode::<T> {
            sink: Sink::new(sender),
            text: TextBuffer::new(options),
            last: None,
            marker: PhantomData,
        };
        Ok(Partial {
            inner: self.spawn_stream(prompt.into(), deps, context, mode, receiver)?,
        })
    }
    pub fn stream_iterable(
        &self,
        prompt: impl Into<String>,
        deps: Arc<D>,
        context: RunContext,
    ) -> Result<Iterable<T>> {
        self.stream_iterable_with_options(prompt, deps, context, StreamOptions::default())
    }
    pub fn stream_iterable_with_options(
        &self,
        prompt: impl Into<String>,
        deps: Arc<D>,
        context: RunContext,
        options: StreamOptions,
    ) -> Result<Iterable<T>> {
        let options = options.validate()?;
        let (sender, receiver) = mpsc::channel(options.capacity);
        let mode = IterableMode::<T> {
            sink: Sink::new(sender),
            text: TextBuffer::new(options),
            parser: ItemParser::default(),
            emitted: 0,
            failure: None,
            marker: PhantomData,
        };
        Ok(Iterable {
            inner: self.spawn_stream(prompt.into(), deps, context, mode, receiver)?,
        })
    }
    fn spawn_stream<P: OutputMode + 'static, E: Send + 'static>(
        &self,
        prompt: String,
        deps: Arc<D>,
        mut context: RunContext,
        mode: P,
        receiver: mpsc::Receiver<OutputEvent<E, P::Output>>,
    ) -> Result<StreamTask<E, P::Output>> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            Error::Configuration("structured streams require a Tokio runtime".into())
        })?;
        context.cancellation = context.cancellation.child_token();
        let cancellation = context.cancellation.clone();
        let agent = self.clone();
        let task = runtime.spawn(async move { agent.run_inner(prompt, deps, context, mode).await });
        Ok(StreamTask {
            receiver,
            task: Some(task),
            cancellation,
        })
    }
}

pub(crate) trait OutputMode: Send {
    type Output: Send + 'static;
    const STREAM: bool;
    fn schema(&self) -> Result<Value>;
    // Check each completed call before its text enters history or tools run.
    // Tool-turn text need not validate as the final typed output.
    fn finish_call(&mut self, _response: &crate::ModelResponse) -> Result<()> {
        Ok(())
    }
    fn start<'a>(
        &'a mut self,
        _attempt: u32,
        _context: &'a RunContext,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn observe<'a>(
        &'a mut self,
        _event: &'a Value,
        _context: &'a RunContext,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn reset<'a>(
        &'a mut self,
        _reason: ResetReason,
        _context: &'a RunContext,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn finish<'a>(
        &'a mut self,
        text: &'a str,
        context: &'a RunContext,
    ) -> BoxFuture<'a, Result<Self::Output>>;
}
pub(crate) struct Plain<T>(PhantomData<fn() -> T>);
impl<T> Plain<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}
impl<T: Model + AsyncCheck + Send + Sync + 'static> OutputMode for Plain<T> {
    type Output = T;
    const STREAM: bool = false;
    fn schema(&self) -> Result<Value> {
        crate::schema::model_schema::<T>()
    }
    fn finish<'a>(&'a mut self, text: &'a str, _: &'a RunContext) -> BoxFuture<'a, Result<T>> {
        Box::pin(async { T::parse_json_async(text).await.map_err(Error::Validation) })
    }
}

struct Sink<P, F> {
    sender: mpsc::Sender<OutputEvent<P, F>>,
    attempt: u32,
}
impl<P: Send, F: Send> Sink<P, F> {
    fn new(sender: mpsc::Sender<OutputEvent<P, F>>) -> Self {
        Self { sender, attempt: 0 }
    }
    async fn send(&self, event: OutputEvent<P, F>, context: &RunContext) -> Result<()> {
        context
            .wait(async { self.sender.send(event).await.map_err(|_| Error::Cancelled) })
            .await
    }
    async fn start(&mut self, attempt: u32, context: &RunContext) -> Result<()> {
        self.attempt = attempt;
        self.send(OutputEvent::Started { attempt }, context).await
    }
    async fn delta(&self, value: P, context: &RunContext) -> Result<()> {
        self.send(
            OutputEvent::Delta {
                attempt: self.attempt,
                value,
            },
            context,
        )
        .await
    }
    async fn reset(&self, reason: ResetReason, context: &RunContext) -> Result<()> {
        self.send(
            OutputEvent::Reset {
                attempt: self.attempt,
                reason,
            },
            context,
        )
        .await
    }
}

// This guard tracks only structural depth and limits. JSON grammar, escapes and
// numeric decoding remain the responsibility of the existing JSON parsers.
struct TextBuffer {
    value: String,
    options: StreamOptions,
    identity: Option<(Value, Value, Value)>,
    saw_delta: bool,
    depth: usize,
    in_string: bool,
    escape: bool,
}
impl TextBuffer {
    fn new(options: StreamOptions) -> Self {
        Self {
            value: String::new(),
            options,
            identity: None,
            saw_delta: false,
            depth: 0,
            in_string: false,
            escape: false,
        }
    }
    fn delta(&mut self, event: &Value) -> Result<bool> {
        if event["type"] != "response.output_text.delta" {
            return Ok(false);
        }
        let delta = event["delta"]
            .as_str()
            .ok_or_else(|| Error::Protocol("non-text output delta".into()))?;
        let identity = (
            event["output_index"].clone(),
            event["content_index"].clone(),
            event["item_id"].clone(),
        );
        if let Some(old) = &self.identity {
            if old != &identity {
                return Err(Error::Protocol(
                    "structured output requires a single text part per model call".into(),
                ));
            }
        } else {
            self.identity = Some(identity);
        }
        self.saw_delta = true;
        self.push(delta)?;
        Ok(true)
    }
    fn push(&mut self, delta: &str) -> Result<()> {
        if delta.len()
            > self
                .options
                .max_output_bytes
                .saturating_sub(self.value.len())
        {
            return Err(Error::Limit("structured output byte limit".into()));
        }
        for byte in delta.bytes() {
            if self.in_string {
                if self.escape {
                    self.escape = false;
                } else if byte == b'\\' {
                    self.escape = true;
                } else if byte == b'"' {
                    self.in_string = false;
                }
            } else {
                match byte {
                    b'"' => self.in_string = true,
                    b'[' | b'{' => {
                        self.depth += 1;
                        if self.depth > 64 {
                            return Err(Error::Limit("structured output nesting limit".into()));
                        }
                    }
                    b']' | b'}' => self.depth = self.depth.saturating_sub(1),
                    _ => (),
                }
            }
        }
        self.value.push_str(delta);
        Ok(())
    }
    fn final_text(&mut self, text: &str) -> Result<()> {
        if self.saw_delta {
            if self.value != text {
                return Err(Error::Protocol(
                    "streamed text differs from completed response".into(),
                ));
            }
        } else {
            self.push(text)?;
        }
        Ok(())
    }
}

struct PartialMode<T> {
    sink: Sink<PartialSnapshot, T>,
    text: TextBuffer,
    last: Option<PartialSnapshot>,
    marker: PhantomData<fn() -> T>,
}
impl<T: Send + Sync> PartialMode<T> {
    async fn preview(&mut self, context: &RunContext) -> Result<()> {
        match partial_json::snapshot(self.text.value.as_bytes()) {
            Ok(snapshot) => {
                if let Some(value) = snapshot.value {
                    let next = PartialSnapshot {
                        value,
                        complete_json: snapshot.status == partial_json::Status::Complete,
                    };
                    if self.last.as_ref() != Some(&next) {
                        self.sink.delta(next.clone(), context).await?;
                        self.last = Some(next);
                    }
                }
            }
            Err(
                partial_json::Error::InputTooLarge
                | partial_json::Error::NestingTooDeep
                | partial_json::Error::UnsupportedNumberLimit { .. },
            ) => return Err(Error::Limit("partial JSON parser resource limit".into())),
            // An unfinished string/key is held back until later bytes arrive.
            // Malformed JSON is handled by final Model validation and correction.
            Err(_) => (),
        }
        Ok(())
    }
}
impl<T: Model + AsyncCheck + Send + Sync + 'static> OutputMode for PartialMode<T> {
    type Output = T;
    const STREAM: bool = true;
    fn schema(&self) -> Result<Value> {
        crate::schema::model_schema::<T>()
    }
    fn finish_call(&mut self, response: &crate::ModelResponse) -> Result<()> {
        self.text.final_text(&response.text()?)
    }
    fn start<'a>(&'a mut self, attempt: u32, context: &'a RunContext) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.text = TextBuffer::new(self.text.options);
            self.last = None;
            self.sink.start(attempt, context).await
        })
    }
    fn observe<'a>(
        &'a mut self,
        event: &'a Value,
        context: &'a RunContext,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.text.delta(event)? {
                self.preview(context).await?;
            }
            Ok(())
        })
    }
    fn reset<'a>(
        &'a mut self,
        reason: ResetReason,
        context: &'a RunContext,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.sink.reset(reason, context).await })
    }
    fn finish<'a>(
        &'a mut self,
        text: &'a str,
        context: &'a RunContext,
    ) -> BoxFuture<'a, Result<T>> {
        Box::pin(async move {
            self.preview(context).await?;
            T::parse_json_async(text).await.map_err(Error::Validation)
        })
    }
}

#[allow(dead_code)]
#[derive(Model)]
struct Envelope<T> {
    items: Vec<T>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnvelope<'a> {
    #[serde(borrow)]
    items: Vec<&'a RawValue>,
}

struct IterableMode<T> {
    sink: Sink<StreamItem<T>, usize>,
    text: TextBuffer,
    parser: ItemParser,
    emitted: usize,
    failure: Option<ValidationErrors>,
    marker: PhantomData<fn() -> T>,
}
impl<T: Model + AsyncCheck + Send + Sync + 'static> IterableMode<T> {
    async fn items(&mut self, context: &RunContext) -> Result<()> {
        while self.failure.is_none() {
            let range = match self.parser.next(&self.text.value) {
                Ok(Some(range)) => range,
                Ok(None) => break,
                Err(errors) => {
                    self.failure = Some(errors);
                    break;
                }
            };
            if self.emitted >= self.text.options.max_items {
                return Err(Error::Limit("structured output item limit".into()));
            }
            match context
                .wait(async {
                    T::parse_json_async(&self.text.value[range])
                        .await
                        .map_err(Error::Validation)
                })
                .await
            {
                Ok(value) => {
                    self.sink
                        .delta(
                            StreamItem {
                                index: self.emitted,
                                value,
                            },
                            context,
                        )
                        .await?;
                    self.emitted += 1;
                }
                Err(Error::Validation(errors)) => {
                    self.failure = Some(rydantic::Context::prepend_errors(
                        rydantic::Context::prepend_errors(errors, self.emitted),
                        "items",
                    ));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}
impl<T: Model + AsyncCheck + Send + Sync + 'static> OutputMode for IterableMode<T> {
    type Output = usize;
    const STREAM: bool = true;
    fn schema(&self) -> Result<Value> {
        crate::schema::model_schema::<Envelope<T>>()
    }
    fn finish_call(&mut self, response: &crate::ModelResponse) -> Result<()> {
        self.text.final_text(&response.text()?)
    }
    fn start<'a>(&'a mut self, attempt: u32, context: &'a RunContext) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.text = TextBuffer::new(self.text.options);
            self.parser = ItemParser::default();
            self.emitted = 0;
            self.failure = None;
            self.sink.start(attempt, context).await
        })
    }
    fn observe<'a>(
        &'a mut self,
        event: &'a Value,
        context: &'a RunContext,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.text.delta(event)? {
                self.items(context).await?;
            }
            Ok(())
        })
    }
    fn reset<'a>(
        &'a mut self,
        reason: ResetReason,
        context: &'a RunContext,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.sink.reset(reason, context).await })
    }
    fn finish<'a>(
        &'a mut self,
        text: &'a str,
        context: &'a RunContext,
    ) -> BoxFuture<'a, Result<usize>> {
        Box::pin(async move {
            self.items(context).await?;
            let envelope: RawEnvelope<'_> =
                serde_json::from_str(text).map_err(|_| Error::Validation(invalid_envelope()))?;
            if let Some(errors) = self.failure.take() {
                return Err(Error::Validation(errors));
            }
            if self.emitted != envelope.items.len() || !self.parser.finished() {
                return Err(Error::Protocol(
                    "iterable parser completion mismatch".into(),
                ));
            }
            Ok(self.emitted)
        })
    }
}

fn invalid_envelope() -> ValidationErrors {
    ValidationError::new(
        "structured_json",
        "expected exactly one JSON object with an items array, without duplicate or extra fields",
    )
    .into()
}
#[derive(Default)]
enum ItemState {
    #[default]
    Open,
    Key,
    Colon,
    Array,
    Item {
        allow_end: bool,
    },
    Close,
    End,
}
#[derive(Default)]
struct ItemParser {
    offset: usize,
    state: ItemState,
}
impl ItemParser {
    fn finished(&self) -> bool {
        matches!(self.state, ItemState::End)
    }
    // Completed elements are parsed through RawValue, keeping exact integer and
    // decimal lexemes. A right delimiter is required before emitting any item.
    fn next(&mut self, text: &str) -> std::result::Result<Option<Range<usize>>, ValidationErrors> {
        loop {
            while text
                .as_bytes()
                .get(self.offset)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.offset += 1;
            }
            let Some(&byte) = text.as_bytes().get(self.offset) else {
                return Ok(None);
            };
            let expect = match self.state {
                ItemState::Open => Some((b'{', ItemState::Key)),
                ItemState::Colon => Some((b':', ItemState::Array)),
                ItemState::Array => Some((b'[', ItemState::Item { allow_end: true })),
                ItemState::Close => Some((b'}', ItemState::End)),
                _ => None,
            };
            if let Some((expected, next)) = expect {
                if byte != expected {
                    return Err(invalid_envelope());
                }
                self.offset += 1;
                self.state = next;
                continue;
            }
            match self.state {
                ItemState::Key => {
                    let mut parser = serde_json::Deserializer::from_str(&text[self.offset..])
                        .into_iter::<String>();
                    match parser.next() {
                        Some(Ok(key)) if key == "items" => {
                            self.offset += parser.byte_offset();
                            self.state = ItemState::Colon;
                        }
                        // Parsers can classify unfinished numeric/escape tokens
                        // as syntax errors. Final strict parsing decides validity.
                        Some(Err(_)) => return Ok(None),
                        _ => return Err(invalid_envelope()),
                    }
                }
                ItemState::Item { allow_end } => {
                    if byte == b']' && allow_end {
                        self.offset += 1;
                        self.state = ItemState::Close;
                        continue;
                    }
                    let mut parser = serde_json::Deserializer::from_str(&text[self.offset..])
                        .into_iter::<&RawValue>();
                    let end = match parser.next() {
                        Some(Ok(_)) => self.offset + parser.byte_offset(),
                        Some(Err(_)) => return Ok(None),
                        _ => return Err(invalid_envelope()),
                    };
                    let mut delimiter = end;
                    while text
                        .as_bytes()
                        .get(delimiter)
                        .is_some_and(u8::is_ascii_whitespace)
                    {
                        delimiter += 1;
                    }
                    let Some(&byte) = text.as_bytes().get(delimiter) else {
                        return Ok(None);
                    };
                    let next = match byte {
                        b',' => ItemState::Item { allow_end: false },
                        b']' => ItemState::Close,
                        _ => return Err(invalid_envelope()),
                    };
                    let range = self.offset..end;
                    self.offset = delimiter + 1;
                    self.state = next;
                    return Ok(Some(range));
                }
                ItemState::End => return Err(invalid_envelope()),
                _ => unreachable!(),
            }
        }
    }
}
