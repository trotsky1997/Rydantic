//! Portable PDF sources and native protocol content parts.
#[cfg(feature = "http")]
use super::string;
use super::{fields, http_url, invalid};
#[cfg(feature = "http")]
use crate::Error;
use crate::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(feature = "http")]
use sha2::{Digest, Sha256};
use std::fmt;

const MAX_PDF_BYTES: usize = 10 * 1024 * 1024;

/// Responses PDF page-image processing. Use `Auto` for portable routing;
/// Chat Completions and Messages reject explicit `Low` or `High` detail.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PdfDetail {
    #[default]
    Auto,
    Low,
    High,
}

#[derive(Clone)]
enum Source {
    Url(String),
    Base64(String),
}

/// A PDF URL or embedded PDF. Debug output redacts its name, URL and data.
/// File IDs are provider-scoped and are outside the portable PDF API.
#[derive(Clone)]
pub struct Pdf {
    source: Source,
    filename: String,
    detail: PdfDetail,
}
impl fmt::Debug for Pdf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pdf")
            .field(
                "source",
                &match self.source {
                    Source::Url(_) => "url",
                    Source::Base64(_) => "base64",
                },
            )
            .field("detail", &self.detail)
            .finish()
    }
}
impl Pdf {
    /// Forward an HTTP(S) reference to Responses or Messages without fetching
    /// it. Chat Completions requires embedded data instead.
    pub fn url(url: impl Into<String>) -> Result<Self> {
        Ok(Self {
            source: Source::Url(http_url(url.into())?),
            filename: "document.pdf".into(),
            detail: PdfDetail::Auto,
        })
    }
    /// Embed canonical, padded Base64 PDF data, bounded to 10 MiB encoded.
    /// Checks the PDF header; full parsing and page limits belong to upstream.
    pub fn base64(filename: impl Into<String>, data: impl AsRef<str>) -> Result<Self> {
        let data = data.as_ref();
        if data.len() > MAX_PDF_BYTES {
            return Err(invalid());
        }
        let bytes = STANDARD.decode(data).map_err(|_| invalid())?;
        header(&bytes)?;
        Self {
            source: Source::Base64(data.into()),
            filename: "document.pdf".into(),
            detail: PdfDetail::Auto,
        }
        .filename(filename)
    }
    /// Encode PDF bytes without reading a file or modifying the document.
    pub fn from_bytes(filename: impl Into<String>, bytes: &[u8]) -> Result<Self> {
        if bytes.len().div_ceil(3).saturating_mul(4) > MAX_PDF_BYTES {
            return Err(invalid());
        }
        header(bytes)?;
        Self {
            source: Source::Base64(STANDARD.encode(bytes)),
            filename: "document.pdf".into(),
            detail: PdfDetail::Auto,
        }
        .filename(filename)
    }
    /// Set the portable document name, mapped to Messages `title`.
    pub fn filename(mut self, filename: impl Into<String>) -> Result<Self> {
        let filename = filename.into();
        if filename.trim().is_empty()
            || filename.len() > 255
            || filename
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\'))
            || matches!(filename.as_str(), "." | "..")
        {
            return Err(invalid());
        }
        self.filename = filename;
        Ok(self)
    }
    pub fn detail(mut self, detail: PdfDetail) -> Self {
        self.detail = detail;
        self
    }
    pub(super) fn responses(&self) -> Value {
        let mut block = json!({"type":"input_file","filename":self.filename,"detail":self.detail});
        match &self.source {
            Source::Url(url) => block["file_url"] = json!(url),
            Source::Base64(data) => {
                block["file_data"] = json!(format!("data:application/pdf;base64,{data}"))
            }
        }
        block
    }
    pub(super) fn decode_responses(block: &Value) -> Result<Self> {
        fields(
            block,
            &["type", "filename", "file_url", "file_data", "detail"],
        )?;
        let name = name(block.get("filename"))?;
        let mut pdf = match (block.get("file_url"), block.get("file_data")) {
            (Some(Value::String(url)), None) => Self::url(url)?.filename(name)?,
            (None, Some(Value::String(data))) => data_source(name, data)?,
            _ => return Err(invalid()),
        };
        pdf.detail = match block.get("detail") {
            None | Some(Value::Null) => PdfDetail::Auto,
            Some(value) => serde_json::from_value(value.clone()).map_err(|_| invalid())?,
        };
        Ok(pdf)
    }
    #[cfg(feature = "http")]
    pub(super) fn decode_chat(block: &Value) -> Result<Self> {
        fields(block, &["type", "file"])?;
        let file = &block["file"];
        fields(file, &["filename", "file_data"])?;
        data_source(name(file.get("filename"))?, string(file, "file_data")?)
    }
    #[cfg(feature = "http")]
    pub(super) fn decode_messages(block: &Value) -> Result<Self> {
        fields(block, &["type", "source", "title"])?;
        let name = name(block.get("title"))?;
        let source = &block["source"];
        match string(source, "type")? {
            "url" => {
                fields(source, &["type", "url"])?;
                Self::url(string(source, "url")?)?.filename(name)
            }
            "base64" => {
                fields(source, &["type", "media_type", "data"])?;
                if string(source, "media_type")? != "application/pdf" {
                    return Err(invalid());
                }
                Self::base64(name, string(source, "data")?)
            }
            _ => Err(invalid()),
        }
    }
    #[cfg(feature = "http")]
    pub(super) fn chat(&self) -> Result<Value> {
        if self.detail != PdfDetail::Auto {
            return Err(Error::Configuration(
                "Chat Completions requires automatic PDF detail".into(),
            ));
        }
        match &self.source {
            Source::Url(_) => Err(Error::Configuration("Chat Completions PDF inputs require embedded data; use Pdf::from_bytes or Pdf::base64".into())),
            Source::Base64(data) => Ok(json!({"type":"file","file":{"filename":self.filename,"file_data":format!("data:application/pdf;base64,{data}")}})),
        }
    }
    #[cfg(feature = "http")]
    pub(super) fn messages(&self) -> Result<Value> {
        if self.detail != PdfDetail::Auto {
            return Err(Error::Configuration(
                "Messages requires automatic PDF detail".into(),
            ));
        }
        let source = match &self.source {
            Source::Url(url) => json!({"type":"url","url":url}),
            Source::Base64(data) => {
                json!({"type":"base64","media_type":"application/pdf","data":data})
            }
        };
        Ok(json!({"type":"document","title":self.filename,"source":source}))
    }
    #[cfg(feature = "http")]
    pub(super) fn fingerprint(&self) -> Value {
        let (kind, data) = match &self.source {
            Source::Url(url) => ("url", url),
            Source::Base64(data) => ("base64", data),
        };
        json!({"type":"input_file","source":kind,"sha256":format!("{:x}", Sha256::digest(data.as_bytes())),"filename":self.filename,"detail":self.detail})
    }
}
fn name(value: Option<&Value>) -> Result<&str> {
    match value {
        None | Some(Value::Null) => Ok("document.pdf"),
        Some(value) => value.as_str().ok_or_else(invalid),
    }
}
fn data_source(filename: &str, data: &str) -> Result<Pdf> {
    let data = if let Some(uri) = data.strip_prefix("data:") {
        uri.strip_prefix("application/pdf;base64,")
            .ok_or_else(invalid)?
    } else {
        data
    };
    Pdf::base64(filename, data)
}
fn header(bytes: &[u8]) -> Result<()> {
    if bytes.len() >= 8
        && bytes.starts_with(b"%PDF-")
        && bytes[5].is_ascii_digit()
        && bytes[6] == b'.'
        && bytes[7].is_ascii_digit()
    {
        Ok(())
    } else {
        Err(invalid())
    }
}
