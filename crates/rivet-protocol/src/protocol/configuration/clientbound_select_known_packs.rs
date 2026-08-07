//! Port of `net.minecraft.network.protocol.configuration.ClientboundSelectKnownPacks`
//! (issue #109).
//!
//! Java: `ClientboundSelectKnownPacks.java` in `working/Paper`. The server's
//! list of `KnownPack`s the client should select from — the opening packet of
//! the configuration registry sync (`SynchronizeRegistriesTask.start`). The
//! list is unbounded (`ByteBufCodecs.list()`).

use crate::codec::byte_buf_codecs::list;
use crate::codec::{CodecOperation, StreamCodec, apply, map};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::configuration::packet_types::clientbound_select_known_packs;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::stream_codecs::known_pack_stream_codec;
use rivet_util::KnownPack;

/// `net.minecraft.network.protocol.configuration.ClientboundSelectKnownPacks` —
/// the record `(List<KnownPack> knownPacks)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundSelectKnownPacks {
    known_packs: Vec<KnownPack>,
}

impl ClientboundSelectKnownPacks {
    /// `new ClientboundSelectKnownPacks(List<KnownPack> knownPacks)`.
    pub fn new(known_packs: Vec<KnownPack>) -> Self {
        ClientboundSelectKnownPacks { known_packs }
    }

    /// `ClientboundSelectKnownPacks.knownPacks()`.
    pub fn known_packs(&self) -> &[KnownPack] {
        &self.known_packs
    }

    /// `ClientboundSelectKnownPacks.STREAM_CODEC` — `KnownPack.STREAM_CODEC
    /// .apply(ByteBufCodecs.list())` (unbounded).
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundSelectKnownPacks> {
        let list: CodecOperation<FriendlyByteBuf, KnownPack, Vec<KnownPack>> = list();
        map(
            apply(known_pack_stream_codec(), list),
            |v: &Vec<KnownPack>| ClientboundSelectKnownPacks::new(v.clone()),
            |p: &ClientboundSelectKnownPacks| p.known_packs.clone(),
        )
    }
}

impl Packet for ClientboundSelectKnownPacks {
    fn packet_type(&self) -> PacketType {
        clientbound_select_known_packs()
    }
}
