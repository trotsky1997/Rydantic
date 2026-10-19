//! SQLModel-style SQLite, PostgreSQL and MySQL persistence backed by SQLx and Tokio.
//!
//! ```
//! use rydantic::{Model, SqlModel, sqlx::sqlite::SqlitePoolOptions};
//! #[derive(Debug, PartialEq, Model, SqlModel)]
//! struct Note {
//!     #[sql(primary_key, auto_increment)]
//!     id: Option<i64>,
//!     #[model(min_length = 1)]
//!     text: String,
//! }
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let pool = SqlitePoolOptions::new().max_connections(1)
//!     .connect("sqlite::memory:").await?;
//! Note::create_table(&pool).await?;
//! let stored = Note::parse_json(r#"{"text":"hello"}"#)?.insert(&pool).await?;
//! assert_eq!(stored.id, Some(1));
//! assert_eq!(Note::get(&pool, &1).await?.as_ref(), Some(&stored));
//! # Ok(()) }
//! # tokio::runtime::Runtime::new()?.block_on(run())?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
mod backend;
mod ddl;
mod parameter;
mod query;
mod table;
use crate::{Context, ValidationError, ValidationErrors};
pub use backend::{Dialect, SqlBackend};
pub use parameter::{AutoKey, Key, SqlParameter, SqlType, SqlValue};
pub use query::Select;
use sqlx::{pool::PoolConnection, Database, SqliteConnection};
pub use table::{SqlModel, SqlRow};

// SQLite extended result codes store their primary result in the low byte.
#[cfg(feature = "axum")]
const SQLITE_CONSTRAINT: u32 = 19;

/// Whether dropping an acquired handle returns its connection to a pool.
///
/// SQL operations await asynchronous model checks only for pool-owned handles.
/// Borrowed connections and transactions may retain the pool's only slot.
pub trait ValidationConnection {
    const RELEASES_POOL_SLOT: bool;
}
impl<DB: Database> ValidationConnection for PoolConnection<DB> {
    const RELEASES_POOL_SLOT: bool = true;
}
impl ValidationConnection for &mut SqliteConnection {
    const RELEASES_POOL_SLOT: bool = false;
}
#[cfg(feature = "postgres")]
impl ValidationConnection for &mut sqlx::PgConnection {
    const RELEASES_POOL_SLOT: bool = false;
}
#[cfg(feature = "mysql")]
impl ValidationConnection for &mut sqlx::MySqlConnection {
    const RELEASES_POOL_SLOT: bool = false;
}
fn check_validation_connection<T: SqlModel, C: ValidationConnection>() -> Result<(), SqlError> {
    if !C::RELEASES_POOL_SLOT && T::async_required(&mut std::collections::HashSet::new()) {
        return Err(field_error(
            "sql_async_connection",
            "async model checks require a pool; a borrowed connection or transaction retains its connection",
        ));
    }
    Ok(())
}

/// Model validation errors remain distinct from driver and schema errors.
#[derive(Debug)]
pub enum SqlError {
    Validation(ValidationErrors),
    Database(sqlx::Error),
    NotFound,
    InvalidColumn(String),
}
impl std::fmt::Display for SqlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(e) => e.fmt(f),
            Self::Database(e) => e.fmt(f),
            Self::NotFound => f.write_str("row not found"),
            Self::InvalidColumn(name) => write!(f, "unknown model column: {name}"),
        }
    }
}
impl std::error::Error for SqlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(e) => Some(e),
            Self::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<ValidationErrors> for SqlError {
    fn from(e: ValidationErrors) -> Self {
        Self::Validation(e)
    }
}
impl From<sqlx::Error> for SqlError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e)
    }
}
impl SqlError {
    /// Attach a model field location to a failed value conversion.
    pub fn at(self, field: &str) -> Self {
        match self {
            Self::Validation(e) => Self::Validation(Context::prepend_errors(e, field)),
            other => other,
        }
    }
}
pub(crate) fn field_error(code: &str, message: &str) -> SqlError {
    SqlError::Validation(ValidationError::new(code, message).into())
}
#[derive(Debug, Clone, Copy)]
pub struct Reference {
    pub table: &'static str,
    pub column: &'static str,
    pub on_delete: OnDelete,
}
#[derive(Debug, Clone, Copy, Default)]
pub enum OnDelete {
    #[default]
    NoAction,
    Restrict,
    Cascade,
    SetNull,
}
impl OnDelete {
    fn sql(self) -> &'static str {
        match self {
            Self::NoAction => "NO ACTION",
            Self::Restrict => "RESTRICT",
            Self::Cascade => "CASCADE",
            Self::SetNull => "SET NULL",
        }
    }
}

