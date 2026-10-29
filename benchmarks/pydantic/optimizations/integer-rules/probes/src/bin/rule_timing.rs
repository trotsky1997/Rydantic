use rydantic::{rules::Rules, Context, JsonType};
use serde_json::{json, Number};
use std::{hint::black_box, time::Instant};
fn probe<T: JsonType>(kind: &str, value: T, label: &str, rules: &Rules) {
    let operation = |direct: bool| {
        let mut ctx = Context::new();
        if direct {
            black_box(&value).validate_rules(black_box(rules), &mut ctx);
        } else {
            black_box(rules).validate(&black_box(&value).encode(&mut ctx), &mut ctx);
        }
        black_box(ctx.result())
    };
    assert_eq!(
        serde_json::to_value(operation(false)).unwrap(),
        serde_json::to_value(operation(true)).unwrap()
    );
    for (round, sequence) in [[false, true, true, false], [true, false, false, true]]
        .into_iter()
        .enumerate()
    {
        for (position, direct) in sequence.into_iter().enumerate() {
            for _ in 0..1000 {
                drop(black_box(operation(direct)));
            }
            let mut samples = Vec::new();
            let mut checksum = 0usize;
            for _ in 0..3 {
                let start = Instant::now();
                for _ in 0..10000 {
                    let result = black_box(operation(direct));
                    checksum += usize::from(result.is_ok());
                    drop(result);
                }
                samples.push(start.elapsed().as_nanos());
            }
            println!(
                "{}",
                json!({"type":kind,"case":label,"route":if direct {"typed"} else {"generic"},"round":round,"position":position,"iterations":10000,"warmup":1000,"repeats":3,"samples_ns":samples,"checksum":checksum})
            );
        }
    }
}
fn n(text: &str) -> Number {
    text.parse().unwrap()
}
fn main() {
    for (label, rules) in [
        ("no_bounds", Rules::default()),
        (
            "lexical_range",
            Rules {
                ge: Some(n("0")),
                le: Some(n("1000000000")),
                ..Rules::default()
            },
        ),
        (
            "lexical_strict",
            Rules {
                gt: Some(n("42")),
                lt: Some(n("42")),
                ..Rules::default()
            },
        ),
        (
            "decimal_equality",
            Rules {
                ge: Some(n("42.0")),
                le: Some(n("4.2e1")),
                ..Rules::default()
            },
        ),
        (
            "mixed_bounds",
            Rules {
                ge: Some(n("43")),
                gt: Some(n("42.0")),
                le: Some(n("41")),
                lt: Some(n("4.2e1")),
                ..Rules::default()
            },
        ),
        (
            "huge_bounds",
            Rules {
                ge: Some(n("-1e999999999999999999999999")),
                le: Some(n("1e999999999999999999999999")),
                ..Rules::default()
            },
        ),
    ] {
        probe("i64", 42i64, label, &rules);
        probe("i128", i128::MIN, label, &rules);
        probe("u128", u128::MAX, label, &rules);
    }
}
