//! Port of `net.minecraft.network.protocol.common.custom` (issue #86) — the
//! custom-payload dispatch machinery.
//!
//! Java classes, one module each (this single file holds all three per the
//! shared custom module):
//! - [`CustomPacketPayload`] — the erased payload interface plus its
//!   identifier-dispatched `codec(fallback, types)` factory. In Rust the erased
//!   value is a closed enum (`Brand`/`Discarded`): Java's interface is open to
//!   plugin payload types, but the #86 slice only ships the vanilla known types,
//!   so an enum is the honest closed model. New known types extend the enum
//!   (or the registration surface switches to a trait object) when a plugin
//!   API needs them.
//! - [`BrandPayload`] — the only vanilla known type: a `utf` brand string.
//! - [`DiscardedPayload`] — the fallback for unknown ids (Paper stores the raw
//!   bytes rather than discarding them): `id` + raw `data`, no length prefix
//!   (Paper's "Always write data").
//!
//! The dispatch codec is built on [`crate::codec::dispatch`] — identifier key,
//! then the per-type payload codec. Java's `writeCap` writes the type id first
//! then the payload; `decode` reads the id, selects the codec (known map or
//! fallback), then decodes. `dispatch` encodes/decodes in exactly that order.
//!
//! The known-types list is `[BrandPayload]` for the clientbound config/gameplay
//! codecs; the serverbound `ServerboundCustomPayloadPacket` uses an empty list
//! (CraftBukkit treats all serverbound payloads the same, so every id falls
//! back to `DiscardedPayload`). See the two packet bodies.

use crate::codec::{CodecError, StreamCodec, dispatch, map, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::stream_codecs::identifier_codec;
use rivet_registry::Identifier;

/// `CustomPacketPayload.Type<T>` — the payload discriminator, an `Identifier`.
///
/// Java's record `Type<T extends CustomPacketPayload>(Identifier id)`; the `T`
/// is erased (a `Type<?>` is used wherever the payload subtype is unknown), so
/// the Rust value is a plain `Identifier` wrapper with no `T`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Type {
    id: Identifier,
}

impl Type {
    /// `CustomPacketPayload.createType(String id)` —
    /// `new Type<>(Identifier.withDefaultNamespace(id))`.
    pub fn create(id: &str) -> Self {
        Type {
            id: Identifier::with_default_namespace(id),
        }
    }

    /// `Type.id()`.
    pub fn id(&self) -> &Identifier {
        &self.id
    }
}

/// `net.minecraft.network.protocol.common.custom.CustomPacketPayload` — the
/// erased custom payload value.
///
/// `type()` returns the payload's [`Type`] (the identifier that keys the
/// dispatch). The enum's variants are the known concrete payloads; the
/// `Discarded` variant is the Paper fallback that preserves unknown bytes.
#[derive(Clone, Debug, PartialEq)]
pub enum CustomPacketPayload {
    /// `BrandPayload` — `minecraft:brand`.
    Brand(BrandPayload),
    /// `DiscardedPayload` — an unknown `type()` id, bytes preserved (Paper).
    Discarded(DiscardedPayload),
}

impl CustomPacketPayload {
    /// `CustomPacketPayload.type()` — the discriminator used by the dispatch
    /// codec. `BrandPayload` returns `BrandPayload.TYPE`; `DiscardedPayload`
    /// returns the id it was constructed with. Owned (the `Type` for `Brand` is
    /// constructed on the fly — `Identifier` has no const constructor), mirroring
    /// Java's per-call `type()` returning the shared `Type` record.
    pub fn type_id(&self) -> Identifier {
        match self {
            CustomPacketPayload::Brand(_) => BrandPayload::TYPE().id().clone(),
            CustomPacketPayload::Discarded(d) => d.id().clone(),
        }
    }

    /// `CustomPacketPayload.codec(FallbackProvider fallback, List<TypeAndCodec>
    /// types)` — the identifier-dispatched codec.
    ///
    /// `known` maps each known type id to its codec; an id not in the map falls
    /// back to `fallback(id)`. Mirroring `ClientboundCustomPayloadPacket`
    /// (`1048576`) and `ServerboundCustomPayloadPacket` (`32767`), the fallback
    /// is a `DiscardedPayload` capped at `max_payload_size`.
    pub fn codec(max_payload_size: i32) -> StreamCodec<FriendlyByteBuf, CustomPacketPayload> {
        let brand_id = BrandPayload::TYPE().id().clone();
        dispatch(
            identifier_codec(),
            |value: &CustomPacketPayload| value.type_id(),
            move |id: &Identifier| {
                if id == &brand_id {
                    map(
                        BrandPayload::stream_codec(),
                        |b: &BrandPayload| CustomPacketPayload::Brand(b.clone()),
                        |v: &CustomPacketPayload| match v {
                            CustomPacketPayload::Brand(b) => b.clone(),
                            CustomPacketPayload::Discarded(_) => {
                                unreachable!("brand codec only decodes brand payloads")
                            }
                        },
                    )
                } else {
                    map(
                        DiscardedPayload::stream_codec(id.clone(), max_payload_size),
                        |d: &DiscardedPayload| CustomPacketPayload::Discarded(d.clone()),
                        |v: &CustomPacketPayload| match v {
                            CustomPacketPayload::Discarded(d) => d.clone(),
                            CustomPacketPayload::Brand(_) => {
                                unreachable!("discarded codec only decodes discarded payloads")
                            }
                        },
                    )
                }
            },
        )
    }
}

