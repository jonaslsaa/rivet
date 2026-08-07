//! Port of `net.minecraft.network.protocol.game.ClientboundLightUpdatePacketData`
//! (issue #94).
//!
//! Java: `ClientboundLightUpdatePacketData.java` in `working/Paper`. The shared
//! light payload of `ClientboundLevelChunkWithLightPacket` and
//! `ClientboundLightUpdatePacket` (the latter alone emits it): four section
//! masks plus two layer lists.
//!
//! The masks are `BitSet`s on the wire: `writeBitSet` writes the long array via
//! `BitSet.toLongArray()` (each mask is at most 19 longs — one bit per light
//! section), so each is a `Vec<u64>` of words. A **layer** is exactly 2048 bytes
//! (`DataLayer.SIZE`, a 16×16×16 byte array); each entry is decoded through
//! `ByteBufCodecs.byteArray(2048)`.
//!
//! Wire order (verified against the PR #194 fixture):
//! 1. `skyYMask` — `writeLongArray(BitSet.toLongArray())`
//! 2. `blockYMask`
//! 3. `emptySkyYMask`
//! 4. `emptyBlockYMask`
//! 5. `skyUpdates` — `[VarInt count][count × (VarInt 2048 + bytes)]`
//! 6. `blockUpdates` — same
//!
//! There is no trailing `trustEdges` byte in 26.2.

use crate::codec::byte_buf_codecs::MAX_INITIAL_COLLECTION_SIZE;
use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;

/// `DataLayer.SIZE` — the fixed byte size of one light layer (2048 = 16³).
pub const DATA_LAYER_SIZE: i32 = 2048;

/// `ClientboundLightUpdatePacketData` — the shared light payload value type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightUpdatePacketData {
    sky_y_mask: Vec<u64>,
    block_y_mask: Vec<u64>,
    empty_sky_y_mask: Vec<u64>,
    empty_block_y_mask: Vec<u64>,
    sky_updates: Vec<Vec<u8>>,
    block_updates: Vec<Vec<u8>>,
}

impl LightUpdatePacketData {
    /// `new ClientboundLightUpdatePacketData(BitSet, BitSet, BitSet, BitSet,
    /// List, List)` — the packet-body value constructor.
    pub fn new(
        sky_y_mask: Vec<u64>,
        block_y_mask: Vec<u64>,
        empty_sky_y_mask: Vec<u64>,
        empty_block_y_mask: Vec<u64>,
        sky_updates: Vec<Vec<u8>>,
        block_updates: Vec<Vec<u8>>,
    ) -> Self {
        LightUpdatePacketData {
            sky_y_mask,
            block_y_mask,
            empty_sky_y_mask,
            empty_block_y_mask,
            sky_updates,
            block_updates,
        }
    }

    /// `getSkyYMask()`.
    pub fn sky_y_mask(&self) -> &[u64] {
        &self.sky_y_mask
    }

    /// `getBlockYMask()`.
    pub fn block_y_mask(&self) -> &[u64] {
        &self.block_y_mask
    }

    /// `getEmptySkyYMask()`.
    pub fn empty_sky_y_mask(&self) -> &[u64] {
        &self.empty_sky_y_mask
    }

    /// `getEmptyBlockYMask()`.
    pub fn empty_block_y_mask(&self) -> &[u64] {
        &self.empty_block_y_mask
    }

    /// `getSkyUpdates()`.
    pub fn sky_updates(&self) -> &[Vec<u8>] {
        &self.sky_updates
    }

    /// `getBlockUpdates()`.
    pub fn block_updates(&self) -> &[Vec<u8>] {
        &self.block_updates
    }

