use rydantic::Input;
use serde::{de::{MapAccess, Visitor}, Deserialize, Deserializer};
use serde_json::value::RawValue;
use std::{collections::BTreeMap, fmt};
pub struct Buffered<'a>(pub BTreeMap<String, Input<'a>>);
impl<'a> Deserialize<'a> for Buffered<'a> {
 fn deserialize<D:Deserializer<'a>>(d:D)->Result<Self,D::Error> {
  struct V;
  impl<'a> Visitor<'a> for V {
   type Value=Buffered<'a>;
   fn expecting(&self,f:&mut fmt::Formatter)->fmt::Result{f.write_str("a map")}
   fn visit_map<M:MapAccess<'a>>(self,mut map:M)->Result<Self::Value,M::Error>{
    let mut entries=Vec::new();
    while let Some((k,v))=map.next_entry::<String,&'a RawValue>()? {entries.push((k,Input::Borrowed(v)));}
    Ok(Buffered(entries.into_iter().collect()))
   }
  }
  d.deserialize_map(V)
 }
}
