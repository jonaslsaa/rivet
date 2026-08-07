//! Port of `net.minecraft.network.protocol.game.ClientboundRemoveEntitiesPacket`
//! (MC 26.2) — `remove_entities` (play clientbound id 77).
//!
//! Blocked (see the codec marker below): this packet never occurs in the #153
//! single-player join fixture (no other entities spawn), so its codec cannot be
//! validated byte-for-byte against the capture — #90 blocks non-join entity
//! packets with a blocked note. The wire body is simple enough to port now
//! (`readIntIdList` = VarInt count then count×VarInt ids, no cap) but the DoD
//! demands capture-proven bodies, so the codec is NOT implemented until a
//! fixture proves it.
//!
//! Java source: `.../network/protocol/game/ClientboundRemoveEntitiesPacket.java`.
//! `handle` is a documented STUB like the serverbound slice.

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ClientboundRemoveEntitiesPacket` — the entity id list. STUB (blocked note
/// above); the struct shape is declared for id stability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientboundRemoveEntitiesPacket {
    /// The entity ids (wire: `readIntIdList`).
    pub entity_ids: Vec<i32>,
}

impl ClientboundRemoveEntitiesPacket {
    /// `new ClientboundRemoveEntitiesPacket(IntList ids)`.
    pub fn new(entity_ids: Vec<i32>) -> Self {
        ClientboundRemoveEntitiesPacket { entity_ids }
    }
}

impl Packet for ClientboundRemoveEntitiesPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("remove_entities")
    }
}

/// STUB(mc.network.protocol.game): no capture proves this body yet — the codec
/// is blocked (see the module doc).
pub fn remove_entities_codec() -> StreamCodec<FriendlyByteBuf, ClientboundRemoveEntitiesPacket> {
    codec(
        |_value, _output| {
            panic!("blocked: remove_entities codec not ported (#90; no join fixture)")
        },
        |_input| panic!("blocked: remove_entities codec not ported (#90; no join fixture)"),
    )
}
