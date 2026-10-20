use super::{field_error, SqlError, SqlParameter, SqlType, SqlValue};
use crate::BoxFuture;
use sqlx::{Database, QueryBuilder};
#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
use sqlx::{Row, ValueRef};

/// SQL syntax and schema conventions for each supported driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Sqlite,
    Postgres,
    MySql,
}
impl Dialect {
    pub fn quote(self, identifier: &str) -> String {
        let delimiter = if self == Self::MySql { '`' } else { '"' };
        format!(
            "{delimiter}{}{delimiter}",
            identifier.replace(delimiter, &delimiter.to_string().repeat(2))
        )
    }
}

/// Driver adapter used by generated models and portable bound parameters.
pub trait SqlBackend: Database + Sync {
    const DIALECT: Dialect;
    fn bind_parameter(value: SqlParameter, query: &mut QueryBuilder<Self>);
    fn read_parameter(
        row: &Self::Row,
        column: &str,
        kind: SqlType,
    ) -> Result<SqlParameter, sqlx::Error>;
    fn bind<V: SqlValue>(value: &V, query: &mut QueryBuilder<Self>) -> Result<(), SqlError> {
        Self::bind_parameter(value.to_parameter()?, query);
        Ok(())
    }
    fn read<V: SqlValue>(row: &Self::Row, column: &str) -> Result<V, sqlx::Error> {
        V::from_parameter(Self::read_parameter(row, column, V::SQL_TYPE)?)
    }
    fn execute<'a>(
        query: &'a mut QueryBuilder<Self>,
        connection: &'a mut Self::Connection,
    ) -> BoxFuture<'a, Result<Self::QueryResult, sqlx::Error>>;
    fn fetch_optional<'a>(
        query: &'a mut QueryBuilder<Self>,
        connection: &'a mut Self::Connection,
    ) -> BoxFuture<'a, Result<Option<Self::Row>, sqlx::Error>>;
    fn fetch_all<'a>(
        query: &'a mut QueryBuilder<Self>,
        connection: &'a mut Self::Connection,
    ) -> BoxFuture<'a, Result<Vec<Self::Row>, sqlx::Error>>;
    fn rows_affected(result: &Self::QueryResult) -> u64;
    fn inserted_id(_result: &Self::QueryResult) -> Result<i64, SqlError> {
        Err(field_error(
            "sql_generated_key",
            "this driver uses RETURNING for generated keys",
        ))
    }
}

#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
macro_rules! backend {
    ($db:ty, $dialect:ident, {$($extra:item)*}) => {
        impl SqlBackend for $db {
            const DIALECT: Dialect = Dialect::$dialect;
            fn bind_parameter(value: SqlParameter, query: &mut QueryBuilder<Self>) {
                match value {
                    SqlParameter::Integer(value) => { query.push_bind(value); }
                    SqlParameter::Real(value) => { query.push_bind(value); }
                    SqlParameter::Boolean(value) => { query.push_bind(value); }
                    SqlParameter::Text(value) => { query.push_bind(value); }
                    SqlParameter::Blob(value) => { query.push_bind(value); }
                    SqlParameter::Null(kind) => match kind {
                        SqlType::Integer => { query.push_bind(Option::<i64>::None); }
                        SqlType::Real => { query.push_bind(Option::<f64>::None); }
                        SqlType::Boolean => { query.push_bind(Option::<bool>::None); }
                        SqlType::Text => { query.push_bind(Option::<String>::None); }
                        SqlType::Blob => { query.push_bind(Option::<Vec<u8>>::None); }
                    },
                }
            }
            fn read_parameter(row: &Self::Row, column: &str, kind: SqlType) -> Result<SqlParameter, sqlx::Error> {
                if row.try_get_raw(column)?.is_null() { return Ok(SqlParameter::Null(kind)); }
                Ok(match kind {
                    SqlType::Integer => SqlParameter::Integer(row.try_get(column)?),
                    SqlType::Real => SqlParameter::Real(row.try_get(column)?),
                    SqlType::Boolean => SqlParameter::Boolean(row.try_get(column)?),
                    SqlType::Text => SqlParameter::Text(row.try_get(column)?),
                    SqlType::Blob => SqlParameter::Blob(row.try_get(column)?),
                })
            }
            fn execute<'a>(query: &'a mut QueryBuilder<Self>, connection: &'a mut Self::Connection)
                -> BoxFuture<'a, Result<Self::QueryResult, sqlx::Error>> {
                Box::pin(async move { query.build().execute(connection).await })
            }
            fn fetch_optional<'a>(query: &'a mut QueryBuilder<Self>, connection: &'a mut Self::Connection)
                -> BoxFuture<'a, Result<Option<Self::Row>, sqlx::Error>> {
                Box::pin(async move { query.build().fetch_optional(connection).await })
            }
            fn fetch_all<'a>(query: &'a mut QueryBuilder<Self>, connection: &'a mut Self::Connection)
                -> BoxFuture<'a, Result<Vec<Self::Row>, sqlx::Error>> {
                Box::pin(async move { query.build().fetch_all(connection).await })
            }
            fn rows_affected(result: &Self::QueryResult) -> u64 { result.rows_affected() }
            $($extra)*
        }
    };
}
#[cfg(feature = "sqlite")]
backend!(sqlx::Sqlite, Sqlite, {});
#[cfg(feature = "postgres")]
backend!(sqlx::Postgres, Postgres, {});
#[cfg(feature = "mysql")]
backend!(sqlx::MySql, MySql, {
    fn inserted_id(result: &Self::QueryResult) -> Result<i64, SqlError> {
        i64::try_from(result.last_insert_id()).map_err(|_| {
            field_error(
                "sql_range",
                "generated key is outside the signed 64-bit range",
            )
        })
    }
});
