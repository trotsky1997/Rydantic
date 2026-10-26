use rydantic::{rules::Rules, Context, JsonType};
use serde_json::{json, Number};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
struct Counter;
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout)
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(size, Ordering::Relaxed);
        System.realloc(pointer, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
fn probe<T: JsonType>(kind: &str, value: T, label: &str, rules: &Rules) {
    for direct in [false, true] {
        let mut ctx = Context::new();
        let calls = CALLS.load(Ordering::Relaxed);
        let bytes = BYTES.load(Ordering::Relaxed);
        if direct {
            black_box(&value).validate_rules(black_box(rules), &mut ctx);
        } else {
            black_box(rules).validate(&black_box(&value).encode(&mut ctx), &mut ctx);
        }
        let calls = CALLS.load(Ordering::Relaxed) - calls;
        let bytes = BYTES.load(Ordering::Relaxed) - bytes;
        let result = ctx.result();
        println!(
            "{}",
            json!({"type":kind,"case":label,"route":if direct {"typed"} else {"generic"},"calls":calls,"bytes":bytes,"result":result})
        );
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
