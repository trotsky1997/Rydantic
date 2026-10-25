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
use rydantic::{Context,Input};
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::hint::black_box;
fn main() {
 for width in [0,1,3,12,32,128,1024] {
  for reverse in [false,true] {
   let mut keys:Vec<_>=vec![0; width]; if reverse {keys.reverse();}
   let text=format!("{{{}}}",keys.iter().map(|i|format!("\"key{i:04}\":{i}")).collect::<Vec<_>>().join(","));
   let raw:&RawValue=serde_json::from_str(&text).unwrap();
   for engine in ["old","new","buffered"] {
    let mut ctx=Context::new();
    let live=LIVE.load(Ordering::Relaxed);PEAK.store(live,Ordering::Relaxed);let c=COUNT.load(Ordering::Relaxed);let b=BYTES.load(Ordering::Relaxed);
    let result:BTreeMap<String,Input<'_>>=if engine=="old" {
     serde_json::from_str::<BTreeMap<String,&RawValue>>(raw.get()).unwrap().into_iter().map(|(k,v)|(k,Input::Borrowed(v))).collect()
    }else if engine=="buffered" {serde_json::from_str::<rydantic_single_map_object_probe::Buffered<'_>>(raw.get()).unwrap().0}else{Input::Borrowed(raw).object(&mut ctx).unwrap()};
    let count=COUNT.load(Ordering::Relaxed)-c;let bytes=BYTES.load(Ordering::Relaxed)-b;
    let peak=PEAK.load(Ordering::Relaxed)-live;let retained=LIVE.load(Ordering::Relaxed)-live;black_box(&result);
    println!("{width}\t{reverse}\t{engine}\t{count}\t{bytes}\t{retained}\t{peak}");
   }
  }
 }
}
