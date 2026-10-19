use crate::{
    Context, Input, JsonType, LengthType, NumericType, SchemaContext, SchemaMode, Stage,
    StringType, TypeKind, ValidationError, Value,
};
use serde_json::{json, Number};
use std::{
    collections::{BTreeMap, HashMap},
    hash::BuildHasher,
};

fn mismatch(ctx: &mut Context, expected: &str, value: &Value) {
    let actual = match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    ctx.push(
        ValidationError::new(
            "type_error",
            format!("expected {expected}, received {actual}"),
        )
        .with_stage(Stage::Parse)
        .with_param("expected", expected)
        .with_param("actual", actual),
    );
}

macro_rules! integer {
    ($($ty:ty),* $(,)?) => {$(
        impl JsonType for $ty {
            fn decode_input(input:Input<'_>,ctx:&mut Context,coerce:bool)->Option<Self>{Self::decode(crate::direct::scalar(input,ctx,"integer")?,ctx,coerce)}
            fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
                if coerce {
                    if let Value::String(text) = &value {
                        return match text.parse::<Self>() {
                            Ok(number) => Some(number),
                            Err(_) => { ctx.push(ValidationError::new("int_parsing", concat!("string must contain a valid ", stringify!($ty))).with_stage(Stage::Parse)); None }
                        };
                    }
                }
                let Value::Number(number) = &value else { mismatch(ctx, "integer", &value); return None; };
                let parsed = crate::rules::integer_text(number).and_then(|text| text.parse::<Self>().ok());
                if parsed.is_none() {
                    ctx.push(ValidationError::new(if number.is_f64() { "int_type" } else { "int_out_of_range" }, concat!("value must be an integer representable as ", stringify!($ty))).with_stage(Stage::Parse));
                }
                parsed
            }
            fn encode(&self, _ctx: &mut Context) -> Value { json!(*self) }
            fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
                let base = json!({"type":"integer", "minimum": <$ty>::MIN, "maximum": <$ty>::MAX});
                if coerce && ctx.mode == SchemaMode::Input { json!({"anyOf":[base, {"type":"string"}]}) } else { base }
            }
            fn kind() -> TypeKind { TypeKind::Number }
        }
        impl NumericType for $ty {}
    )*};
}
integer!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

macro_rules! float {
    ($($ty:ty),*) => {$(
        impl JsonType for $ty {
            fn decode_input(input:Input<'_>,ctx:&mut Context,coerce:bool)->Option<Self>{Self::decode(crate::direct::scalar(input,ctx,"number")?,ctx,coerce)}
            fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
                let parsed = match &value {
                    Value::Number(number) => number.as_f64().and_then(|number| {
                        (number >= -(<$ty>::MAX as f64) && number <= <$ty>::MAX as f64).then_some(number as Self)
                    }),
                    Value::String(text) if coerce => text.parse::<Self>().ok(),
                    _ => { mismatch(ctx, "number", &value); return None; }
                };
                match parsed {
                    Some(number) if number.is_finite() => Some(number),
                    _ => { ctx.push(ValidationError::new("float_parsing", concat!("value must be a finite ", stringify!($ty))).with_stage(Stage::Parse)); None }
                }
            }
            fn encode(&self, ctx: &mut Context) -> Value {
                match Number::from_f64(*self as f64) {
                    Some(number) => Value::Number(number),
                    None => { ctx.push(ValidationError::new("finite_number", "NaN and infinity cannot be represented in JSON").with_stage(Stage::Serialize)); Value::Null }
                }
            }
            fn validate_into(&self, ctx: &mut Context) {
                if !self.is_finite() { ctx.push(ValidationError::new("finite_number", "value must be finite")); }
            }
            fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
                let base = json!({"type":"number", "minimum": -(<$ty>::MAX as f64), "maximum": <$ty>::MAX as f64});
                if coerce && ctx.mode == SchemaMode::Input { json!({"anyOf":[base, {"type":"string"}]}) } else { base }
            }
            fn kind() -> TypeKind { TypeKind::Number }
        }
        impl NumericType for $ty {}
    )*};
}
float!(f32, f64);

