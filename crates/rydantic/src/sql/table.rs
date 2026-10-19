use super::{
    check_validation_connection, ddl, field_error, Column, Dialect, Select, SqlBackend, SqlError,
    SqlParameter, SqlValue, ValidationConnection,
};
use crate::{AsyncCheck, Context, Model};
use sqlx::{Acquire, QueryBuilder, Sqlite};
use std::future::Future;

/// Typed row decoding and binding for a specific SQLx driver.
/// Generated once for all supported drivers by `#[derive(SqlModel)]`.
pub trait SqlRow<DB: SqlBackend>: SqlModel {
    fn decode_sql_row(row: &DB::Row) -> Result<Self, sqlx::Error>;
    fn bind_sql_column(&self, index: usize, query: &mut QueryBuilder<DB>) -> Result<(), SqlError>;
}

/// One model with driver-inferred SQLite, PostgreSQL and MySQL operations.
///
/// Writes validate before acquiring a connection and recheck returned rows
/// before commit. Pooled reads release their connection before async checks.
/// Models with async rules require a pool. MySQL schema creation also requires
/// a pool because its DDL implicitly commits an existing transaction.
pub trait SqlModel: Model + AsyncCheck + Send + 'static {
    type Key: SqlValue;
    const TABLE: &'static str;
    const COLUMNS: &'static [Column];
    #[doc(hidden)]
    fn decode_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error>;
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
        Self::create_table_sql_for::<Sqlite>()
    }
    fn create_index_sql() -> Vec<String> {
        Self::create_index_sql_for::<Sqlite>()
    }
    fn create_table_sql_for<DB: SqlBackend>() -> String {
        ddl::table::<Self, DB>()
    }
    fn create_index_sql_for<DB: SqlBackend>() -> Vec<String> {
        ddl::indexes::<Self, DB>()
    }

    fn create_table<'c, A>(connection: A) -> impl Future<Output = Result<(), SqlError>> + Send
    where
        A: Acquire<'c> + Send,
        A::Database: SqlBackend,
        A::Connection: ValidationConnection,
    {
        async move {
            ddl::check_identifiers::<Self, A::Database>()?;
            let mut query =
                QueryBuilder::<A::Database>::new(Self::create_table_sql_for::<A::Database>());
            if <A::Database as SqlBackend>::DIALECT == Dialect::MySql {
                if !A::Connection::RELEASES_POOL_SLOT {
                    return Err(field_error(
                        "sql_ddl_connection",
                        "MySQL schema creation requires a pool because DDL implicitly commits",
                    ));
                }
                let mut connection = connection.acquire().await?;
                A::Database::execute(&mut query, &mut *connection).await?;
            } else {
                let mut transaction = connection.begin().await?;
                A::Database::execute(&mut query, &mut transaction).await?;
                for statement in Self::create_index_sql_for::<A::Database>() {
                    A::Database::execute(&mut QueryBuilder::new(statement), &mut transaction)
                        .await?;
                }
                transaction.commit().await?;
            }
            Ok(())
        }
    }
    fn insert<'c, A>(&self, connection: A) -> impl Future<Output = Result<Self, SqlError>> + Send
    where
        A: Acquire<'c> + Send,
        A::Database: SqlBackend,
        A::Connection: ValidationConnection,
        Self: SqlRow<A::Database>,
    {
        async move {
            ddl::check_identifiers::<Self, A::Database>()?;
            check_validation_connection::<Self, A::Connection>()?;
            self.validate_async().await?;
            if self.key().is_none() && !Self::primary_key().auto_increment {
                return Err(field_error(
                    "sql_primary_key_missing",
                    "insert requires a primary key",
                )
                .at(Self::primary_key().name));
            }
            let dialect = <A::Database as SqlBackend>::DIALECT;
            let included = Self::COLUMNS
                .iter()
                .enumerate()
                .filter(|(i, _)| self.include_insert_column(*i))
                .collect::<Vec<_>>();
            let mut query = QueryBuilder::<A::Database>::new(format!(
                "INSERT INTO {}",
                dialect.quote(Self::TABLE)
            ));
            if included.is_empty() {
                query.push(if dialect == Dialect::MySql {
                    " () VALUES ()"
                } else {
                    " DEFAULT VALUES"
                });
            } else {
                query.push(format!(
                    " ({}) VALUES (",
                    included
                        .iter()
                        .map(|(_, c)| dialect.quote(c.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                for (position, (index, _)) in included.iter().enumerate() {
                    if position > 0 {
                        query.push(", ");
                    }
                    self.bind_sql_column(*index, &mut query)?;
                }
                query.push(")");
            }
            let mut transaction = connection.begin().await?;
            let row = if dialect == Dialect::MySql {
                let result = A::Database::execute(&mut query, &mut transaction).await?;
                let mut query = select_query::<Self, A::Database>();
                query.push(format!(
                    " WHERE {} = ",
                    dialect.quote(Self::primary_key().name)
                ));
                let generated = Self::primary_key().auto_increment
                    && match self.key() {
                        None => true,
                        Some(key) => matches!(key.to_parameter()?, SqlParameter::Integer(0)),
                    };
                if generated {
                    A::Database::bind_parameter(
                        SqlParameter::Integer(A::Database::inserted_id(&result)?),
                        &mut query,
                    );
                } else {
                    A::Database::bind(self.key().expect("insert key was checked"), &mut query)?;
                }
                A::Database::fetch_optional(&mut query, &mut transaction)
                    .await?
                    .ok_or(SqlError::NotFound)?
            } else {
                query.push(format!(" RETURNING {}", column_list::<Self, A::Database>()));
                A::Database::fetch_optional(&mut query, &mut transaction)
                    .await?
                    .ok_or(SqlError::NotFound)?
            };
            let value = checked_write_row::<Self, A::Database>(&row)?;
            transaction.commit().await?;
            Ok(value)
        }
    }
    fn update<'c, A>(&self, connection: A) -> impl Future<Output = Result<Self, SqlError>> + Send
    where
        A: Acquire<'c> + Send,
        A::Database: SqlBackend,
        A::Connection: ValidationConnection,
        Self: SqlRow<A::Database>,
    {
        async move {
            ddl::check_identifiers::<Self, A::Database>()?;
            check_validation_connection::<Self, A::Connection>()?;
            self.validate_async().await?;
            let key = self.key().ok_or_else(|| {
                field_error("sql_primary_key_missing", "update requires a primary key")
                    .at(Self::primary_key().name)
            })?;
            let dialect = <A::Database as SqlBackend>::DIALECT;
            let mut query = QueryBuilder::<A::Database>::new(format!(
                "UPDATE {} SET ",
                dialect.quote(Self::TABLE)
            ));
            let mutable = Self::COLUMNS
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.primary_key)
                .collect::<Vec<_>>();
            if mutable.is_empty() {
                query.push(format!(
                    "{0} = {0}",
                    dialect.quote(Self::primary_key().name)
                ));
            }
            for (position, (index, column)) in mutable.iter().enumerate() {
                if position > 0 {
                    query.push(", ");
                }
                query.push(format!("{} = ", dialect.quote(column.name)));
                self.bind_sql_column(*index, &mut query)?;
            }
            query.push(format!(
                " WHERE {} = ",
                dialect.quote(Self::primary_key().name)
            ));
            A::Database::bind(key, &mut query)
                .map_err(|error| error.at(Self::primary_key().name))?;
            let mut transaction = connection.begin().await?;
            let row = if dialect == Dialect::MySql {
                A::Database::execute(&mut query, &mut transaction).await?;
                let mut query = select_query::<Self, A::Database>();
                query.push(format!(
                    " WHERE {} = ",
                    dialect.quote(Self::primary_key().name)
                ));
                A::Database::bind(key, &mut query)?;
                A::Database::fetch_optional(&mut query, &mut transaction)
                    .await?
                    .ok_or(SqlError::NotFound)?
            } else {
                query.push(format!(" RETURNING {}", column_list::<Self, A::Database>()));
                A::Database::fetch_optional(&mut query, &mut transaction)
                    .await?
                    .ok_or(SqlError::NotFound)?
            };
            let value = checked_write_row::<Self, A::Database>(&row)?;
            transaction.commit().await?;
            Ok(value)
        }
    }
    fn get<'c, A>(
        connection: A,
        key: &Self::Key,
    ) -> impl Future<Output = Result<Option<Self>, SqlError>> + Send
    where
        A: Acquire<'c> + Send,
        A::Database: SqlBackend,
        A::Connection: ValidationConnection,
        Self: SqlRow<A::Database>,
    {
        async move {
            ddl::check_identifiers::<Self, A::Database>()?;
            check_validation_connection::<Self, A::Connection>()?;
            let mut query = select_query::<Self, A::Database>();
            query.push(format!(
                " WHERE {} = ",
                <A::Database as SqlBackend>::DIALECT.quote(Self::primary_key().name)
            ));
            A::Database::bind(key, &mut query)
                .map_err(|error| error.at(Self::primary_key().name))?;
            let mut connection = connection.acquire().await?;
            let row = A::Database::fetch_optional(&mut query, &mut *connection).await?;
            drop(connection);
            match row {
                Some(row) => checked_row::<Self, A::Database>(&row).await.map(Some),
                None => Ok(None),
            }
        }
    }
    fn delete<'c, A>(
        connection: A,
        key: &Self::Key,
    ) -> impl Future<Output = Result<bool, SqlError>> + Send
    where
        A: Acquire<'c> + Send,
        A::Database: SqlBackend,
    {
        async move {
            ddl::check_identifiers::<Self, A::Database>()?;
            let mut query = QueryBuilder::<A::Database>::new(format!(
                "DELETE FROM {} WHERE {} = ",
                <A::Database as SqlBackend>::DIALECT.quote(Self::TABLE),
                <A::Database as SqlBackend>::DIALECT.quote(Self::primary_key().name)
            ));
            A::Database::bind(key, &mut query)
                .map_err(|error| error.at(Self::primary_key().name))?;
            let mut connection = connection.acquire().await?;
            Ok(A::Database::rows_affected(
                &A::Database::execute(&mut query, &mut *connection).await?,
            ) > 0)
        }
    }
    fn select() -> Select<Self> {
        Select::new()
    }
}

fn column_list<T: SqlModel, DB: SqlBackend>() -> String {
    T::COLUMNS
        .iter()
        .map(|c| DB::DIALECT.quote(c.name))
        .collect::<Vec<_>>()
        .join(", ")
}
pub(super) fn select_query<T: SqlModel, DB: SqlBackend>() -> QueryBuilder<DB> {
    QueryBuilder::new(format!(
        "SELECT {} FROM {}",
        column_list::<T, DB>(),
        DB::DIALECT.quote(T::TABLE)
    ))
}
pub(super) async fn checked_row<T: SqlRow<DB>, DB: SqlBackend>(
    row: &DB::Row,
) -> Result<T, SqlError> {
    let value = T::decode_sql_row(row)?;
    value.validate_async().await?;
    Ok(value)
}
fn checked_write_row<T: SqlRow<DB>, DB: SqlBackend>(row: &DB::Row) -> Result<T, SqlError> {
    let value = T::decode_sql_row(row)?;
    let mut ctx = Context::new();
    value.validate_into(&mut ctx);
    ctx.result()?;
    Ok(value)
}
