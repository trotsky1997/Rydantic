//! Incremental JSON sequences, NDJSON and array visitors. Readers are synchronous.
use crate::{BorrowModel, Context, Model, ValidationError, ValidationErrors};
use serde::{
    de::{DeserializeSeed, SeqAccess, Visitor},
    Deserialize,
};
use serde_json::{
    de::{IoRead, StrRead},
    value::RawValue,
    Deserializer, StreamDeserializer,
};
use std::{
    fmt,
    io::{BufRead, Read},
    marker::PhantomData,
};

/// A whitespace-separated sequence of JSON documents. Validation failures are recoverable;
/// malformed JSON terminates this iterator. Only the current document is materialized.
pub struct JsonStream<R: Read, T: Model> {
    inner: StreamDeserializer<'static, IoRead<R>, Box<RawValue>>,
    index: usize,
    marker: PhantomData<T>,
}
pub fn json_stream<R: Read, T: Model>(reader: R) -> JsonStream<R, T> {
    JsonStream {
        inner: Deserializer::from_reader(reader).into_iter(),
        index: 0,
        marker: PhantomData,
    }
}
impl<R: Read, T: Model> Iterator for JsonStream<R, T> {
    type Item = Result<T, ValidationErrors>;
    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.next()?;
        let index = self.index;
        self.index += 1;
        Some(
            item.map_err(|e| crate::borrowed::json_error(e).into())
                .and_then(|raw| T::parse_json(raw.get()))
                .map_err(|e| Context::prepend_errors(e, index)),
        )
    }
}
/// Borrowing sequence iterator; returned fields can point into the entire input buffer.
pub struct BorrowStream<'de, T: BorrowModel<'de>> {
    inner: StreamDeserializer<'de, StrRead<'de>, &'de RawValue>,
    index: usize,
    marker: PhantomData<T>,
}
pub fn borrow_stream<'de, T: BorrowModel<'de>>(input: &'de str) -> BorrowStream<'de, T> {
    BorrowStream {
        inner: Deserializer::from_str(input).into_iter(),
        index: 0,
        marker: PhantomData,
    }
}
impl<'de, T: BorrowModel<'de>> Iterator for BorrowStream<'de, T> {
    type Item = Result<T, ValidationErrors>;
    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.next()?;
        let index = self.index;
        self.index += 1;
        Some(
            item.map_err(|e| crate::borrowed::json_error(e).into())
                .and_then(T::parse_raw)
                .map_err(|e| Context::prepend_errors(e, index)),
        )
    }
}
/// One JSON document per non-empty line. Both validation and JSON syntax errors recover on the next line.
pub struct Ndjson<R: BufRead, T: Model> {
    reader: R,
    line: usize,
    buffer: String,
    done: bool,
    marker: PhantomData<T>,
}
pub fn ndjson<R: BufRead, T: Model>(reader: R) -> Ndjson<R, T> {
    Ndjson {
        reader,
        line: 0,
        buffer: String::new(),
        done: false,
        marker: PhantomData,
    }
}
impl<R: BufRead, T: Model> Iterator for Ndjson<R, T> {
    type Item = Result<T, ValidationErrors>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            self.buffer.clear();
            match self.reader.read_line(&mut self.buffer) {
                Ok(0) => {
                    self.done = true;
                    return None;
                }
                Ok(_) => self.line += 1,
                Err(error) => {
                    self.done = true;
                    return Some(Err(ValidationError::new("io_error", error.to_string())
                        .with_stage(crate::Stage::Parse)
                        .into()));
                }
            }
            if self.buffer.trim().is_empty() {
                continue;
            }
            return Some(T::parse_json(&self.buffer).map_err(|mut errors| {
                for e in &mut errors.0 {
                    e.params.insert("stream_line".into(), self.line.into());
                }
                errors
            }));
        }
    }
}
struct ArraySeed<'a, T, F> {
    callback: &'a mut F,
    error: &'a mut Option<ValidationErrors>,
    marker: PhantomData<T>,
}
impl<'de, T: Model, F: FnMut(Result<T, ValidationErrors>) -> Result<(), ValidationErrors>>
    DeserializeSeed<'de> for ArraySeed<'_, T, F>
{
    type Value = usize;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<usize, D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de, T: Model, F: FnMut(Result<T, ValidationErrors>) -> Result<(), ValidationErrors>>
    Visitor<'de> for ArraySeed<'_, T, F>
{
    type Value = usize;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<usize, A::Error> {
        let mut count = 0;
        while let Some(raw) = seq.next_element::<Box<RawValue>>()? {
            let item =
                T::parse_json(raw.get()).map_err(|errors| Context::prepend_errors(errors, count));
            count += 1;
            if let Err(error) = (self.callback)(item) {
                *self.error = Some(error);
                return Err(serde::de::Error::custom("array visitor stopped"));
            }
        }
        Ok(count)
    }
}
/// Visit array elements without holding the entire array in memory. The callback receives
/// indexed validation errors and decides whether to continue. Its error stops reading immediately.
pub fn visit_array<
    R: Read,
    T: Model,
    F: FnMut(Result<T, ValidationErrors>) -> Result<(), ValidationErrors>,
>(
    reader: R,
    mut callback: F,
) -> Result<usize, ValidationErrors> {
    let mut de = Deserializer::from_reader(reader);
    let mut callback_error = None;
    let result = ArraySeed::<T, F> {
        callback: &mut callback,
        error: &mut callback_error,
        marker: PhantomData,
    }
    .deserialize(&mut de);
    if let Some(error) = callback_error {
        return Err(error);
    }
    let count = result.map_err(crate::borrowed::json_error)?;
    de.end().map_err(crate::borrowed::json_error)?;
    Ok(count)
}
/// Read one raw document. Useful for custom stream adapters and async validation pipelines.
pub fn read_document<R: Read>(reader: R) -> Result<Box<RawValue>, ValidationErrors> {
    let mut de = Deserializer::from_reader(reader);
    let raw = Box::<RawValue>::deserialize(&mut de).map_err(crate::borrowed::json_error)?;
    de.end().map_err(crate::borrowed::json_error)?;
    Ok(raw)
}

/// Incremental raw documents for pipelines that call `Model::parse_json_async` per record.
/// Reading is synchronous; no async runtime is required by Rydantic.
pub fn documents<R: Read>(
    reader: R,
) -> impl Iterator<Item = Result<Box<RawValue>, ValidationErrors>> {
    Deserializer::from_reader(reader)
        .into_iter::<Box<RawValue>>()
        .enumerate()
        .map(|(index, item)| {
            item.map_err(|e| Context::prepend_errors(crate::borrowed::json_error(e).into(), index))
        })
}
struct BorrowArraySeed<'a, 'de, T, F> {
    callback: &'a mut F,
    error: &'a mut Option<ValidationErrors>,
    marker: PhantomData<(&'de str, T)>,
}
impl<
        'de,
        T: BorrowModel<'de>,
        F: FnMut(Result<T, ValidationErrors>) -> Result<(), ValidationErrors>,
    > DeserializeSeed<'de> for BorrowArraySeed<'_, 'de, T, F>
{
    type Value = usize;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<usize, D::Error> {
        d.deserialize_seq(self)
    }
}
impl<
        'de,
        T: BorrowModel<'de>,
        F: FnMut(Result<T, ValidationErrors>) -> Result<(), ValidationErrors>,
    > Visitor<'de> for BorrowArraySeed<'_, 'de, T, F>
{
    type Value = usize;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<usize, A::Error> {
        let mut count = 0;
        while let Some(raw) = seq.next_element::<&'de RawValue>()? {
            let item = T::parse_raw(raw).map_err(|errors| Context::prepend_errors(errors, count));
            count += 1;
            if let Err(error) = (self.callback)(item) {
                *self.error = Some(error);
                return Err(serde::de::Error::custom("array visitor stopped"));
            }
        }
        Ok(count)
    }
}
/// Visit a JSON array in a borrowed buffer while retaining references into that buffer.
pub fn visit_borrowed_array<
    'de,
    T: BorrowModel<'de>,
    F: FnMut(Result<T, ValidationErrors>) -> Result<(), ValidationErrors>,
>(
    input: &'de str,
    mut callback: F,
) -> Result<usize, ValidationErrors> {
    let mut de = Deserializer::from_str(input);
    let mut callback_error = None;
    let result = BorrowArraySeed::<T, F> {
        callback: &mut callback,
        error: &mut callback_error,
        marker: PhantomData,
    }
    .deserialize(&mut de);
    if let Some(error) = callback_error {
        return Err(error);
    }
    let count = result.map_err(crate::borrowed::json_error)?;
    de.end().map_err(crate::borrowed::json_error)?;
    Ok(count)
}
