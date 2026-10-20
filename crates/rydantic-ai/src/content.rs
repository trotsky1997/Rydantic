//! Ordered image/text input shared by agents, HTTP adapters and the gateway.
use crate::{Error, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(feature = "http")]
use sha2::{Digest, Sha256};
use std::fmt;

pub(crate) const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_IMAGES: usize = 64;

/// Image preprocessing for Responses and Chat Completions. Use `Auto` for
/// portable routing to Messages, which has its own image preprocessing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    #[default]
    Auto,
    Low,
    High,
    Original,
}

/// A validated image reference. Debug output redacts URLs and encoded data.
#[derive(Clone)]
pub struct Image {
    url: String,
    detail: ImageDetail,
}
impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image")
            .field(
                "source",
                &if self.url.starts_with("data:") {
                    "base64"
                } else {
                    "url"
                },
            )
            .field("detail", &self.detail)
            .finish()
    }
}
fn invalid() -> Error {
    Error::Configuration("invalid or unsupported image/text input".into())
}
fn media_type(value: &str) -> Result<()> {
    if matches!(
        value,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    ) {
        Ok(())
    } else {
        Err(invalid())
    }
}
fn fields(value: &Value, allowed: &[&str]) -> Result<()> {
    if value
        .as_object()
        .is_none_or(|object| object.keys().any(|key| !allowed.contains(&key.as_str())))
    {
        return Err(invalid());
    }
    Ok(())
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key].as_str().ok_or_else(invalid)
}
impl Image {
    /// Reference an HTTP(S) image. The library forwards the reference without
    /// downloading it. Signed URL query strings are preserved.
    pub fn url(url: impl Into<String>) -> Result<Self> {
        let url = url.into();
        if url.len() > 16 * 1024
            || url.chars().any(char::is_whitespace)
            || url.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        if !url.split_once("://").is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http")
        }) {
            return Err(invalid());
        }
        let parsed = url::Url::parse(&url).map_err(|_| invalid())?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(invalid());
        }
        Ok(Self {
            url,
            detail: ImageDetail::Auto,
        })
    }
    /// Embed canonical, padded Base64 data for PNG, JPEG, GIF or WebP.
    /// The encoded payload is bounded to 10 MiB; pixel decoding is upstream.
    pub fn base64(mime: &str, data: impl AsRef<str>) -> Result<Self> {
        media_type(mime)?;
        let data = data.as_ref();
        if data.is_empty() || data.len() > MAX_IMAGE_BYTES {
            return Err(invalid());
        }
        STANDARD.decode(data).map_err(|_| invalid())?;
        Ok(Self {
            url: format!("data:{mime};base64,{data}"),
            detail: ImageDetail::Auto,
        })
    }
    /// Encode image bytes without reading files or changing the image.
    pub fn from_bytes(mime: &str, data: &[u8]) -> Result<Self> {
        media_type(mime)?;
        if data.is_empty() || data.len().div_ceil(3).saturating_mul(4) > MAX_IMAGE_BYTES {
            return Err(invalid());
        }
        Ok(Self {
            url: format!("data:{mime};base64,{}", STANDARD.encode(data)),
            detail: ImageDetail::Auto,
        })
    }
    pub fn detail(mut self, detail: ImageDetail) -> Self {
        self.detail = detail;
        self
    }
    fn reference(url: &str) -> Result<Self> {
        if let Some(data) = url.strip_prefix("data:") {
            let (mime, data) = data.split_once(";base64,").ok_or_else(invalid)?;
            Self::base64(mime, data)
        } else {
            Self::url(url)
        }
    }
    fn responses(&self) -> Value {
        json!({"type":"input_image","image_url":self.url,"detail":self.detail})
    }
    #[cfg(feature = "http")]
    fn messages(&self) -> Result<Value> {
        if self.detail != ImageDetail::Auto {
            return Err(Error::Configuration(
                "Messages requires automatic image detail".into(),
            ));
        }
        let source = if let Some(data) = self.url.strip_prefix("data:") {
            let (mime, data) = data.split_once(";base64,").ok_or_else(invalid)?;
            json!({"type":"base64","media_type":mime,"data":data})
        } else {
            json!({"type":"url","url":self.url})
        };
        Ok(json!({"type":"image","source":source}))
    }
}