impl JsonType for String {
    fn decode_input(input: Input<'_>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        Self::decode(crate::direct::scalar(input, ctx, "string")?, ctx, coerce)
    }
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        match value {
            Value::String(text) => Some(text),
            Value::Number(number) if coerce => Some(number.to_string()),
            Value::Bool(boolean) if coerce => Some(boolean.to_string()),
            value => {
                mismatch(ctx, "string", &value);
                None
            }
        }
    }
    fn encode(&self, _: &mut Context) -> Value {
        Value::String(self.clone())
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        if coerce && ctx.mode == SchemaMode::Input {
            json!({"type":["string","number","boolean"]})
        } else {
            json!({"type":"string"})
        }
    }
    fn kind() -> TypeKind {
        TypeKind::String
    }
}
impl StringType for String {}
impl LengthType for String {}

impl JsonType for bool {
    fn decode_input(input: Input<'_>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        Self::decode(crate::direct::scalar(input, ctx, "boolean")?, ctx, coerce)
    }
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        match value {
            Value::Bool(boolean) => Some(boolean),
            Value::String(text) if coerce && (text == "true" || text == "false") => {
                Some(text == "true")
            }
            value => {
                mismatch(ctx, "boolean", &value);
                None
            }
        }
    }
    fn encode(&self, _: &mut Context) -> Value {
        json!(*self)
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        if coerce && ctx.mode == SchemaMode::Input {
            json!({"anyOf":[{"type":"boolean"},{"enum":["true","false"]}]})
        } else {
            json!({"type":"boolean"})
        }
    }
    fn kind() -> TypeKind {
        TypeKind::Boolean
    }
}

impl<T: JsonType> JsonType for Option<T> {
    fn decode_input(input: Input<'_>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        if input.is_null() {
            Some(None)
        } else {
            T::decode_input(input, ctx, coerce).map(Some)
        }
    }
    fn normalize(self, rules: &crate::rules::Rules, ctx: &mut Context) -> Option<Self> {
        match self {
            Some(value) => value.normalize(rules, ctx).map(Some),
            None => Some(None),
        }
    }
    fn validate_rules(&self, rules: &crate::rules::Rules, ctx: &mut Context) {
        if let Some(value) = self {
            value.validate_rules(rules, ctx)
        }
    }

    fn async_required(seen: &mut std::collections::HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        if value.is_null() {
            Some(None)
        } else {
            T::decode(value, ctx, coerce).map(Some)
        }
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        self.as_ref().map_or(Value::Null, |value| value.encode(ctx))
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        self.as_ref()
            .map_or(Value::Null, |value| value.encode_input(ctx))
    }
    fn validate_into(&self, ctx: &mut Context) {
        if let Some(value) = self {
            value.validate_into(ctx);
        }
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        json!({"anyOf":[T::schema(ctx, coerce), {"type":"null"}]})
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
}
impl<T: StringType> StringType for Option<T> {}
impl<T: NumericType> NumericType for Option<T> {}
impl<T: LengthType> LengthType for Option<T> {}

impl<T: JsonType> JsonType for Box<T> {
    fn decode_input(input: Input<'_>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        T::decode_input(input, ctx, coerce).map(Box::new)
    }
    fn normalize(self, rules: &crate::rules::Rules, ctx: &mut Context) -> Option<Self> {
        (*self).normalize(rules, ctx).map(Box::new)
    }
    fn validate_rules(&self, rules: &crate::rules::Rules, ctx: &mut Context) {
        (**self).validate_rules(rules, ctx)
    }

