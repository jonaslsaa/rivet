//! Port of `net.minecraft.network.protocol.game.ClientboundSetChunkCacheCenterPacket`
//! (MC 26.2) — `set_chunk_cache_center` (play clientbound id 94).
//!
//! Java source: `.../network/protocol/game/ClientboundSetChunkCacheCenterPacket.java`.
//! Wire body: `x` VarInt then `z` VarInt. The Moonrise chunk-loader `add` sends
//! this last of the three cache packets (after radius + simulation distance) so
//! the client's cache is sized before the center moves; the captured join body
//! is `0000` — center `(0, 0)`, the superflat spawn chunk.

use crate::codec::byte_buf_codecs::var_int;
use crate::codec::{StreamCodec, composite_2};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_set_chunk_cache_center;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundSetChunkCacheCenterPacket` — the record `(int x, int z)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundSetChunkCacheCenterPacket {
    /// `x`.
    x: i32,
    /// `z`.
    z: i32,
}

impl ClientboundSetChunkCacheCenterPacket {
    /// The record's canonical constructor.
    pub fn new(x: i32, z: i32) -> Self {
        ClientboundSetChunkCacheCenterPacket { x, z }
    }

    /// `ClientboundSetChunkCacheCenterPacket.getX()`.
    pub fn x(&self) -> i32 {
        self.x
    }

    /// `ClientboundSetChunkCacheCenterPacket.getZ()`.
    pub fn z(&self) -> i32 {
        self.z
    }

    /// `STREAM_CODEC` — `writeVarInt(x)`, `writeVarInt(z)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundSetChunkCacheCenterPacket> {
        composite_2(
            var_int(),
            ClientboundSetChunkCacheCenterPacket::x,
            var_int(),
            ClientboundSetChunkCacheCenterPacket::z,
            ClientboundSetChunkCacheCenterPacket::new,
        )
    }
}

impl Packet for ClientboundSetChunkCacheCenterPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_set_chunk_cache_center()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn captured_golden_body_round_trips() {
        // Capture: `0000` — center (0, 0) (the superflat spawn chunk).
        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![0x00, 0x00].as_slice()));
        let decoded = ClientboundSetChunkCacheCenterPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, ClientboundSetChunkCacheCenterPacket::new(0, 0));
        assert_eq!(input.readable_bytes(), 0);

        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundSetChunkCacheCenterPacket::stream_codec()
            .encode(&mut out, &decoded)
            .unwrap();
        assert_eq!(out.as_slice().to_vec(), vec![0x00, 0x00]);
    }

    #[test]
    fn varint_coordinates_round_trip() {
        // VarInts, not BE ints: a coordinate outside the 7-bit range still
        // round trips (e.g. the ±5 view edge).
        let packet = ClientboundSetChunkCacheCenterPacket::new(-5, 5);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundSetChunkCacheCenterPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        // -5 is the 5-byte VarInt 0xFB 0xFF 0xFF 0xFF 0x0F, 5 is 0x05.
        assert_eq!(bytes, vec![0xFB, 0xFF, 0xFF, 0xFF, 0x0F, 0x05]);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ClientboundSetChunkCacheCenterPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, packet);
        assert_eq!(input.readable_bytes(), 0);
    }
}
