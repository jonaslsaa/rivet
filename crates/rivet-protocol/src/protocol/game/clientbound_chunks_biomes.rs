//! Port of `net.minecraft.network.protocol.game.ClientboundChunksBiomesPacket`
//! (issue #94).
//!
//! Java: `ClientboundChunksBiomesPacket.java` in `working/Paper`. A record
//! holding one `ChunkBiomeData` per chunk: a `ChunkPos` (packed long, big-endian)
//! plus an **opaque** per-section biome buffer (`LevelChunkSection.getBiomes()
//! .getSerializedSize()` bytes). The buffer is read through
//! `input.readByteArray(TWO_MEGABYTES)`, whose `DecoderException`
//! (`"ByteArray with size N is bigger than allowed 2097152"`) over the cap is
//! surfaced as `Err` at the codec boundary via the `byte_array_max` codec (the
//! same `TWO_MEGABYTES` value as the chunk packet's sections buffer — note that
//! *this* packet's message differs from the chunk packet's
//! `"Chunk Packet trying to allocate too much memory on read."` RuntimeException,
//! which lives on `ClientboundLevelChunkPacketData`).

use crate::codec::byte_buf_codecs::{MAX_INITIAL_COLLECTION_SIZE, byte_array_max};
use crate::codec::{CodecError, StreamCodec, StreamDecoder, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_chunks_biomes;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_registry::core::ChunkPos;

/// `ClientboundChunksBiomesPacket.TWO_MEGABYTES` — the decode guard on each
/// per-chunk biome buffer (the same value as `ClientboundLevelChunkPacketData`'s
/// sections buffer).
pub const TWO_MEGABYTES: i32 = 2097152;

/// `ClientboundChunksBiomesPacket.ChunkBiomeData` — a chunk position plus its
/// opaque per-section biome bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkBiomeData {
    pos: ChunkPos,
    buffer: Vec<u8>,
}

impl ChunkBiomeData {
    /// `new ChunkBiomeData(ChunkPos, byte[])` — the packet-body value
    /// constructor.
    pub fn new(pos: ChunkPos, buffer: Vec<u8>) -> Self {
        ChunkBiomeData { pos, buffer }
    }

    /// `ChunkBiomeData.pos()`.
    pub fn pos(&self) -> ChunkPos {
        self.pos
    }

    /// `ChunkBiomeData.buffer()` — the opaque biome buffer.
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// `ChunkBiomeData(FriendlyByteBuf)` — the decode ctor.
    fn read(input: &mut FriendlyByteBuf) -> Result<ChunkBiomeData, CodecError> {
        let pos = input.read_chunk_pos();
        // `readByteArray(TWO_MEGABYTES)`: a `DecoderException` over the cap is
        // `Err`; a negative size hits `new byte[-N]` (Java
        // `NegativeArraySizeException`, message is the size — a panic); a short
        // read surfaces as `Err` (netty `IndexOutOfBounds` from `readBytes`).
        let buffer = byte_array_max(TWO_MEGABYTES).decode(input)?;
        Ok(ChunkBiomeData { pos, buffer })
    }

    /// `ChunkBiomeData.write(FriendlyByteBuf)` — `writeChunkPos(pos)` then
    /// `writeByteArray(buffer)`.
    fn write(&self, output: &mut FriendlyByteBuf) -> Result<(), CodecError> {
        output.write_chunk_pos(&self.pos);
        output.write_var_int(self.buffer.len() as i32);
        output.write_bytes(&self.buffer);
        Ok(())
    }
}

/// `ClientboundChunksBiomesPacket` — the biome-update list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundChunksBiomesPacket {
    chunk_biome_data: Vec<ChunkBiomeData>,
}

impl ClientboundChunksBiomesPacket {
    /// `new ClientboundChunksBiomesPacket(List<ChunkBiomeData>)`.
    pub fn new(chunk_biome_data: Vec<ChunkBiomeData>) -> Self {
        ClientboundChunksBiomesPacket { chunk_biome_data }
    }

    /// `ClientboundChunksBiomesPacket.chunkBiomeData()`.
    pub fn chunk_biome_data(&self) -> &[ChunkBiomeData] {
        &self.chunk_biome_data
    }

    /// `STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundChunksBiomesPacket> {
        codec(
            |value: &ClientboundChunksBiomesPacket, output: &mut FriendlyByteBuf| {
                // `writeCollection(this.chunkBiomeData, (o, c) -> c.write(o))`.
                output.write_var_int(value.chunk_biome_data.len() as i32);
                for data in &value.chunk_biome_data {
                    data.write(output)?;
                }
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                // `readList(ChunkBiomeData::new)` — no cap, but Guava's
                // `newArrayListWithCapacity` rejects a negative count with
                // `IllegalArgumentException`.
                let count = input.read_var_int();
                if count < 0 {
                    panic!("initialArraySize cannot be negative but was: {count}");
                }
                let mut chunk_biome_data =
                    Vec::with_capacity((count as usize).min(MAX_INITIAL_COLLECTION_SIZE as usize));
                for _ in 0..count {
                    chunk_biome_data.push(ChunkBiomeData::read(input)?);
                }
                Ok(ClientboundChunksBiomesPacket { chunk_biome_data })
            },
        )
    }
}

impl Packet for ClientboundChunksBiomesPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_chunks_biomes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn chunk_pos_then_buffer_round_trips() {
        // ChunkPos -5/-4 packs to `(x & mask) | ((z & mask) << 32)`.
        let pos = ChunkPos::new(-5, -4);
        let packet = ClientboundChunksBiomesPacket::new(vec![ChunkBiomeData::new(
            pos,
            vec![0xDE, 0xAD, 0xBE, 0xEF],
        )]);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundChunksBiomesPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        // count 1, then the packed long BE, then size 4, then the bytes.
        assert_eq!(bytes[0], 1);
        assert_eq!(&bytes[1..9], &pos.pack().to_be_bytes());
        assert_eq!(bytes[9], 4);
        assert_eq!(&bytes[10..14], &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(bytes.len(), 14);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ClientboundChunksBiomesPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, packet);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn oversize_biome_buffer_errors_with_byte_array_message() {
        // A per-chunk buffer over TWO_MEGABYTES -> `Err` with
        // `readByteArray(TWO_MEGABYTES)`'s `DecoderException` message (the
        // `"Chunk Packet trying to allocate too much memory on read."`
        // RuntimeException belongs to `ClientboundLevelChunkPacketData` only).
        let mut input = FriendlyByteBuf::new(BytesMut::new());
        input.write_var_int(1); // one chunk
        input.write_chunk_pos(&ChunkPos::new(0, 0));
        input.write_var_int(TWO_MEGABYTES + 1);
        let err = ClientboundChunksBiomesPacket::stream_codec()
            .decode(&mut input)
            .unwrap_err();
        assert_eq!(
            err.message,
            format!(
                "ByteArray with size {} is bigger than allowed {TWO_MEGABYTES}",
                TWO_MEGABYTES + 1
            )
        );
    }

    #[test]
    fn negative_chunk_list_count_panics_like_guava() {
        // `readList` -> `Lists.newArrayListWithCapacity(-1)` ->
        // `IllegalArgumentException("initialArraySize cannot be negative but
        // was: -1")`, raised at the ctor before any element read.
        let mut input = FriendlyByteBuf::new(BytesMut::new());
        input.write_var_int(-1);
        let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ClientboundChunksBiomesPacket::stream_codec().decode(&mut input);
        }))
        .unwrap_err();
        assert_eq!(
            msg.downcast_ref::<String>().unwrap(),
            "initialArraySize cannot be negative but was: -1"
        );
    }
}
