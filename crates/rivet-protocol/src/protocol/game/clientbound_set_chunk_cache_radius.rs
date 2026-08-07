//! Port of `net.minecraft.network.protocol.game.ClientboundSetChunkCacheRadiusPacket`
//! (MC 26.2) — `set_chunk_cache_radius` (play clientbound id 95).
//!
//! Java source: `.../network/protocol/game/ClientboundSetChunkCacheRadiusPacket.java`.
//! Wire body: `radius` VarInt. The Moonrise chunk-loader `add` sends this FIRST
//! of the three cache packets so the client's chunk-cache radius is bounded
//! before any chunk arrives; the captured join body is `04` — radius 4 (the
//! `view-distance=4` fixture).

use crate::codec::byte_buf_codecs::var_int;
use crate::codec::{StreamCodec, map};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_set_chunk_cache_radius;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundSetChunkCacheRadiusPacket` — the record `(int radius)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundSetChunkCacheRadiusPacket {
    /// `radius`.
    radius: i32,
}

impl ClientboundSetChunkCacheRadiusPacket {
    /// The record's canonical constructor.
    pub fn new(radius: i32) -> Self {
        ClientboundSetChunkCacheRadiusPacket { radius }
    }

    /// `ClientboundSetChunkCacheRadiusPacket.getRadius()`.
    pub fn radius(&self) -> i32 {
        self.radius
    }

    /// `STREAM_CODEC` — `writeVarInt(radius)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundSetChunkCacheRadiusPacket> {
        map(
            var_int(),
            |radius| ClientboundSetChunkCacheRadiusPacket::new(*radius),
            ClientboundSetChunkCacheRadiusPacket::radius,
        )
    }
}

impl Packet for ClientboundSetChunkCacheRadiusPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_set_chunk_cache_radius()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn captured_golden_body_round_trips() {
        // Capture: `04` — radius 4 (the `view-distance=4` fixture).
        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![0x04].as_slice()));
        let decoded = ClientboundSetChunkCacheRadiusPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, ClientboundSetChunkCacheRadiusPacket::new(4));
        assert_eq!(input.readable_bytes(), 0);

        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundSetChunkCacheRadiusPacket::stream_codec()
            .encode(&mut out, &decoded)
            .unwrap();
        assert_eq!(out.as_slice().to_vec(), vec![0x04]);
    }
}
