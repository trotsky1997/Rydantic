use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{any::Any, borrow::Cow, collections::BTreeMap, fmt, sync::Arc};

/// A location in a nested input. Object keys and array indices remain distinct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathSegment {
    Field(String),
    Index(usize),
}

impl From<&str> for PathSegment {
    fn from(value: &str) -> Self {
        Self::Field(value.to_owned())
    }
}
impl From<String> for PathSegment {
    fn from(value: String) -> Self {
        Self::Field(value)
    }
}
impl From<usize> for PathSegment {
    fn from(value: usize) -> Self {
        Self::Index(value)
    }
}

#[derive(Clone)]
enum ContextPathSegment {
    Field(Cow<'static, str>),
    Index(usize),
}
impl From<PathSegment> for ContextPathSegment {
    fn from(value: PathSegment) -> Self {
        match value {
            PathSegment::Field(field) => Self::Field(Cow::Owned(field)),
            PathSegment::Index(index) => Self::Index(index),
        }
    }
}
impl ContextPathSegment {
    fn owned(&self) -> PathSegment {
        match self {
            Self::Field(field) => PathSegment::Field(field.to_string()),
            Self::Index(index) => PathSegment::Index(*index),
        }
    }
}
impl fmt::Debug for ContextPathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.owned().fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Parse,
    Validate,
    Serialize,
}

/// Machine-readable validation failure. Raw input is deliberately not retained.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationError {
    pub loc: Vec<PathSegment>,
    pub code: String,
    pub message: String,
    pub stage: Stage,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

impl ValidationError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            loc: vec![],
            code: code.into(),
            message: message.into(),
            stage: Stage::Validate,
            params: BTreeMap::new(),
        }
    }
    pub fn custom(message: impl Into<String>) -> Self {
        Self::new("custom", message)
    }
    /// Add a relative location (outermost segment first).
    pub fn at(mut self, segment: impl Into<PathSegment>) -> Self {
        self.loc.push(segment.into());
        self
    }
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }
    pub fn with_stage(mut self, stage: Stage) -> Self {
        self.stage = stage;
        self
    }
    pub fn pointer(&self) -> String {
        self.loc
            .iter()
            .map(|part| match part {
                PathSegment::Field(key) => {
                    format!("/{}", key.replace('~', "~0").replace('/', "~1"))
                }
                PathSegment::Index(index) => format!("/{index}"),
            })
            .collect()
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}: {} [{}]", self.pointer(), self.message, self.code)
    }
}
impl std::error::Error for ValidationError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ValidationErrors(pub Vec<ValidationError>);

impl ValidationErrors {
    pub fn new(errors: Vec<ValidationError>) -> Self {
        Self(errors)
    }
    pub fn errors(&self) -> &[ValidationError] {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("error fields are JSON-compatible")
    }
}
impl From<ValidationError> for ValidationErrors {
    fn from(error: ValidationError) -> Self {
        Self(vec![error])
    }
}
impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} validation error(s)", self.len())?;
        for (index, error) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}
impl std::error::Error for ValidationErrors {}

/// Shared error collector used by generated code and custom `JsonType` implementations.
#[derive(Default)]
pub struct Context {
    path: Vec<ContextPathSegment>,
    errors: Vec<ValidationError>,
    data: Option<Arc<dyn Any + Send + Sync>>,
}
impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("path", &self.path)
            .field("errors", &self.errors)
            .field("has_data", &self.data.is_some())
            .finish()
    }
}
impl Context {
    pub fn new() -> Self {
        Self::default()
    }
    /// Create a fresh collector carrying one explicitly supplied business context.
    /// The data is shared, never copied, with child collectors. `Any` requires
    /// owned (`'static`) data; `Send + Sync` keeps async validation futures Send.
    /// Interior mutation and deliberately sharing an Arc between requests remain
    /// the caller's responsibility. Debug output never includes this data.
    pub fn with_data<C: Any + Send + Sync>(data: Arc<C>) -> Self {
        Self {
            data: Some(data),
            ..Self::default()
        }
    }
    /// Retrieve the exact supplied type, or return a relative validation error.
    /// Callbacks should propagate this error with `?`; low-level implementations
    /// should push it into the collector to attach the current location.
    pub fn require_data<C: Any + Send + Sync>(&self) -> Result<&C, ValidationError> {
        let data = self.data.as_ref().ok_or_else(|| {
            ValidationError::new("context_missing", "validation requires business context")
        })?;
        data.downcast_ref::<C>().ok_or_else(|| {
            ValidationError::new(
                "context_type_mismatch",
                "business context has an unexpected type",
            )
        })
    }
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
    pub fn push(&mut self, mut error: ValidationError) {
        let mut path = self
            .path
            .iter()
            .map(ContextPathSegment::owned)
            .collect::<Vec<_>>();
        path.append(&mut error.loc);
        error.loc = path;
        self.errors.push(error);
    }
    pub fn extend(&mut self, errors: ValidationErrors) {
        for error in errors.0 {
            self.push(error);
        }
    }
    pub fn at<T>(&mut self, segment: impl Into<PathSegment>, f: impl FnOnce(&mut Self) -> T) -> T {
        self.path.push(segment.into().into());
        let result = f(self);
        self.path.pop();
        result
    }
    #[doc(hidden)]
    pub fn at_static<T>(&mut self, segment: &'static str, f: impl FnOnce(&mut Self) -> T) -> T {
        self.path
            .push(ContextPathSegment::Field(Cow::Borrowed(segment)));
        let result = f(self);
        self.path.pop();
        result
    }
    pub fn child(&self, segment: Option<PathSegment>) -> Self {
        let mut path = self.path.clone();
        if let Some(segment) = segment {
            path.push(segment.into());
        }
        Self {
            path,
            errors: vec![],
            data: self.data.clone(),
        }
    }
    /// Start an isolated branch trial with relative diagnostic paths and the
    /// same application data. Unlike `child`, this resets the path because trial
    /// errors are embedded beneath a single outer union error rather than absorbed.
    #[doc(hidden)]
    pub fn trial(&self) -> Self {
        Self {
            path: vec![],
            errors: vec![],
            data: self.data.clone(),
        }
    }
    pub fn absorb(&mut self, child: Self) {
        self.errors.extend(child.errors);
    }
    pub fn prepend_errors(
        errors: ValidationErrors,
        segment: impl Into<PathSegment>,
    ) -> ValidationErrors {
        let segment = segment.into();
        ValidationErrors(
            errors
                .0
                .into_iter()
                .map(|mut error| {
                    error.loc.insert(0, segment.clone());
                    error
                })
                .collect(),
        )
    }
    pub fn finish<T>(self, value: Option<T>) -> Result<T, ValidationErrors> {
        if !self.errors.is_empty() {
            return Err(ValidationErrors(self.errors));
        }
        value.ok_or_else(|| {
            ValidationError::new("internal", "decoder returned no value without an error").into()
        })
    }
    pub fn result(self) -> Result<(), ValidationErrors> {
        self.finish(Some(()))
    }
}
