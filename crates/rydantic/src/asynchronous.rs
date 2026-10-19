use crate::{Context, Model, ValidationError, ValidationErrors, Value};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    future::Future,
    hash::BuildHasher,
    pin::Pin,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// Asynchronous checks, executed after synchronous parsing and validation succeed.
/// Implementations must leave the supplied context usable if the future is cancelled.
pub trait AsyncCheck: Sync {
    fn check_async<'a>(&'a self, _ctx: &'a mut Context) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}
pub(crate) fn sync_guard(required: bool) -> Result<(), ValidationErrors> {
    if required {
        Err(ValidationError::new(
            "async_validation_required",
            "this model requires an asynchronous validation entry point",
        )
        .into())
    } else {
        Ok(())
    }
}
macro_rules! ready {($($ty:ty),*$(,)?)=>{$(impl AsyncCheck for $ty {})*};}
ready!(
    String,
    str,
    &str,
    Cow<'_, str>,
    bool,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    f32,
    f64,
    Value,
    ()
);
#[cfg(feature = "chrono")]
ready!(
    chrono::NaiveDate,
    chrono::NaiveTime,
    chrono::NaiveDateTime,
    chrono::DateTime<chrono::Utc>,
    chrono::DateTime<chrono::FixedOffset>
);
#[cfg(feature = "decimal")]
ready!(rust_decimal::Decimal);
impl<T: AsyncCheck> AsyncCheck for Option<T> {
    fn check_async<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if let Some(value) = self {
                value.check_async(ctx).await;
            }
        })
    }
}
impl<T: AsyncCheck> AsyncCheck for Box<T> {
    fn check_async<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, ()> {
        (**self).check_async(ctx)
    }
}
impl<T: AsyncCheck> AsyncCheck for Vec<T> {
    fn check_async<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            for (i, value) in self.iter().enumerate() {
                let mut child = ctx.child(Some(i.into()));
                value.check_async(&mut child).await;
                ctx.absorb(child);
            }
        })
    }
}
impl<T: AsyncCheck, const N: usize> AsyncCheck for [T; N] {
    fn check_async<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            for (i, value) in self.iter().enumerate() {
                let mut child = ctx.child(Some(i.into()));
                value.check_async(&mut child).await;
                ctx.absorb(child);
            }
        })
    }
}
impl<T: AsyncCheck> AsyncCheck for BTreeMap<String, T> {
    fn check_async<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            for (key, value) in self {
                let mut child = ctx.child(Some(key.clone().into()));
                value.check_async(&mut child).await;
                ctx.absorb(child);
            }
        })
    }
}
impl<T: AsyncCheck, S: BuildHasher + Sync> AsyncCheck for HashMap<String, T, S> {
    fn check_async<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            for (key, value) in self {
                let mut child = ctx.child(Some(key.clone().into()));
                value.check_async(&mut child).await;
                ctx.absorb(child);
            }
        })
    }
}
/// Immutable result of successful synchronous and asynchronous validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsyncValidated<T: Model + AsyncCheck>(T);
impl<T: Model + AsyncCheck> AsyncValidated<T> {
    pub async fn new(value: T) -> Result<Self, ValidationErrors> {
        value.validate_async().await?;
        Ok(Self(value))
    }
    pub(crate) fn checked(value: T) -> Self {
        Self(value)
    }
    pub fn get(&self) -> &T {
        &self.0
    }
    pub fn into_inner(self) -> T {
        self.0
    }
}
impl<T: Model + AsyncCheck> std::ops::Deref for AsyncValidated<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T: Model + AsyncCheck> serde::Serialize for AsyncValidated<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut ctx = Context::new();
        self.0.validate_into(&mut ctx);
        let value = self.0.encode(&mut ctx);
        ctx.finish(Some(value))
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}
