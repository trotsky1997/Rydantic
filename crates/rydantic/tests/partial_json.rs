#![cfg(feature = "partial-json")]

use rydantic::partial_json::{snapshot, Error, Status};
use serde_json::Value;
#[test]
fn every_byte_truncation() {
    for full in [
        r#"{"a":[true,false,null,123456789012345678901234567890,0.12345678901234567890e+10],"b":{"c":"中文😊\n\uD83D\uDE00"},"a":[1,2]}"#,
        r#"[[],{},"escaped\"quote",-1.23e-40]"#,
    ] {
        for n in 0..=full.len() {
            let bytes = &full.as_bytes()[..n];
            match snapshot(bytes) {
                Ok(result) => {
                    assert_eq!(
                        (result.status == Status::Complete),
                        serde_json::from_slice::<Value>(bytes).is_ok(),
                        "{bytes:?}"
                    );
                    if result.status == Status::Complete {
                        assert_eq!(result.value, Some(serde_json::from_slice(bytes).unwrap()))
                    }
                }
                Err(Error::IncompleteString { .. }) => {}
                Err(e) => panic!("{bytes:?}: {e:?}"),
            }
        }
    }
}
#[test]
fn incomplete_string_and_key_are_rejected() {
    for bytes in [
        b"\"a".as_slice(),
        b"[\"a",
        b"{\"a",
        b"{\"ok\":1,\"a",
        b"\"\\uZ",
        b"[\"\\uZZ",
        b"{\"\\uZZZ",
        b"\"\xff",
        b"\"\xe4",
        b"\"a\\",
        b"\"\\ud800",
    ] {
        assert!(snapshot(bytes).is_err(), "{bytes:?}")
    }
}
#[test]
fn malformed_prefix_is_never_recovered() {
    for s in [
        "[1 x",
        "{\"a\":1 x",
        "[1,]",
        "{\"a\":1,}",
        "[01",
        "[1e]",
        "[1.]",
        "[truX",
        "[nullx",
        "true false",
        "[NaN",
        "[Infinity",
        "\"\\q",
        "\"\\uZ000",
        "\"\\ud800x",
        "{\"a\" x",
        "{x",
        "[\\",
        "[\u{0001}",
    ] {
        assert!(snapshot(s.as_bytes()).is_err(), "{s}")
    }
}
#[test]
fn partial_container_snapshots() {
    for (s, expected) in [
        ("[", "[]"),
        ("[1,", "[1]"),
        ("{", "{}"),
        ("{\"a\":", "{}"),
        ("{\"a\":1,\"b\":[2,", "{\"a\":1,\"b\":[2]}"),
        ("[1e", "[]"),
        ("[false,t", "[false]"),
    ] {
        let r = snapshot(s.as_bytes()).unwrap();
        assert!(r.status == Status::Incomplete);
        assert_eq!(
            r.value,
            Some(serde_json::from_str::<Value>(expected).unwrap())
        );
    }
}
#[test]
fn duplicate_keys_last_completed_or_partial_value_wins() {
    let r = snapshot(b"{\"a\":1,\"a\":2,").unwrap();
    assert_eq!(r.value, Some(serde_json::json!({"a":2})));
    let r = snapshot(b"{\"a\":1,\"a\":").unwrap();
    assert_eq!(r.value, Some(serde_json::json!({"a":1})));
}
#[test]
fn bounds_and_precision() {
    assert!(matches!(
        snapshot(&vec![b' '; 1024 * 1024 + 1]),
        Err(Error::InputTooLarge)
    ));
    assert!(matches!(snapshot(&[b'['; 66]), Err(Error::NestingTooDeep)));
    let n = "123456789012345678901234567890.123456789012345678901234567890";
    let r = snapshot(format!("[{n},").as_bytes()).unwrap();
    assert_eq!(
        r.value.unwrap()[0],
        serde_json::from_str::<Value>(n).unwrap()
    );
}

#[test]
fn exact_nesting_boundary() {
    for n in [63, 64] {
        let input = format!("{}{}", "[".repeat(n), "]".repeat(n));
        assert_eq!(snapshot(input.as_bytes()).unwrap().status, Status::Complete);
        assert_eq!(
            snapshot("[".repeat(n).as_bytes()).unwrap().status,
            Status::Incomplete
        );
    }
    assert!(matches!(
        snapshot("[".repeat(65).as_bytes()),
        Err(Error::NestingTooDeep)
    ));
}