/// Database metadata inferred by `#[derive(SqlModel)]`.
#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub name: &'static str,
    pub sql_type: SqlType,
    pub nullable: bool,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub unique: bool,
    pub index: bool,
    pub references: Option<Reference>,
    /// Optional bounded text/binary length used by MySQL schema generation.
    pub length: Option<usize>,
}
impl Column {
    /// MySQL requires bounded text/binary columns for keys and indexes.
    pub fn mysql_length(&self) -> Option<usize> {
        if matches!(self.sql_type, SqlType::Text | SqlType::Blob)
            && (self.length.is_some()
                || self.primary_key
                || self.unique
                || self.index
                || self.references.is_some())
        {
            Some(self.length.unwrap_or(191))
        } else {
            None
        }
    }
}
#[doc(hidden)]
pub fn bind_value<DB: SqlBackend, V: SqlValue>(
    value: &V,
    column: &Column,
    query: &mut sqlx::QueryBuilder<DB>,
) -> Result<(), SqlError> {
    if DB::DIALECT == Dialect::MySql {
        let parameter = value.to_parameter()?;
        if let Some(limit) = column.mysql_length() {
            let length = match &parameter {
                SqlParameter::Text(value) => value.chars().count(),
                SqlParameter::Blob(value) => value.len(),
                _ => 0,
            };
            if length > limit {
                return Err(field_error(
                    "sql_length",
                    "value exceeds the database column length",
                ));
            }
        }
        DB::bind_parameter(parameter, query);
        Ok(())
    } else {
        DB::bind(value, query)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Compare {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}
impl Compare {
    fn sql(self) -> &'static str {
        match self {
            Self::Equal => "=",
            Self::NotEqual => "<>",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Order {
    Ascending,
    Descending,
}
#[cfg(feature = "axum")]
impl ::axum::response::IntoResponse for SqlError {
    fn into_response(self) -> ::axum::response::Response {
        use ::axum::http::StatusCode;
        let (status, code, message) = match self {
            Self::Validation(errors) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    ::axum::Json(crate::json!({"detail":errors.to_json()})),
                )
                    .into_response()
            }
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found", "row not found"),
            Self::InvalidColumn(_) => (
                StatusCode::BAD_REQUEST,
                "sql_column",
                "unknown model column",
            ),
            Self::Database(sqlx::Error::Database(ref error))
                if matches!(
                    error.kind(),
                    sqlx::error::ErrorKind::UniqueViolation
                        | sqlx::error::ErrorKind::ForeignKeyViolation
                        | sqlx::error::ErrorKind::NotNullViolation
                        | sqlx::error::ErrorKind::CheckViolation
                        | sqlx::error::ErrorKind::ExclusionViolation
                ) || error.code().is_some_and(|code| code.starts_with("23"))
                    || error
                        .code()
                        .and_then(|code| code.parse::<u32>().ok())
                        .is_some_and(|code| code & 0xff == SQLITE_CONSTRAINT) =>
            {
                (
                    StatusCode::CONFLICT,
                    "sql_constraint",
                    "database constraint violation",
                )
            }
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "sql_error",
                "database operation failed",
            ),
        };
        let error = ValidationError::new(code, message);
        (status, ::axum::Json(crate::json!({"detail":[error]}))).into_response()
    }
}

// These macros select drivers using Rydantic's features, including when the
// consuming crate renames its dependency or uses different feature names.
#[doc(hidden)]
#[macro_export]
#[cfg(feature = "postgres")]
macro_rules! __rydantic_postgres { ($($item:tt)*) => { $($item)* }; }
#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "postgres"))]
macro_rules! __rydantic_postgres {
    ($($item:tt)*) => {};
}
#[doc(hidden)]
#[macro_export]
#[cfg(feature = "mysql")]
macro_rules! __rydantic_mysql { ($($item:tt)*) => { $($item)* }; }
#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "mysql"))]
macro_rules! __rydantic_mysql {
    ($($item:tt)*) => {};
}
