use crate::{Error, ModelProvider, ModelRequest, Result, RunContext, Tool, Usage};
use rydantic::{AsyncCheck, Model};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    marker::PhantomData,
    sync::Arc,
};
use tracing::Instrument;

#[derive(Debug, Clone)]
pub struct AgentLimits {
    pub max_model_calls: u32,
    pub max_tool_calls: u32,
    pub max_validation_retries: u32,
    pub max_total_tokens: u64,
    pub max_output_tokens: u32,
}
impl Default for AgentLimits {
    fn default() -> Self {
        Self {
            max_model_calls: 12,
            max_tool_calls: 32,
            max_validation_retries: 2,
            max_total_tokens: 100_000,
            max_output_tokens: 2048,
        }
    }
}

pub struct RunResult<O> {
    pub output: O,
    pub usage: Usage,
    pub history: Vec<Value>,
    pub model_calls: u32,
    pub tool_calls: u32,
}

pub struct Agent<D, O> {
    provider: Arc<dyn ModelProvider>,
    model: String,
    instructions: String,
    tools: BTreeMap<String, Arc<dyn Tool<D>>>,
    limits: AgentLimits,
    output: PhantomData<fn() -> O>,
}

impl<D, O> Agent<D, O>
where
    D: Send + Sync + 'static,
    O: Model + AsyncCheck + Send + Sync + 'static,
{
    pub fn new(provider: Arc<dyn ModelProvider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            instructions: String::new(),
            tools: BTreeMap::new(),
            limits: AgentLimits::default(),
            output: PhantomData,
        }
    }
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = instructions.into();
        self
    }
    pub fn limits(mut self, limits: AgentLimits) -> Self {
        self.limits = limits;
        self
    }
    pub fn tool(mut self, tool: impl Tool<D> + 'static) -> Result<Self> {
        let definition = tool.definition();
        if self.tools.contains_key(&definition.name) {
            return Err(Error::Configuration("duplicate tool name".into()));
        }
        self.tools.insert(definition.name, Arc::new(tool));
        Ok(self)
    }
    pub async fn run(
        &self,
        prompt: impl Into<String>,
        deps: Arc<D>,
        context: RunContext,
    ) -> Result<RunResult<O>> {
        let prompt = prompt.into();
        self.run_inner(prompt, deps, context)
            .instrument(tracing::info_span!("rydantic_ai.agent", model = %self.model))
            .await
    }
    async fn run_inner(
        &self,
        prompt: String,
        deps: Arc<D>,
        context: RunContext,
    ) -> Result<RunResult<O>> {
        if self.limits.max_model_calls == 0 || self.limits.max_output_tokens == 0 {
            return Err(Error::Configuration("agent limits must be positive".into()));
        }
        let capabilities = self.provider.capabilities();
        let schema = crate::schema::model_schema::<O>()?;
        if capabilities.native_json_schema && schema["type"] != "object" {
            return Err(Error::Configuration("native structured output requires an object model; use an instruction-based provider for other shapes".into()));
        }
        let mut request = ModelRequest::new(&self.model, prompt);
        request.instructions = Some(format!("{}\nReturn the final answer as one JSON value matching this schema. Do not use Markdown fences.\n{}", self.instructions, schema));
        request.tools = self.tools.values().map(|t| t.definition()).collect();
        if capabilities.native_json_schema {
            request.text = Some(
                json!({"format":{"type":"json_schema","name":"result","schema":schema,"strict":false}}),
            );
        }
        if capabilities.output_token_limit {
            request.max_output_tokens = Some(self.limits.max_output_tokens);
        }
        let mut usage = Usage::default();
        let mut retries = 0;
        let mut tool_calls = 0;
        let mut seen_calls = HashSet::new();
        for model_calls in 1..=self.limits.max_model_calls {
            context.check()?;
            let response = context
                .wait(self.provider.complete(request.clone(), context.clone()))
                .await?;
            usage.add(response.usage)?;
            tracing::info!(
                model_calls,
                input_tokens = response.usage.input_tokens,
                output_tokens = response.usage.output_tokens,
                "model completed"
            );
            if usage.total()? > self.limits.max_total_tokens {
                return Err(Error::Limit("agent total token limit".into()));
            }
            let calls = response.tool_calls()?;
            request.input.extend_from_slice(response.output());
            if calls.is_empty() {
                let text = response.text()?;
                match context
                    .wait(async { O::parse_json_async(&text).await.map_err(Error::Validation) })
                    .await
                {
                    Ok(output) => {
                        return Ok(RunResult {
                            output,
                            usage,
                            history: request.input,
                            model_calls,
                            tool_calls,
                        })
                    }
                    Err(Error::Validation(errors))
                        if retries < self.limits.max_validation_retries =>
                    {
                        retries += 1;
                        tracing::warn!(retries, "output validation failed");
                        request.input.push(json!({"role":"user","content":format!("Correct the final JSON. Validation errors: {errors}")}));
                    }
                    Err(error) => return Err(error),
                }
                continue;
            }
            for call in calls {
                if tool_calls >= self.limits.max_tool_calls {
                    return Err(Error::Limit("tool call limit".into()));
                }
                if !seen_calls.insert(call.call_id.clone()) {
                    return Err(Error::Protocol("duplicate tool call id".into()));
                }
                tool_calls += 1;
                let result = if let Some(tool) = self.tools.get(&call.name) {
                    context
                        .wait(tool.call(call.arguments, deps.clone(), context.clone()))
                        .instrument(tracing::info_span!("rydantic_ai.tool", tool = %call.name))
                        .await
                } else {
                    Err(Error::UnknownTool)
                };
                let output = match result {
                    Ok(output) => output,
                    Err(error @ (Error::ToolArguments(_) | Error::UnknownTool))
                        if retries < self.limits.max_validation_retries =>
                    {
                        retries += 1;
                        json!({"error":"invalid_tool_arguments","message":error.to_string()})
                            .to_string()
                    }
                    Err(error) => return Err(error),
                };
                request.input.push(
                    json!({"type":"function_call_output","call_id":call.call_id,"output":output}),
                );
            }
        }
        Err(Error::Limit("model call limit".into()))
    }
}