/// `net.minecraft.network.protocol.common.custom.BrandPayload` — a `utf` brand
/// string, `minecraft:brand`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrandPayload {
    brand: String,
}

impl BrandPayload {
    /// `BrandPayload.TYPE = createType("brand")`.
    ///
    /// `Identifier` has no const constructor (`namespace`/`path` are private
    /// owned `String`s), so `TYPE` is a function returning a fresh `Type` each
    /// call — the value is the same `minecraft:brand` identifier, mirroring how
    /// `PacketType::serverbound` builds its `Identifier` at call time.
    // Mirrors the Java field name `BrandPayload.TYPE` exactly.
    #[allow(non_snake_case)]
    pub fn TYPE() -> Type {
        Type::create("brand")
    }

    /// `new BrandPayload(String brand)`.
    pub fn new(brand: String) -> Self {
        BrandPayload { brand }
    }

    /// `BrandPayload.brand()`.
    pub fn brand(&self) -> &str {
        &self.brand
    }

    /// `BrandPayload.STREAM_CODEC` — `Packet.codec(BrandPayload::write,
    /// BrandPayload::new)` over a `utf` string.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, BrandPayload> {
        of(
            |output: &mut FriendlyByteBuf, value: &BrandPayload| {
                output.write_utf(&value.brand);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(BrandPayload::new(input.read_utf())),
        )
    }
}

/// `net.minecraft.network.protocol.common.custom.DiscardedPayload` — the
/// fallback for unknown payload ids.
///
/// Paper's fork preserves the raw bytes instead of dropping them. The codec
/// writes `data` with **no length prefix** ("Always write data") and decodes
/// the entire remaining readable bytes, rejecting a payload over
/// `max_payload_size` with Java's `IllegalArgumentException` message surfaced
/// as `Err` at the codec boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscardedPayload {
    id: Identifier,
    data: Vec<u8>,
}

impl DiscardedPayload {
    /// `new DiscardedPayload(Identifier id, byte[] data)`.
    pub fn new(id: Identifier, data: Vec<u8>) -> Self {
        DiscardedPayload { id, data }
    }

    /// `DiscardedPayload.id()`.
    pub fn id(&self) -> &Identifier {
        &self.id
    }

    /// `DiscardedPayload.data()`.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// `DiscardedPayload.codec(Identifier id, int maxPayloadSize)` — writes the
    /// raw data (no length prefix) and decodes the whole readable buffer.
    pub fn stream_codec(
        id: Identifier,
        max_payload_size: i32,
    ) -> StreamCodec<FriendlyByteBuf, DiscardedPayload> {
        of(
            move |output: &mut FriendlyByteBuf, payload: &DiscardedPayload| {
                output.write_bytes(&payload.data);
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                let length = input.readable_bytes() as i32;
                if length >= 0 && length <= max_payload_size {
                    let data = input.read_slice(length);
                    Ok(DiscardedPayload::new(id.clone(), data))
                } else {
                    Err(CodecError::new(format!(
                        "Payload may not be larger than {max_payload_size} bytes"
                    )))
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    fn written(b: FriendlyByteBuf) -> Vec<u8> {
        b.into_inner().to_vec()
    }

    #[test]
    fn brand_payload_wire_is_utf_string() {
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        BrandPayload::stream_codec()
            .encode(&mut out, &BrandPayload::new("Paper".to_string()))
            .unwrap();
        // `utf`: varint length 5, then "Paper".
        assert_eq!(written(out), b"\x05Paper".to_vec());
    }

    #[test]
    fn custom_payload_dispatch_round_trips_brand() {
        let codec = CustomPacketPayload::codec(32767);
        let value = CustomPacketPayload::Brand(BrandPayload::new("Paper".to_string()));
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        codec.encode(&mut out, &value).unwrap();
        // Identifier "minecraft:brand" (len 10), then utf "Paper".
        let bytes = written(out);
        assert_eq!(bytes, b"\x0Fminecraft:brand\x05Paper".to_vec());
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), value);
    }

    #[test]
    fn custom_payload_dispatch_falls_back_to_discarded() {
        let codec = CustomPacketPayload::codec(32767);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        codec
            .encode(
                &mut out,
                &CustomPacketPayload::Discarded(DiscardedPayload::new(
                    Identifier::with_default_namespace("register"),
                    vec![1u8, 2, 3],
                )),
            )
            .unwrap();
        // Identifier "minecraft:register" (len 16), then raw data (no prefix).
        let bytes = written(out);
        let prefix = b"\x12minecraft:register".to_vec();
        assert_eq!(&bytes[..prefix.len()], &prefix);
        assert_eq!(&bytes[prefix.len()..], &[1u8, 2, 3]);

        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = codec.decode(&mut input).unwrap();
        match decoded {
            CustomPacketPayload::Discarded(d) => {
                assert_eq!(d.id(), &Identifier::with_default_namespace("register"));
                assert_eq!(d.data(), &[1u8, 2, 3]);
            }
            other => panic!("expected Discarded, got {other:?}"),
        }
    }

    #[test]
    fn discarded_payload_over_max_size_errors() {
        let codec = CustomPacketPayload::codec(2);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        // A brand payload is well under the size cap, but decoding a discarded
        // payload whose readable bytes exceed the cap errors with Java's message.
        out.write_utf("minecraft:register");
        out.write_bytes(&[1u8, 2, 3]);
        let mut input = FriendlyByteBuf::new(BytesMut::from(written(out).as_slice()));
        let err = codec.decode(&mut input).unwrap_err();
        assert_eq!(err.message, "Payload may not be larger than 2 bytes");
    }
}