#[cfg(feature = "derive")]
#[test]
fn recovered_values_do_not_relax_model_validation() {
    use rydantic::Model;
    #[derive(Debug, Model)]
    struct Required {
        value: u32,
    }
    let preview = snapshot(b"{").unwrap();
    assert!(Required::parse_value(preview.value.unwrap()).is_err());
    let complete = snapshot(br#"{"value":1}"#).unwrap();
    assert_eq!(
        Required::parse_value(complete.value.unwrap())
            .unwrap()
            .value,
        1
    );
}

#[cfg(test)]
mod tests {
    use jiter::{Jiter, JiterErrorType, JsonErrorType};
    use serde_json::Value;

    #[test]
    fn numeric_bytes_preserve_arbitrary_precision() {
        for s in [
            "1234567890123456789012345678901234567890",
            "0.123456789012345678901234567890123456789",
            "1e9999999999999999999999",
            "-0.0000000000000000000000000000000000001",
        ] {
            let mut parser = Jiter::new(s.as_bytes());
            let raw = parser.next_number_bytes().unwrap();
            assert_eq!(raw, s.as_bytes());
            let via_raw: Value = serde_json::from_slice(raw).unwrap();
            assert_eq!(via_raw, serde_json::from_str::<Value>(s).unwrap());
            parser.finish().unwrap();
        }
    }

    #[test]
    fn every_numeric_prefix_matches_strict_parser_acceptance() {
        for s in [
            "-123456789012345678901234567890.12345678901234567890e+999",
            "1e-20",
            "0.001",
        ] {
            for n in 0..=s.len() {
                let bytes = &s.as_bytes()[..n];
                let strict = serde_json::from_slice::<Value>(bytes);
                let mut parser = Jiter::new(bytes);
                let raw = parser.next_number_bytes();
                match (strict, raw) {
                    (Ok(expected), Ok(raw)) => {
                        assert_eq!(serde_json::from_slice::<Value>(raw).unwrap(), expected)
                    }
                    (Err(expected), Err(actual)) => {
                        assert!(expected.is_eof(), "{bytes:?}: {expected:?}");
                        assert_eq!(
                            actual.error_type,
                            JiterErrorType::JsonError(JsonErrorType::EofWhileParsingValue)
                        );
                    }
                    pair => panic!("prefix disagreement {bytes:?}: {pair:?}"),
                }
            }
        }
    }

    #[test]
    fn malformed_numbers_are_not_eof() {
        for s in ["01", "1e]", "1.]", "--1", "1e+x"] {
            let error = Jiter::new(s.as_bytes()).next_number_bytes().unwrap_err();
            assert_ne!(
                error.error_type,
                JiterErrorType::JsonError(JsonErrorType::EofWhileParsingValue)
            );
        }
    }

    #[test]
    fn reproduce_invalid_partial_unicode_escape_acceptance() {
        // These prefixes cannot become valid JSON by appending bytes, but jiter
        // reports EOF and its partial string mode accepts them. A wrapper that
        // treats every EOF as recoverable would therefore hide invalid syntax.
        for s in [r#""\uZ"#, r#""\uZZ"#, r#""\uZZZ"#] {
            let error = Jiter::new(s.as_bytes()).next_str().unwrap_err();
            assert_eq!(
                error.error_type,
                JiterErrorType::JsonError(JsonErrorType::EofWhileParsingString)
            );
            assert_eq!(
                Jiter::new(s.as_bytes())
                    .with_allow_partial_strings()
                    .next_str()
                    .unwrap(),
                ""
            );
            let completed = format!("{s}0000\"");
            assert!(serde_json::from_str::<Value>(&completed).is_err());
        }
    }

    #[test]
    fn invalid_utf8_is_also_hidden_by_strict_eof() {
        let data = b"\"\xff";
        let error = Jiter::new(data).next_str().unwrap_err();
        assert_eq!(
            error.error_type,
            JiterErrorType::JsonError(JsonErrorType::EofWhileParsingString)
        );
        let error = Jiter::new(data)
            .with_allow_partial_strings()
            .next_str()
            .unwrap_err();
        assert_eq!(
            error.error_type,
            JiterErrorType::JsonError(JsonErrorType::InvalidUnicodeCodePoint)
        );
    }
}

#[test]
fn integer_part_limit_is_explicit_not_invalid_json() {
    for sign in ["", "-"] {
        for suffix in ["", ".25", "e+10", ".25e-10"] {
            for digits in [4300, 4301] {
                let number = format!("{sign}{}{suffix}", "1".repeat(digits));
                assert!(serde_json::from_str::<Value>(&number).is_ok());
                for (input, complete) in [(number.clone(), true), (format!("[{number},"), false)] {
                    if digits == 4301 {
                        assert!(
                            matches!(
                                snapshot(input.as_bytes()),
                                Err(Error::UnsupportedNumberLimit { .. })
                            ),
                            "sign={sign} suffix={suffix} complete={complete}"
                        );
                    } else {
                        let result = snapshot(input.as_bytes()).unwrap();
                        assert_eq!(
                            result.status,
                            if complete {
                                Status::Complete
                            } else {
                                Status::Incomplete
                            }
                        );
                        let actual = if complete {
                            result.value.unwrap()
                        } else {
                            result.value.unwrap()[0].clone()
                        };
                        assert_eq!(actual, serde_json::from_str::<Value>(&number).unwrap());
                    }
                }
            }
        }
    }
}

#[test]
fn lower_resource_limits_and_hard_ceilings() {
    use rydantic::partial_json::{snapshot_with_limits, Limits};
    let limits = Limits {
        max_input_bytes: 8,
        max_depth: 1,
    };
    assert_eq!(
        snapshot_with_limits(b"[1]", limits).unwrap().status,
        Status::Complete
    );
    assert!(matches!(
        snapshot_with_limits(b"[[", limits),
        Err(Error::NestingTooDeep)
    ));
    assert!(matches!(
        snapshot_with_limits(b"123456789", limits),
        Err(Error::InputTooLarge)
    ));
    assert!(matches!(
        snapshot_with_limits(
            b"0",
            Limits {
                max_input_bytes: 1024 * 1024 + 1,
                max_depth: 1
            }
        ),
        Err(Error::InvalidLimits)
    ));
    assert!(matches!(
        snapshot_with_limits(
            b"0",
            Limits {
                max_input_bytes: 1,
                max_depth: 65
            }
        ),
        Err(Error::InvalidLimits)
    ));
    assert!(snapshot_with_limits(
        b"0",
        Limits {
            max_input_bytes: 1,
            max_depth: 0
        }
    )
    .is_ok());
    assert!(matches!(
        snapshot_with_limits(
            b"[",
            Limits {
                max_input_bytes: 1,
                max_depth: 0
            }
        ),
        Err(Error::NestingTooDeep)
    ));
}
