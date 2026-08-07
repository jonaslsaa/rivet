//! Port of `net.minecraft.network.protocol.game.ServerboundChunkBatchReceivedPacket`
//! (issue #94).
//!
//! Java: `ServerboundChunkBatchReceivedPacket.java` in `working/Paper`. The
//! client's chunk-batch acknowledgement: the `desiredChunksPerTick` float it
//! wants the server to target (raw-bits round trip like every float codec).

use crate::codec::{StreamCodec, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::serverbound_chunk_batch_received;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ServerboundChunkBatchReceivedPacket` — `desiredChunksPerTick` (float).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServerboundChunkBatchReceivedPacket {
    desired_chunks_per_tick: f32,
}

impl ServerboundChunkBatchReceivedPacket {
    /// `new ServerboundChunkBatchReceivedPacket(float desiredChunksPerTick)`.
    pub fn new(desired_chunks_per_tick: f32) -> Self {
        ServerboundChunkBatchReceivedPacket {
            desired_chunks_per_tick,
        }
    }

    /// `ServerboundChunkBatchReceivedPacket.desiredChunksPerTick()`.
    pub fn desired_chunks_per_tick(&self) -> f32 {
        self.desired_chunks_per_tick
    }

    /// `STREAM_CODEC` — a single float.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundChunkBatchReceivedPacket> {
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
}

impl Packet for ServerboundChunkBatchReceivedPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_chunk_batch_received()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn float_desired_chunks_round_trips() {
        let packet = ServerboundChunkBatchReceivedPacket::new(4.0);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundChunkBatchReceivedPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes, 4.0f32.to_be_bytes().to_vec());
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ServerboundChunkBatchReceivedPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, packet);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn nan_payload_bits_pass_through() {
        // `writeFloat` uses `floatToRawIntBits`; a NaN payload round-trips raw.
        let nan = f32::from_bits(0x7fc0_1234);
        let packet = ServerboundChunkBatchReceivedPacket::new(nan);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundChunkBatchReceivedPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes, 0x7fc0_1234u32.to_be_bytes().to_vec());
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ServerboundChunkBatchReceivedPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded.desired_chunks_per_tick().to_bits(), 0x7fc0_1234u32);
    }
}
