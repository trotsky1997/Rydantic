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

// Keep compile-time labels borrowed until an error needs its public owned location.
#[derive(Debug, Clone)]
enum ContextSegment {
    Field(Cow<'static, str>),
    Index(usize),
}
impl From<PathSegment> for ContextSegment {
    fn from(segment: PathSegment) -> Self {
        match segment {
            PathSegment::Field(field) => Self::Field(Cow::Owned(field)),
            PathSegment::Index(index) => Self::Index(index),
        }
    }
}
impl ContextSegment {
    fn to_path_segment(&self) -> PathSegment {
        match self {
            Self::Field(field) => PathSegment::Field(field.to_string()),
            Self::Index(index) => PathSegment::Index(*index),
        }
    }
}

/// Shared error collector used by generated code and custom `JsonType` implementations.
#[derive(Default)]
pub struct Context {
    path: Vec<ContextSegment>,
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
            .map(ContextSegment::to_path_segment)
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
        self.at_segment(segment.into().into(), f)
    }
    /// Enter a compile-time field label without allocating it on the success path.
    #[doc(hidden)]
    pub fn at_static<T>(&mut self, field: &'static str, f: impl FnOnce(&mut Self) -> T) -> T {
        self.at_segment(ContextSegment::Field(Cow::Borrowed(field)), f)
    }
    fn at_segment<T>(&mut self, segment: ContextSegment, f: impl FnOnce(&mut Self) -> T) -> T {
        self.path.push(segment);
        let result = f(self);
        self.path.pop();
        result
    }
    pub fn child(&self, segment: Option<PathSegment>) -> Self {
        self.child_segment(segment.map(Into::into))
    }
    /// Start a child collector beneath a compile-time field label.
    #[doc(hidden)]
    pub fn child_static(&self, field: &'static str) -> Self {
        self.child_segment(Some(ContextSegment::Field(Cow::Borrowed(field))))
    }
    fn child_segment(&self, segment: Option<ContextSegment>) -> Self {
        let mut path = self.path.clone();
        if let Some(segment) = segment {
            path.push(segment);
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

#[cfg(test)]
mod context_path_tests {
    use super::*;

    #[test]
    fn static_labels_remain_borrowed_in_children_until_an_error_is_recorded() {
        let mut ctx = Context::with_data(Arc::new(42u64));
        ctx.at_static("outer", |ctx| {
            assert!(matches!(
                ctx.path[0],
                ContextSegment::Field(Cow::Borrowed("outer"))
            ));
            let mut child = ctx.child_static("inner");
            assert!(matches!(
                child.path[1],
                ContextSegment::Field(Cow::Borrowed("inner"))
            ));
            assert_eq!(*child.require_data::<u64>().unwrap(), 42);
            child.push(ValidationError::custom("failed").at(7usize));
            assert_eq!(
                child.errors[0].loc,
                vec!["outer".into(), "inner".into(), 7usize.into()]
            );
            ctx.absorb(child);
        });
        assert!(ctx.path.is_empty());
        assert_eq!(
            ctx.result().unwrap_err().errors()[0].pointer(),
            "/outer/inner/7"
        );
    }

    fn diagnostics(static_labels: bool) -> (String, ValidationErrors) {
        fn enter<T>(
            ctx: &mut Context,
            label: &'static str,
            static_labels: bool,
            f: impl FnOnce(&mut Context) -> T,
        ) -> T {
            if static_labels {
                ctx.at_static(label, f)
            } else {
                ctx.at(label, f)
            }
        }
        let mut ctx = Context::new();
        let debug = enter(&mut ctx, "outer/~雪", static_labels, |ctx| {
            let debug = format!("{ctx:?}");
            // Dynamic strings remain accepted and owned beyond the caller's lifetime.
            let key = String::from("dynamic/~");
            ctx.at(key.as_str(), |ctx| {
                ctx.at(2usize, |ctx| {
                    ctx.extend(ValidationErrors(vec![
                        ValidationError::custom("first").at("relative")
                    ]));
                    let mut child = if static_labels {
                        ctx.child_static("child")
                    } else {
                        ctx.child(Some("child".into()))
                    };
                    child.push(ValidationError::new("second", "second error").with_param("n", 3));
                    ctx.absorb(child);
                    let mut trial = ctx.trial();
                    trial.push(ValidationError::custom("relative trial"));
                    assert!(trial.errors[0].loc.is_empty());
                    let mut same_path = ctx.child(None);
                    same_path.push(ValidationError::custom("same path"));
                    ctx.absorb(same_path);
                    None::<()>
                })
            });
            ctx.push(ValidationError::custom("after early return"));
            debug
        });
        ctx.push(ValidationError::custom("root"));
        (debug, ctx.result().unwrap_err())
    }

    #[test]
    fn static_and_owned_routes_have_identical_full_diagnostics_and_stack_recovery() {
        let (static_debug, static_errors) = diagnostics(true);
        let (owned_debug, owned_errors) = diagnostics(false);
        assert_eq!(static_debug, owned_debug);
        assert_eq!(static_errors, owned_errors);
        assert_eq!(static_errors.to_json(), owned_errors.to_json());
        assert_eq!(static_errors.errors()[3].pointer(), "/outer~1~0雪");
        assert_eq!(static_errors.errors()[4].pointer(), "");
        assert_eq!(
            serde_json::from_value::<ValidationErrors>(static_errors.to_json()).unwrap(),
            static_errors
        );
    }
}