    fn async_required(seen: &mut std::collections::HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        T::decode(value, ctx, coerce).map(Box::new)
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        (**self).encode(ctx)
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        (**self).encode_input(ctx)
    }
    fn validate_into(&self, ctx: &mut Context) {
        (**self).validate_into(ctx);
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        T::schema(ctx, coerce)
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
}
impl<T: StringType> StringType for Box<T> {}
impl<T: NumericType> NumericType for Box<T> {}
impl<T: LengthType> LengthType for Box<T> {}

impl<T: JsonType> JsonType for Vec<T> {
    fn decode_input(input: Input<'_>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let items = input.array(ctx)?;
        let mut values = Vec::with_capacity(items.len());
        let mut complete = true;
        for (index, item) in items.into_iter().enumerate() {
            match ctx.at(index, |ctx| T::decode_input(item, ctx, coerce)) {
                Some(value) => values.push(value),
                None => complete = false,
            }
        }
        complete.then_some(values)
    }
    fn async_required(seen: &mut std::collections::HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let Value::Array(items) = value else {
            mismatch(ctx, "array", &value);
            return None;
        };
        let mut result = Vec::with_capacity(items.len());
        let mut complete = true;
        for (index, item) in items.into_iter().enumerate() {
            match ctx.at(index, |ctx| T::decode(item, ctx, coerce)) {
                Some(item) => result.push(item),
                None => complete = false,
            }
        }
        complete.then_some(result)
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        Value::Array(
            self.iter()
                .enumerate()
                .map(|(index, item)| ctx.at(index, |ctx| item.encode(ctx)))
                .collect(),
        )
    }
    fn validate_into(&self, ctx: &mut Context) {
        for (index, item) in self.iter().enumerate() {
            ctx.at(index, |ctx| item.validate_into(ctx));
        }
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        Value::Array(
            self.iter()
                .enumerate()
                .map(|(i, item)| ctx.at(i, |ctx| item.encode_input(ctx)))
                .collect(),
        )
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        json!({"type":"array", "items":T::schema(ctx, coerce)})
    }
    fn kind() -> TypeKind {
        TypeKind::Array
    }
}
impl<T> LengthType for Vec<T> {}

impl<T: JsonType, const N: usize> JsonType for [T; N] {
    fn decode_input(input: Input<'_>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let values = Vec::<T>::decode_input(input, ctx, coerce)?;
        let len = values.len();
        values
            .try_into()
            .map_err(|_| {
                ctx.push(
                    ValidationError::new(
                        "array_length",
                        format!("array must contain exactly {N} items"),
                    )
                    .with_param("expected", N)
                    .with_param("actual", len),
                )
            })
            .ok()
    }
    fn async_required(seen: &mut std::collections::HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let values = Vec::<T>::decode(value, ctx, coerce)?;
        let length = values.len();
        match values.try_into() {
            Ok(array) => Some(array),
            Err(_) => {
                ctx.push(
                    ValidationError::new(
                        "array_length",
                        format!("array must contain exactly {N} items"),
                    )
                    .with_param("expected", N)
                    .with_param("actual", length),
                );
                None
            }
        }
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        Value::Array(
            self.iter()
                .enumerate()
                .map(|(i, item)| ctx.at(i, |ctx| item.encode(ctx)))
                .collect(),
        )
    }
    fn validate_into(&self, ctx: &mut Context) {
        for (i, item) in self.iter().enumerate() {
            ctx.at(i, |ctx| item.validate_into(ctx));
        }
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        Value::Array(
            self.iter()
                .enumerate()
                .map(|(i, item)| ctx.at(i, |ctx| item.encode_input(ctx)))
                .collect(),
        )
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        json!({"type":"array", "items":T::schema(ctx, coerce), "minItems":N, "maxItems":N})
    }
    fn kind() -> TypeKind {
        TypeKind::Array
    }
}
impl<T, const N: usize> LengthType for [T; N] {}

impl<T: JsonType> JsonType for BTreeMap<String, T> {
    fn decode_input(input: Input<'_>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let mut result = Self::new();
        let mut complete = true;
        for (key, item) in input.object(ctx)? {
            match ctx.at(key.clone(), |ctx| T::decode_input(item, ctx, coerce)) {
                Some(value) => {
                    result.insert(key, value);
                }
                None => complete = false,
            }
        }
        complete.then_some(result)
    }
    fn async_required(seen: &mut std::collections::HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let Value::Object(items) = value else {
            mismatch(ctx, "object", &value);
            return None;
        };
        let mut result = Self::new();
        let mut complete = true;
        for (key, value) in items {
            match ctx.at(key.clone(), |ctx| T::decode(value, ctx, coerce)) {
                Some(value) => {
                    result.insert(key, value);
                }
                None => complete = false,
            }
        }
        complete.then_some(result)
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        Value::Object(
            self.iter()
                .map(|(key, value)| (key.clone(), ctx.at(key.clone(), |ctx| value.encode(ctx))))
                .collect(),
        )
    }
    fn validate_into(&self, ctx: &mut Context) {
        for (key, value) in self {
            ctx.at(key.clone(), |ctx| value.validate_into(ctx));
        }
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        Value::Object(
            self.iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        ctx.at(key.clone(), |ctx| value.encode_input(ctx)),
                    )
                })
                .collect(),
        )
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        json!({"type":"object", "additionalProperties":T::schema(ctx, coerce)})
    }
    fn kind() -> TypeKind {
        TypeKind::Object
    }
}
impl<T> LengthType for BTreeMap<String, T> {}

