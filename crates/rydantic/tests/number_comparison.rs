use rydantic::{
    rules::{compare_numbers, Rules},
    Context, ValidationError,
};
use serde_json::{Number, Value};
use std::cmp::Ordering;

// Frozen pre-optimization algorithm: differential oracle, not a float oracle.
fn original_compare_numbers(left: &Number, right: &Number) -> Ordering {
    fn parts(number: &Number) -> (bool, Vec<u8>, i64) {
        let text = number.to_string();
        let negative = text.starts_with('-');
        let text = text.trim_start_matches('-');
        let (mantissa, exponent) = text.split_once(['e', 'E']).map_or((text, 0), |(m, e)| {
            (
                m,
                e.parse::<i64>().unwrap_or(if e.starts_with('-') {
                    i64::MIN / 2
                } else {
                    i64::MAX / 2
                }),
            )
        });
        let decimals = mantissa
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len() as i64);
        let digits: Vec<u8> = mantissa
            .bytes()
            .filter(u8::is_ascii_digit)
            .skip_while(|digit| *digit == b'0')
            .collect();
        let magnitude = (digits.len() as i64)
            .saturating_add(exponent)
            .saturating_sub(decimals);
        (negative && !digits.is_empty(), digits, magnitude)
    }
    let (ln, ld, lm) = parts(left);
    let (rn, rd, rm) = parts(right);
    if ld.is_empty() && rd.is_empty() {
        return Ordering::Equal;
    }
    if ln != rn {
        return if ln {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    let order = if ld.is_empty() {
        Ordering::Less
    } else if rd.is_empty() {
        Ordering::Greater
    } else {
        lm.cmp(&rm).then_with(|| {
            (0..ld.len().max(rd.len()))
                .map(|i| {
                    ld.get(i)
                        .copied()
                        .unwrap_or(b'0')
                        .cmp(&rd.get(i).copied().unwrap_or(b'0'))
                })
                .find(|order| *order != Ordering::Equal)
                .unwrap_or(Ordering::Equal)
        })
    };
    if ln {
        order.reverse()
    } else {
        order
    }
}

fn corpus() -> Vec<Number> {
    let mut texts: Vec<String> = [
        "0",
        "-0",
        "0.0",
        "-0.0",
        "0e999999999999999999999999",
        "-0e-999999999999999999999999",
        "1",
        "-1",
        "42",
        "42.0",
        "4.2e1",
        "420e-1",
        "0.42e2",
        "42.00000000000000000000000000000000001",
        "9007199254740991",
        "9007199254740992",
        "9007199254740993",
        "-9223372036854775809",
        "-9223372036854775808",
        "9223372036854775807",
        "9223372036854775808",
        "18446744073709551615",
        "18446744073709551616",
        "-170141183460469231731687303715884105729",
        "-170141183460469231731687303715884105728",
        "170141183460469231731687303715884105727",
        "170141183460469231731687303715884105728",
        "340282366920938463463374607431768211455",
        "340282366920938463463374607431768211456",
        "1e9223372036854775807",
        "1e9223372036854775808",
        "1e-9223372036854775808",
        "1e-9223372036854775809",
        "-1e999999999999999999999999",
        "1e999999999999999999999999",
        "1e-999999999999999999999999",
        "-1e-999999999999999999999999",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for n in [
        -1000000000i128,
        -43,
        -42,
        -2,
        -1,
        0,
        1,
        2,
        41,
        42,
        43,
        1000000000,
    ] {
        for suffix in ["", ".0", "e0", "e+1", "e-1"] {
            texts.push(format!("{n}{suffix}"));
        }
    }
    texts.into_iter().map(|s| s.parse().unwrap()).collect()
}

#[test]
fn boundary_cartesian_comparison_and_full_rule_errors_match_original() {
    let numbers = corpus();
    for left in &numbers {
        for right in &numbers {
            let order = original_compare_numbers(left, right);
            assert_eq!(compare_numbers(left, right), order, "{left} vs {right}");
            let rules = Rules {
                ge: Some(right.clone()),
                gt: Some(right.clone()),
                le: Some(right.clone()),
                lt: Some(right.clone()),
                ..Rules::default()
            };
            let mut actual = Context::new();
            rules.validate(&Value::Number(left.clone()), &mut actual);
            let mut expected = Context::new();
            for (invalid, operator, code) in [
                (order == Ordering::Less, ">=", "greater_than_equal"),
                (order != Ordering::Greater, ">", "greater_than"),
                (order == Ordering::Greater, "<=", "less_than_equal"),
                (order != Ordering::Less, "<", "less_than"),
            ] {
                if invalid {
                    expected.push(
                        ValidationError::new(code, format!("value must be {operator} {right}"))
                            .with_param("limit", right.clone()),
                    );
                }
            }
            assert_eq!(
                serde_json::to_value(actual.result().unwrap_err()).unwrap(),
                serde_json::to_value(expected.result().unwrap_err()).unwrap(),
                "{left} vs {right}"
            );
        }
    }
}

#[test]
fn generated_full_width_integers_match_exact_integer_order() {
    let mut state = 0x6a09e667f3bcc908bb67ae8584caa73bu128;
    for _ in 0..10000 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let a = state as i128;
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let b = state as i128;
        let left = Number::from_i128(a).unwrap();
        let right = Number::from_i128(b).unwrap();
        assert_eq!(compare_numbers(&left, &right), a.cmp(&b));
        assert_eq!(
            compare_numbers(&left, &right),
            original_compare_numbers(&left, &right)
        );
    }
}

#[test]
fn decimal_exponent_and_signed_zero_equivalence_is_retained() {
    for (a, b, expected) in [
        ("42", "42.0", Ordering::Equal),
        ("42", "4.2e1", Ordering::Equal),
        ("-0", "0", Ordering::Equal),
        ("9007199254740993", "9007199254740992", Ordering::Greater),
        (
            "340282366920938463463374607431768211455",
            "170141183460469231731687303715884105727",
            Ordering::Greater,
        ),
    ] {
        assert_eq!(
            compare_numbers(&a.parse().unwrap(), &b.parse().unwrap()),
            expected
        );
    }
}
