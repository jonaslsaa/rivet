//! Port of `net.minecraft.network.protocol.game.ClientboundSetHeldSlotPacket`
//! (issue #87) — `set_held_slot` (play clientbound id 105).
//!
//! Java source: `.../network/protocol/game/ClientboundSetHeldSlotPacket.java`.
//! Wire body: a single VarInt slot (`ByteBufCodecs.VAR_INT`). The captured
//! golden body is `00` (slot 0).

use crate::codec::byte_buf_codecs::var_int;
use crate::codec::{StreamCodec, map};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_set_held_slot;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundSetHeldSlotPacket` — the record `(int slot)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundSetHeldSlotPacket {
    /// `slot`.
    slot: i32,
}

impl ClientboundSetHeldSlotPacket {
    /// The record's canonical constructor.
    pub fn new(slot: i32) -> Self {
        ClientboundSetHeldSlotPacket { slot }
    }

    /// `ClientboundSetHeldSlotPacket.slot()`.
    pub fn slot(&self) -> i32 {
        self.slot
    }

    /// `STREAM_CODEC` — a single VarInt.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundSetHeldSlotPacket> {
        map(
            var_int(),
            |slot: &i32| ClientboundSetHeldSlotPacket::new(*slot),
            |packet: &ClientboundSetHeldSlotPacket| packet.slot,
        )
    }
}

impl Packet for ClientboundSetHeldSlotPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_set_held_slot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn var_int_slot_round_trips() {
        let packet = ClientboundSetHeldSlotPacket::new(0);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundSetHeldSlotPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        assert_eq!(out.as_slice().to_vec(), vec![0x00]);
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ClientboundSetHeldSlotPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
        assert_eq!(input.readable_bytes(), 0);
    }
}
