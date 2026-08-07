//! Port of `net.minecraft.network.protocol.game.ServerboundChunkBatchReceivedPacket`
//! (MC 26.2).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/network/
//! protocol/game/ServerboundChunkBatchReceivedPacket.java`. A record with a
//! single `float desiredChunksPerTick`; wire body is one big-endian float.
//! `handle` is a documented STUB (the `chunkSender.onChunkBatchReceivedByClient`
//! handling is server-side).

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ServerboundChunkBatchReceivedPacket` — the chunk-batch rate ack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerboundChunkBatchReceivedPacket {
    pub desired_chunks_per_tick: f32,
}

impl ServerboundChunkBatchReceivedPacket {
    /// `new ServerboundChunkBatchReceivedPacket(float desiredChunksPerTick)`.
    pub fn new(desired_chunks_per_tick: f32) -> Self {
        ServerboundChunkBatchReceivedPacket {
            desired_chunks_per_tick,
        }
    }

    /// `desiredChunksPerTick()` — the record accessor.
    pub fn desired_chunks_per_tick(&self) -> f32 {
        self.desired_chunks_per_tick
    }
}

impl Packet for ServerboundChunkBatchReceivedPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::serverbound("chunk_batch_received")
    }
}

/// `STREAM_CODEC` — the record codec over one big-endian float.
pub fn chunk_batch_received_codec()
-> StreamCodec<FriendlyByteBuf, ServerboundChunkBatchReceivedPacket> {
    codec(
        |value: &ServerboundChunkBatchReceivedPacket, output: &mut FriendlyByteBuf| {
            output.write_float(value.desired_chunks_per_tick);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            Ok(ServerboundChunkBatchReceivedPacket {
                desired_chunks_per_tick: input.read_float(),
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
        let codec = chunk_batch_received_codec();
        let mut out = buf();
        codec
            .encode(&mut out, &ServerboundChunkBatchReceivedPacket::new(3.5))
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), 3.5f32.to_be_bytes().to_vec());

        let mut out = buf();
        codec
            .encode(&mut out, &ServerboundChunkBatchReceivedPacket::new(-1.0))
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let decoded = codec.decode(&mut input).unwrap();
        assert_eq!(decoded, ServerboundChunkBatchReceivedPacket::new(-1.0));
        assert_eq!(decoded.desired_chunks_per_tick(), -1.0);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn nan_payload_round_trips_raw_bits() {
        let nan = f32::from_bits(0x7fc0_1234);
        let codec = chunk_batch_received_codec();
        let mut out = buf();
        codec
            .encode(&mut out, &ServerboundChunkBatchReceivedPacket::new(nan))
            .unwrap();
        assert_eq!(
            out.into_inner().to_vec(),
            0x7fc0_1234u32.to_be_bytes().to_vec()
        );
        let mut out = buf();
        codec
            .encode(&mut out, &ServerboundChunkBatchReceivedPacket::new(nan))
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let decoded = codec.decode(&mut input).unwrap();
        assert_eq!(decoded.desired_chunks_per_tick().to_bits(), 0x7fc0_1234u32);
    }
}
