use std::alloc::{GlobalAlloc, Layout, System};
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
use rydantic::{json, Model, ValidationErrors, Value};
use serde::Deserialize;
use std::{
    hint::black_box,
    io::{self, Read},
    time::Instant,
};

#[derive(Model)]
#[model(extra = "forbid")]
struct Flat {
    #[model(ge = 0, le = 1000000000)]
    id: i64,
    #[model(min_length = 1, max_length = 64)]
    name: String,
    active: bool,
}
#[derive(Model)]
#[model(extra = "forbid")]
struct Nested {
    user: Flat,
    tags: Vec<String>,
}
#[derive(Model)]
#[model(extra = "forbid")]
struct Collection {
    items: Vec<Flat>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    input: String,
    mode: String,
    iterations: u64,
    warmup: u64,
    repeats: u64,
}
fn category(code: &str) -> &str {
    match code {
        "missing" => "missing",
        "extra_forbidden" => "extra",
        "greater_than_equal" | "less_than_equal" | "int_out_of_range" => "range",
        "too_short" | "too_long" => "length",
        "json_invalid" => "json_syntax",
        "type_error" | "int_type" => "type",
        _ => panic!("unmapped native error code: {code}"),
    }
}
fn run<M: Model>(r: &Request) -> Result<Value, Box<dyn std::error::Error>> {
    // Parsing the immutable source tree is deliberately outside value_copy timing.
    let value: Option<Value> = if r.mode == "value_copy" {
        Some(serde_json::from_str(&r.input)?)
    } else {
        None
    };
    let operation = || -> Result<M, ValidationErrors> {
        match &value {
            Some(v) => M::parse_value(black_box(v).clone()),
            None => M::parse_json(black_box(r.input.as_str())),
        }
    };
    // Prepare LazyLock model rules outside the measured allocation interval.
    drop(black_box(operation()));
    let calls = CALLS.load(Ordering::Relaxed);
    let bytes = BYTES.load(Ordering::Relaxed);
    let checked = operation();
    let calls = CALLS.load(Ordering::Relaxed) - calls;
    let bytes = BYTES.load(Ordering::Relaxed) - bytes;
    eprintln!("{{\"calls\":{calls},\"bytes\":{bytes}}}");
    let accepted = checked.is_ok();
    let (output, mut errors) = match checked {
        Ok(model) => (model.dump()?, vec![]),
        Err(errors) => (
            Value::Null,
            errors
                .errors()
                .iter()
                .map(|e| json!({"path":e.pointer(),"category":category(&e.code)}))
                .collect::<Vec<_>>(),
        ),
    };
    errors.sort_by_key(|v| v.to_string());
    for _ in 0..r.warmup {
        drop(black_box(operation()));
    }
    let mut samples = Vec::new();
    let mut baselines = Vec::new();
    let mut checksum = 0u64;
    for _ in 0..r.repeats {
        let start = Instant::now();
        for _ in 0..r.iterations {
            let result = black_box(operation());
            checksum += u64::from(result.is_ok());
            drop(result); // Include model/error disposal, not just allocation.
        }
        samples.push(start.elapsed().as_nanos());
        let start = Instant::now();
        let mut baseline_checksum = 0u64;
        for _ in 0..r.iterations {
            baseline_checksum += u64::from(black_box(accepted));
        }
        black_box(baseline_checksum);
        baselines.push(start.elapsed().as_nanos());
    }
    Ok(
        json!({"accepted":accepted,"output":output,"errors":errors,"samples_ns":samples,"baseline_ns":baselines,"iterations":r.iterations,"checksum":checksum,"engine":{"name":"rydantic","version":env!("CARGO_PKG_VERSION"),"profile":if cfg!(debug_assertions) {"debug"} else {"release"}}}),
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin()
        .take(16 * 1024 * 1024)
        .read_to_string(&mut input)?;
    let r: Request = serde_json::from_str(&input)?;
    if !matches!(r.mode.as_str(), "json" | "value_copy")
        || r.iterations == 0
        || r.iterations > 10_000_000
        || r.warmup > 1_000_000
        || r.repeats == 0
        || r.repeats > 100
    {
        return Err("invalid mode or run bounds".into());
    }
    let result = match r.schema.as_str() {
        "flat" => run::<Flat>(&r),
        "nested" => run::<Nested>(&r),
        "collection" => run::<Collection>(&r),
        _ => return Err("unknown schema".into()),
    }?;
    println!("{result}");
    Ok(())
}
