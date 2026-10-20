//! Typed TypeSafe Jev System One questions, answers and an optional HTTP client.
//!
//! Noul answers are probabilities. Scores may be fractional. The complete answer
//! distribution remains available when projecting results into a Rydantic model.

#[cfg(feature = "http")]
mod client;
#[cfg(feature = "http")]
pub use client::JevClient;

use crate::{Error, Result, RunContext, Usage};
use rydantic::{AsyncCheck, Model};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// JSON content accepted by Jev. Null descriptions use `Option<Content>`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Object(Map<String, Value>),
    Array(Vec<Value>),
}
impl<'de> Deserialize<'de> for Content {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        Self::try_from(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl From<&str> for Content {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}
impl From<String> for Content {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<Map<String, Value>> for Content {
    fn from(value: Map<String, Value>) -> Self {
        Self::Object(value)
    }
}
impl From<Vec<Value>> for Content {
    fn from(value: Vec<Value>) -> Self {
        Self::Array(value)
    }
}
impl TryFrom<Value> for Content {
    type Error = Error;
    fn try_from(value: Value) -> Result<Self> {
        match value {
            Value::String(value) => Ok(Self::Text(value)),
            Value::Object(value) => Ok(Self::Object(value)),
            Value::Array(value) => Ok(Self::Array(value)),
            _ => Err(Error::Configuration(
                "Jev content must be text, an object or an array".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoulCriteria {
    #[serde(rename = "true", default, skip_serializing_if = "Option::is_none")]
    pub when_true: Option<Content>,
    #[serde(rename = "false", default, skip_serializing_if = "Option::is_none")]
    pub when_false: Option<Content>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Question {
    Noul {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<Content>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<Content>,
        criteria: BTreeMap<String, Option<Content>>,
    },
    Score {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<Content>,
        criteria: Vec<Content>,
    },
}
impl Question {
    pub fn noul(instructions: impl Into<Content>) -> Self {
        Self::Noul {
            instructions: Some(instructions.into()),
            criteria: None,
        }
    }
    pub fn choice<K: Into<String>, V: Into<Content>>(
        instructions: impl Into<Content>,
        criteria: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Self::Choice {
            instructions: Some(instructions.into()),
            criteria: criteria
                .into_iter()
                .map(|(k, v)| (k.into(), Some(v.into())))
                .collect(),
        }
    }
    pub fn score<V: Into<Content>>(
        instructions: impl Into<Content>,
        criteria: impl IntoIterator<Item = V>,
    ) -> Self {
        Self::Score {
            instructions: Some(instructions.into()),
            criteria: criteria.into_iter().map(Into::into).collect(),
        }
    }
    fn validate(&self) -> Result<()> {
        let valid = match self {
            Self::Noul { .. } => true,
            Self::Choice { criteria, .. } => (1..=255).contains(&criteria.len()),
            Self::Score { criteria, .. } => (1..=10).contains(&criteria.len()),
        };
        if valid {
            Ok(())
        } else {
            Err(Error::Configuration(
                "Jev choice needs 1..=255 options; score needs 1..=10 levels".into(),
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemOneRequest {
    pub model: String,
    pub state: Content,
    pub questions: BTreeMap<String, Question>,
}
impl SystemOneRequest {
    pub fn new(model: impl Into<String>, state: impl Into<Content>) -> Self {
        Self {
            model: model.into(),
            state: state.into(),
            questions: BTreeMap::new(),
        }
    }
    /// Duplicate names fail instead of silently replacing a question.
    pub fn question(mut self, name: impl Into<String>, question: Question) -> Result<Self> {
        let name = name.into();
        if self.questions.contains_key(&name) {
            return Err(Error::Configuration("duplicate Jev question name".into()));
        }
        question.validate()?;
        self.questions.insert(name, question);
        Ok(self)
    }
    pub fn validate(&self) -> Result<()> {
        if self.model.trim().is_empty() || self.questions.is_empty() {
            return Err(Error::Configuration(
                "Jev needs a model and at least one question".into(),
            ));
        }
        for question in self.questions.values() {
            question.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulAnswer {
    pub noul: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceAnswer {
    pub choice: String,
    pub confidence: f64,
    pub probabilities: BTreeMap<String, f64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreAnswer {
    pub score: f64,
    pub confidence: f64,
    pub legend: BTreeMap<String, Content>,
    pub probabilities: BTreeMap<String, f64>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Noul(NoulAnswer),
    Choice(ChoiceAnswer),
    Score(ScoreAnswer),
}
impl<'de> Deserialize<'de> for Answer {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        // Decode numeric values before discriminating. Serde's internally tagged
        // buffering otherwise conflicts with serde_json's arbitrary_precision floats.
        let value = Value::deserialize(deserializer)?;
        match value.get("type").and_then(Value::as_str) {
            Some("noul") => serde_json::from_value(value).map(Self::Noul),
            Some("choice") => serde_json::from_value(value).map(Self::Choice),
            Some("score") => serde_json::from_value(value).map(Self::Score),
            _ => return Err(serde::de::Error::custom("unsupported Jev answer type")),
        }
        .map_err(serde::de::Error::custom)
    }
}

fn invalid_response() -> Error {
    Error::Protocol("invalid Jev answer or question/answer mismatch".into())
}
fn probability(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
fn distribution(values: &BTreeMap<String, f64>) -> bool {
    !values.is_empty() && values.values().copied().all(probability)
}
impl Answer {
    fn valid(&self) -> bool {
        match self {
            Self::Noul(answer) => probability(answer.noul),
            Self::Choice(answer) => {
                probability(answer.confidence)
                    && distribution(&answer.probabilities)
                    && answer.probabilities.contains_key(&answer.choice)
            }
            Self::Score(answer) => {
                (1..=10).contains(&answer.legend.len())
                    && answer.score.is_finite()
                    && (0.0..=(answer.legend.len() - 1) as f64).contains(&answer.score)
                    && probability(answer.confidence)
                    && distribution(&answer.probabilities)
                    && answer.legend.keys().eq(answer.probabilities.keys())
                    && (0..answer.legend.len()).all(|i| answer.legend.contains_key(&i.to_string()))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOneResponse {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub usage: Usage,
}
impl SystemOneResponse {
    pub fn validate(&self) -> Result<()> {
        if self.model.trim().is_empty()
            || self.answers.is_empty()
            || !self.answers.values().all(Answer::valid)
        {
            return Err(invalid_response());
        }
        self.usage.total()?;
        Ok(())
    }
    /// Check answer names, variants and option/level domains against the sent request.
    pub fn validate_for(&self, request: &SystemOneRequest) -> Result<()> {
        request.validate()?;
        self.validate()?;
        if !self.answers.keys().eq(request.questions.keys()) {
            return Err(invalid_response());
        }
        for (name, question) in &request.questions {
            let valid = match (question, &self.answers[name]) {
                (Question::Noul { .. }, Answer::Noul(_)) => true,
                (Question::Choice { criteria, .. }, Answer::Choice(answer)) => {
                    criteria.keys().eq(answer.probabilities.keys())
                }
                (Question::Score { criteria, .. }, Answer::Score(answer)) => {
                    criteria.len() == answer.legend.len()
                }
                _ => false,
            };
            if !valid {
                return Err(invalid_response());
            }
        }
        Ok(())
    }
    pub fn noul(&self, name: &str) -> Result<&NoulAnswer> {
        match self.answers.get(name) {
            Some(Answer::Noul(answer)) => Ok(answer),
            _ => Err(invalid_response()),
        }
    }
    pub fn choice(&self, name: &str) -> Result<&ChoiceAnswer> {
        match self.answers.get(name) {
            Some(Answer::Choice(answer)) => Ok(answer),
            _ => Err(invalid_response()),
        }
    }
    pub fn score(&self, name: &str) -> Result<&ScoreAnswer> {
        match self.answers.get(name) {
            Some(Answer::Score(answer)) => Ok(answer),
            _ => Err(invalid_response()),
        }
    }
    /// Project primary values while keeping confidence, distributions and legends in `answers`.
    pub fn values(&self) -> Result<Value> {
        self.validate()?;
        Ok(Value::Object(
            self.answers
                .iter()
                .map(|(name, answer)| {
                    let value = match answer {
                        Answer::Noul(answer) => Value::from(answer.noul),
                        Answer::Choice(answer) => Value::from(answer.choice.clone()),
                        Answer::Score(answer) => Value::from(answer.score),
                    };
                    (name.clone(), value)
                })
                .collect(),
        ))
    }
    pub async fn parse<T: Model + AsyncCheck + Send>(&self, context: &RunContext) -> Result<T> {
        context.check()?;
        let values = self.values()?;
        context
            .wait(async {
                T::parse_value_async(values)
                    .await
                    .map_err(Error::Validation)
            })
            .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCard {
    pub name: String,
    pub description: String,
    pub release_date: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelList {
    pub models: Vec<ModelCard>,
}
