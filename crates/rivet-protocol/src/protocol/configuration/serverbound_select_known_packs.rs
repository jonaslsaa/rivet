//! Port of `net.minecraft.network.protocol.configuration.ServerboundSelectKnownPacks`
//! (issue #109).
//!
//! Java: `ServerboundSelectKnownPacks.java` in `working/Paper`. The client's
//! reply to `ClientboundSelectKnownPacks`: the `KnownPack`s it actually has,
//! **bounded at 64** (`ByteBufCodecs.list(64)`). Sent at the start of the
//! configuration registry sync (`ServerConfigurationPacketListenerImpl
//! .handleSelectKnownPacks`).

use crate::codec::byte_buf_codecs::list_max;
use crate::codec::{CodecOperation, StreamCodec, apply, map};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::configuration::packet_types::serverbound_select_known_packs;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::stream_codecs::known_pack_stream_codec;
use rivet_util::KnownPack;

/// `net.minecraft.network.protocol.configuration.ServerboundSelectKnownPacks` —
/// the record `(List<KnownPack> knownPacks)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerboundSelectKnownPacks {
    known_packs: Vec<KnownPack>,
}

impl ServerboundSelectKnownPacks {
    /// `new ServerboundSelectKnownPacks(List<KnownPack> knownPacks)`.
    pub fn new(known_packs: Vec<KnownPack>) -> Self {
        ServerboundSelectKnownPacks { known_packs }
    }

    /// `ServerboundSelectKnownPacks.knownPacks()`.
    pub fn known_packs(&self) -> &[KnownPack] {
        &self.known_packs
    }

    /// `ServerboundSelectKnownPacks.STREAM_CODEC` — `KnownPack.STREAM_CODEC
    /// .apply(ByteBufCodecs.list(64))`.
    ///
    /// The decode side rejects a count above 64 with Java's `DecoderException`
    /// message (`"{count} elements exceeded max size of: 64"`); a negative
    /// count panics with `ArrayList(int)`'s `IllegalArgumentException` (the
    /// list constructor), exactly Java's composition.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundSelectKnownPacks> {
        let list: CodecOperation<FriendlyByteBuf, KnownPack, Vec<KnownPack>> = list_max(64);
        map(
            apply(known_pack_stream_codec(), list),
            |v: &Vec<KnownPack>| ServerboundSelectKnownPacks::new(v.clone()),
            |p: &ServerboundSelectKnownPacks| p.known_packs.clone(),
        )
    }
}

impl Packet for ServerboundSelectKnownPacks {
    fn packet_type(&self) -> PacketType {
        serverbound_select_known_packs()
    }
}
