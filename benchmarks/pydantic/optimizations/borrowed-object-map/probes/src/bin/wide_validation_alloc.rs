use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize,Ordering};
struct Counter;
static COUNT: AtomicUsize=AtomicUsize::new(0);
static PEAK: AtomicUsize=AtomicUsize::new(0);
static LIVE: AtomicUsize=AtomicUsize::new(0);
static BYTES: AtomicUsize=AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counter {
 unsafe fn alloc(&self,l:Layout)->*mut u8 {COUNT.fetch_add(1,Ordering::Relaxed);BYTES.fetch_add(l.size(),Ordering::Relaxed);let current=LIVE.fetch_add(l.size(),Ordering::Relaxed)+l.size();PEAK.fetch_max(current,Ordering::Relaxed);System.alloc(l)}
 unsafe fn dealloc(&self,p:*mut u8,l:Layout){LIVE.fetch_sub(l.size(),Ordering::Relaxed);System.dealloc(p,l)}
 unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8 {COUNT.fetch_add(1,Ordering::Relaxed);BYTES.fetch_add(n,Ordering::Relaxed);LIVE.fetch_add(n,Ordering::Relaxed);LIVE.fetch_sub(l.size(),Ordering::Relaxed);PEAK.fetch_max(LIVE.load(Ordering::Relaxed),Ordering::Relaxed);System.realloc(p,l,n)}
}
#[global_allocator] static ALLOC:Counter=Counter;
use rydantic::{BorrowType, Context, Input};
use serde_json::value::RawValue;
use std::{collections::BTreeMap,hint::black_box};
fn operation(text: &str, mode: u8) -> BTreeMap<String, i64> {
    let raw: &RawValue = serde_json::from_str(text).unwrap();
    let input = if mode == 0 {
        Input::Object(serde_json::from_str::<BTreeMap<String, &RawValue>>(raw.get()).unwrap()
            .into_iter().map(|(k, v)| (k, Input::Borrowed(v))).collect())
    } else if mode == 2 {
        Input::Object(serde_json::from_str::<rydantic_single_map_object_probe::Buffered<'_>>(raw.get()).unwrap().0)
    } else { Input::Borrowed(raw) };
    let mut ctx = Context::new();
    let output = BTreeMap::<String, i64>::decode(input, &mut ctx, false);
    ctx.finish(output).unwrap()
}
fn main(){
 for duplicate in [false,true] { for width in [3,12,32,128,1024] {
  let keys:Vec<_>=if duplicate {vec![0;width]} else {(0..width).collect()};
  let text=format!("{{{}}}",keys.iter().map(|i|format!("\"key{i:04}\":{i}")).collect::<Vec<_>>().join(","));
  for mode in [0,1,2] {
   let live=LIVE.load(Ordering::Relaxed);PEAK.store(live,Ordering::Relaxed);
   let c=COUNT.load(Ordering::Relaxed);let b=BYTES.load(Ordering::Relaxed);
   let result=black_box(operation(&text,mode));
   let calls=COUNT.load(Ordering::Relaxed)-c;let bytes=BYTES.load(Ordering::Relaxed)-b;
   let retained=LIVE.load(Ordering::Relaxed)-live;let peak=PEAK.load(Ordering::Relaxed)-live;
   println!("{width}\t{duplicate}\t{mode}\t{calls}\t{bytes}\t{retained}\t{peak}");
   drop(result);
  }
 }}
}