#[derive(Debug, Clone)]
enum Part {
    Text(String),
    Image(Image),
}
/// A user prompt with ordered text and images. String prompts convert directly.
#[derive(Debug, Clone, Default)]
pub struct Prompt {
    parts: Vec<Part>,
}
impl Prompt {
    pub fn new(text: impl Into<String>) -> Self {
        Self::default().text(text)
    }
    pub fn text(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        if let Some(Part::Text(previous)) = self.parts.last_mut() {
            previous.push_str(&text);
        } else if !text.is_empty() {
            self.parts.push(Part::Text(text));
        }
        self
    }
    pub fn image(mut self, image: Image) -> Self {
        self.parts.push(Part::Image(image));
        self
    }
    pub(crate) fn responses(&self) -> Value {
        match self.parts.as_slice() {
            [] => json!(""),
            [Part::Text(text)] => json!(text),
            parts => Value::Array(
                parts
                    .iter()
                    .map(|part| match part {
                        Part::Text(text) => json!({"type":"input_text","text":text}),
                        Part::Image(image) => image.responses(),
                    })
                    .collect(),
            ),
        }
    }
    pub(crate) fn images(&self) -> usize {
        self.parts
            .iter()
            .filter(|part| matches!(part, Part::Image(_)))
            .count()
    }
}
impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}
impl From<&str> for Prompt {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}
impl From<&String> for Prompt {
    fn from(text: &String) -> Self {
        Self::new(text)
    }
}
impl From<&Prompt> for Prompt {
    fn from(prompt: &Prompt) -> Self {
        prompt.clone()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Format {
    Responses,
    #[cfg(feature = "http")]
    Chat,
    #[cfg(feature = "http")]
    Messages,
}
pub(crate) fn decode(value: &Value, format: Format) -> Result<Prompt> {
    if let Some(text) = value.as_str() {
        return Ok(Prompt::new(text));
    }
    let mut prompt = Prompt::default();
    for block in value.as_array().ok_or_else(invalid)? {
        let kind = string(block, "type")?;
        match (format, kind) {
            (Format::Responses, "input_text") => {
                fields(block, &["type", "text"])?;
                prompt = prompt.text(string(block, "text")?);
            }
            #[cfg(feature = "http")]
            (Format::Chat | Format::Messages, "text") => {
                fields(block, &["type", "text"])?;
                prompt = prompt.text(string(block, "text")?);
            }
            (Format::Responses, "input_image") => {
                fields(block, &["type", "image_url", "detail"])?;
                let mut image = Image::reference(string(block, "image_url")?)?;
                image.detail = detail(block.get("detail"))?;
                prompt = prompt.image(image);
            }
            #[cfg(feature = "http")]
            (Format::Chat, "image_url") => {
                fields(block, &["type", "image_url"])?;
                let source = &block["image_url"];
                fields(source, &["url", "detail"])?;
                let mut image = Image::reference(string(source, "url")?)?;
                image.detail = detail(source.get("detail"))?;
                prompt = prompt.image(image);
            }
            #[cfg(feature = "http")]
            (Format::Messages, "image") => {
                fields(block, &["type", "source"])?;
                let source = &block["source"];
                let image = match string(source, "type")? {
                    "url" => {
                        fields(source, &["type", "url"])?;
                        Image::url(string(source, "url")?)?
                    }
                    "base64" => {
                        fields(source, &["type", "media_type", "data"])?;
                        Image::base64(string(source, "media_type")?, string(source, "data")?)?
                    }
                    _ => return Err(invalid()),
                };
                prompt = prompt.image(image);
            }
            _ => return Err(invalid()),
        }
    }
    if prompt.images() > MAX_IMAGES {
        return Err(invalid());
    }
    Ok(prompt)
}
fn detail(value: Option<&Value>) -> Result<ImageDetail> {
    match value {
        None | Some(Value::Null) => Ok(ImageDetail::Auto),
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| invalid()),
    }
}
pub(crate) fn validate_input(input: &[Value]) -> Result<()> {
    let mut count = 0usize;
    for item in input.iter().filter(|item| item["role"] == "user") {
        count += decode(&item["content"], Format::Responses)?.images();
        if count > MAX_IMAGES {
            return Err(invalid());
        }
    }
    Ok(())
}
/// Compact image identity keeps large prompts inside the replay cache budget.
#[cfg(feature = "http")]
pub(crate) fn fingerprint(value: &Value) -> Value {
    let Ok(prompt) = decode(value, Format::Responses) else {
        return json!({"invalid_content":value});
    };
    Value::Array(prompt.parts.iter().map(|part| match part {
        Part::Text(text) => json!({"type":"input_text","text":text}),
        Part::Image(image) => json!({"type":"input_image","sha256":format!("{:x}", Sha256::digest(image.url.as_bytes())),"detail":image.detail}),
    }).collect())
}
#[cfg(feature = "http")]
pub(crate) fn chat(value: &Value) -> Result<Value> {
    let prompt = decode(value, Format::Responses)?;
    if prompt.images() == 0 {
        return Ok(prompt.responses());
    }
    Ok(Value::Array(
        prompt
            .parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => json!({"type":"text","text":text}),
                Part::Image(image) => {
                    json!({"type":"image_url","image_url":{"url":image.url,"detail":image.detail}})
                }
            })
            .collect(),
    ))
}
#[cfg(feature = "http")]
pub(crate) fn messages(value: &Value) -> Result<Vec<Value>> {
    decode(value, Format::Responses)?
        .parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => Ok(json!({"type":"text","text":text})),
            Part::Image(image) => image.messages(),
        })
        .collect()
}
