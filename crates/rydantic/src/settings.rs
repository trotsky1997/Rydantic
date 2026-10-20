//! Explicit, ordered settings sources validated by [`crate::Model`].
//!
//! No files or environment variables are read until a source method is called.
//! Later sources override earlier sources. Objects merge recursively; every other
//! value (including arrays and null) replaces the previous value. Model defaults
//! apply only after merging. See the settings guide for the deliberately small
//! `.env` grammar and environment key/value rules.
//!
//! ```
//! use rydantic::{Model, json, settings::{Settings, ValueMode}};
//! #[derive(Model)]
//! struct Config { host: String, port: u16 }
//! let config: Config = Settings::new()
//!     .json(json!({"host": "localhost", "port": 8000}))?
//!     .pairs([("APP_PORT", "8080")], "APP_", ValueMode::JsonOrString)?
//!     .load()?;
//! assert_eq!(config.port, 8080);
//! # Ok::<(), rydantic::settings::SettingsError>(())
//! ```

use crate::{Model, Value};
use serde_json::Map;
use std::{collections::BTreeMap, fmt, fs, io, path::Path};

/// Interpretation of values supplied by environment, dotenv, and secrets sources.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ValueMode {
    /// Valid JSON becomes that JSON value; everything else is a literal string.
    /// For example `true`, `42`, and `null` are not strings in this mode.
    #[default]
    JsonOrString,
    /// All values remain strings, including JSON-looking credentials.
    String,
    /// Every value must be valid JSON. JSON strings must include their quotes.
    Json,
}

/// A redacted source or validation failure. Never retains source values, paths,
/// variable names, parser diagnostics, or custom validator messages/parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingsError {
    Io(io::ErrorKind),
    Syntax,
    RootNotObject,
    InvalidKey,
    ConflictingKeys,
    InvalidUnicode,
    NotRegularFile,
    /// Number of model validation errors, with their potentially sensitive details removed.
    Validation {
        count: usize,
    },
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "settings source I/O failed ({kind:?})"),
            Self::Syntax => f.write_str("invalid settings source syntax"),
            Self::RootNotObject => f.write_str("settings source must be a JSON object"),
            Self::InvalidKey => f.write_str("invalid settings source key"),
            Self::ConflictingKeys => f.write_str("conflicting settings keys in one source"),
            Self::InvalidUnicode => f.write_str("settings source is not valid Unicode"),
            Self::NotRegularFile => {
                f.write_str("settings source must be a regular file, not a symlink")
            }
            Self::Validation { count } => write!(
                f,
                "settings validation failed ({count} errors; details redacted)"
            ),
        }
    }
}
impl std::error::Error for SettingsError {}

/// Accumulates explicitly ordered sources without printing their values in Debug.
///
/// The returned model is an ordinary model: its Debug/serialization is NOT
/// automatically redacted. Do not log it when it contains secrets.
#[derive(Clone, Default)]
pub struct Settings {
    values: Map<String, Value>,
}

impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Settings { values: [REDACTED] }")
    }
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge a JSON object. Source order is the only precedence rule.
    pub fn json(mut self, value: Value) -> Result<Self, SettingsError> {
        let Value::Object(object) = value else {
            return Err(SettingsError::RootNotObject);
        };
        merge(&mut self.values, object);
        Ok(self)
    }

    /// Read one explicit UTF-8 JSON file. Missing files are errors, not ignored.
    /// JSON duplicate keys follow serde_json's last-key-wins behavior.
    pub fn json_file(self, path: impl AsRef<Path>) -> Result<Self, SettingsError> {
        let text = read_file(path.as_ref())?;
        self.json(serde_json::from_str(&text).map_err(|_| SettingsError::Syntax)?)
    }

    /// Read only variables whose names start with the exact, nonempty prefix.
    /// Names after the prefix are ASCII-lowercased; `__` separates object keys.
    /// This does not modify the process environment. Use `pairs` for deterministic
    /// tests instead of changing global environment variables.
    pub fn env(self, prefix: &str, mode: ValueMode) -> Result<Self, SettingsError> {
        check_prefix(prefix)?;
        let mut selected = Vec::new();
        for (key, value) in std::env::vars_os() {
            if key.as_encoded_bytes().starts_with(prefix.as_bytes()) {
                let key = key.to_str().ok_or(SettingsError::InvalidUnicode)?;
                selected.push((
                    key.to_owned(),
                    value
                        .into_string()
                        .map_err(|_| SettingsError::InvalidUnicode)?,
                ));
            }
        }
        self.pairs(selected, prefix, mode)
    }

    /// Load explicitly supplied key/value pairs using the same rules as `env`.
    /// Unmatched names are ignored. Prefix must be nonempty ASCII alphanumeric/
    /// underscore. Matching suffix segments must be nonempty ASCII identifiers.
    /// Paths are limited to 64 segments. Duplicate normalized keys and parent/child keys in the SAME source are
    /// errors, independent of iteration order, even when the parent holds an object.
    pub fn pairs<I, K, V>(
        self,
        pairs: I,
        prefix: &str,
        mode: ValueMode,
    ) -> Result<Self, SettingsError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        check_prefix(prefix)?;
        let mut entries = BTreeMap::new();
        for (key, value) in pairs {
            if let Some(suffix) = key.as_ref().strip_prefix(prefix) {
                let path = key_path(suffix)?;
                if entries
                    .insert(path, parse_value(value.as_ref(), mode)?)
                    .is_some()
                {
                    return Err(SettingsError::ConflictingKeys);
                }
            }
        }
        self.json(Value::Object(nest(entries)?))
    }

    /// Read a strict dotenv subset without interpolation or process mutation.
    /// Blank lines and whole-line `#` comments are ignored; all other lines must
    /// be `KEY=VALUE`. Whitespace around keys/values is trimmed. Unquoted values
    /// are literal (including `#`, `$`, and backslashes). Single quotes wrap a
    /// literal single-line string; double quotes use JSON string escapes. No
    /// `export`, inline comments, multiline quoting, or variable expansion.
    /// Quoting is removed BEFORE ValueMode is applied. All lines are parsed,
    /// including nonmatching keys; invalid syntax always fails closed.
    pub fn dotenv_file(
        self,
        path: impl AsRef<Path>,
        prefix: &str,
        mode: ValueMode,
    ) -> Result<Self, SettingsError> {
        check_prefix(prefix)?;
        let text = read_file(path.as_ref())?;
        let mut pairs = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=').ok_or(SettingsError::Syntax)?;
            let key = key.trim();
            if !identifier(key) {
                return Err(SettingsError::InvalidKey);
            }
            let value = value.trim();
            let value = if value.starts_with('"') {
                serde_json::from_str::<String>(value).map_err(|_| SettingsError::Syntax)?
            } else if let Some(inner) = value.strip_prefix('\'') {
                let inner = inner.strip_suffix('\'').ok_or(SettingsError::Syntax)?;
                if inner.contains('\'') {
                    return Err(SettingsError::Syntax);
                }
                inner.to_owned()
            } else {
                value.to_owned()
            };
            pairs.push((key.to_owned(), value));
        }
        self.pairs(pairs, prefix, mode)
    }

    /// Load each file in an explicit flat directory as a key/value pair.
    /// Filenames follow environment suffix rules (ASCII lowercase + `__`
    /// nesting); there is no prefix. Removes at most one final LF or CRLF,
    /// preserving all other whitespace. Rejects symlinks, subdirectories,
    /// non-Unicode names/contents, and conflicting keys. Use trusted directories
    /// that are not concurrently modified; this is not a filesystem sandbox.
    /// Checks reject final file-entry symlinks, not symlinks in directory/ancestor paths.
    pub fn secrets_dir(
        self,
        path: impl AsRef<Path>,
        mode: ValueMode,
    ) -> Result<Self, SettingsError> {
        let mut entries = BTreeMap::new();
        for entry in fs::read_dir(path).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| SettingsError::InvalidUnicode)?;
            let key = key_path(&name)?;
            let text = read_file(&entry.path())?;
            let text = text
                .strip_suffix("\r\n")
                .or_else(|| text.strip_suffix('\n'))
                .unwrap_or(&text);
            if entries.insert(key, parse_value(text, mode)?).is_some() {
                return Err(SettingsError::ConflictingKeys);
            }
        }
        self.json(Value::Object(nest(entries)?))
    }

    /// Validate the merged input with the existing Model rules/defaults.
    /// Error details are intentionally discarded because custom validators can
    /// include secrets in messages, locations, codes, or parameters.
    pub fn load<T: Model>(self) -> Result<T, SettingsError> {
        T::parse_value(Value::Object(self.values)).map_err(|errors| SettingsError::Validation {
            count: errors.0.len(),
        })
    }
}

