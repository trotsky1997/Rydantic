//! Native Cap'n Proto integration with automatic protocols and existing schemas.
//! Model data maps directly to official readers and builders.
//!
//! ```
//! use rydantic::{CapnpModel, CapnpSchema};
//! #[rydantic::capnp_models(namespace = "docs.users")]
//! mod models {
//!     #[derive(Debug, PartialEq)]
//!     pub struct User { pub name: String, pub age: u8 }
//! }
//! let user = models::User { name: "Alice".into(), age: 25 };
//! assert_eq!(models::User::parse_capnp(&user.dump_capnp()?)?, user);
//! assert!(models::User::capnp_schema().contains("struct User"));
//! # Ok::<(), rydantic::ValidationErrors>(())
//! ```
use crate::{AsyncCheck, Context, Model, Stage, ValidationError, ValidationErrors};
use capnp::{
    message::{Builder, HeapAllocator, ReaderOptions},
    traits::Owned,
};

/// The exported protocol derived by `#[rydantic::capnp_models]`.
pub trait CapnpSchema: Model {
    const CAPNP_SCHEMA: &'static str;
    /// Root declaration for this model (including enum root wrappers).
    const CAPNP_TYPE: &'static str;
    fn capnp_schema() -> &'static str {
        Self::CAPNP_SCHEMA
    }
    fn write_capnp_schema(path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, Self::CAPNP_SCHEMA)
    }
}

