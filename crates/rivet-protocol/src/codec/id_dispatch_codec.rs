//! Port of `net.minecraft.network.codec.IdDispatchCodec`.
//!
//! Java: `IdDispatchCodec.java` in `working/Paper` (vanilla 26.2). The wire
//! format is a varint packet id followed by the per-id payload; `build()`
//! panics on a duplicate registration (`IllegalStateException`), encode errors
//! with `"Sending unknown packet '<type>'"`, and decode errors with
//! `"Received unknown packet id <n>"`.
//!
//! The value type `V` is the (subtype-polymorphic) packet value and `T` is the
//! discriminator produced by `type_getter`. With the only buffer being
//! `FriendlyByteBuf`, Java's `Entry<B, V, T>` erasure
//! (`StreamCodec<? super B, ? extends V>` stored as `(StreamCodec<? super B,
//! V>)`) becomes exactly `StreamCodec<FriendlyByteBuf, V>` — the same erasure
//! the worktree and `StreamCodec.dispatch` use.
//!
//! `DontDecorateException` has no Rust analogue. Its only purpose is to let an
//! inner codec signal "already decorated, rethrow as-is"; with every entry error
//! already a [`CodecError`], `IdDispatchCodec` decorates by construction and the
//! inner message is deliberately dropped (matching netty's prefix-only text).
//! This also removes the need for a `catch_unwind`/`is::<...>` chain.

use crate::codec::stream_codec::{CodecError, StreamCodec, StreamCodecDyn};
use crate::codec::stream_decoder::StreamDecoder;
use crate::codec::stream_encoder::StreamEncoder;
use crate::friendly_byte_buf::FriendlyByteBuf;
use std::collections::HashMap;
use std::fmt::Display;

/// `IdDispatchCodec.Entry` — `(StreamCodec<? super B, ? extends V>, T)` erased
/// to `StreamCodec<FriendlyByteBuf, V>`.
struct Entry<V: 'static, T> {
    codec: StreamCodec<FriendlyByteBuf, V>,
    ty: T,
}

/// `IdDispatchCodec<FriendlyByteBuf, V, T>` — dispatch on a varint id.
pub struct IdDispatchCodec<V: 'static, T> {
    type_getter: Box<dyn Fn(&V) -> T + Send + Sync>,
    by_id: Vec<Entry<V, T>>,
    to_id: HashMap<T, i32>,
}

impl<V: 'static, T: Display + Eq + std::hash::Hash + Send + Sync + 'static>
    StreamDecoder<FriendlyByteBuf, V> for IdDispatchCodec<V, T>
{
    fn decode(&self, input: &mut FriendlyByteBuf) -> Result<V, CodecError> {
        let id = input.read_var_int();
        if id >= 0 && (id as usize) < self.by_id.len() {
            let entry = &self.by_id[id as usize];
            entry
                .codec
                .decode(input)
                .map_err(|_| CodecError::new(format!("Failed to decode packet '{}'", entry.ty)))
        } else {
            Err(CodecError::new(format!("Received unknown packet id {id}")))
        }
    }
}

impl<V: 'static, T: Display + Eq + std::hash::Hash + Send + Sync + 'static>
    StreamEncoder<FriendlyByteBuf, V> for IdDispatchCodec<V, T>
{
    fn encode(&self, output: &mut FriendlyByteBuf, value: &V) -> Result<(), CodecError> {
        let ty = (self.type_getter)(value);
        let id = match self.to_id.get(&ty) {
            Some(id) => *id,
            None => return Err(CodecError::new(format!("Sending unknown packet '{ty}'"))),
        };
        output.write_var_int(id);
        let entry = &self.by_id[id as usize];
        entry
            .codec
            .encode(output, value)
            .map_err(|_| CodecError::new(format!("Failed to encode packet '{ty}'")))
    }
}

impl<V: 'static, T: Display + Eq + std::hash::Hash + Send + Sync + 'static>
    StreamCodecDyn<FriendlyByteBuf, V> for IdDispatchCodec<V, T>
{
}

/// `IdDispatchCodec.Builder<B, V, T>` — collects `(type, codec)` entries and
/// builds the dispatch table. Ids are registration order.
pub struct Builder<V: 'static, T> {
    entries: Vec<Entry<V, T>>,
    type_getter: Box<dyn Fn(&V) -> T + Send + Sync>,
}

impl<V: 'static, T> Builder<V, T> {
    /// `add(T type, StreamCodec<? super B, ? extends V> serializer)`.
    pub fn add(mut self, ty: T, serializer: StreamCodec<FriendlyByteBuf, V>) -> Self {
        self.entries.push(Entry {
            codec: serializer,
            ty,
        });
        self
    }

    /// `build()` — panics on a duplicate type registration (Java
    /// `IllegalStateException("Duplicate registration for type ...")`).
    pub fn build(self) -> IdDispatchCodec<V, T>
    where
        T: Clone + Display + Eq + std::hash::Hash + Send + Sync + 'static,
    {
        let mut to_id: HashMap<T, i32> = HashMap::new();
        for entry in &self.entries {
            if to_id.contains_key(&entry.ty) {
                panic!("Duplicate registration for type {}", entry.ty);
            }
            let id = to_id.len() as i32;
            to_id.insert(entry.ty.clone(), id);
        }
        IdDispatchCodec {
            type_getter: self.type_getter,
            by_id: self.entries,
            to_id,
        }
    }
}

