//! Cap’n Proto messages over gRPC, using tonic's custom codec API.
//!
//! This is **not** native Cap’n Proto RPC or protobuf interoperability. Both peers
//! must agree on the Cap’n Proto schema and the gRPC service/method path. The
//! demonstrated integration is unary; no streaming API is promised.
//!
//! Configure tonic's `max_decoding_message_size` and `max_encoding_message_size`
//! to the same byte limit as this codec. Tonic buffers the framed message before
//! invoking the decoder, so the codec alone cannot bound transport buffering.
//! Compression is not enabled by this adapter.
use std::marker::PhantomData;

use bytes::{Buf, BufMut};
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::{Code, Status};

use crate::CapnpModel;

/// Finite limits for an unpacked Cap’n Proto message (excluding the gRPC header).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    bytes: usize,
    traversal_words: usize,
    nesting: i32,
}

impl Limits {
    /// Reject zero limits and nonpositive nesting depths.
    pub fn new(bytes: usize, traversal_words: usize, nesting: i32) -> Result<Self, Status> {
        if bytes == 0 || traversal_words == 0 || nesting <= 0 {
            return Err(Status::invalid_argument(
                "gRPC codec limits must be positive",
            ));
        }
        Ok(Self {
            bytes,
            traversal_words,
            nesting,
        })
    }

    /// Set the corresponding tonic send/receive limit to this value as well.
    pub fn message_bytes(self) -> usize {
        self.bytes
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 4 * 1024 * 1024,
            traversal_words: 8 * 1024 * 1024,
            nesting: 64,
        }
    }
}

/// Typed codec for synchronous `CapnpModel` validation.
///
/// `E` is the outgoing model and `D` is the incoming model. Parsing runs the
/// existing Cap’n Proto parse/normalization pipeline; sending validates but does
/// not normalize. Models requiring asynchronous checks are rejected, not skipped.
/// Errors contain no model values, field names or validator messages.
#[derive(Debug)]
pub struct CapnpCodec<E, D> {
    limits: Limits,
    encode_error: Code,
    decode_error: Code,
    marker: PhantomData<fn() -> (E, D)>,
}

impl<E, D> CapnpCodec<E, D> {
    /// Client: invalid incoming responses are `Internal`. Use [`request`] before
    /// dispatch to obtain `InvalidArgument` for invalid local requests: tonic
    /// wraps errors raised during encoding as `Internal`.
    pub fn client(limits: Limits) -> Self {
        Self {
            limits,
            encode_error: Code::InvalidArgument,
            decode_error: Code::Internal,
            marker: PhantomData,
        }
    }
    /// Server: invalid incoming requests are `InvalidArgument`; invalid outgoing
    /// responses are `Internal`.
    pub fn server(limits: Limits) -> Self {
        Self {
            limits,
            encode_error: Code::Internal,
            decode_error: Code::InvalidArgument,
            marker: PhantomData,
        }
    }
}

impl<E: CapnpModel + Send + 'static, D: CapnpModel + Send + 'static> Codec for CapnpCodec<E, D> {
    type Encode = E;
    type Decode = D;
    type Encoder = CapnpEncoder<E>;
    type Decoder = CapnpDecoder<D>;
    fn encoder(&mut self) -> Self::Encoder {
        CapnpEncoder {
            limits: self.limits,
            error: self.encode_error,
            marker: PhantomData,
        }
    }
    fn decoder(&mut self) -> Self::Decoder {
        CapnpDecoder {
            limits: self.limits,
            error: self.decode_error,
            marker: PhantomData,
        }
    }
}

/// Encoder returned by [`CapnpCodec`].
pub struct CapnpEncoder<T> {
    limits: Limits,
    error: Code,
    marker: PhantomData<fn() -> T>,
}
/// Decoder returned by [`CapnpCodec`].
pub struct CapnpDecoder<T> {
    limits: Limits,
    error: Code,
    marker: PhantomData<fn() -> T>,
}

