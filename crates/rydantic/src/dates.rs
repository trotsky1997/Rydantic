use crate::{Context, JsonType, SchemaContext, Stage, TypeKind, ValidationError, Value};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde_json::json;
fn invalid(ctx: &mut Context) {
    ctx.push(
        ValidationError::new("date_parsing", "expected a valid ISO 8601 date/time string")
            .with_stage(Stage::Parse),
    );
}
fn parse_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return None;
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}
fn parse_time(text: &str) -> Option<NaiveTime> {
    let (base, fraction) = text
        .split_once('.')
        .map_or((text, None), |(b, f)| (b, Some(f)));
    let bytes = base.as_bytes();
    if bytes.len() != 8
        || bytes[2] != b':'
        || bytes[5] != b':'
        || bytes
            .iter()
            .enumerate()
            .any(|(i, b)| i != 2 && i != 5 && !b.is_ascii_digit())
    {
        return None;
    }
    if fraction
        .is_some_and(|f| f.is_empty() || f.len() > 9 || !f.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    NaiveTime::parse_from_str(text, "%H:%M:%S%.f").ok()
}
fn parse_naive_datetime(text: &str) -> Option<NaiveDateTime> {
    let (date, time) = text.split_once('T')?;
    Some(NaiveDateTime::new(parse_date(date)?, parse_time(time)?))
}
macro_rules! date_type {
    ($ty:ty,$parse:expr,$format:expr,$schema:expr) => {
        impl JsonType for $ty {
            fn decode_input(
                input: crate::Input<'_>,
                ctx: &mut Context,
                coerce: bool,
            ) -> Option<Self> {
                Self::decode(crate::direct::scalar(input, ctx, "date/time")?, ctx, coerce)
            }
            fn decode(value: Value, ctx: &mut Context, _: bool) -> Option<Self> {
                let Value::String(text) = value else {
                    invalid(ctx);
                    return None;
                };
                let value = ($parse)(&text);
                if value.is_none() {
                    invalid(ctx);
                }
                value
            }
            fn encode(&self, ctx: &mut Context) -> Value {
                let start = ctx.error_count();
                self.validate_into(ctx);
                if ctx.error_count() != start {
                    return Value::Null;
                }
                Value::String(($format)(self))
            }
            fn validate_into(&self, ctx: &mut Context) {
                if ($parse)(&($format)(self)).is_none() {
                    ctx.push(ValidationError::new(
                        "date_range",
                        "date/time is outside the supported ISO representation",
                    ));
                }
            }
            fn schema(_: &mut SchemaContext, _: bool) -> Value {
                $schema
            }
            fn kind() -> TypeKind {
                TypeKind::String
            }
        }
    };
}
date_type!(
    NaiveDate,
    parse_date,
    |v: &NaiveDate| v.format("%Y-%m-%d").to_string(),
    json!({"type":"string","format":"date","pattern":"^[0-9]{4}-[0-9]{2}-[0-9]{2}$","x-rydantic-local-rules":["chrono_date"]})
);
date_type!(
    NaiveTime,
    parse_time,
    |v: &NaiveTime| v.format("%H:%M:%S%.f").to_string(),
    json!({"type":"string","pattern":"^[0-9]{2}:[0-9]{2}:[0-9]{2}(\\.[0-9]+)?$","x-rydantic-local-rules":["chrono_naive_time"]})
);
date_type!(
    NaiveDateTime,
    parse_naive_datetime,
    |v: &NaiveDateTime| v.format("%Y-%m-%dT%H:%M:%S%.f").to_string(),
    json!({"type":"string","x-rydantic-local-rules":["chrono_naive_datetime"]})
);
macro_rules! zoned {
    ($ty:ty,$convert:expr) => {
        impl JsonType for $ty {
            fn decode_input(input:crate::Input<'_>,ctx:&mut Context,coerce:bool)->Option<Self>{Self::decode(crate::direct::scalar(input,ctx,"date/time")?,ctx,coerce)}
            fn decode(value:Value,ctx:&mut Context,_:bool)->Option<Self>{
                let Value::String(text)=value else{invalid(ctx);return None;};
                if text.split_once('.').is_some_and(|(_,fraction)|fraction.bytes().take_while(u8::is_ascii_digit).count()>9){invalid(ctx);return None;}
                match DateTime::parse_from_rfc3339(&text){Ok(value)=>{let value=($convert)(value);value.validate_into(ctx);Some(value)},Err(_)=>{invalid(ctx);None}}
            }
            fn encode(&self,ctx:&mut Context)->Value{let start=ctx.error_count();self.validate_into(ctx);if ctx.error_count()!=start{return Value::Null;}Value::String(self.to_rfc3339())}
            fn validate_into(&self,ctx:&mut Context){let offset=self.offset().fix().local_minus_utc();let local=self.naive_utc().checked_add_signed(chrono::TimeDelta::seconds(i64::from(offset)));if local.is_none_or(|local|!(0..=9999).contains(&local.year())) || offset%60!=0 {ctx.push(ValidationError::new("date_range","RFC 3339 requires a four-digit year and minute-aligned timezone"));}}
            fn schema(_:&mut SchemaContext,_:bool)->Value{json!({"type":"string","format":"date-time","x-rydantic-local-rules":["chrono_rfc3339"]})}
            fn kind()->TypeKind{TypeKind::String}
        }
    };
}
use chrono::Offset;
zoned!(DateTime<FixedOffset>, |v| v);
zoned!(DateTime<Utc>, |v: DateTime<FixedOffset>| v
    .with_timezone(&Utc));