    /// `STREAM_CODEC` — over [`FriendlyByteBuf`]: the light payload needs no
    /// registry (both `ClientboundLightUpdatePacket`'s and the chunk packet's
    /// halves are registry-independent).
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, LightUpdatePacketData> {
        let layer_encoder = crate::codec::byte_buf_codecs::byte_array_max(DATA_LAYER_SIZE);
        let layer_decoder = crate::codec::byte_buf_codecs::byte_array_max(DATA_LAYER_SIZE);
        codec(
            move |value: &LightUpdatePacketData, output: &mut FriendlyByteBuf| {
                output.write_bit_set(&value.sky_y_mask);
                output.write_bit_set(&value.block_y_mask);
                output.write_bit_set(&value.empty_sky_y_mask);
                output.write_bit_set(&value.empty_block_y_mask);
                output.write_var_int(value.sky_updates.len() as i32);
                for layer in &value.sky_updates {
                    layer_encoder.encode(output, layer)?;
                }
                output.write_var_int(value.block_updates.len() as i32);
                for layer in &value.block_updates {
                    layer_encoder.encode(output, layer)?;
                }
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                let sky_y_mask = input.read_bit_set();
                let block_y_mask = input.read_bit_set();
                let empty_sky_y_mask = input.read_bit_set();
                let empty_block_y_mask = input.read_bit_set();
                // `readList` -> `Lists.newArrayListWithCapacity` (Guava
                // `IllegalArgumentException` on a negative count).
                let sky_count = input.read_var_int();
                if sky_count < 0 {
                    panic!("initialArraySize cannot be negative but was: {sky_count}");
                }
                let mut sky_updates = Vec::with_capacity(
                    (sky_count as usize).min(MAX_INITIAL_COLLECTION_SIZE as usize),
                );
                for _ in 0..sky_count {
                    sky_updates.push(layer_decoder.decode(input)?);
                }
                let block_count = input.read_var_int();
                if block_count < 0 {
                    panic!("initialArraySize cannot be negative but was: {block_count}");
                }
                let mut block_updates = Vec::with_capacity(
                    (block_count as usize).min(MAX_INITIAL_COLLECTION_SIZE as usize),
                );
                for _ in 0..block_count {
                    block_updates.push(layer_decoder.decode(input)?);
                }
                Ok(LightUpdatePacketData {
                    sky_y_mask,
                    block_y_mask,
                    empty_sky_y_mask,
                    empty_block_y_mask,
                    sky_updates,
                    block_updates,
                })
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::StreamDecoder;
    use bytes::BytesMut;

    #[test]
    fn empty_payload_round_trips_to_six_bytes() {
        // All masks empty, no layers: 4 masks (each `[0]` count) + 2 empty lists.
        let data = LightUpdatePacketData::new(vec![], vec![], vec![], vec![], vec![], vec![]);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        LightUpdatePacketData::stream_codec()
            .encode(&mut out, &data)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes, vec![0, 0, 0, 0, 0, 0]);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = LightUpdatePacketData::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, data);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn a_layer_longer_than_2048_errors_on_encode() {
        let mut data = LightUpdatePacketData::new(
            vec![0x06],
            vec![],
            vec![0x01],
            vec![0x07],
            vec![vec![0u8; 2048]],
            vec![],
        );
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        // A 2049-byte layer must fail the 2048 cap on encode.
        data.sky_updates[0] = vec![0u8; 2049];
        let err = LightUpdatePacketData::stream_codec()
            .encode(&mut out, &data)
            .unwrap_err();
        assert_eq!(
            err.message,
            "ByteArray with size 2049 is bigger than allowed 2048"
        );
    }

    #[test]
    fn negative_layer_count_panics_like_guava() {
        // A hostile `sky_count` of -1 -> `readList` ->
        // `Lists.newArrayListWithCapacity(-1)` -> `IllegalArgumentException`,
        // NOT silently accepted as empty. Wire: 4 empty masks then the count.
        let mut input = FriendlyByteBuf::new(BytesMut::new());
        input.write_var_int(0);
        input.write_var_int(0);
        input.write_var_int(0);
        input.write_var_int(0);
        input.write_var_int(-1);
        let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = LightUpdatePacketData::stream_codec().decode(&mut input);
        }))
        .unwrap_err();
        assert_eq!(
            msg.downcast_ref::<String>().unwrap(),
            "initialArraySize cannot be negative but was: -1"
        );
    }
}
