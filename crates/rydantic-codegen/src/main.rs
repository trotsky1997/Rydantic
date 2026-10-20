use std::io::{self, Read, Write};
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args_os().len() != 1 {
        return Err("usage: rydantic-codegen < schema.json > models.rs".into());
    }
    let mut input = Vec::new();
    io::stdin()
        .take(rydantic_codegen::MAX_SCHEMA_BYTES as u64 + 1)
        .read_to_end(&mut input)?;
    if input.len() > rydantic_codegen::MAX_SCHEMA_BYTES {
        return Err("schema exceeds 65536 bytes".into());
    }
    let _: UniqueKeys = serde_json::from_slice(&input)?;
    let schema = serde_json::from_slice(&input)?;
    let source = rydantic_codegen::generate(&schema)?;
    io::stdout().write_all(source.as_bytes())?;
    Ok(())
}

// Reject duplicate schema keywords before Value would keep only the last one.
// This uses serde_json's parser, not a separate JSON lexer.
struct UniqueKeys;
impl<'de> serde::Deserialize<'de> for UniqueKeys {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueKeys;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<UniqueKeys, E> {
                Ok(UniqueKeys)
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<UniqueKeys, E> {
                Ok(UniqueKeys)
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<UniqueKeys, E> {
                Ok(UniqueKeys)
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<UniqueKeys, E> {
                Ok(UniqueKeys)
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<UniqueKeys, E> {
                Ok(UniqueKeys)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<UniqueKeys, E> {
                Ok(UniqueKeys)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<UniqueKeys, A::Error> {
                while a.next_element::<UniqueKeys>()?.is_some() {}
                Ok(UniqueKeys)
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<UniqueKeys, A::Error> {
                let mut keys = std::collections::BTreeSet::new();
                while let Some(key) = a.next_key::<String>()? {
                    if !keys.insert(key) {
                        return Err(serde::de::Error::custom("duplicate schema object key"));
                    }
                    a.next_value::<UniqueKeys>()?;
                }
                Ok(UniqueKeys)
            }
        }
        d.deserialize_any(Visitor)
    }
}
