//! SQLModel-style SQLite persistence backed by SQLx and Tokio.
mod value;
use crate::{AsyncCheck, Context, Model, ValidationError, ValidationErrors};
use sqlx::{sqlite::SqliteRow, Acquire, QueryBuilder, Sqlite};
use std::{future::Future, marker::PhantomData};
pub use value::{AutoKey, Key, SqlType, SqlValue};

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
fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
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
}

/// SQLite table and CRUD operations generated from one Rydantic model.
///
/// Operations accept a pool, connection, or mutable SQLx transaction through
/// `Acquire`. Writes validate before executing and revalidate returned rows
/// inside a transaction/savepoint. Row reads validate stored typed values;
/// normalization remains part of model parsing.
pub trait SqlModel: Model + AsyncCheck + Send + 'static {
    type Key: SqlValue;
    const TABLE: &'static str;
    const COLUMNS: &'static [Column];
    #[doc(hidden)]
    fn decode_row(row: &SqliteRow) -> Result<Self, sqlx::Error>;
    #[doc(hidden)]
    fn bind_column(&self, index: usize, query: &mut QueryBuilder<Sqlite>) -> Result<(), SqlError>;
    #[doc(hidden)]
    fn include_insert_column(&self, index: usize) -> bool;
    fn key(&self) -> Option<&Self::Key>;

    fn primary_key() -> &'static Column {
        Self::COLUMNS
            .iter()
            .find(|c| c.primary_key)
            .expect("SqlModel requires a primary key")
    }
    fn create_table_sql() -> String {
        let mut definitions = Vec::new();
        for col in Self::COLUMNS {
            let mut definition = format!("{} {}", quoted(col.name), col.sql_type.declaration());
            if col.primary_key {
                definition.push_str(" PRIMARY KEY");
            }
            if col.auto_increment {
                definition.push_str(" AUTOINCREMENT");
            } else if !col.nullable || col.primary_key {
                definition.push_str(" NOT NULL");
            }
            if col.unique && !col.primary_key {
                definition.push_str(" UNIQUE");
            }
            if let Some(reference) = col.references {
                definition.push_str(&format!(
                    " REFERENCES {} ({}) ON DELETE {}",
                    quoted(reference.table),
                    quoted(reference.column),
                    reference.on_delete.sql()
                ));
            }
            definitions.push(definition);
        }
        format!(
            "CREATE TABLE IF NOT EXISTS {} ({})",
            quoted(Self::TABLE),
            definitions.join(", ")
        )
    }
    fn create_index_sql() -> Vec<String> {
        Self::COLUMNS
            .iter()
            .filter(|col| col.index && !col.unique && !col.primary_key)
            .map(|col| {
                format!(
                    "CREATE INDEX IF NOT EXISTS {} ON {} ({})",
                    quoted(&format!(
                        "rydantic_{}_{}_{}",
                        Self::TABLE.len(),
                        Self::TABLE,
                        col.name
                    )),
                    quoted(Self::TABLE),
                    quoted(col.name)
                )
            })
            .collect()
    }
    fn create_table<'c, A>(connection: A) -> impl Future<Output = Result<(), SqlError>> + Send
    where
        A: Acquire<'c, Database = Sqlite> + Send,
    {
        async move {
            let mut transaction = connection.begin().await?;
            sqlx::query(sqlx::AssertSqlSafe(Self::create_table_sql()))
                .execute(&mut *transaction)
                .await?;
            for statement in Self::create_index_sql() {
                sqlx::query(sqlx::AssertSqlSafe(statement))
                    .execute(&mut *transaction)
                    .await?;
            }
            transaction.commit().await?;
            Ok(())
        }
    }
    fn insert<'c, A>(&self, connection: A) -> impl Future<Output = Result<Self, SqlError>> + Send
    where
        A: Acquire<'c, Database = Sqlite> + Send,
    {
        async move {
            self.validate_async().await?;
            if self.key().is_none() && !Self::primary_key().auto_increment {
                return Err(field_error(
                    "sql_primary_key_missing",
                    "insert requires a primary key",
                )
                .at(Self::primary_key().name));
            }
            let included = Self::COLUMNS
                .iter()
                .enumerate()
                .filter(|(i, _)| self.include_insert_column(*i))
                .collect::<Vec<_>>();
            let mut query =
                QueryBuilder::<Sqlite>::new(format!("INSERT INTO {}", quoted(Self::TABLE)));
            if included.is_empty() {
                query.push(" DEFAULT VALUES");
            } else {
                query.push(format!(
                    " ({}) VALUES (",
                    included
                        .iter()
                        .map(|(_, c)| quoted(c.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                for (position, (index, _)) in included.iter().enumerate() {
                    if position > 0 {
                        query.push(", ");
                    }
                    self.bind_column(*index, &mut query)?;
                }
                query.push(")");
            }
            query.push(format!(" RETURNING {}", column_list::<Self>()));
            let mut transaction = connection.begin().await?;
            let row = query.build().fetch_one(&mut *transaction).await?;
            let value = checked_row::<Self>(&row).await?;
            transaction.commit().await?;
            Ok(value)
        }
    }
    fn update<'c, A>(&self, connection: A) -> impl Future<Output = Result<Self, SqlError>> + Send
    where
        A: Acquire<'c, Database = Sqlite> + Send,
    {
        async move {
            self.validate_async().await?;
            let key = self.key().ok_or_else(|| {
                field_error("sql_primary_key_missing", "update requires a primary key")
                    .at(Self::primary_key().name)
            })?;
            let mut query =
                QueryBuilder::<Sqlite>::new(format!("UPDATE {} SET ", quoted(Self::TABLE)));
            let mutable = Self::COLUMNS
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.primary_key)
                .collect::<Vec<_>>();
            if mutable.is_empty() {
                query.push(format!(
                    "{} = {}",
                    quoted(Self::primary_key().name),
                    quoted(Self::primary_key().name)
                ));
            }
            for (position, (index, column)) in mutable.iter().enumerate() {
                if position > 0 {
                    query.push(", ");
                }
                query.push(format!("{} = ", quoted(column.name)));
                self.bind_column(*index, &mut query)?;
            }
            query.push(format!(" WHERE {} = ", quoted(Self::primary_key().name)));
            key.bind(&mut query)?;
            query.push(format!(" RETURNING {}", column_list::<Self>()));
            let mut transaction = connection.begin().await?;
            let row = query
                .build()
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(SqlError::NotFound)?;
            let value = checked_row::<Self>(&row).await?;
            transaction.commit().await?;
            Ok(value)
        }
    }
    fn get<'c, A>(
        connection: A,
        key: &Self::Key,
    ) -> impl Future<Output = Result<Option<Self>, SqlError>> + Send
    where
        A: Acquire<'c, Database = Sqlite> + Send,
    {
        async move {
            let mut query = select_query::<Self>();
            query.push(format!(" WHERE {} = ", quoted(Self::primary_key().name)));
            key.bind(&mut query)?;
            let mut connection = connection.acquire().await?;
            match query.build().fetch_optional(&mut *connection).await? {
                Some(row) => checked_row::<Self>(&row).await.map(Some),
                None => Ok(None),
            }
        }
    }
    fn delete<'c, A>(
        connection: A,
        key: &Self::Key,
    ) -> impl Future<Output = Result<bool, SqlError>> + Send
    where
        A: Acquire<'c, Database = Sqlite> + Send,
    {
        async move {
            let mut query = QueryBuilder::<Sqlite>::new(format!(
                "DELETE FROM {} WHERE {} = ",
                quoted(Self::TABLE),
                quoted(Self::primary_key().name)
            ));
            key.bind(&mut query)?;
            let mut connection = connection.acquire().await?;
            Ok(query
                .build()
                .execute(&mut *connection)
                .await?
                .rows_affected()
                > 0)
        }
    }
    fn select() -> Select<Self> {
        Select {
            query: select_query::<Self>(),
            filters: 0,
            order: Vec::new(),
            limit: None,
            offset: 0,
            model: PhantomData,
        }
    }
}
fn column_list<T: SqlModel>() -> String {
    T::COLUMNS
        .iter()
        .map(|c| quoted(c.name))
        .collect::<Vec<_>>()
        .join(", ")
}
fn select_query<T: SqlModel>() -> QueryBuilder<Sqlite> {
    QueryBuilder::new(format!(
        "SELECT {} FROM {}",
        column_list::<T>(),
        quoted(T::TABLE)
    ))
}
async fn checked_row<T: SqlModel>(row: &SqliteRow) -> Result<T, SqlError> {
    let value = T::decode_row(row)?;
    value.validate_async().await?;
    Ok(value)
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
/// Parameterized model queries with checked column names and explicit pagination.
pub struct Select<T: SqlModel> {
    query: QueryBuilder<Sqlite>,
    filters: usize,
    order: Vec<(&'static str, Order)>,
    limit: Option<u32>,
    offset: u64,
    model: PhantomData<T>,
}
impl<T: SqlModel> Select<T> {
    fn column(name: &str) -> Result<&'static Column, SqlError> {
        T::COLUMNS
            .iter()
            .find(|c| c.name == name)
            .ok_or_else(|| SqlError::InvalidColumn(name.to_owned()))
    }
    fn conjunction(&mut self) {
        self.query.push(if self.filters == 0 {
            " WHERE "
        } else {
            " AND "
        });
        self.filters += 1;
    }
    pub fn filter<V: SqlValue>(
        mut self,
        column: &str,
        compare: Compare,
        value: &V,
    ) -> Result<Self, SqlError> {
        let column = Self::column(column)?;
        if column.sql_type != V::SQL_TYPE {
            return Err(field_error(
                "sql_type",
                "filter value does not match the column's storage type",
            )
            .at(column.name));
        }
        self.conjunction();
        if value.is_null() {
            let suffix = match compare {
                Compare::Equal => "IS NULL",
                Compare::NotEqual => "IS NOT NULL",
                _ => {
                    return Err(field_error(
                        "sql_null_comparison",
                        "NULL supports equality or inequality filters",
                    ))
                }
            };
            self.query.push(format!("{} {suffix}", quoted(column.name)));
        } else {
            self.query
                .push(format!("{} {} ", quoted(column.name), compare.sql()));
            value.bind(&mut self.query).map_err(|e| e.at(column.name))?;
        }
        Ok(self)
    }
    pub fn where_eq<V: SqlValue>(self, column: &str, value: &V) -> Result<Self, SqlError> {
        self.filter(column, Compare::Equal, value)
    }
    pub fn order_by(mut self, column: &str, order: Order) -> Result<Self, SqlError> {
        let column = Self::column(column)?;
        self.order.push((column.name, order));
        Ok(self)
    }
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = offset;
        self
    }
    pub async fn fetch_all<'c, A>(mut self, connection: A) -> Result<Vec<T>, SqlError>
    where
        A: Acquire<'c, Database = Sqlite> + Send,
    {
        for (index, (column, order)) in self.order.iter().enumerate() {
            self.query
                .push(if index == 0 { " ORDER BY " } else { ", " });
            self.query.push(quoted(column));
            self.query.push(match order {
                Order::Ascending => " ASC",
                Order::Descending => " DESC",
            });
        }
        if let Some(limit) = self.limit {
            self.query.push(" LIMIT ").push_bind(i64::from(limit));
        } else if self.offset > 0 {
            self.query.push(" LIMIT -1");
        }
        if self.offset > 0 {
            self.query.push(" OFFSET ");
            self.offset.bind(&mut self.query)?;
        }
        let mut connection = connection.acquire().await?;
        let rows = self.query.build().fetch_all(&mut *connection).await?;
        let mut values = Vec::with_capacity(rows.len());
        for row in rows {
            values.push(checked_row::<T>(&row).await?);
        }
        Ok(values)
    }
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
                if error.is_unique_violation()
                    || error.is_foreign_key_violation()
                    || error.is_check_violation() =>
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
        (
            status,
            ::axum::Json(crate::json!({"detail":[{"code":code,"message":message}]})),
        )
            .into_response()
    }
}
