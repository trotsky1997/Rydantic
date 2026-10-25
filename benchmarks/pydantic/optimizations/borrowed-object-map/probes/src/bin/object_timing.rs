use rydantic::{Context, Input};
use serde_json::value::RawValue;
use std::{collections::BTreeMap, hint::black_box, time::Instant};
fn operation<'a>(raw: &'a RawValue, mode: u8) -> BTreeMap<String, Input<'a>> {
    if mode == 0 {
        serde_json::from_str::<BTreeMap<String, &'a RawValue>>(raw.get()).unwrap()
            .into_iter().map(|(k, v)| (k, Input::Borrowed(v))).collect()
    } else if mode == 2 {
        serde_json::from_str::<rydantic_single_map_object_probe::Buffered<'_>>(raw.get()).unwrap().0
    } else {
        Input::Borrowed(raw).object(&mut Context::new()).unwrap()
    }
}
fn main() {
    println!("width\treverse\tmode\tposition\tsample\tns_per_op");
    for width in [3, 12, 32, 128, 1024] {
        for reverse in [false, true] {
            let mut keys: Vec<_> = (0..width).collect();
            if reverse { keys.reverse(); }
            let text = format!("{{{}}}", keys.iter().map(|i| format!("\"key{i:04}\":{i}")).collect::<Vec<_>>().join(","));
            let raw: &RawValue = serde_json::from_str(&text).unwrap();
            for (position, mode) in [0, 1, 2, 2, 1, 0].into_iter().enumerate() {
                for _ in 0..100 { drop(black_box(operation(black_box(raw), mode))); }
                for sample in 0..5 {
                    let start = Instant::now();
                    for _ in 0..2000 { drop(black_box(operation(black_box(raw), mode))); }
                    let nanos = start.elapsed().as_nanos() as f64 / 2000.0;
                    println!("{width}\t{reverse}\t{mode}\t{position}\t{sample}\t{nanos}");
                }
            }
        }
    }
}
