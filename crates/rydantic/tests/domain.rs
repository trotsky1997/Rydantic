#![cfg(any(feature = "phone", feature = "country", feature = "currency"))]
use rydantic::{Context, JsonType, SchemaContext, SchemaMode, Value};
use serde::{de::DeserializeOwned, Serialize};
use std::{fmt::Debug, str::FromStr};
fn contract<T: JsonType + Serialize + DeserializeOwned + FromStr + Debug>(
    good: &[&str],
    bad: &[&str],
) {
    for s in good {
        let value = Value::String((*s).into());
        let mut ctx = Context::new();
        let decoded = T::decode(value.clone(), &mut ctx, false);
        let decoded = ctx.finish(decoded).unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
        assert_eq!(decoded.encode(&mut Context::new()), value);
        assert!(serde_json::from_value::<T>(value.clone()).is_ok());
        for mode in [SchemaMode::Input, SchemaMode::Output] {
            let schema = T::schema(&mut SchemaContext::new(mode), false);
            assert!(schema.get("format").is_none());
            assert!(schema["x-rydantic-local-rules"].is_array());
            assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&value));
        }
    }
    for value in bad.iter().map(|s| Value::String((*s).into())).chain([
        Value::Null,
        Value::Bool(true),
        serde_json::json!(123),
        serde_json::json!([]),
        serde_json::json!({}),
    ]) {
        assert!(serde_json::from_value::<T>(value.clone()).is_err());
        for coerce in [false, true] {
            let mut ctx = Context::new();
            let result = T::decode(value.clone(), &mut ctx, coerce);
            assert!(ctx.finish(result).is_err(), "accepted {value}");
        }
    }
    assert!(!T::allows_missing());
    assert!(T::missing().is_none());
}
#[cfg(feature = "country")]
#[test]
fn country_catalog_case_and_former_codes() {
    use rydantic::domain::{CountryAlpha2, CountryAlpha3};
    contract::<CountryAlpha2>(
        &["US", "CN", "GB", "AQ"],
        &[
            "us", "Us", " US", "US\n", "USA", "UK", "SU", "ZZ", "ＵＳ", "",
        ],
    );
    contract::<CountryAlpha3>(
        &["USA", "CHN", "GBR", "ATA"],
        &["usa", "US", "SUN", "ZZZ", "USA ", ""],
    );
    for country in rust_iso3166::ALL {
        assert!(country.alpha2.parse::<CountryAlpha2>().is_ok());
        assert!(country.alpha3.parse::<CountryAlpha3>().is_ok());
    }
}
#[cfg(feature = "currency")]
#[test]
fn currency_keeps_legacy_fund_and_special_identity() {
    use rydantic::domain::CurrencyCode;
    contract::<CurrencyCode>(
        &["USD", "CNY", "EUR", "VES", "VED", "BOV", "XDR", "XXX"],
        &["usd", "Usd", " USD", "USD\n", "ZZZ", "840", "US", ""],
    );
    assert_eq!("VES".parse::<CurrencyCode>().unwrap().as_str(), "VES");
}
#[cfg(feature = "phone")]
#[test]
fn phone_is_strict_canonical_e164() {
    use rydantic::domain::PhoneNumber;
    contract::<PhoneNumber>(
        &["+12025550123", "+442079460123"],
        &[
            "2025550123",
            "12025550123",
            "+1 202 555 0123",
            "+12025550123 ext 4",
            "tel:+12025550123",
            "+0123456",
            "+999123456789",
            "+1202555012345678",
            "",
            "+",
            "+1",
            "+１２３",
        ],
    );
}
#[cfg(feature = "phone")]
#[test]
fn phone_region_is_explicit_and_extensions_are_not_dropped() {
    use rydantic::domain::PhoneNumber;
    let parsed = PhoneNumber::parse_with_region("(202) 555-0123", "US").unwrap();
    assert_eq!(parsed.as_str(), "+12025550123");
    for text in [
        "2025550123 ext 45",
        "2025550123x45",
        "2025550123;ext=45",
        "2025550123#45",
        "2025550123,45",
        "call 2025550123",
        "2025550123\n",
    ] {
        assert!(
            PhoneNumber::parse_with_region(text, "US").is_err(),
            "accepted {text}"
        );
    }
    for region in ["", "us", "ZZ", "USA", " US"] {
        assert!(PhoneNumber::parse_with_region("2025550123", region).is_err());
    }
}
#[cfg(all(feature = "derive", feature = "country"))]
#[test]
fn model_missing_null_and_schema_contract() {
    use rydantic::{domain::CountryAlpha2, Model};
    #[derive(Debug, Model)]
    struct Address {
        country: CountryAlpha2,
        optional: Option<CountryAlpha2>,
    }
    assert!(Address::parse_json(r#"{}"#).is_err());
    assert!(Address::parse_json(r#"{"country":null}"#).is_err());
    let parsed = Address::parse_json(r#"{"country":"CN","optional":null}"#).unwrap();
    assert_eq!(parsed.dump().unwrap()["country"], "CN");
    assert!(Address::parse_json(r#"{"country":"CN"}"#).is_ok());
}

#[cfg(all(
    feature = "derive",
    feature = "phone",
    feature = "country",
    feature = "currency"
))]
#[test]
fn raw_model_input_matches_value_and_serde() {
    use rydantic::{
        domain::{CountryAlpha3, CurrencyCode, PhoneNumber},
        Model,
    };
    #[derive(Debug, Model)]
    struct Contact {
        phone: PhoneNumber,
        country: CountryAlpha3,
        currency: CurrencyCode,
    }
    let text = r#"{"phone":"+12025550123","country":"USA","currency":"USD"}"#;
    let parsed = Contact::parse_json(text).unwrap();
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        parsed.dump().unwrap(),
        Contact::parse_value(value.clone()).unwrap().dump().unwrap()
    );
    for mode in [SchemaMode::Input, SchemaMode::Output] {
        let schema = Contact::json_schema(mode);
        assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&value));
    }
    for (field, invalid) in [
        ("phone", "+1 2025550123"),
        ("country", "usa"),
        ("currency", "usd"),
    ] {
        let mut bad = value.clone();
        bad[field] = serde_json::json!(invalid);
        assert!(Contact::parse_json(&bad.to_string()).is_err());
        assert!(Contact::parse_value(bad).is_err());
    }
}

#[cfg(feature = "phone")]
#[test]
fn phone_constructor_bounds_and_no_implicit_region_requirement() {
    use rydantic::domain::PhoneNumber;
    assert!(PhoneNumber::parse_with_region(&"1".repeat(129), "US").is_err());
    // Region is a parsing hint, not an international country restriction.
    assert_eq!(
        PhoneNumber::parse_with_region("+44 20 7946 0123", "US")
            .unwrap()
            .as_str(),
        "+442079460123"
    );
    let error = PhoneNumber::parse_with_region("secret extension 1234", "US").unwrap_err();
    assert!(!error.to_string().contains("secret"));
}

#[cfg(feature = "phone")]
#[test]
fn national_metadata_cannot_exceed_e164_output_contract() {
    use rydantic::domain::PhoneNumber;
    for text in ["0301234567890123", "030123456789012", "+49301234567890123"] {
        assert!(PhoneNumber::parse_with_region(text, "DE").is_err());
    }
    let boundary = PhoneNumber::parse_with_region("03012345678901", "DE").unwrap();
    assert_eq!(boundary.as_str().len(), 16);
    assert_eq!(boundary.as_str().parse::<PhoneNumber>().unwrap(), boundary);
}
