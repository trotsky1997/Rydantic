//! Optional, bounded, precision-preserving JSON snapshots.
//!
//! Separate from strict JsonStream/NDJSON APIs. Incomplete strings/keys are not
//! supported: jiter 0.17 can classify invalid truncated Unicode escapes as EOF.
//! This module never enables jiter partial-string recovery or converts via f64.
use jiter::{Jiter, JiterError, JiterErrorType, JsonErrorType, Peek};
use serde_json::{Map, Value};
/// A provisional view of the current accumulated input buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Recovered value, or None when no value is available yet.
    pub value: Option<Value>,
    /// Whether the entire current buffer is syntactically complete JSON.
    /// Complete does not mean the upstream producer has finished sending bytes.
    pub status: Status,
}
/// Syntactic status of this buffer, not transport or model-validation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Complete,
    Incomplete,
}
/// Per-call resource limits, bounded by hard ceilings of 1 MiB and 64 containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum input bytes (at most 1 MiB).
    pub max_input_bytes: usize,
    /// Maximum nested containers (at most 64).
    pub max_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 1024 * 1024,
            max_depth: 64,
        }
    }
}
/// Input errors deliberately do not include the input data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Input exceeded the configured byte limit.
    InputTooLarge,
    /// Requested resource limits exceed the hard ceilings.
    InvalidLimits,
    /// jiter 0.17 rejected a number under its integer-part digit limit.
    /// This does not imply invalid JSON, and no rounding is performed.
    UnsupportedNumberLimit { offset: usize },
    /// Input exceeded the configured nesting limit.
    NestingTooDeep,
    /// Incomplete strings and object keys are unsupported, including escapes/UTF-8.
    IncompleteString { offset: usize },
    /// Input is malformed; this is not treated as recoverable truncation.
    InvalidJson { offset: usize },
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLimits => f.write_str("partial JSON limits exceed hard ceilings"),
            Self::UnsupportedNumberLimit { offset } => {
                write!(f, "number exceeds supported parser limit at byte {offset}")
            }
            Self::InputTooLarge => {
                f.write_str("partial JSON input exceeds the configured byte limit")
            }
            Self::NestingTooDeep => {
                f.write_str("partial JSON nesting exceeds the configured depth limit")
            }
            Self::IncompleteString { offset } => write!(
                f,
                "incomplete strings and keys are unsupported at byte {offset}"
            ),
            Self::InvalidJson { offset } => write!(f, "invalid JSON at byte {offset}"),
        }
    }
}
impl std::error::Error for Error {}
fn syntax(error: JiterError) -> Error {
    if matches!(
        error.error_type,
        JiterErrorType::JsonError(JsonErrorType::NumberOutOfRange)
    ) {
        return Error::UnsupportedNumberLimit {
            offset: error.index,
        };
    }
    if matches!(
        error.error_type,
        JiterErrorType::JsonError(JsonErrorType::EofWhileParsingString)
    ) {
        Error::IncompleteString {
            offset: error.index,
        }
    } else {
        Error::InvalidJson {
            offset: error.index,
        }
    }
}
fn eof(error: &JiterError) -> bool {
    matches!(
        error.error_type,
        JiterErrorType::JsonError(
            JsonErrorType::EofWhileParsingList
                | JsonErrorType::EofWhileParsingObject
                | JsonErrorType::EofWhileParsingValue
        )
    )
}
fn recover<T>(result: Result<T, JiterError>) -> Result<Option<T>, Error> {
    match result {
        Ok(v) => Ok(Some(v)),
        Err(e) if eof(&e) => Ok(None),
        Err(e) => Err(syntax(e)),
    }
}
/// Parse a snapshot of an accumulated JSON buffer (maximum 1 MiB, 64 levels).
///
/// Numbers with more than 4300 integer-part digits return an explicit resource
/// error from the pinned parser, even when valid JSON. Accepted numbers are not
/// rounded. Complete-input equivalence applies within the supported limits.
///
/// This reparses each buffer; it is not an incremental or zero-copy stream parser.
/// Missing array/object tails and unfinished numbers/literals may be recovered.
/// Incomplete strings or object keys always return an error. Required Model fields
/// are not defaulted here. Validate only when appropriate for the application's
/// trust boundary, and require a final complete document before taking actions.
///
/// ```
/// use rydantic::partial_json::{snapshot, Status};
/// let preview = snapshot(br#"{"items":[1,2,"#)?;
/// assert_eq!(preview.status, Status::Incomplete);
/// assert_eq!(preview.value.unwrap(), serde_json::json!({"items": [1, 2]}));
/// # Ok::<(), rydantic::partial_json::Error>(())
/// ```
pub fn snapshot(data: &[u8]) -> Result<Snapshot, Error> {
    snapshot_with_limits(data, Limits::default())
}
/// Like [`snapshot`], with caller-selected lower size/depth resource limits.
///
/// Limits above 1 MiB or 64 nested containers return [`Error::InvalidLimits`].
/// jiter's separate 4300-digit integer-part limit cannot be increased here.
pub fn snapshot_with_limits(data: &[u8], limits: Limits) -> Result<Snapshot, Error> {
    if limits.max_input_bytes > 1024 * 1024 || limits.max_depth > 64 {
        return Err(Error::InvalidLimits);
    }
    if data.len() > limits.max_input_bytes {
        return Err(Error::InputTooLarge);
    }
    let mut parser = Jiter::new(data);
    let result = node(&mut parser, 0, limits.max_depth)?;
    if result.status == Status::Complete {
        parser.finish().map_err(syntax)?;
        // Complete-input output is exactly the existing strict parser's value,
        // including its arbitrary-precision number representation.
        let value =
            serde_json::from_slice(data).map_err(|_| Error::InvalidJson { offset: data.len() })?;
        Ok(Snapshot {
            value: Some(value),
            status: Status::Complete,
        })
    } else {
        Ok(result)
    }
}
fn node(p: &mut Jiter<'_>, depth: usize, max_depth: usize) -> Result<Snapshot, Error> {
    if depth > max_depth {
        return Err(Error::NestingTooDeep);
    }
    let Some(peek) = recover(p.peek())? else {
        return Ok(Snapshot {
            value: None,
            status: Status::Incomplete,
        });
    };
    if depth >= max_depth && matches!(peek, Peek::Array | Peek::Object) {
        return Err(Error::NestingTooDeep);
    }
    let result = match peek {
        Peek::Array => {
            let mut values = Vec::new();
            let Some(mut next) = recover(p.next_array())? else {
                return Ok(Snapshot {
                    value: Some(Value::Array(values)),
                    status: Status::Incomplete,
                });
            };
            while next.is_some() {
                let child = node(p, depth + 1, max_depth)?;
                if let Some(value) = child.value {
                    values.push(value)
                }
                if child.status == Status::Incomplete {
                    return Ok(Snapshot {
                        value: Some(Value::Array(values)),
                        status: Status::Incomplete,
                    });
                }
                let Some(step) = recover(p.array_step())? else {
                    return Ok(Snapshot {
                        value: Some(Value::Array(values)),
                        status: Status::Incomplete,
                    });
                };
                next = step;
            }
            Value::Array(values)
        }
        Peek::Object => {
            let mut values = Map::new();
            let Some(mut next) = recover(p.next_object().map(|key| key.map(str::to_owned)))? else {
                return Ok(Snapshot {
                    value: Some(Value::Object(values)),
                    status: Status::Incomplete,
                });
            };
            while let Some(key) = next {
                let child = node(p, depth + 1, max_depth)?;
                if let Some(value) = child.value {
                    values.insert(key, value);
                }
                if child.status == Status::Incomplete {
                    return Ok(Snapshot {
                        value: Some(Value::Object(values)),
                        status: Status::Incomplete,
                    });
                }
                let Some(step) = recover(p.next_key().map(|key| key.map(str::to_owned)))? else {
                    return Ok(Snapshot {
                        value: Some(Value::Object(values)),
                        status: Status::Incomplete,
                    });
                };
                next = step;
            }
            Value::Object(values)
        }
        Peek::String => Value::String(p.next_str().map_err(syntax)?.to_owned()),
        Peek::Null => {
            if recover(p.next_null())?.is_none() {
                return Ok(Snapshot {
                    value: None,
                    status: Status::Incomplete,
                });
            }
            Value::Null
        }
        Peek::True | Peek::False => {
            let Some(value) = recover(p.next_bool())? else {
                return Ok(Snapshot {
                    value: None,
                    status: Status::Incomplete,
                });
            };
            Value::Bool(value)
        }
        _ => {
            let Some(bytes) = recover(p.next_number_bytes())? else {
                return Ok(Snapshot {
                    value: None,
                    status: Status::Incomplete,
                });
            };
            serde_json::from_slice(bytes).map_err(|_| Error::InvalidJson {
                offset: p.current_index(),
            })?
        }
    };
    Ok(Snapshot {
        value: Some(result),
        status: Status::Complete,
    })
}
