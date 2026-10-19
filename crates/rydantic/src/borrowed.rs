use crate::{
    rules::Rules, AsyncCheck, Context, LengthType, SchemaContext, SchemaMode, SchemaReport, Stage,
    StringType, TypeKind, ValidationError, ValidationErrors, Value,
};
use serde::de::{self, Visitor};
use serde_json::{json, value::RawValue};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    hash::BuildHasher,
    marker::PhantomData,
};

/// A JSON subtree that can retain references into the original input buffer.
#[derive(Debug, Clone)]
pub enum Input<'de> {
    Borrowed(&'de RawValue),
    Owned(Value),
    Object(BTreeMap<String, Input<'de>>),
}
impl<'de> Input<'de> {
    pub fn into_value(self, ctx: &mut Context) -> Option<Value> {
        match self {
            Self::Object(object) => Self::from_object(object, ctx),
            Self::Owned(value) => Some(value),
            Self::Borrowed(raw) => serde_json::from_str(raw.get())
                .map_err(|e| ctx.push(json_error(e)))
                .ok(),
        }
    }
    pub fn object(self, ctx: &mut Context) -> Option<BTreeMap<String, Self>> {
        match self {
            Self::Object(object) => Some(object),
            Self::Borrowed(raw) => {
                serde_json::from_str::<BTreeMap<String, &'de RawValue>>(raw.get())
                    .map(|map| {
                        map.into_iter()
                            .map(|(k, v)| (k, Self::Borrowed(v)))
                            .collect()
                    })
                    .map_err(|_| ctx.push(type_error("object")))
                    .ok()
            }
            Self::Owned(Value::Object(map)) => {
                Some(map.into_iter().map(|(k, v)| (k, Self::Owned(v))).collect())
            }
            _ => {
                ctx.push(type_error("object"));
                None
            }
        }
    }
    pub fn array(self, ctx: &mut Context) -> Option<Vec<Self>> {
        match self {
            Self::Borrowed(raw) => serde_json::from_str::<Vec<&'de RawValue>>(raw.get())
                .map(|items| items.into_iter().map(Self::Borrowed).collect())
                .map_err(|_| ctx.push(type_error("array")))
                .ok(),
            Self::Owned(Value::Array(items)) => Some(items.into_iter().map(Self::Owned).collect()),
            _ => {
                ctx.push(type_error("array"));
                None
            }
        }
    }
    pub fn from_object(object: BTreeMap<String, Self>, ctx: &mut Context) -> Option<Value> {
        let mut result = serde_json::Map::new();
        for (key, value) in object {
            result.insert(key, value.into_value(ctx)?);
        }
        Some(Value::Object(result))
    }
    pub fn is_null(&self) -> bool {
        match self {
            Self::Borrowed(raw) => raw.get() == "null",
            Self::Owned(v) => v.is_null(),
            Self::Object(_) => false,
        }
    }
}
pub(crate) fn json_error(error: serde_json::Error) -> ValidationError {
    ValidationError::new("json_invalid", error.to_string())
        .with_stage(Stage::Parse)
        .with_param("line", error.line())
        .with_param("column", error.column())
}
fn type_error(expected: &str) -> ValidationError {
    ValidationError::new("type_error", format!("expected {expected}")).with_stage(Stage::Parse)
}

/// Lifetime-aware counterpart of `JsonType`. Borrowing models use this extension point.
pub trait BorrowType<'de>: Sized {
    fn decode(value: Input<'de>, ctx: &mut Context, coerce: bool) -> Option<Self>;
    fn encode(&self, ctx: &mut Context) -> Value;
    fn encode_input(&self, ctx: &mut Context) -> Value {
        self.encode(ctx)
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value;
    fn validate_into(&self, _ctx: &mut Context) {}
    fn prepare_default(self, ctx: &mut Context) -> Option<Self> {
        self.validate_into(ctx);
        Some(self)
    }
    fn normalize(self, rules: &Rules, ctx: &mut Context) -> Option<Self> {
        if !rules.normalizes() {
            return Some(self);
        }
        let mut value = self.encode_input(ctx);
        rules.prepare(&mut value);
        Self::decode(Input::Owned(value), ctx, false)
    }
    fn validate_rules(&self, rules: &Rules, ctx: &mut Context) {
        rules.validate(&self.encode(ctx), ctx);
    }
    fn missing() -> Option<Self> {
        None
    }
    fn allows_missing() -> bool {
        false
    }
    fn kind() -> TypeKind {
        TypeKind::Any
    }
    fn async_required(_seen: &mut HashSet<&'static str>) -> bool {
        false
    }
}

/// Parsing API for a model containing references or `Cow` values.
pub trait BorrowModel<'de>: BorrowType<'de> {
    fn parse_json(input: &'de str) -> Result<Self, ValidationErrors> {
        let raw = serde_json::from_str::<&'de RawValue>(input).map_err(json_error)?;
        Self::parse_raw(raw)
    }
    fn parse_slice(input: &'de [u8]) -> Result<Self, ValidationErrors> {
        let raw = serde_json::from_slice::<&'de RawValue>(input).map_err(json_error)?;
        Self::parse_raw(raw)
    }
    fn parse_raw(raw: &'de RawValue) -> Result<Self, ValidationErrors> {
        crate::asynchronous::sync_guard(Self::async_required(&mut HashSet::new()))?;
        let mut ctx = Context::new();
        let value = Self::decode(Input::Borrowed(raw), &mut ctx, false);
        ctx.finish(value)
    }
    fn validate(&self) -> Result<(), ValidationErrors> {
        crate::asynchronous::sync_guard(Self::async_required(&mut HashSet::new()))?;
        let mut ctx = Context::new();
        self.validate_into(&mut ctx);
        ctx.result()
    }
    fn dump(&self) -> Result<Value, ValidationErrors> {
        self.validate()?;
        let mut ctx = Context::new();
        let value = self.encode(&mut ctx);
        ctx.finish(Some(value))
    }
    fn dump_json(&self) -> Result<String, ValidationErrors> {
        Ok(self.dump()?.to_string())
    }
    fn json_schema(mode: SchemaMode) -> Value {
        let mut ctx = SchemaContext::new(mode);
        let root = Self::schema(&mut ctx, false);
        ctx.finish(root)
    }
    fn schema_report(mode: SchemaMode) -> SchemaReport {
        SchemaReport::new(Self::json_schema(mode))
    }
    fn parse_validated_json(
        input: &'de str,
    ) -> Result<BorrowValidated<'de, Self>, ValidationErrors> {
        Self::parse_json(input).map(|value| BorrowValidated(value, PhantomData))
    }
    fn parse_json_async(
        input: &'de str,
    ) -> impl std::future::Future<Output = Result<Self, ValidationErrors>> + Send
    where
        Self: AsyncCheck + Send,
    {
        async move {
            let raw = serde_json::from_str::<&'de RawValue>(input).map_err(json_error)?;
            let mut ctx = Context::new();
            let value = Self::decode(Input::Borrowed(raw), &mut ctx, false);
            let value = ctx.finish(value)?;
            let mut ctx = Context::new();
            value.check_async(&mut ctx).await;
            ctx.result()?;
            Ok(value)
        }
    }
    fn validate_async(
        &self,
    ) -> impl std::future::Future<Output = Result<(), ValidationErrors>> + Send
    where
        Self: AsyncCheck,
    {
        async move {
            let mut ctx = Context::new();
            self.validate_into(&mut ctx);
            ctx.result()?;
            let mut ctx = Context::new();
            self.check_async(&mut ctx).await;
            ctx.result()
        }
    }
    fn dump_async(
        &self,
    ) -> impl std::future::Future<Output = Result<Value, ValidationErrors>> + Send
    where
        Self: AsyncCheck,
    {
        async move {
            self.validate_async().await?;
            let mut ctx = Context::new();
            let value = self.encode(&mut ctx);
            ctx.finish(Some(value))
        }
    }
    fn dump_json_async(
        &self,
    ) -> impl std::future::Future<Output = Result<String, ValidationErrors>> + Send
    where
        Self: AsyncCheck,
    {
        async move { Ok(self.dump_async().await?.to_string()) }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorrowValidated<'de, T: BorrowModel<'de>>(T, PhantomData<&'de str>);
impl<'de, T: BorrowModel<'de>> BorrowValidated<'de, T> {
    pub fn new(value: T) -> Result<Self, ValidationErrors> {
        value.validate()?;
        Ok(Self(value, PhantomData))
    }
    pub fn get(&self) -> &T {
        &self.0
    }
    pub fn into_inner(self) -> T {
        self.0
    }
}
impl<'de, T: BorrowModel<'de>> std::ops::Deref for BorrowValidated<'de, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<'de, T: BorrowModel<'de>> serde::Serialize for BorrowValidated<'de, T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0
            .dump()
            .map_err(serde::ser::Error::custom)?
            .serialize(s)
    }
}

impl<'de, T: BorrowModel<'de>> serde::Deserialize<'de> for BorrowValidated<'de, T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = <&'de RawValue as serde::Deserialize>::deserialize(deserializer)?;
        T::parse_raw(raw)
            .map(|value| Self(value, PhantomData))
            .map_err(serde::de::Error::custom)
    }
}

