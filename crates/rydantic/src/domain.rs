//! Optional string domain types backed by versioned third-party catalogs.
//!
//! These types validate syntax/catalog membership, not ownership, reachability,
//! current legal tender, or political status. See the domain-types guide for data provenance.
//!
//! ```
//! # #[cfg(all(feature = "country", feature = "currency", feature = "phone"))] {
//! use rydantic::domain::{CountryAlpha2, CurrencyCode, PhoneNumber};
//! let country: CountryAlpha2 = "CN".parse().unwrap();
//! let currency: CurrencyCode = "CNY".parse().unwrap();
//! let phone = PhoneNumber::parse_with_region("(202) 555-0123", "US").unwrap();
//! assert_eq!(phone.as_str(), "+12025550123");
//! assert_eq!(serde_json::to_string(&country).unwrap(), "\"CN\"");
//! assert_eq!(currency.as_str(), "CNY");
//! # }
//! ```
use crate::{Context, JsonType, SchemaContext, Stage, TypeKind, ValidationError, Value};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

/// A domain input failed validation. The input itself is never included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainError(&'static str);
impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for DomainError {}

macro_rules! string_domain {
    ($name:ident, $rule:literal, $pattern:literal) => {
        impl crate::AsyncCheck for $name {}
        impl $name {
            /// Canonical validated string representation.
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.as_str()) }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer)?.parse().map_err(serde::de::Error::custom)
            }
        }
        impl JsonType for $name {
            fn decode_input(input: crate::Input<'_>, ctx: &mut Context, coerce: bool) -> Option<Self> {
                Self::decode(crate::direct::scalar(input, ctx, "domain string")?, ctx, coerce)
            }
            fn decode(value: Value, ctx: &mut Context, _: bool) -> Option<Self> {
                if let Value::String(text) = value {
                    match text.parse() {
                        Ok(value) => return Some(value),
                        Err(error) => ctx.push(ValidationError::new($rule, format!("{error}")).with_stage(Stage::Parse)),
                    }
                } else {
                    ctx.push(ValidationError::new($rule, "expected a domain string").with_stage(Stage::Parse));
                }
                None
            }
            fn encode(&self, _: &mut Context) -> Value { Value::String(self.0.clone()) }
            fn schema(_: &mut SchemaContext, _: bool) -> Value {
                serde_json::json!({"type":"string", "pattern":$pattern, "x-rydantic-local-rules":[$rule]})
            }
            fn kind() -> TypeKind { TypeKind::String }
        }
    };
}

/// ISO 3166-1 uppercase alpha-2 code in rust_iso3166's pinned catalog.
#[cfg(feature = "country")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CountryAlpha2(String);
#[cfg(feature = "country")]
impl FromStr for CountryAlpha2 {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() == 2
            && s.bytes().all(|b| b.is_ascii_uppercase())
            && rust_iso3166::from_alpha2(s).is_some()
        {
            Ok(Self(s.into()))
        } else {
            Err(DomainError(
                "expected an uppercase ISO 3166-1 alpha-2 catalog code",
            ))
        }
    }
}
#[cfg(feature = "country")]
string_domain!(CountryAlpha2, "country_alpha2_catalog", "^[A-Z]{2}$");

/// ISO 3166-1 uppercase alpha-3 code in rust_iso3166's pinned catalog.
#[cfg(feature = "country")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CountryAlpha3(String);
#[cfg(feature = "country")]
impl FromStr for CountryAlpha3 {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() == 3
            && s.bytes().all(|b| b.is_ascii_uppercase())
            && rust_iso3166::from_alpha3(s).is_some()
        {
            Ok(Self(s.into()))
        } else {
            Err(DomainError(
                "expected an uppercase ISO 3166-1 alpha-3 catalog code",
            ))
        }
    }
}
#[cfg(feature = "country")]
string_domain!(CountryAlpha3, "country_alpha3_catalog", "^[A-Z]{3}$");

/// Known uppercase currency catalog code, including legacy, fund and special codes.
/// This does not assert that a currency is current or legal tender.
#[cfg(feature = "currency")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CurrencyCode(String);
#[cfg(feature = "currency")]
impl FromStr for CurrencyCode {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() == 3
            && s.bytes().all(|b| b.is_ascii_uppercase())
            && iso_currency::Currency::from_code(s).is_some()
        {
            Ok(Self(s.into()))
        } else {
            Err(DomainError(
                "expected an uppercase code in the currency catalog",
            ))
        }
    }
}
#[cfg(feature = "currency")]
string_domain!(CurrencyCode, "currency_catalog", "^[A-Z]{3}$");

/// Metadata-valid canonical E.164 phone number, without extensions.
/// Does not verify assignment, reachability, ownership or consent to contact.
#[cfg(feature = "phone")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhoneNumber(String);
#[cfg(feature = "phone")]
impl PhoneNumber {
    /// Parse a national or international number with an explicit uppercase region.
    /// Uses the dependency's human-input parser and normalizes to E.164.
    /// Extensions are unsupported and rejected rather than silently discarded.
    pub fn parse_with_region(text: &str, region: &str) -> Result<Self, DomainError> {
        if region.len() != 2 || !region.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(DomainError("expected an uppercase phone region code"));
        }
        if text.len() > 128
            || !text
                .bytes()
                .all(|b| b.is_ascii_digit() || b"+()- .".contains(&b))
        {
            return Err(DomainError(
                "only digits and phone punctuation are accepted; extensions are unsupported",
            ));
        }
        let region = region
            .parse()
            .map_err(|_| DomainError("unknown phone region code"))?;
        Self::parsed(text, Some(region))
    }
    fn parsed(text: &str, region: Option<phonenumber::country::Id>) -> Result<Self, DomainError> {
        let number =
            phonenumber::parse(region, text).map_err(|_| DomainError("invalid phone number"))?;
        if number.extension().is_some() {
            return Err(DomainError("phone extensions are unsupported"));
        }
        if !phonenumber::is_valid(&number) {
            return Err(DomainError(
                "phone number fails numbering-plan metadata validation",
            ));
        }
        let canonical = number.format().mode(phonenumber::Mode::E164).to_string();
        // Some national metadata accepts numbers longer than E.164 permits.
        // Every constructor must preserve the same serialization contract.
        if !Self::is_e164_shape(&canonical) {
            return Err(DomainError("phone number exceeds canonical E.164 limits"));
        }
        Ok(Self(canonical))
    }
    fn is_e164_shape(text: &str) -> bool {
        let bytes = text.as_bytes();
        (3..=16).contains(&bytes.len())
            && bytes[0] == b'+'
            && (b'1'..=b'9').contains(&bytes[1])
            && bytes[2..].iter().all(u8::is_ascii_digit)
    }
}
#[cfg(feature = "phone")]
impl FromStr for PhoneNumber {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !Self::is_e164_shape(s) {
            return Err(DomainError(
                "expected canonical E.164 (+ and at most 15 digits), without extension",
            ));
        }
        let parsed = Self::parsed(s, None)?;
        if parsed.as_str() != s {
            return Err(DomainError("expected canonical E.164"));
        }
        Ok(parsed)
    }
}
#[cfg(feature = "phone")]
string_domain!(PhoneNumber, "phone_e164_metadata", "^\\+[1-9][0-9]{1,14}$");
