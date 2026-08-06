//! Port of `net.minecraft.network.protocol.common.ServerboundCustomClickActionPacket`
//! (issue #86).
//!
//! Java: `ServerboundCustomClickActionPacket.java` in `working/Paper`. An
//! `Identifier` id and an `Optional<Tag>` payload:
//! `optionalTagCodec(() -> new NbtAccounter(32768L, 16)).apply(lengthPrefixed(65536))`
//! — the optional tag is encoded/decoded through a scratch buffer length-prefixed
//! at 65536 bytes. Registered in play and configuration serverbound.

use crate::codec::byte_buf_codecs;
use crate::codec::{StreamCodec, apply, composite_2};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::serverbound_custom_click_action;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::stream_codecs::identifier_codec;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;

/// `ServerboundCustomClickActionPacket.UNTRUSTED_TAG_CODEC` —
/// `optionalTagCodec(NbtAccounter(32768, 16)).lengthPrefixed(65536)`.
pub fn untrusted_tag_codec() -> StreamCodec<FriendlyByteBuf, Option<Tag>> {
    apply(
        byte_buf_codecs::optional_tag_codec(|| NbtAccounter::new(32768, 16)),
        byte_buf_codecs::length_prefixed(65536),
    )
}

/// `net.minecraft.network.protocol.common.ServerboundCustomClickActionPacket`.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerboundCustomClickActionPacket {
    id: Identifier,
    payload: Option<Tag>,
}

impl ServerboundCustomClickActionPacket {
    /// `new ServerboundCustomClickActionPacket(Identifier id, Optional<Tag>
    /// payload)`.
    pub fn new(id: Identifier, payload: Option<Tag>) -> Self {
        ServerboundCustomClickActionPacket { id, payload }
    }

    /// `ServerboundCustomClickActionPacket.id()`.
    pub fn id(&self) -> &Identifier {
        &self.id
    }

    /// `ServerboundCustomClickActionPacket.payload()`.
    pub fn payload(&self) -> Option<&Tag> {
        self.payload.as_ref()
    }

    /// `ServerboundCustomClickActionPacket.STREAM_CODEC` — a two-field
    /// composite (`Identifier.STREAM_CODEC`, then the untrusted tag codec).
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundCustomClickActionPacket> {
        composite_2(
            identifier_codec(),
            |p: &ServerboundCustomClickActionPacket| p.id.clone(),
            untrusted_tag_codec(),
            |p: &ServerboundCustomClickActionPacket| p.payload.clone(),
            ServerboundCustomClickActionPacket::new,
        )
    }
}

impl Packet for ServerboundCustomClickActionPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_custom_click_action()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::int_tag::IntTag;
    use rivet_nbt::tag::Tag;

    #[test]
    fn round_trips_some_payload() {
        let mut compound = CompoundTag::new();
        compound.put("k".to_string(), Tag::Int(IntTag::value_of(5)));
        let packet = ServerboundCustomClickActionPacket::new(
            Identifier::with_default_namespace("button"),
            Some(Tag::Compound(compound.clone())),
        );
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundCustomClickActionPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ServerboundCustomClickActionPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }

    #[test]
    fn round_trips_none_payload() {
        let packet = ServerboundCustomClickActionPacket::new(
            Identifier::with_default_namespace("button"),
            None,
        );
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundCustomClickActionPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ServerboundCustomClickActionPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }
}