macro_rules! owned {
    ($($ty:ty),* $(,)?) => {$(impl<'de> BorrowType<'de> for $ty {
        fn decode(value:Input<'de>,ctx:&mut Context,coerce:bool)->Option<Self>{<Self as crate::JsonType>::decode(value.into_value(ctx)?,ctx,coerce)}
        fn encode(&self,ctx:&mut Context)->Value{<Self as crate::JsonType>::encode(self,ctx)}
        fn schema(ctx:&mut SchemaContext,coerce:bool)->Value{<Self as crate::JsonType>::schema(ctx,coerce)}
        fn validate_into(&self,ctx:&mut Context){<Self as crate::JsonType>::validate_into(self,ctx)}
        fn validate_rules(&self,rules:&Rules,ctx:&mut Context){<Self as crate::JsonType>::validate_rules(self,rules,ctx)}
        fn kind()->TypeKind{<Self as crate::JsonType>::kind()}
    })*};
}
owned!(
    String,
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
owned!(
    chrono::NaiveDate,
    chrono::NaiveTime,
    chrono::NaiveDateTime,
    chrono::DateTime<chrono::Utc>,
    chrono::DateTime<chrono::FixedOffset>
);
#[cfg(feature = "decimal")]
owned!(rust_decimal::Decimal);

impl<'de: 'a, 'a> BorrowType<'de> for &'a str {
    fn decode(value: Input<'de>, ctx: &mut Context, _: bool) -> Option<Self> {
        match value {
            Input::Borrowed(raw) => serde_json::from_str::<&'de str>(raw.get())
                .map_err(|_| {
                    ctx.push(
                        ValidationError::new(
                            "borrowed_string",
                            "expected an unescaped JSON string; use Cow<str> for escaped strings",
                        )
                        .with_stage(Stage::Parse),
                    )
                })
                .ok(),
            _ => {
                ctx.push(
                    ValidationError::new(
                        "borrowed_string",
                        "a borrowed string requires input backed by the original buffer",
                    )
                    .with_stage(Stage::Parse),
                );
                None
            }
        }
    }
    fn encode(&self, _: &mut Context) -> Value {
        Value::String((*self).to_owned())
    }
    fn schema(ctx: &mut SchemaContext, _: bool) -> Value {
        if ctx.mode == SchemaMode::Input {
            json!({"type":"string","x-rydantic-local-rules":["borrowed_unescaped_string"]})
        } else {
            json!({"type":"string"})
        }
    }
    fn normalize(self, rules: &Rules, ctx: &mut Context) -> Option<Self> {
        let text = if rules.trim { self.trim() } else { self };
        if (rules.lowercase && text.to_lowercase() != text)
            || (rules.uppercase && text.to_uppercase() != text)
        {
            ctx.push(ValidationError::new(
                "borrowed_transform",
                "case conversion requires Cow<str> or String",
            ));
            None
        } else {
            Some(text)
        }
    }
    fn kind() -> TypeKind {
        TypeKind::String
    }
}
impl StringType for &str {}
impl LengthType for &str {}
struct CowVisitor;
impl<'de> Visitor<'de> for CowVisitor {
    type Value = Cow<'de, str>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON string")
    }
    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(Cow::Borrowed(v))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(Cow::Owned(v.to_owned()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(Cow::Owned(v))
    }
}
impl<'de: 'a, 'a> BorrowType<'de> for Cow<'a, str> {
    fn decode(value: Input<'de>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        match value {
            Input::Borrowed(raw) if raw.get().starts_with('"') => {
                let mut deserializer = serde_json::Deserializer::from_str(raw.get());
                serde::Deserializer::deserialize_str(&mut deserializer, CowVisitor)
                    .map_err(|e| ctx.push(json_error(e)))
                    .ok()
            }
            value => <String as BorrowType>::decode(value, ctx, coerce).map(Cow::Owned),
        }
    }
    fn encode(&self, _: &mut Context) -> Value {
        Value::String(self.to_string())
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        <String as crate::JsonType>::schema(ctx, coerce)
    }
    fn normalize(self, rules: &Rules, _: &mut Context) -> Option<Self> {
        let value = match self {
            Cow::Borrowed(v) => Cow::Borrowed(if rules.trim { v.trim() } else { v }),
            Cow::Owned(v) => Cow::Owned(if rules.trim { v.trim().to_owned() } else { v }),
        };
        if rules.lowercase {
            let v = value.to_lowercase();
            if v != value {
                return Some(Cow::Owned(v));
            }
        }
        if rules.uppercase {
            let v = value.to_uppercase();
            if v != value {
                return Some(Cow::Owned(v));
            }
        }
        Some(value)
    }
    fn kind() -> TypeKind {
        TypeKind::String
    }
}
impl StringType for Cow<'_, str> {}
impl LengthType for Cow<'_, str> {}

