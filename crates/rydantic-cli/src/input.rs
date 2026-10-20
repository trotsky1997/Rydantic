use crate::{fail, Result, LIMIT};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::{fmt, fs::File, io::Read, path::Path};

pub fn read(path: &Path, limit: usize) -> Result<String> {
    // Refuse FIFOs/devices before opening; never intentionally consume a stream.
    if !path
        .metadata()
        .map_err(|_| fail(4, "RD_INPUT_IO", "Cannot inspect input file."))?
        .is_file()
    {
        return Err(fail(4, "RD_INPUT_IO", "Input must be a regular file."));
    }
    let file = File::open(path).map_err(|_| fail(4, "RD_INPUT_IO", "Cannot read input file."))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| fail(4, "RD_INPUT_IO", "Cannot read input file."))?;
    if bytes.len() > limit {
        return Err(fail(7, "RD_LIMIT_EXCEEDED", "Input byte limit exceeded."));
    }
    String::from_utf8(bytes).map_err(|_| fail(2, "RD_INPUT_ENCODING", "Input must be UTF-8."))
}
pub fn toml(path: &Path) -> Result<toml::Value> {
    toml::from_str(&read(path, LIMIT)?)
        .map_err(|_| fail(2, "RD_MANIFEST_INVALID", "Invalid TOML input."))
}
pub fn json(path: &Path, limit: usize) -> Result<Value> {
    parse_json(&read(path, limit)?)
}
pub fn parse_json(s: &str) -> Result<Value> {
    let mut de = serde_json::Deserializer::from_str(s);
    let _checked = Bounded(0).deserialize(&mut de).map_err(|e| {
        if e.to_string().contains("depth limit") {
            fail(7, "RD_LIMIT_EXCEEDED", "JSON depth limit exceeded.")
        } else {
            fail(
                2,
                "RD_INPUT_JSON_INVALID",
                "Invalid JSON or duplicate object key.",
            )
        }
    })?;
    de.end()
        .map_err(|_| fail(2, "RD_INPUT_JSON_INVALID", "Unexpected content after JSON."))?;
    reject_reserved_objects(s)?;
    serde_json::from_str(s).map_err(|_| fail(2, "RD_INPUT_JSON_INVALID", "Invalid JSON input."))
}
struct Bounded(usize);
impl<'de> DeserializeSeed<'de> for Bounded {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> std::result::Result<Value, D::Error> {
        if self.0 > 64 {
            return Err(de::Error::custom("depth limit"));
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Bounded {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("bounded JSON without duplicate keys")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| de::Error::custom("invalid number"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        let mut v = vec![];
        while let Some(x) = a.next_element_seed(Bounded(self.0 + 1))? {
            v.push(x);
        }
        Ok(Value::Array(v))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        let mut v = Map::new();
        while let Some(k) = a.next_key::<String>()? {
            if v.contains_key(&k) {
                return Err(de::Error::custom("duplicate key"));
            }
            v.insert(k, a.next_value_seed(Bounded(self.0 + 1))?);
        }
        Ok(Value::Object(v))
    }
}

// With arbitrary_precision, serde_json represents numeric lexemes using a private
// map token. Inspect raw objects before Value conversion so an attacker cannot
// impersonate that token (for example turning an object location into an index).
// Syntax, duplicate keys and depth have already been checked above. RawValue
// keeps numeric spelling intact; this is not a second hand-written JSON lexer.
fn reject_reserved_objects(s: &str) -> Result<()> {
    use serde_json::value::RawValue;
    let invalid = || {
        fail(
            2,
            "RD_INPUT_JSON_INVALID",
            "Reserved JSON object member is not accepted.",
        )
    };
    match s.trim_start().as_bytes().first() {
        Some(b'{') => {
            let map: std::collections::BTreeMap<String, &RawValue> =
                serde_json::from_str(s).map_err(|_| invalid())?;
            if map.contains_key("$serde_json::private::Number")
                || map.contains_key("$serde_json::private::RawValue")
            {
                return Err(invalid());
            }
            for value in map.values() {
                reject_reserved_objects(value.get())?;
            }
        }
        Some(b'[') => {
            let array: Vec<&RawValue> = serde_json::from_str(s).map_err(|_| invalid())?;
            for value in array {
                reject_reserved_objects(value.get())?;
            }
        }
        _ => {}
    }
    Ok(())
}