impl<T: CapnpModel> CapnpEncoder<T> {
    fn checked_message(
        &self,
        item: &T,
    ) -> Result<capnp::message::Builder<capnp::message::HeapAllocator>, Status> {
        // Building may allocate proportionally to the local model. Check the wire
        // size before allocating a serialized copy or extending tonic's buffer.
        let message = item
            .to_capnp_message()
            .map_err(|_| Status::new(self.error, "invalid Cap'n Proto model"))?;
        let size = capnp::serialize::compute_serialized_size_in_words(&message).checked_mul(8);
        if size.is_none_or(|size| size > self.limits.bytes) {
            return Err(Status::new(
                self.error,
                "Cap'n Proto message exceeds byte limit",
            ));
        }
        Ok(message)
    }
}
impl<T: CapnpModel> Encoder for CapnpEncoder<T> {
    type Item = T;
    type Error = Status;
    fn encode(&mut self, item: T, dst: &mut EncodeBuf<'_>) -> Result<(), Status> {
        let message = self.checked_message(&item)?;
        dst.put_slice(&capnp::serialize::write_message_to_words(&message));
        Ok(())
    }
}
impl<T: CapnpModel> CapnpDecoder<T> {
    fn decode_bytes(&self, bytes: &[u8]) -> Result<T, Status> {
        if bytes.len() > self.limits.bytes {
            return Err(Status::new(
                self.error,
                "Cap'n Proto message exceeds byte limit",
            ));
        }
        let mut options = capnp::message::ReaderOptions::new();
        options.traversal_limit_in_words(Some(self.limits.traversal_words));
        options.nesting_limit(self.limits.nesting);
        T::parse_capnp_with_options(bytes, options)
            .map_err(|_| Status::new(self.error, "invalid Cap'n Proto message"))
    }
}
impl<T: CapnpModel> Decoder for CapnpDecoder<T> {
    type Item = T;
    type Error = Status;
    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<T>, Status> {
        // DecodeBuf is backed by contiguous BytesMut; do not copy attacker data.
        let value = self.decode_bytes(src.chunk())?;
        src.advance(src.remaining());
        Ok(Some(value))
    }
}

/// Preflight a client request before tonic dispatches it.
///
/// Invalid models or an oversized encoded message return redacted
/// `InvalidArgument`. Tonic otherwise wraps encoder errors as `Internal`.
/// Validation does not normalize or mutate the model. The codec validates again
/// at encoding; custom validators should be deterministic and side-effect free.
pub fn request<T: CapnpModel>(model: T, limits: Limits) -> Result<tonic::Request<T>, Status> {
    let encoder = CapnpEncoder::<T> {
        limits,
        error: Code::InvalidArgument,
        marker: PhantomData,
    };
    encoder.checked_message(&model)?;
    Ok(tonic::Request::new(model))
}

#[cfg(all(test, feature = "derive"))]
mod tests {
    use super::*;
    #[crate::capnp_models(namespace = "rydantic.grpc.tests")]
    mod models {
        #[derive(crate::Model)]
        pub struct Item {
            pub text: String,
        }
        #[derive(crate::Model)]
        pub struct AsyncItem {
            #[model(async_validate = super::check)]
            pub text: String,
        }
    }
    async fn check(_: &String) -> Result<(), crate::ValidationError> {
        Ok(())
    }
    #[test]
    fn byte_limit_applies_before_model_parsing() {
        let mut codec =
            CapnpCodec::<models::Item, models::Item>::server(Limits::new(8, 1024, 64).unwrap());
        let error = codec.decoder().decode_bytes(&[0; 9]).err().unwrap();
        assert_eq!(error.code(), Code::InvalidArgument);
        assert_eq!(error.message(), "Cap'n Proto message exceeds byte limit");
        assert_eq!(
            codec
                .encoder()
                .checked_message(&models::Item {
                    text: "large".into()
                })
                .err()
                .unwrap()
                .code(),
            Code::Internal
        );
    }
    #[test]
    fn exact_byte_limit_is_inclusive() {
        let item = models::Item {
            text: "hello".into(),
        };
        let bytes = item.dump_capnp().unwrap();
        let exact = Limits::new(bytes.len(), 1024, 64).unwrap();
        let smaller = Limits::new(bytes.len() - 1, 1024, 64).unwrap();
        let mut allowed = CapnpCodec::<models::Item, models::Item>::server(exact);
        assert!(allowed.decoder().decode_bytes(&bytes).is_ok());
        assert!(allowed.encoder().checked_message(&item).is_ok());
        let mut denied = CapnpCodec::<models::Item, models::Item>::server(smaller);
        assert_eq!(
            denied.decoder().decode_bytes(&bytes).err().unwrap().code(),
            Code::InvalidArgument
        );
        assert_eq!(
            denied
                .encoder()
                .checked_message(&item)
                .err()
                .unwrap()
                .code(),
            Code::Internal
        );
    }
    #[test]
    fn async_validation_is_never_silently_skipped() {
        assert_eq!(
            request(
                models::AsyncItem {
                    text: "secret".into()
                },
                Limits::default()
            )
            .err()
            .unwrap()
            .code(),
            Code::InvalidArgument
        );
        let mut codec =
            CapnpCodec::<models::AsyncItem, models::AsyncItem>::server(Limits::default());
        let bytes = models::Item {
            text: "secret".into(),
        }
        .dump_capnp()
        .unwrap();
        assert_eq!(
            codec.decoder().decode_bytes(&bytes).err().unwrap().code(),
            Code::InvalidArgument
        );
        assert_eq!(
            codec
                .encoder()
                .checked_message(&models::AsyncItem {
                    text: "secret".into()
                })
                .err()
                .unwrap()
                .code(),
            Code::Internal
        );
    }
}