impl<'de, T: BorrowType<'de>> BorrowType<'de> for Option<T> {
    fn prepare_default(self, ctx: &mut Context) -> Option<Self> {
        match self {
            Some(value) => value.prepare_default(ctx).map(Some),
            None => Some(None),
        }
    }
    fn decode(value: Input<'de>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        if value.is_null() {
            Some(None)
        } else {
            T::decode(value, ctx, coerce).map(Some)
        }
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        self.as_ref().map_or(Value::Null, |v| v.encode(ctx))
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        self.as_ref().map_or(Value::Null, |v| v.encode_input(ctx))
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        json!({"anyOf":[T::schema(ctx,coerce),{"type":"null"}]})
    }
    fn validate_into(&self, ctx: &mut Context) {
        if let Some(v) = self {
            v.validate_into(ctx)
        }
    }
    fn normalize(self, rules: &Rules, ctx: &mut Context) -> Option<Self> {
        match self {
            Some(v) => v.normalize(rules, ctx).map(Some),
            None => Some(None),
        }
    }
    fn validate_rules(&self, rules: &Rules, ctx: &mut Context) {
        if let Some(v) = self {
            v.validate_rules(rules, ctx)
        }
    }
    fn missing() -> Option<Self> {
        Some(None)
    }
    fn allows_missing() -> bool {
        true
    }
    fn kind() -> TypeKind {
        T::kind()
    }
    fn async_required(seen: &mut HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
}
impl<'de, T: BorrowType<'de>> BorrowType<'de> for Box<T> {
    fn prepare_default(self, ctx: &mut Context) -> Option<Self> {
        (*self).prepare_default(ctx).map(Box::new)
    }
    fn decode(value: Input<'de>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        T::decode(value, ctx, coerce).map(Box::new)
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        (**self).encode(ctx)
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        (**self).encode_input(ctx)
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        T::schema(ctx, coerce)
    }
    fn validate_into(&self, ctx: &mut Context) {
        (**self).validate_into(ctx)
    }
    fn normalize(self, rules: &Rules, ctx: &mut Context) -> Option<Self> {
        (*self).normalize(rules, ctx).map(Box::new)
    }
    fn validate_rules(&self, rules: &Rules, ctx: &mut Context) {
        (**self).validate_rules(rules, ctx)
    }
    fn missing() -> Option<Self> {
        T::missing().map(Box::new)
    }
    fn allows_missing() -> bool {
        T::allows_missing()
    }
    fn kind() -> TypeKind {
        T::kind()
    }
    fn async_required(seen: &mut HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
}
impl<'de, T: BorrowType<'de>> BorrowType<'de> for Vec<T> {
    fn prepare_default(self, ctx: &mut Context) -> Option<Self> {
        let mut result = Vec::with_capacity(self.len());
        let mut complete = true;
        for (i, value) in self.into_iter().enumerate() {
            match ctx.at(i, |ctx| value.prepare_default(ctx)) {
                Some(v) => result.push(v),
                None => complete = false,
            }
        }
        complete.then_some(result)
    }
    fn decode(value: Input<'de>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let items = value.array(ctx)?;
        let mut result = Vec::with_capacity(items.len());
        let mut complete = true;
        for (i, value) in items.into_iter().enumerate() {
            match ctx.at(i, |ctx| T::decode(value, ctx, coerce)) {
                Some(v) => result.push(v),
                None => complete = false,
            }
        }
        complete.then_some(result)
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        Value::Array(
            self.iter()
                .enumerate()
                .map(|(i, v)| ctx.at(i, |ctx| v.encode(ctx)))
                .collect(),
        )
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        Value::Array(
            self.iter()
                .enumerate()
                .map(|(i, v)| ctx.at(i, |ctx| v.encode_input(ctx)))
                .collect(),
        )
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        json!({"type":"array","items":T::schema(ctx,coerce)})
    }
    fn validate_into(&self, ctx: &mut Context) {
        for (i, v) in self.iter().enumerate() {
            ctx.at(i, |ctx| v.validate_into(ctx));
        }
    }
    fn kind() -> TypeKind {
        TypeKind::Array
    }
    fn async_required(seen: &mut HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
}
impl<'de, T: BorrowType<'de>, const N: usize> BorrowType<'de> for [T; N] {
    fn prepare_default(self, ctx: &mut Context) -> Option<Self> {
        <Vec<T> as BorrowType>::prepare_default(Vec::from(self), ctx)?
            .try_into()
            .ok()
    }
    fn decode(value: Input<'de>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let items = <Vec<T> as BorrowType>::decode(value, ctx, coerce)?;
        let len = items.len();
        items
            .try_into()
            .map_err(|_| {
                ctx.push(
                    ValidationError::new(
                        "array_length",
                        format!("array must contain exactly {N} items"),
                    )
                    .with_param("actual", len),
                )
            })
            .ok()
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        Value::Array(
            self.iter()
                .enumerate()
                .map(|(i, v)| ctx.at(i, |ctx| v.encode(ctx)))
                .collect(),
        )
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        Value::Array(
            self.iter()
                .enumerate()
                .map(|(i, v)| ctx.at(i, |ctx| v.encode_input(ctx)))
                .collect(),
        )
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        json!({"type":"array","items":T::schema(ctx,coerce),"minItems":N,"maxItems":N})
    }
    fn validate_into(&self, ctx: &mut Context) {
        for (i, v) in self.iter().enumerate() {
            ctx.at(i, |ctx| v.validate_into(ctx));
        }
    }
    fn kind() -> TypeKind {
        TypeKind::Array
    }
    fn async_required(seen: &mut HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
}
impl<'de, T: BorrowType<'de>> BorrowType<'de> for BTreeMap<String, T> {
    fn prepare_default(self, ctx: &mut Context) -> Option<Self> {
        let mut result = Self::new();
        let mut complete = true;
        for (k, v) in self {
            match ctx.at(k.clone(), |ctx| v.prepare_default(ctx)) {
                Some(v) => {
                    result.insert(k, v);
                }
                None => complete = false,
            }
        }
        complete.then_some(result)
    }
    fn decode(value: Input<'de>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let mut result = Self::new();
        let mut complete = true;
        for (k, v) in value.object(ctx)? {
            match ctx.at(k.clone(), |ctx| T::decode(v, ctx, coerce)) {
                Some(v) => {
                    result.insert(k, v);
                }
                None => complete = false,
            }
        }
        complete.then_some(result)
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        Value::Object(
            self.iter()
                .map(|(k, v)| (k.clone(), ctx.at(k.clone(), |ctx| v.encode(ctx))))
                .collect(),
        )
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        Value::Object(
            self.iter()
                .map(|(k, v)| (k.clone(), ctx.at(k.clone(), |ctx| v.encode_input(ctx))))
                .collect(),
        )
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        json!({"type":"object","additionalProperties":T::schema(ctx,coerce)})
    }
    fn validate_into(&self, ctx: &mut Context) {
        for (k, v) in self {
            ctx.at(k.clone(), |ctx| v.validate_into(ctx));
        }
    }
    fn kind() -> TypeKind {
        TypeKind::Object
    }
    fn async_required(seen: &mut HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
}
impl<'de, T: BorrowType<'de>, S: BuildHasher + Default> BorrowType<'de> for HashMap<String, T, S> {
    fn prepare_default(self, ctx: &mut Context) -> Option<Self> {
        <BTreeMap<String, T> as BorrowType>::prepare_default(self.into_iter().collect(), ctx)
            .map(|v| v.into_iter().collect())
    }
    fn decode(value: Input<'de>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        <BTreeMap<String, T> as BorrowType>::decode(value, ctx, coerce)
            .map(|v| v.into_iter().collect())
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        Value::Object(
            self.iter()
                .map(|(k, v)| (k.clone(), ctx.at(k.clone(), |ctx| v.encode(ctx))))
                .collect(),
        )
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        Value::Object(
            self.iter()
                .map(|(k, v)| (k.clone(), ctx.at(k.clone(), |ctx| v.encode_input(ctx))))
                .collect(),
        )
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        <BTreeMap<String, T> as BorrowType>::schema(ctx, coerce)
    }
    fn validate_into(&self, ctx: &mut Context) {
        for (k, v) in self {
            ctx.at(k.clone(), |ctx| v.validate_into(ctx));
        }
    }
    fn kind() -> TypeKind {
        TypeKind::Object
    }
    fn async_required(seen: &mut HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
}