/// `IdDispatchCodec.builder(Function<V, ? extends T> typeGetter)`.
pub fn builder<V: 'static, T: 'static>(
    type_getter: impl Fn(&V) -> T + Send + Sync + 'static,
) -> Builder<V, T> {
    Builder {
        entries: Vec::new(),
        type_getter: Box::new(type_getter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::stream_codec::{StreamCodec, of};
    use crate::friendly_byte_buf::FriendlyByteBuf;
    use bytes::BytesMut;
    use std::panic::catch_unwind;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    fn written(b: FriendlyByteBuf) -> Vec<u8> {
        b.into_inner().to_vec()
    }

    fn panic_message<F: FnOnce() -> R, R>(f: F) -> String {
        let err = match catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(_) => panic!("expected the closure to panic"),
            Err(err) => err,
        };
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "non-string panic payload".to_string())
    }

    /// A dispatch over two packet variants distinguished by a string type.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Packet {
        A(i32),
        B(i32),
    }

    fn dispatch_codec() -> IdDispatchCodec<Packet, String> {
        builder(|p: &Packet| match p {
            Packet::A(_) => "A".to_string(),
            Packet::B(_) => "B".to_string(),
        })
        .add(
            "A".to_string(),
            map_to_packet(Packet::A, |p| match p {
                Packet::A(v) => *v,
                Packet::B(_) => unreachable!(),
            }),
        )
        .add(
            "B".to_string(),
            map_to_packet(Packet::B, |p| match p {
                Packet::B(v) => *v,
                Packet::A(_) => unreachable!(),
            }),
        )
        .build()
    }

    fn map_to_packet(
        to: impl Fn(i32) -> Packet + Send + Sync + 'static,
        from: impl Fn(&Packet) -> i32 + Send + Sync + 'static,
    ) -> StreamCodec<FriendlyByteBuf, Packet> {
        of(
            move |output: &mut FriendlyByteBuf, value: &Packet| {
                let v = from(value);
                output.write_var_int(v);
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| Ok(to(input.read_var_int())),
        )
    }

    #[test]
    fn round_trips_by_id_with_exact_bytes() {
        let codec = StreamCodec::new(dispatch_codec());
        let mut out = buf();
        codec.encode(&mut out, &Packet::A(5)).unwrap();
        // id 0 varint, then the short payload varint 5.
        let bytes_a = written(out);
        assert_eq!(bytes_a, vec![0, 5]);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes_a.as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), Packet::A(5));

        let mut out = buf();
        codec.encode(&mut out, &Packet::B(5)).unwrap();
        // id 1 varint, then the short payload varint 5.
        let bytes_b = written(out);
        assert_eq!(bytes_b, vec![1, 5]);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes_b.as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), Packet::B(5));
    }

    #[test]
    fn decode_unknown_id_errors() {
        let codec = StreamCodec::new(dispatch_codec());
        let mut input = buf();
        input.write_var_int(5);
        let err = codec.decode(&mut input).unwrap_err();
        assert_eq!(err.message, "Received unknown packet id 5");
    }

    #[test]
    fn encode_unregistered_type_errors() {
        let codec = StreamCodec::new(builder(|_: &Packet| "C".to_string()).build());
        let mut out = buf();
        let err = codec.encode(&mut out, &Packet::A(0)).unwrap_err();
        assert_eq!(err.message, "Sending unknown packet 'C'");
    }

    #[test]
    fn encode_entry_error_is_decorated() {
        // An entry codec that always fails on encode.
        let failing: StreamCodec<FriendlyByteBuf, Packet> = of(
            |_output: &mut FriendlyByteBuf, _value: &Packet| Err(CodecError::new("inner boom")),
            |input: &mut FriendlyByteBuf| Ok(Packet::A(input.read_var_int())),
        );
        let codec = StreamCodec::new(
            builder(|_: &Packet| "A".to_string())
                .add("A".to_string(), failing)
                .build(),
        );
        let mut out = buf();
        let err = codec.encode(&mut out, &Packet::A(1)).unwrap_err();
        assert_eq!(err.message, "Failed to encode packet 'A'");
    }

    #[test]
    fn decode_entry_error_is_decorated() {
        // An entry codec that always fails on decode.
        let failing: StreamCodec<FriendlyByteBuf, Packet> = of(
            |_output: &mut FriendlyByteBuf, _value: &Packet| Ok(()),
            |_input: &mut FriendlyByteBuf| Err(CodecError::new("inner boom")),
        );
        let codec = StreamCodec::new(
            builder(|_: &Packet| "A".to_string())
                .add("A".to_string(), failing)
                .build(),
        );
        let mut input = buf();
        input.write_var_int(0);
        let err = codec.decode(&mut input).unwrap_err();
        assert_eq!(err.message, "Failed to decode packet 'A'");
    }

    #[test]
    fn duplicate_registration_panics_with_java_message() {
        let builder = builder(|_: &Packet| "A".to_string())
            .add(
                "A".to_string(),
                map_to_packet(Packet::A, |p| match p {
                    Packet::A(v) => *v,
                    Packet::B(_) => unreachable!(),
                }),
            )
            .add(
                "A".to_string(),
                map_to_packet(Packet::A, |p| match p {
                    Packet::A(v) => *v,
                    Packet::B(_) => unreachable!(),
                }),
            );
        let msg = panic_message(|| builder.build());
        assert_eq!(msg, "Duplicate registration for type A");
    }
}