impl<T: JsonType, S: BuildHasher + Default> JsonType for HashMap<String, T, S> {
    fn decode_input(input: Input<'_>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        BTreeMap::<String, T>::decode_input(input, ctx, coerce)
            .map(|values| values.into_iter().collect())
    }
    fn async_required(seen: &mut std::collections::HashSet<&'static str>) -> bool {
        T::async_required(seen)
    }
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        BTreeMap::<String, T>::decode(value, ctx, coerce).map(|values| values.into_iter().collect())
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        Value::Object(
            self.iter()
                .map(|(key, value)| (key.clone(), ctx.at(key.clone(), |ctx| value.encode(ctx))))
                .collect(),
        )
    }
    fn validate_into(&self, ctx: &mut Context) {
        for (key, value) in self {
            ctx.at(key.clone(), |ctx| value.validate_into(ctx));
        }
    }
    fn encode_input(&self, ctx: &mut Context) -> Value {
        Value::Object(
            self.iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        ctx.at(key.clone(), |ctx| value.encode_input(ctx)),
                    )
                })
                .collect(),
        )
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        BTreeMap::<String, T>::schema(ctx, coerce)
    }
    fn kind() -> TypeKind {
        TypeKind::Object
    }
}
impl<T, S: BuildHasher> LengthType for HashMap<String, T, S> {}

impl JsonType for Value {
    fn decode(value: Value, _: &mut Context, _: bool) -> Option<Self> {
        Some(value)
    }
    fn encode(&self, _: &mut Context) -> Value {
        self.clone()
    }
    fn schema(_: &mut SchemaContext, _: bool) -> Value {
        json!({})
    }
}

impl JsonType for () {
    fn decode_input(input: Input<'_>, ctx: &mut Context, coerce: bool) -> Option<Self> {
        Self::decode(crate::direct::scalar(input, ctx, "null")?, ctx, coerce)
    }
    fn decode(value: Value, ctx: &mut Context, _: bool) -> Option<Self> {
        if value.is_null() {
            Some(())
        } else {
            mismatch(ctx, "null", &value);
            None
        }
    }
    fn encode(&self, _: &mut Context) -> Value {
        Value::Null
    }
    fn schema(_: &mut SchemaContext, _: bool) -> Value {
        json!({"type":"null"})
    }
}

impl JsonType for Box<crate::RawValue> {
    fn decode(value: Value, ctx: &mut Context, _: bool) -> Option<Self> {
        serde_json::value::to_raw_value(&value)
            .map_err(|error| ctx.push(ValidationError::new("raw_json", error.to_string())))
            .ok()
    }
    fn decode_input(input: Input<'_>, ctx: &mut Context, _: bool) -> Option<Self> {
        match input {
            Input::Borrowed(raw) => Some(raw.to_owned()),
            value => Self::decode(value.into_value(ctx)?, ctx, false),
        }
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        serde_json::from_str(self.get()).unwrap_or_else(|error| {
            ctx.push(ValidationError::new("raw_json", error.to_string()));
            Value::Null
        })
    }
    fn schema(_: &mut SchemaContext, _: bool) -> Value {
        json!({})
    }
}
