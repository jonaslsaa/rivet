//! Port of `net.minecraft.network.protocol.game.ServerboundAcceptTeleportationPacket`
//! (MC 26.2).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/network/
//! protocol/game/ServerboundAcceptTeleportationPacket.java`. Wire body is a
//! single VarInt (`int id`). `handle` is a documented STUB; the id-matching
//! server handling (`awaitingTeleport`) is #158/M3.

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ServerboundAcceptTeleportationPacket` — the teleport ack id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerboundAcceptTeleportationPacket {
    pub id: i32,
}

impl ServerboundAcceptTeleportationPacket {
    /// `new ServerboundAcceptTeleportationPacket(int id)`.
    pub fn new(id: i32) -> Self {
        ServerboundAcceptTeleportationPacket { id }
    }

    /// `getId()`.
    pub fn get_id(&self) -> i32 {
        self.id
    }
}

impl Packet for ServerboundAcceptTeleportationPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::serverbound("accept_teleportation")
    }
}

/// `STREAM_CODEC` — `Packet.codec(ServerboundAcceptTeleportationPacket::write,
/// ServerboundAcceptTeleportationPacket::new)` over `VarInt`.
pub fn accept_teleportation_codec()
-> StreamCodec<FriendlyByteBuf, ServerboundAcceptTeleportationPacket> {
    codec(
        |value: &ServerboundAcceptTeleportationPacket, output: &mut FriendlyByteBuf| {
            output.write_var_int(value.id);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            Ok(ServerboundAcceptTeleportationPacket {
                id: input.read_var_int(),
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    #[test]
    fn round_trips_exact_bytes() {
        let codec = accept_teleportation_codec();
        // id 0 -> a single 0x00 byte.
        let mut out = buf();
        codec
            .encode(&mut out, &ServerboundAcceptTeleportationPacket::new(0))
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), vec![0x00]);

        // id 300 -> two-byte varint 0xAC 0x02.
        let mut out = buf();
        codec
            .encode(&mut out, &ServerboundAcceptTeleportationPacket::new(300))
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), vec![0xAC, 0x02]);

        // decode round-trip.
        let mut out = buf();
        codec
            .encode(&mut out, &ServerboundAcceptTeleportationPacket::new(-1))
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let decoded = codec.decode(&mut input).unwrap();
        assert_eq!(decoded, ServerboundAcceptTeleportationPacket::new(-1));
        assert_eq!(decoded.get_id(), -1);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn packet_type_is_accept_teleportation() {
        assert_eq!(
            ServerboundAcceptTeleportationPacket::new(0).packet_type(),
            PacketType::serverbound("accept_teleportation")
        );
    }
}
