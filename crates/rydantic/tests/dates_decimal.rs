#![cfg(any(feature = "chrono", feature = "decimal"))]
use rydantic::{json, Model, SchemaMode};
#[cfg(feature = "chrono")]
#[test]
fn dates_validate_calendar_and_round_trip_rfc3339() {
    use rydantic::chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
    #[derive(Debug, Model)]
    struct Event {
        date: NaiveDate,
        time: NaiveTime,
        naive: NaiveDateTime,
        utc: DateTime<Utc>,
        offset: DateTime<FixedOffset>,
    }
    let x=Event::parse_json(r#"{"date":"2024-02-29","time":"12:30:45.123","naive":"2024-02-29T12:30:45","utc":"2024-02-29T12:30:45+08:00","offset":"2024-02-29T12:30:45+08:00"}"#).unwrap();
    let dump = x.dump().unwrap();
    assert_eq!(dump["utc"], "2024-02-29T04:30:45+00:00");
    assert_eq!(dump["offset"], "2024-02-29T12:30:45+08:00");
    assert_eq!(dump["time"], "12:30:45.123");
    assert!(
        jsonschema::validator_for(&Event::json_schema(SchemaMode::Output))
            .unwrap()
            .is_valid(&dump)
    );
    let mut bad = dump;
    bad["date"] = json!("2023-02-29");
    bad["time"] = json!("25:01:00");
    bad["utc"] = json!("2024-02-29T12:30:45");
    assert_eq!(Event::parse_value(bad).unwrap_err().len(), 3);
}
#[cfg(feature = "decimal")]
#[test]
fn decimal_is_exact_for_large_coefficients_scale_and_constraints() {
    use rydantic::rust_decimal::Decimal;
    #[derive(Debug, Model)]
    struct Money {
        #[model(ge = "0.1000000000000000000000000001")]
        amount: Decimal,
        #[model(coerce)]
        numeric: Decimal,
        optional: Option<Decimal>,
    }
    let x=Money::parse_json(r#"{"amount":"12345678901234567890.123456789","numeric":0.1234567890123456789012345678,"optional":"1.2300"}"#).unwrap();
    assert_eq!(x.amount.to_string(), "12345678901234567890.123456789");
    assert_eq!(x.numeric.to_string(), "0.1234567890123456789012345678");
    assert_eq!(x.dump().unwrap()["optional"], "1.2300");
    let err = Money::parse_json(r#"{"amount":"0.1000000000000000000000000000","numeric":1}"#)
        .unwrap_err();
    assert_eq!(err.errors()[0].code, "greater_than_equal");
    #[derive(Debug, Model)]
    struct Exact {
        amount: Decimal,
    }
    for value in [
        json!(0.1),
        json!("0.12345678901234567890123456789"),
        json!("1e-29"),
        json!("NaN"),
        json!("79228162514264337593543950336"),
    ] {
        assert!(Exact::parse_value(json!({"amount":value})).is_err());
    }
    let x = Exact::parse_json(r#"{"amount":"1.23e2"}"#).unwrap();
    assert_eq!(x.amount.to_string(), "123");
    assert!(
        jsonschema::validator_for(&Exact::json_schema(SchemaMode::Output))
            .unwrap()
            .is_valid(&x.dump().unwrap())
    );
    let report = Money::schema_report(SchemaMode::Output);
    assert!(report
        .local_rules
        .iter()
        .any(|r| r.rule == "decimal_bounds"));
}

#[cfg(feature = "chrono")]
#[test]
fn invalid_typed_dates_and_subsecond_offsets_return_errors_without_panics() {
    use rydantic::chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime};
    fn invalid() -> DateTime<FixedOffset> {
        DateTime::from_naive_utc_and_offset(NaiveDateTime::MAX, FixedOffset::east_opt(33).unwrap())
    }
    #[derive(Debug, Model)]
    struct Invalid {
        #[model(default_factory=invalid)]
        date: DateTime<FixedOffset>,
    }
    assert!(Invalid::parse_json("{}").is_err());
    #[derive(Debug, Model)]
    struct Date {
        date: NaiveDate,
    }
    assert!(Date {
        date: NaiveDate::from_ymd_opt(10000, 1, 1).unwrap()
    }
    .dump()
    .is_err());
    #[derive(Debug, Model)]
    struct Timestamp {
        value: DateTime<FixedOffset>,
    }
    assert!(Timestamp::parse_json(r#"{"value":"2024-02-29T12:30:45.1234567891Z"}"#).is_err());
}