fn io_error(error: io::Error) -> SettingsError {
    SettingsError::Io(error.kind())
}
fn read_file(path: &Path) -> Result<String, SettingsError> {
    if !fs::symlink_metadata(path)
        .map_err(io_error)?
        .file_type()
        .is_file()
    {
        return Err(SettingsError::NotRegularFile);
    }
    fs::read_to_string(path).map_err(|error| {
        if error.kind() == io::ErrorKind::InvalidData {
            SettingsError::InvalidUnicode
        } else {
            io_error(error)
        }
    })
}
fn identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn check_prefix(prefix: &str) -> Result<(), SettingsError> {
    if prefix.is_empty()
        || !prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        Err(SettingsError::InvalidKey)
    } else {
        Ok(())
    }
}
fn key_path(key: &str) -> Result<Vec<String>, SettingsError> {
    if key.split("__").count() > 64 {
        return Err(SettingsError::InvalidKey);
    }
    key.split("__")
        .map(|part| {
            if identifier(part) {
                Ok(part.to_ascii_lowercase())
            } else {
                Err(SettingsError::InvalidKey)
            }
        })
        .collect()
}
fn parse_value(text: &str, mode: ValueMode) -> Result<Value, SettingsError> {
    match mode {
        ValueMode::String => Ok(Value::String(text.to_owned())),
        ValueMode::Json => serde_json::from_str(text).map_err(|_| SettingsError::Syntax),
        ValueMode::JsonOrString => {
            Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned())))
        }
    }
}
fn nest(entries: BTreeMap<Vec<String>, Value>) -> Result<Map<String, Value>, SettingsError> {
    // BTree ordering puts a parent immediately before its first descendant.
    let mut previous: Option<Vec<String>> = None;
    let mut object = Map::new();
    for (path, value) in entries {
        if previous
            .as_ref()
            .is_some_and(|parent| path.starts_with(parent))
        {
            return Err(SettingsError::ConflictingKeys);
        }
        insert(&mut object, &path, value);
        previous = Some(path);
    }
    Ok(object)
}
fn insert(object: &mut Map<String, Value>, path: &[String], value: Value) {
    if path.len() == 1 {
        object.insert(path[0].clone(), value);
    } else {
        let child = object
            .entry(path[0].clone())
            .or_insert_with(|| Value::Object(Map::new()));
        // Paths have already been checked for scalar/parent conflicts.
        insert(
            child.as_object_mut().expect("validated settings path"),
            &path[1..],
            value,
        );
    }
}
fn merge(target: &mut Map<String, Value>, source: Map<String, Value>) {
    for (key, value) in source {
        match (target.get_mut(&key), value) {
            (Some(Value::Object(old)), Value::Object(new)) => merge(old, new),
            (_, value) => {
                target.insert(key, value);
            }
        }
    }
}
