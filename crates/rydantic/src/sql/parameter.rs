use super::{field_error, SqlError};

/// Portable storage types mapped to native declarations by each SQL dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlType {
    Integer,
    Real,
    Boolean,
    Text,
    Blob,
}
/// Owned parameters let a query choose its driver at execution time.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlParameter {
    Null(SqlType),
    Integer(i64),
    Real(f64),
    Boolean(bool),
    Text(String),
    Blob(Vec<u8>),
}
impl SqlParameter {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null(_))
    }
}

/// Portable codecs shared by all SQL drivers and deferred query filters.
/// Implement both conversions for custom field types.
pub trait SqlValue: Sized + Send + Sync {
    const SQL_TYPE: SqlType;
    const NULLABLE: bool = false;
    fn to_parameter(&self) -> Result<SqlParameter, SqlError>;
    fn from_parameter(value: SqlParameter) -> Result<Self, sqlx::Error>;
}
pub(crate) fn decode_error(error: impl std::error::Error + Send + Sync + 'static) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(error))
}
fn wrong_type() -> sqlx::Error {
    decode_error(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "SQL value has an incompatible storage type",
    ))
}
macro_rules! integers {
    ($($ty:ty),*) => {$(impl SqlValue for $ty {
        const SQL_TYPE: SqlType = SqlType::Integer;
        fn to_parameter(&self) -> Result<SqlParameter, SqlError> {
            i64::try_from(*self).map(SqlParameter::Integer)
                .map_err(|_|field_error("sql_range", "integer is outside the signed 64-bit SQL range"))
        }
        fn from_parameter(value: SqlParameter) -> Result<Self, sqlx::Error> {
            let SqlParameter::Integer(value) = value else { return Err(wrong_type()); };
            Self::try_from(value).map_err(decode_error)
        }
    })*};
}
integers!(i8, i16, i32, isize, u8, u16, u32, u64, usize);
macro_rules! native {
    ($ty:ty, $kind:ident, $variant:ident) => {
        impl SqlValue for $ty {
            const SQL_TYPE: SqlType = SqlType::$kind;
            fn to_parameter(&self) -> Result<SqlParameter, SqlError> {
                Ok(SqlParameter::$variant(self.to_owned()))
            }
            fn from_parameter(value: SqlParameter) -> Result<Self, sqlx::Error> {
                match value {
                    SqlParameter::$variant(value) => Ok(value),
                    _ => Err(wrong_type()),
                }
            }
        }
    };
}
native!(i64, Integer, Integer);
native!(f64, Real, Real);
native!(bool, Boolean, Boolean);
native!(String, Text, Text);
native!(Vec<u8>, Blob, Blob);
impl SqlValue for f32 {
    const SQL_TYPE: SqlType = SqlType::Real;
    fn to_parameter(&self) -> Result<SqlParameter, SqlError> {
        Ok(SqlParameter::Real(f64::from(*self)))
    }
    fn from_parameter(value: SqlParameter) -> Result<Self, sqlx::Error> {
        let SqlParameter::Real(value) = value else {
            return Err(wrong_type());
        };
        Ok(value as f32)
    }
}
macro_rules! wide {
    ($($ty:ty),*) => {$(impl SqlValue for $ty {
        const SQL_TYPE: SqlType = SqlType::Text;
        fn to_parameter(&self) -> Result<SqlParameter, SqlError> { Ok(SqlParameter::Text(self.to_string())) }
        fn from_parameter(value: SqlParameter) -> Result<Self, sqlx::Error> {
            let SqlParameter::Text(value) = value else { return Err(wrong_type()); };
            value.parse().map_err(decode_error)
        }
    })*};
}
wide!(i128, u128);
impl<T: SqlValue> SqlValue for Option<T> {
    const SQL_TYPE: SqlType = T::SQL_TYPE;
    const NULLABLE: bool = true;
    fn to_parameter(&self) -> Result<SqlParameter, SqlError> {
        match self {
            Some(value) => value.to_parameter(),
            None => Ok(SqlParameter::Null(T::SQL_TYPE)),
        }
    }
    fn from_parameter(value: SqlParameter) -> Result<Self, sqlx::Error> {
        match value {
            SqlParameter::Null(_) => Ok(None),
            value => T::from_parameter(value).map(Some),
        }
    }
}
impl<T: SqlValue> SqlValue for Box<T> {
    const SQL_TYPE: SqlType = T::SQL_TYPE;
    const NULLABLE: bool = T::NULLABLE;
    fn to_parameter(&self) -> Result<SqlParameter, SqlError> {
        (**self).to_parameter()
    }
    fn from_parameter(value: SqlParameter) -> Result<Self, sqlx::Error> {
        T::from_parameter(value).map(Box::new)
    }
}
/// Supported primary keys, including optional generated keys.
pub trait Key: SqlValue {
    type Value: SqlValue;
    fn value(&self) -> Option<&Self::Value>;
}
macro_rules! keys { ($($ty:ty),*) => {$(impl Key for $ty {
    type Value = Self; fn value(&self) -> Option<&Self> { Some(self) }
})*}; }
keys!(String, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, i128, u128);
impl<T: SqlValue> Key for Option<T> {
    type Value = T;
    fn value(&self) -> Option<&T> {
        self.as_ref()
    }
}
/// Zero or None requests an auto-generated i64 key.
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
            fn to_parameter(&self) -> Result<SqlParameter, SqlError> {
                let mut ctx = crate::Context::new();
                let value = <Self as crate::JsonType>::encode(self, &mut ctx);
                ctx.result()?;
                let crate::Value::String(value) = value else {
                    return Err(field_error(
                        "sql_type",
                        "expected a scalar string representation",
                    ));
                };
                Ok(SqlParameter::Text(value))
            }
            fn from_parameter(value: SqlParameter) -> Result<Self, sqlx::Error> {
                let SqlParameter::Text(value) = value else {
                    return Err(wrong_type());
                };
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
