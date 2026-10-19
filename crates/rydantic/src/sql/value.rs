use super::{field_error, SqlError};
use sqlx::{sqlite::SqliteRow, QueryBuilder, Row, Sqlite, ValueRef};

/// SQLite storage classes inferred from Rust field types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlType {
    Integer,
    Real,
    Text,
    Blob,
}
impl SqlType {
    pub const fn declaration(self) -> &'static str {
        match self {
            Self::Integer => "INTEGER",
            Self::Real => "REAL",
            Self::Text => "TEXT",
            Self::Blob => "BLOB",
        }
    }
}

/// Typed field encoding. Values are always sent through bound SQL parameters.
pub trait SqlValue: Sized + Send + Sync {
    const SQL_TYPE: SqlType;
    const NULLABLE: bool = false;
    fn bind(&self, query: &mut QueryBuilder<Sqlite>) -> Result<(), SqlError>;
    fn read(row: &SqliteRow, column: &str) -> Result<Self, sqlx::Error>;
    fn is_null(&self) -> bool {
        false
    }
}
pub(crate) fn decode_error(error: impl std::error::Error + Send + Sync + 'static) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(error))
}

macro_rules! integers {
    ($($ty:ty),*)=>{$(
        impl SqlValue for $ty {
            const SQL_TYPE:SqlType=SqlType::Integer;
            fn bind(&self,query:&mut QueryBuilder<Sqlite>)->Result<(),SqlError>{
                let value=i64::try_from(*self).map_err(|_|field_error("sql_range","integer is outside SQLite's signed 64-bit range"))?;
                query.push_bind(value);Ok(())
            }
            fn read(row:&SqliteRow,column:&str)->Result<Self,sqlx::Error>{
                let value:i64=row.try_get(column)?;
                Self::try_from(value).map_err(decode_error)
            }
        }
    )*};
}
integers!(i8, i16, i32, isize, u8, u16, u32, u64, usize);
macro_rules! wide {
    ($($ty:ty),*)=>{$(
        impl SqlValue for $ty {
            const SQL_TYPE:SqlType=SqlType::Text;
            fn bind(&self,query:&mut QueryBuilder<Sqlite>)->Result<(),SqlError>{query.push_bind(self.to_string());Ok(())}
            fn read(row:&SqliteRow,column:&str)->Result<Self,sqlx::Error>{let value:String=row.try_get(column)?;value.parse().map_err(decode_error)}
        }
    )*};
}
wide!(i128, u128);
macro_rules! native {
    ($ty:ty,$storage:ident) => {
        impl SqlValue for $ty {
            const SQL_TYPE: SqlType = SqlType::$storage;
            fn bind(&self, query: &mut QueryBuilder<Sqlite>) -> Result<(), SqlError> {
                query.push_bind(*self);
                Ok(())
            }
            fn read(row: &SqliteRow, column: &str) -> Result<Self, sqlx::Error> {
                row.try_get(column)
            }
        }
    };
}
native!(bool, Integer);
native!(i64, Integer);
native!(f32, Real);
native!(f64, Real);
impl SqlValue for String {
    const SQL_TYPE: SqlType = SqlType::Text;
    fn bind(&self, query: &mut QueryBuilder<Sqlite>) -> Result<(), SqlError> {
        query.push_bind(self.as_str());
        Ok(())
    }
    fn read(row: &SqliteRow, column: &str) -> Result<Self, sqlx::Error> {
        row.try_get(column)
    }
}
impl SqlValue for Vec<u8> {
    const SQL_TYPE: SqlType = SqlType::Blob;
    fn bind(&self, query: &mut QueryBuilder<Sqlite>) -> Result<(), SqlError> {
        query.push_bind(self.as_slice());
        Ok(())
    }
    fn read(row: &SqliteRow, column: &str) -> Result<Self, sqlx::Error> {
        row.try_get(column)
    }
}
impl<T: SqlValue> SqlValue for Option<T> {
    const SQL_TYPE: SqlType = T::SQL_TYPE;
    const NULLABLE: bool = true;
    fn bind(&self, query: &mut QueryBuilder<Sqlite>) -> Result<(), SqlError> {
        match self {
            Some(value) => value.bind(query),
            None => {
                query.push_bind(Option::<i64>::None);
                Ok(())
            }
        }
    }
    fn read(row: &SqliteRow, column: &str) -> Result<Self, sqlx::Error> {
        if row.try_get_raw(column)?.is_null() {
            Ok(None)
        } else {
            T::read(row, column).map(Some)
        }
    }
    fn is_null(&self) -> bool {
        self.is_none()
    }
}
impl<T: SqlValue> SqlValue for Box<T> {
    const SQL_TYPE: SqlType = T::SQL_TYPE;
    const NULLABLE: bool = T::NULLABLE;
    fn bind(&self, query: &mut QueryBuilder<Sqlite>) -> Result<(), SqlError> {
        (**self).bind(query)
    }
    fn read(row: &SqliteRow, column: &str) -> Result<Self, sqlx::Error> {
        T::read(row, column).map(Box::new)
    }
    fn is_null(&self) -> bool {
        (**self).is_null()
    }
}

/// Supported primary-key representations, including optional generated keys.
pub trait Key: SqlValue {
    type Value: SqlValue;
    fn value(&self) -> Option<&Self::Value>;
}
macro_rules! keys {
    ($($ty:ty),*)=>{$(impl Key for $ty{type Value=Self;fn value(&self)->Option<&Self>{Some(self)}})*};
}
keys!(String, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, i128, u128);
impl<T: SqlValue> Key for Option<T> {
    type Value = T;
    fn value(&self) -> Option<&T> {
        self.as_ref()
    }
}

/// Auto-increment keys use i64; zero or None requests a generated value.
pub trait AutoKey: Key {
    fn needs_generation(&self) -> bool;
}
impl AutoKey for i64 {
    fn needs_generation(&self) -> bool {
        *self == 0
    }
}
impl AutoKey for Option<i64> {
    fn needs_generation(&self) -> bool {
        self.is_none()
    }
}

#[cfg(any(feature = "chrono", feature = "decimal"))]
macro_rules! text {
    ($ty:ty) => {
        impl SqlValue for $ty {
            const SQL_TYPE: SqlType = SqlType::Text;
            fn bind(&self, query: &mut QueryBuilder<Sqlite>) -> Result<(), SqlError> {
                let mut ctx = crate::Context::new();
                let value = <Self as crate::JsonType>::encode(self, &mut ctx);
                ctx.result()?;
                let crate::Value::String(value) = value else {
                    return Err(field_error(
                        "sql_type",
                        "expected a scalar string representation",
                    ));
                };
                query.push_bind(value);
                Ok(())
            }
            fn read(row: &SqliteRow, column: &str) -> Result<Self, sqlx::Error> {
                let value: String = row.try_get(column)?;
                let mut ctx = crate::Context::new();
                let value =
                    <Self as crate::JsonType>::decode(crate::Value::String(value), &mut ctx, false);
                ctx.finish(value).map_err(decode_error)
            }
        }
    };
}
#[cfg(feature = "chrono")]
text!(chrono::NaiveDate);
#[cfg(feature = "chrono")]
text!(chrono::NaiveTime);
#[cfg(feature = "chrono")]
text!(chrono::NaiveDateTime);
#[cfg(feature = "chrono")]
text!(chrono::DateTime<chrono::Utc>);
#[cfg(feature = "chrono")]
text!(chrono::DateTime<chrono::FixedOffset>);
#[cfg(feature = "decimal")]
text!(rust_decimal::Decimal);
