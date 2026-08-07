//! Port of `net.minecraft.network.protocol.game.ClientboundEntityEventPacket`
//! (MC 26.2) — `entity_event` (play clientbound id 34).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! network/protocol/game/ClientboundEntityEventPacket.java`. Wire body is a
//! **4-byte big-endian int** entity id (NOT a VarInt — this differs from every
//! other entity packet) then a **byte** event id. The buffer is `FriendlyByteBuf`
//! in Java; `handle` (listener dispatch) is a documented STUB like the
//! serverbound slice.

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ClientboundEntityEventPacket` — `(entityId int32, eventId byte)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientboundEntityEventPacket {
    pub entity_id: i32,
    pub event_id: i8,
}

impl ClientboundEntityEventPacket {
    /// `new ClientboundEntityEventPacket(Entity, byte eventId)` — the entity
    /// constructor (entity id taken from the entity).
    pub fn new(entity_id: i32, event_id: i8) -> Self {
        ClientboundEntityEventPacket {
            entity_id,
            event_id,
        }
    }

    /// `getEventId()`.
    pub fn get_event_id(&self) -> i8 {
        self.event_id
    }
}

impl Packet for ClientboundEntityEventPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("entity_event")
    }
}

/// `STREAM_CODEC` — `Packet.codec(write, new)` over `FriendlyByteBuf`:
/// `writeInt(entityId) + writeByte(eventId)`.
pub fn entity_event_codec() -> StreamCodec<FriendlyByteBuf, ClientboundEntityEventPacket> {
    codec(
        |value: &ClientboundEntityEventPacket, output: &mut FriendlyByteBuf| {
            output.write_int(value.entity_id);
            output.write_byte(value.event_id);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            Ok(ClientboundEntityEventPacket {
                entity_id: input.read_int(),
                event_id: input.read_byte(),
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
        let codec = entity_event_codec();
        let mut out = buf();
        codec
            .encode(&mut out, &ClientboundEntityEventPacket::new(1, 0))
            .unwrap();
        // writeInt(1) = 00 00 00 01, writeByte(0) = 00. Entity id is NOT a VarInt.
        assert_eq!(
            out.into_inner().to_vec(),
            vec![0x00, 0x00, 0x00, 0x01, 0x00]
        );

        let mut out = buf();
        codec
            .encode(&mut out, &ClientboundEntityEventPacket::new(300, -1))
            .unwrap();
        assert_eq!(
            out.into_inner().to_vec(),
            vec![0x00, 0x00, 0x01, 0x2C, 0xFF]
        );

        let mut out = buf();
        codec
            .encode(&mut out, &ClientboundEntityEventPacket::new(-1, 127))
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            codec.decode(&mut input).unwrap(),
            ClientboundEntityEventPacket::new(-1, 127)
        );
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn packet_type_is_entity_event() {
        assert_eq!(
            ClientboundEntityEventPacket::new(1, 0).packet_type(),
            PacketType::clientbound("entity_event")
        );
    }
}
