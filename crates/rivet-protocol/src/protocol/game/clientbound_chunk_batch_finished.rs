//! Port of `net.minecraft.network.protocol.game.ClientboundChunkBatchFinishedPacket`
//! (issue #94).
//!
//! Java: `ClientboundChunkBatchFinishedPacket.java` in `working/Paper`. A record
//! carrying the number of chunks in the batch that just finished, as a VarInt.

use crate::codec::{StreamCodec, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_chunk_batch_finished;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundChunkBatchFinishedPacket` — `batchSize` (VarInt).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundChunkBatchFinishedPacket {
    batch_size: i32,
}

impl ClientboundChunkBatchFinishedPacket {
    /// `new ClientboundChunkBatchFinishedPacket(int batchSize)`.
    pub fn new(batch_size: i32) -> Self {
        ClientboundChunkBatchFinishedPacket { batch_size }
    }

    /// `ClientboundChunkBatchFinishedPacket.batchSize()`.
    pub fn batch_size(&self) -> i32 {
        self.batch_size
    }

    /// `STREAM_CODEC` — a single VarInt.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundChunkBatchFinishedPacket> {
        codec(
            |value: &ClientboundChunkBatchFinishedPacket, output: &mut FriendlyByteBuf| {
                output.write_var_int(value.batch_size);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                Ok(ClientboundChunkBatchFinishedPacket {
                    batch_size: input.read_var_int(),
                })
            },
        )
    }
}

impl Packet for ClientboundChunkBatchFinishedPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_chunk_batch_finished()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn var_int_batch_size_round_trips() {
        let packet = ClientboundChunkBatchFinishedPacket::new(117);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundChunkBatchFinishedPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes, vec![0x75]); // 117 as a varint
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ClientboundChunkBatchFinishedPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, packet);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn negative_batch_size_round_trips_as_wrapped_var_int() {
        // Java `writeVarInt(int)` wraps; -1 is the two's-complement 32-bit
        // varint 0xffffffff -> 5 bytes.
        let packet = ClientboundChunkBatchFinishedPacket::new(-1);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundChunkBatchFinishedPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes, vec![0xff, 0xff, 0xff, 0xff, 0x0f]);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ClientboundChunkBatchFinishedPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, packet);
    }
}