pub trait CapnpEnum: Sized {
    type Wire;
    fn read_enum(value: Self::Wire, ctx: &mut Context) -> Option<Self>;
    fn write_enum(&self) -> Self::Wire;
}
/// A model bound to official Cap'n Proto readers/builders.
/// Use `#[rydantic::capnp_models]` to infer the protocol automatically, or
/// `#[model(capnp = "wire::module")]` to bind an existing schema.
pub trait CapnpModel: Model {
    type Wire: Owned;
    fn read_capnp(reader: <Self::Wire as Owned>::Reader<'_>, ctx: &mut Context) -> Option<Self>;
    fn write_capnp(&self, builder: <Self::Wire as Owned>::Builder<'_>, ctx: &mut Context);
    fn parse_capnp(bytes: &[u8]) -> Result<Self, ValidationErrors> {
        Self::parse_capnp_with_options(bytes, ReaderOptions::new())
    }
    fn parse_capnp_with_options(
        bytes: &[u8],
        options: ReaderOptions,
    ) -> Result<Self, ValidationErrors> {
        crate::asynchronous::sync_guard(Self::async_required(
            &mut std::collections::HashSet::new(),
        ))?;
        decode::<Self>(bytes, false, options)
    }
    fn parse_capnp_packed(bytes: &[u8]) -> Result<Self, ValidationErrors> {
        crate::asynchronous::sync_guard(Self::async_required(
            &mut std::collections::HashSet::new(),
        ))?;
        decode::<Self>(bytes, true, ReaderOptions::new())
    }
    fn from_capnp_reader(
        reader: <Self::Wire as Owned>::Reader<'_>,
    ) -> Result<Self, ValidationErrors> {
        crate::asynchronous::sync_guard(Self::async_required(
            &mut std::collections::HashSet::new(),
        ))?;
        let mut ctx = Context::new();
        let value = Self::read_capnp(reader, &mut ctx);
        let value = ctx.finish(value)?;
        let mut ctx = Context::new();
        let value = value.prepare_default(&mut ctx);
        ctx.finish(value)
    }
    fn to_capnp_message(&self) -> Result<Builder<HeapAllocator>, ValidationErrors> {
        self.validate()?;
        build(self)
    }
    fn dump_capnp(&self) -> Result<Vec<u8>, ValidationErrors> {
        let message = self.to_capnp_message()?;
        Ok(capnp::serialize::write_message_to_words(&message))
    }
    fn dump_capnp_packed(&self) -> Result<Vec<u8>, ValidationErrors> {
        let message = self.to_capnp_message()?;
        packed_bytes(&message)
    }
    fn parse_capnp_async(
        bytes: &[u8],
    ) -> impl std::future::Future<Output = Result<Self, ValidationErrors>> + Send
    where
        Self: AsyncCheck + Send,
    {
        async move {
            let value = decode::<Self>(bytes, false, ReaderOptions::new())?;
            let mut ctx = Context::new();
            value.check_async(&mut ctx).await;
            ctx.result()?;
            Ok(value)
        }
    }
    fn parse_capnp_packed_async(
        bytes: &[u8],
    ) -> impl std::future::Future<Output = Result<Self, ValidationErrors>> + Send
    where
        Self: AsyncCheck + Send,
    {
        async move {
            let value = decode::<Self>(bytes, true, ReaderOptions::new())?;
            let mut ctx = Context::new();
            value.check_async(&mut ctx).await;
            ctx.result()?;
            Ok(value)
        }
    }
    fn dump_capnp_async(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, ValidationErrors>> + Send
    where
        Self: AsyncCheck,
    {
        async move {
            self.validate_async().await?;
            Ok(capnp::serialize::write_message_to_words(&build(self)?))
        }
    }
    fn dump_capnp_packed_async(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, ValidationErrors>> + Send
    where
        Self: AsyncCheck,
    {
        async move {
            self.validate_async().await?;
            packed_bytes(&build(self)?)
        }
    }
}
fn decode<T: CapnpModel>(
    mut bytes: &[u8],
    packed: bool,
    options: ReaderOptions,
) -> Result<T, ValidationErrors> {
    let mut ctx = Context::new();
    let value = if packed {
        let message =
            capnp::serialize_packed::read_message(&mut bytes, options).map_err(parse_error)?;
        let root = message
            .get_root::<<T::Wire as Owned>::Reader<'_>>()
            .map_err(parse_error)?;
        T::read_capnp(root, &mut ctx)
    } else {
        let message = capnp::serialize::read_message_from_flat_slice(&mut bytes, options)
            .map_err(parse_error)?;
        let root = message
            .get_root::<<T::Wire as Owned>::Reader<'_>>()
            .map_err(parse_error)?;
        T::read_capnp(root, &mut ctx)
    };
    if !bytes.is_empty() {
        ctx.push(
            ValidationError::new(
                "capnp_trailing",
                "unexpected bytes after Cap'n Proto message",
            )
            .with_stage(Stage::Parse),
        );
    }
    let value = ctx.finish(value)?;
    let mut ctx = Context::new();
    let value = value.prepare_default(&mut ctx);
    ctx.finish(value)
}
fn build<T: CapnpModel>(value: &T) -> Result<Builder<HeapAllocator>, ValidationErrors> {
    let mut message = Builder::new_default();
    let mut ctx = Context::new();
    value.write_capnp(
        message.init_root::<<T::Wire as Owned>::Builder<'_>>(),
        &mut ctx,
    );
    ctx.result()?;
    Ok(message)
}
fn packed_bytes(message: &Builder<HeapAllocator>) -> Result<Vec<u8>, ValidationErrors> {
    let mut bytes = Vec::new();
    capnp::serialize_packed::write_message(&mut bytes, message).map_err(|error| {
        ValidationError::new("capnp_encode", error.to_string()).with_stage(Stage::Serialize)
    })?;
    Ok(bytes)
}
fn parse_error(error: impl std::fmt::Display) -> ValidationErrors {
    ValidationError::new("capnp_decode", error.to_string())
        .with_stage(Stage::Parse)
        .into()
}
#[doc(hidden)]
pub fn read_result<T, E: std::fmt::Display>(value: Result<T, E>, ctx: &mut Context) -> Option<T> {
    value
        .map_err(|error| {
            ctx.push(
                ValidationError::new("capnp_decode", error.to_string()).with_stage(Stage::Parse),
            )
        })
        .ok()
}
#[doc(hidden)]
pub fn which_name(value: capnp::dynamic_value::Reader<'_>, ctx: &mut Context) -> Option<String> {
    let capnp::dynamic_value::Reader::Struct(reader) = value else {
        ctx.push(ValidationError::new(
            "capnp_schema",
            "expected a union struct",
        ));
        return None;
    };
    let field = read_result(reader.which(), ctx)?;
    let Some(field) = field else {
        ctx.push(ValidationError::new(
            "capnp_union",
            "schema has no active union field",
        ));
        return None;
    };
    let name = read_result(field.get_proto().get_name(), ctx)?;
    Some(read_result(name.to_str(), ctx)?.to_owned())
}
#[doc(hidden)]
pub fn optional_some(value: capnp::dynamic_value::Reader<'_>, ctx: &mut Context) -> Option<bool> {
    match which_name(value, ctx)?.as_str() {
        "some" => Some(true),
        "none" => Some(false),
        _ => {
            ctx.push(ValidationError::new(
                "capnp_schema",
                "optional union must contain none and some",
            ));
            None
        }
    }
}
#[doc(hidden)]
pub trait SetResult {
    fn checked(self, ctx: &mut Context) -> Option<()>;
}
impl SetResult for () {
    fn checked(self, _: &mut Context) -> Option<()> {
        Some(())
    }
}
impl SetResult for capnp::Result<()> {
    fn checked(self, ctx: &mut Context) -> Option<()> {
        self.map_err(|error| {
            ctx.push(
                ValidationError::new("capnp_encode", error.to_string())
                    .with_stage(Stage::Serialize),
            )
        })
        .ok()
    }
}
#[doc(hidden)]
pub fn write_result(value: impl SetResult, ctx: &mut Context) -> Option<()> {
    value.checked(ctx)
}
#[doc(hidden)]
pub fn list_len(length: usize, ctx: &mut Context) -> Option<u32> {
    if length >= (1 << 29) {
        ctx.push(
            ValidationError::new(
                "capnp_length",
                "lists are limited to fewer than 2^29 elements",
            )
            .with_stage(Stage::Serialize),
        );
        return None;
    }
    u32::try_from(length)
        .map_err(|_| {
            ctx.push(
                ValidationError::new("capnp_length", "list exceeds the wire format length")
                    .with_stage(Stage::Serialize),
            )
        })
        .ok()
}
#[doc(hidden)]
pub fn converted<T, U: TryFrom<T>>(value: T, ctx: &mut Context) -> Option<U> {
    U::try_from(value)
        .map_err(|_| {
            ctx.push(ValidationError::new(
                "capnp_range",
                "numeric value exceeds target wire/model range",
            ))
        })
        .ok()
}

/// Native dynamic values used for union payloads. Implemented by derived models.
#[doc(hidden)]
pub trait CapnpRead: Sized {
    fn read_value(value: capnp::dynamic_value::Reader<'_>, ctx: &mut Context) -> Option<Self>;
}
#[doc(hidden)]
pub fn mismatch(ctx: &mut Context) {
    ctx.push(ValidationError::new(
        "capnp_schema",
        "wire field type does not match the model",
    ));
}
#[doc(hidden)]
pub fn field_value<'a>(
    value: capnp::dynamic_value::Reader<'a>,
    name: &str,
    ctx: &mut Context,
) -> Option<capnp::dynamic_value::Reader<'a>> {
    let capnp::dynamic_value::Reader::Struct(reader) = value else {
        mismatch(ctx);
        return None;
    };
    read_result(reader.get_named(name), ctx)
}
#[doc(hidden)]
pub fn text_value<'a>(
    value: capnp::dynamic_value::Reader<'a>,
    ctx: &mut Context,
) -> Option<&'a str> {
    let capnp::dynamic_value::Reader::Text(text) = value else {
        mismatch(ctx);
        return None;
    };
    read_result(text.to_str(), ctx)
}
#[doc(hidden)]
pub fn data_value<'a>(
    value: capnp::dynamic_value::Reader<'a>,
    ctx: &mut Context,
) -> Option<&'a [u8]> {
    let capnp::dynamic_value::Reader::Data(data) = value else {
        mismatch(ctx);
        return None;
    };
    Some(data)
}
#[doc(hidden)]
pub fn struct_value<'a, T: capnp::traits::OwnedStruct>(
    value: capnp::dynamic_value::Reader<'a>,
    ctx: &mut Context,
) -> Option<T::Reader<'a>> {
    let capnp::dynamic_value::Reader::Struct(reader) = value else {
        mismatch(ctx);
        return None;
    };
    let capnp::introspect::TypeVariant::Struct(schema) = T::introspect().which() else {
        mismatch(ctx);
        return None;
    };
    if reader.get_schema() != capnp::schema::StructSchema::new(schema) {
        mismatch(ctx);
        return None;
    }
    Some(reader.downcast::<T>())
}
macro_rules! integer_read {
    ($($ty:ty),*) => {$ (
        impl CapnpRead for $ty {
            fn read_value(value: capnp::dynamic_value::Reader<'_>, ctx: &mut Context) -> Option<Self> {
                use capnp::dynamic_value::Reader::*;
                match value {
                    Int8(n) => converted(n,ctx), Int16(n) => converted(n,ctx), Int32(n) => converted(n,ctx), Int64(n) => converted(n,ctx),
                    UInt8(n) => converted(n,ctx), UInt16(n) => converted(n,ctx), UInt32(n) => converted(n,ctx), UInt64(n) => converted(n,ctx),
                    Text(text) => <Self as crate::JsonType>::decode(crate::Value::String(read_result(text.to_str(),ctx)?.to_owned()),ctx,true),
                    _ => {mismatch(ctx);None}
                }
            }
        }
    )*};
}
integer_read!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);
macro_rules! scalar_read {
    ($ty:ty,$variant:ident) => {
        impl CapnpRead for $ty {
            fn read_value(
                value: capnp::dynamic_value::Reader<'_>,
                ctx: &mut Context,
            ) -> Option<Self> {
                match value {
                    capnp::dynamic_value::Reader::$variant(value) => Some(value),
                    _ => {
                        mismatch(ctx);
                        None
                    }
                }
            }
        }
    };
}
scalar_read!(bool, Bool);
scalar_read!(f32, Float32);
scalar_read!(f64, Float64);
impl CapnpRead for String {
    fn read_value(value: capnp::dynamic_value::Reader<'_>, ctx: &mut Context) -> Option<Self> {
        Some(text_value(value, ctx)?.to_owned())
    }
}
impl<T: CapnpRead> CapnpRead for Box<T> {
    fn read_value(value: capnp::dynamic_value::Reader<'_>, ctx: &mut Context) -> Option<Self> {
        Some(Box::new(T::read_value(value, ctx)?))
    }
}
impl<T: CapnpRead> CapnpRead for Option<T> {
    fn read_value(value: capnp::dynamic_value::Reader<'_>, ctx: &mut Context) -> Option<Self> {
        if optional_some(value, ctx)? {
            Some(Some(T::read_value(field_value(value, "some", ctx)?, ctx)?))
        } else {
            Some(None)
        }
    }
}
impl<T: CapnpRead> CapnpRead for Vec<T> {
    fn read_value(value: capnp::dynamic_value::Reader<'_>, ctx: &mut Context) -> Option<Self> {
        if let capnp::dynamic_value::Reader::Data(data) = value {
            return data
                .iter()
                .enumerate()
                .map(|(index, byte)| {
                    ctx.at(index, |ctx| {
                        T::read_value(capnp::dynamic_value::Reader::UInt8(*byte), ctx)
                    })
                })
                .collect();
        }
        let capnp::dynamic_value::Reader::List(list) = value else {
            mismatch(ctx);
            return None;
        };
        let mut values = Vec::with_capacity(list.len() as usize);
        for index in 0..list.len() {
            values.push(ctx.at(index as usize, |ctx| {
                T::read_value(read_result(list.get(index), ctx)?, ctx)
            })?);
        }
        Some(values)
    }
}
impl<T: CapnpRead, const N: usize> CapnpRead for [T; N] {
    fn read_value(value: capnp::dynamic_value::Reader<'_>, ctx: &mut Context) -> Option<Self> {
        Vec::<T>::read_value(value, ctx)?
            .try_into()
            .map_err(|_| {
                ctx.push(ValidationError::new(
                    "capnp_length",
                    format!("expected {N} array elements"),
                ))
            })
            .ok()
    }
}
macro_rules! map_read {
    ($map:path) => {
        impl<T: CapnpRead> CapnpRead for $map {
            fn read_value(
                value: capnp::dynamic_value::Reader<'_>,
                ctx: &mut Context,
            ) -> Option<Self> {
                let capnp::dynamic_value::Reader::List(list) = value else {
                    mismatch(ctx);
                    return None;
                };
                let mut values = Self::new();
                for index in 0..list.len() {
                    let entry = read_result(list.get(index), ctx)?;
                    let key = String::read_value(field_value(entry, "key", ctx)?, ctx)?;
                    let value = ctx.at(key.clone(), |ctx| {
                        T::read_value(field_value(entry, "value", ctx)?, ctx)
                    })?;
                    if values.insert(key.clone(), value).is_some() {
                        ctx.at(key, |ctx| {
                            ctx.push(ValidationError::new(
                                "capnp_duplicate_key",
                                "duplicate map key",
                            ))
                        });
                        return None;
                    }
                }
                Some(values)
            }
        }
    };
}
map_read!(std::collections::BTreeMap<String,T>);
map_read!(std::collections::HashMap<String,T>);

#[cfg(any(feature = "chrono", feature = "decimal"))]
macro_rules! text_read {
    ($ty:ty) => {
        impl CapnpRead for $ty {
            fn read_value(
                value: capnp::dynamic_value::Reader<'_>,
                ctx: &mut Context,
            ) -> Option<Self> {
                <Self as crate::JsonType>::decode(
                    crate::Value::String(text_value(value, ctx)?.to_owned()),
                    ctx,
                    true,
                )
            }
        }
    };
}
#[cfg(feature = "chrono")]
text_read!(chrono::NaiveDate);
#[cfg(feature = "chrono")]
text_read!(chrono::NaiveTime);
#[cfg(feature = "chrono")]
text_read!(chrono::NaiveDateTime);
#[cfg(feature = "chrono")]
text_read!(chrono::DateTime<chrono::Utc>);
#[cfg(feature = "chrono")]
text_read!(chrono::DateTime<chrono::FixedOffset>);
#[cfg(feature = "decimal")]
text_read!(rust_decimal::Decimal);
