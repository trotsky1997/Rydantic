use super::{
    check_validation_connection, ddl, field_error,
    table::{checked_row, select_query},
    Column, Compare, Dialect, Order, SqlBackend, SqlError, SqlModel, SqlParameter, SqlRow,
    SqlValue, ValidationConnection,
};
use sqlx::Acquire;
use std::marker::PhantomData;

/// Parameterized queries whose driver is inferred from the supplied connection.
pub struct Select<T: SqlModel> {
    filters: Vec<(&'static str, Compare, SqlParameter)>,
    order: Vec<(&'static str, Order)>,
    limit: Option<u32>,
    offset: u64,
    model: PhantomData<T>,
}
impl<T: SqlModel> Select<T> {
    pub(super) fn new() -> Self {
        Self {
            filters: Vec::new(),
            order: Vec::new(),
            limit: None,
            offset: 0,
            model: PhantomData,
        }
    }
    fn column(name: &str) -> Result<&'static Column, SqlError> {
        T::COLUMNS
            .iter()
            .find(|c| c.name == name)
            .ok_or_else(|| SqlError::InvalidColumn(name.to_owned()))
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
        let value = value
            .to_parameter()
            .map_err(|error| error.at(column.name))?;
        if value.is_null() && !matches!(compare, Compare::Equal | Compare::NotEqual) {
            return Err(field_error(
                "sql_null_comparison",
                "NULL supports equality or inequality filters",
            )
            .at(column.name));
        }
        self.filters.push((column.name, compare, value));
        Ok(self)
    }
    pub fn where_eq<V: SqlValue>(self, column: &str, value: &V) -> Result<Self, SqlError> {
        self.filter(column, Compare::Equal, value)
    }
    pub fn order_by(mut self, column: &str, order: Order) -> Result<Self, SqlError> {
        self.order.push((Self::column(column)?.name, order));
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
    pub async fn fetch_all<'c, A>(self, connection: A) -> Result<Vec<T>, SqlError>
    where
        A: Acquire<'c> + Send,
        A::Database: SqlBackend,
        A::Connection: ValidationConnection,
        T: SqlRow<A::Database>,
    {
        ddl::check_identifiers::<T, A::Database>()?;
        check_validation_connection::<T, A::Connection>()?;
        let dialect = <A::Database as SqlBackend>::DIALECT;
        let mut query = select_query::<T, A::Database>();
        for (index, (column, compare, value)) in self.filters.into_iter().enumerate() {
            query.push(if index == 0 { " WHERE " } else { " AND " });
            query.push(dialect.quote(column));
            if value.is_null() {
                query.push(if matches!(compare, Compare::Equal) {
                    " IS NULL"
                } else {
                    " IS NOT NULL"
                });
            } else {
                query.push(format!(" {} ", compare.sql()));
                A::Database::bind_parameter(value, &mut query);
            }
        }
        for (index, (column, order)) in self.order.into_iter().enumerate() {
            query.push(if index == 0 { " ORDER BY " } else { ", " });
            query.push(dialect.quote(column));
            query.push(match order {
                Order::Ascending => " ASC",
                Order::Descending => " DESC",
            });
        }
        if let Some(limit) = self.limit {
            query.push(" LIMIT ");
            A::Database::bind_parameter(SqlParameter::Integer(i64::from(limit)), &mut query);
        } else if self.offset > 0 {
            match dialect {
                Dialect::Sqlite => {
                    query.push(" LIMIT -1");
                }
                Dialect::MySql => {
                    query.push(" LIMIT 18446744073709551615");
                }
                Dialect::Postgres => {}
            }
        }
        if self.offset > 0 {
            let offset = self.offset.to_parameter()?;
            query.push(" OFFSET ");
            A::Database::bind_parameter(offset, &mut query);
        }
        let mut connection = connection.acquire().await?;
        let rows = A::Database::fetch_all(&mut query, &mut *connection).await?;
        drop(connection);
        let mut values = Vec::with_capacity(rows.len());
        for row in rows {
            values.push(checked_row::<T, A::Database>(&row).await?);
        }
        Ok(values)
    }
}
