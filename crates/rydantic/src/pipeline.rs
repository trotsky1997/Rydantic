//! Explicit, typed callback composition.
//!
//! A pipeline does not imply model validation. Use a model parsing/validation
//! entry point as a stage when that guarantee is required. `plain` replaces a
//! selected stage; `wrap` can skip or retry its continuation deliberately.

use crate::BoxFuture;
use std::sync::Arc;

type Step<'p, I, O, C, E> = dyn Fn(I, &C) -> Result<O, E> + Send + Sync + 'p;

/// A reusable typed pipeline, sequential and fail-fast between stages.
///
/// Context is borrowed for each call, never stored. Error types are preserved:
/// use `ValidationError` for field callbacks and `ValidationErrors` for model
/// callbacks. Synchronous pipelines may borrow captured data for `'p`.
///
/// ```
/// use rydantic::{pipeline::Pipeline, ValidationError};
/// let parse = Pipeline::plain(|text: &str, _: &usize| {
///     text.parse::<usize>().map_err(|_| ValidationError::new("number", "invalid"))
/// }).check(|value, limit| {
///     if value <= limit { Ok(()) } else { Err(ValidationError::new("limit", "too large")) }
/// }).then(|value, _| Ok(value.to_string()));
/// assert_eq!(parse.run("3", &5).unwrap(), "3");
/// ```
///
/// Adjacent stage types and context types are checked by Rust:
/// ```compile_fail
/// use rydantic::{pipeline::Pipeline, ValidationError};
/// let p = Pipeline::plain(|_: (), _: &()| Ok::<u32, ValidationError>(1));
/// let wrong = p.then(|value: String, _: &()| Ok(value));
/// ```
pub struct Pipeline<'p, I, O, C, E> {
    step: Arc<Step<'p, I, O, C, E>>,
}
impl<I, O, C, E> Clone for Pipeline<'_, I, O, C, E> {
    fn clone(&self) -> Self {
        Self {
            step: self.step.clone(),
        }
    }
}
impl<'p, I: 'p, O: 'p, C: 'p, E: 'p> Pipeline<'p, I, O, C, E> {
    /// Construct a plain stage. No implicit decoding, checks or normalization.
    pub fn plain(f: impl Fn(I, &C) -> Result<O, E> + Send + Sync + 'p) -> Self {
        Self { step: Arc::new(f) }
    }
    pub fn run(&self, input: I, context: &C) -> Result<O, E> {
        (self.step)(input, context)
    }
    /// Transform the preceding output. A failure skips all subsequent stages.
    pub fn then<N: 'p>(
        self,
        f: impl Fn(O, &C) -> Result<N, E> + Send + Sync + 'p,
    ) -> Pipeline<'p, I, N, C, E> {
        Pipeline::plain(move |input, context| f(self.run(input, context)?, context))
    }
    /// Check without changing the output; failure stops this pipeline.
    pub fn check(self, f: impl Fn(&O, &C) -> Result<(), E> + Send + Sync + 'p) -> Self {
        self.then(move |value, context| {
            f(&value, context)?;
            Ok(value)
        })
    }
    /// Intercept this pipeline. The continuation is reusable, not automatically
    /// invoked. Retrying repeats its side effects; skipping bypasses its checks.
    pub fn wrap(
        self,
        f: impl Fn(I, &C, &dyn Fn(I, &C) -> Result<O, E>) -> Result<O, E> + Send + Sync + 'p,
    ) -> Self {
        Self::plain(move |input, context| {
            f(input, context, &|value, context| self.run(value, context))
        })
    }
}

type AsyncStep<I, O, C, E> = dyn for<'a> Fn(I, &'a C) -> BoxFuture<'a, Result<O, E>> + Send + Sync;

/// Owned, Send asynchronous composition with borrowed per-call context.
///
/// Stages execute sequentially, without spawning tasks or implicit retries.
/// Dropping a pending run drops its current stage and never starts later stages.
/// Completed side effects are not rolled back; synchronous code is not preempted.
/// Inputs, outputs and captured configuration are owned (`'static`); the context
/// and future may borrow for the duration of one call.
pub struct AsyncPipeline<I, O, C, E> {
    step: Arc<AsyncStep<I, O, C, E>>,
}
impl<I, O, C, E> Clone for AsyncPipeline<I, O, C, E> {
    fn clone(&self) -> Self {
        Self {
            step: self.step.clone(),
        }
    }
}
impl<I: Send + 'static, O: Send + 'static, C: Sync + 'static, E: Send + 'static>
    AsyncPipeline<I, O, C, E>
{
    /// A plain async stage. Returning a value does not assert model validity.
    pub fn plain(
        f: impl for<'a> Fn(I, &'a C) -> BoxFuture<'a, Result<O, E>> + Send + Sync + 'static,
    ) -> Self {
        Self { step: Arc::new(f) }
    }
    pub fn run<'a>(&'a self, input: I, context: &'a C) -> BoxFuture<'a, Result<O, E>> {
        let step = self.step.clone();
        Box::pin(async move { step(input, context).await })
    }
    pub fn then<N: Send + 'static>(
        self,
        f: impl for<'a> Fn(O, &'a C) -> BoxFuture<'a, Result<N, E>> + Send + Sync + 'static,
    ) -> AsyncPipeline<I, N, C, E> {
        let f = Arc::new(f);
        AsyncPipeline::plain(move |input, context| {
            let previous = self.clone();
            let f = f.clone();
            Box::pin(async move { f(previous.run(input, context).await?, context).await })
        })
    }
    pub fn check(
        self,
        f: impl for<'a> Fn(&'a O, &'a C) -> BoxFuture<'a, Result<(), E>> + Send + Sync + 'static,
    ) -> Self
    where
        O: Sync,
    {
        let f = Arc::new(f);
        self.then(move |value, context| {
            let f = f.clone();
            Box::pin(async move {
                f(&value, context).await?;
                Ok(value)
            })
        })
    }
    /// The wrapper controls whether and how often the previous pipeline runs.
    pub fn wrap(
        self,
        f: impl for<'a> Fn(I, &'a C, &'a Self) -> BoxFuture<'a, Result<O, E>> + Send + Sync + 'static,
    ) -> Self {
        let f = Arc::new(f);
        Self::plain(move |input, context| {
            let previous = self.clone();
            let f = f.clone();
            Box::pin(async move { f(input, context, &previous).await })
        })
    }
}
