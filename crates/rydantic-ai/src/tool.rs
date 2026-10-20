use crate::{BoxFuture, Error, Result, RunContext, ToolDefinition};
use rydantic::{AsyncCheck, Model};
use std::{future::Future, marker::PhantomData, sync::Arc};

pub trait Tool<D>: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    fn call(
        &self,
        arguments: String,
        deps: Arc<D>,
        context: RunContext,
    ) -> BoxFuture<'_, Result<String>>;
}

pub struct TypedTool<D, A, O, F> {
    name: String,
    description: String,
    function: F,
    parameters: serde_json::Value,
    marker: PhantomData<fn(D, A) -> O>,
}

impl<D, A, O, F, Fut> TypedTool<D, A, O, F>
where
    A: Model + AsyncCheck + Send + Sync + 'static,
    O: Model + AsyncCheck + Send + Sync + 'static,
    D: Send + Sync + 'static,
    F: Fn(A, Arc<D>, RunContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<O>> + Send,
{
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        function: F,
    ) -> Result<Self> {
        let name = name.into();
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(Error::Configuration("invalid tool name".into()));
        }
        let parameters = crate::schema::model_schema::<A>()?;
        if parameters["type"] != "object" {
            return Err(Error::Configuration(
                "tool argument model must be an object".into(),
            ));
        }
        Ok(Self {
            name,
            description: description.into(),
            function,
            parameters,
            marker: PhantomData,
        })
    }
}

impl<D, A, O, F, Fut> Tool<D> for TypedTool<D, A, O, F>
where
    A: Model + AsyncCheck + Send + Sync + 'static,
    O: Model + AsyncCheck + Send + Sync + 'static,
    D: Send + Sync + 'static,
    F: Fn(A, Arc<D>, RunContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<O>> + Send,
{
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            kind: "function".into(),
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
            strict: false,
        }
    }
    fn call(
        &self,
        arguments: String,
        deps: Arc<D>,
        context: RunContext,
    ) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            // Argument validation happens before the handler. Only this error can be retried safely.
            let args = context
                .wait(async {
                    A::parse_json_async(&arguments)
                        .await
                        .map_err(Error::ToolArguments)
                })
                .await?;
            let output = context
                .wait((self.function)(args, deps, context.clone()))
                .await
                .map_err(|error| match error {
                    Error::Cancelled | Error::Timeout => error,
                    _ => Error::Tool(error.to_string()),
                })?;
            context
                .wait(async {
                    output
                        .dump_json_async()
                        .await
                        .map_err(|_| Error::Tool("invalid tool result after execution".into()))
                })
                .await
        })
    }
}
