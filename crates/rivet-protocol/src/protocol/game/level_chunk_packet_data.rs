//! Port of `net.minecraft.network.protocol.game.ClientboundLevelChunkPacketData`
//! (issue #94).
//!
//! Java: `ClientboundLevelChunkPacketData.java` in `working/Paper`. The payload
//! of `ClientboundLevelChunkWithLightPacket`: a heightmap map, an **opaque**
//! sections buffer, and a block-entity list.
//!
//! The sections `buffer` is opaque bytes to this crate: `LevelChunkSection.write`
//! runs in `rivet-world` (issue #100) and fills the `Vec<u8>` this packet carries.
//! The block-entity `type` decodes through
//! `ByteBufCodecs.registry(Registries.BLOCK_ENTITY_TYPE)` (varint registry id),
//! so the codec runs over [`RegistryFriendlyByteBuf`].
//!
//! Wire order (verified against the PR #194 fixture):
//! 1. `heightmaps` — `[VarInt count][count × ([VarInt type-id][VarInt longCount]
//!    [longCount × i64 big-endian])]`. The key is `Heightmap.Types.STREAM_CODEC`
//!    (`idMapper`, varint id); the value `LONG_ARRAY`.
//! 2. `buffer` — `[VarInt size][size bytes]`, decode-guarded at
//!    `TWO_MEGABYTES` (Java `RuntimeException("Chunk Packet trying to allocate
//!    too much memory on read.")`).
//! 3. `blockEntitiesData` — `[VarInt count][count × BlockEntityInfo]`.

use crate::codec::byte_buf_codecs::MAX_INITIAL_COLLECTION_SIZE;
use crate::codec::registry_byte_buf_codecs;
use crate::codec::{CodecError, StreamCodec, StreamDecoder, StreamEncoder, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::heightmap_types::HeightmapType;
use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag::Tag;
use rivet_registry::registries::{BLOCK_ENTITY_TYPE, BlockEntityType};
use std::sync::Arc;

/// `ClientboundLevelChunkPacketData.TWO_MEGABYTES` — the decode guard on the
/// sections buffer (`ClientboundChunksBiomesPacket` carries the same value for
/// its biome buffers).
pub const TWO_MEGABYTES: i32 = 2097152;

/// `ClientboundLevelChunkPacketData.BlockEntityInfo` — one packed block entity.
///
/// `packedXZ` is a signed byte: `sectionRelative(x) << 4 | sectionRelative(z)`
/// (both `& 15`), `y` the absolute block Y, `type` the registry id, `tag` the
/// update NBT (null -> EndTag, not length-prefixed).
///
/// `PartialEq` only (not `Eq`): the NBT `CompoundTag` value type has no `Eq`.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockEntityInfo {
    packed_xz: i8,
    y: i16,
    entity_type: Arc<BlockEntityType>,
    tag: Option<CompoundTag>,
}

impl BlockEntityInfo {
    /// `new BlockEntityInfo(int packedXZ, int y, BlockEntityType<?> type,
    /// @Nullable CompoundTag tag)`.
    pub fn new(
        packed_xz: i8,
        y: i16,
        entity_type: Arc<BlockEntityType>,
        tag: Option<CompoundTag>,
    ) -> Self {
        BlockEntityInfo {
            packed_xz,
            y,
            entity_type,
            tag,
        }
    }

    /// `BlockEntityInfo.packedXZ`.
    pub fn packed_xz(&self) -> i8 {
        self.packed_xz
    }

    /// `BlockEntityInfo.y`.
    pub fn y(&self) -> i16 {
        self.y
    }

    /// `BlockEntityInfo.type`.
    pub fn entity_type(&self) -> &Arc<BlockEntityType> {
        &self.entity_type
    }

    /// `BlockEntityInfo.tag`.
    pub fn tag(&self) -> Option<&CompoundTag> {
        self.tag.as_ref()
    }

    /// `BlockEntityInfo.STREAM_CODEC` — `StreamCodec.ofMember(write, new)`.
    pub fn stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, BlockEntityInfo> {
        codec(
            |value: &BlockEntityInfo, output: &mut RegistryFriendlyByteBuf| value.write(output),
            |input: &mut RegistryFriendlyByteBuf| Ok(Self::read(input)),
        )
    }

    /// `write(RegistryFriendlyByteBuf)` — `writeByte(packedXZ)` then
    /// `writeShort(y)` then the registry varint then `writeNbt(tag)`.
    pub fn write(&self, output: &mut RegistryFriendlyByteBuf) -> Result<(), CodecError> {
        output.inner_mut().write_byte(self.packed_xz);
        output.inner_mut().write_short(self.y);
        registry_byte_buf_codecs::registry(&*BLOCK_ENTITY_TYPE)
            .encode(output, &self.entity_type)?;
        output.write_nbt(self.tag.as_ref().map(|c| Tag::Compound(c.clone())).as_ref());
        Ok(())
    }

    /// `BlockEntityInfo(RegistryFriendlyByteBuf)` — the decode ctor.
    pub fn read(input: &mut RegistryFriendlyByteBuf) -> BlockEntityInfo {
        let packed_xz = input.inner_mut().read_byte();
        let y = input.inner_mut().read_short();
        let entity_type = registry_byte_buf_codecs::registry(&*BLOCK_ENTITY_TYPE)
            .decode(input)
            .expect("block-entity type registry id");
        let tag = input.read_nbt();
        BlockEntityInfo {
            packed_xz,
            y,
            entity_type,
            tag,
        }
    }
}

/// `ClientboundLevelChunkPacketData` — the chunk payload value type.
///
/// The heightmap map is a `Vec<(HeightmapType, Vec<i64>)>` in **ascending type
/// id order** — the Rust stand-in for Java's `EnumMap<Heightmap.Types, long[]>`,
/// whose iteration order is the enum declaration order. Decode normalizes to
/// that order (Java's `map` decode stores into the `EnumMap`), so a decode →
/// re-encode round trip always writes the heightmaps in ascending id order even
/// if a hostile wire ordered them otherwise. A duplicate type id on the wire is
/// also deduplicated last-wins (`EnumMap.put`), so the re-encoded count matches
/// Java's.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelChunkPacketData {
    heightmaps: Vec<(HeightmapType, Vec<i64>)>,
    /// The opaque sections buffer (`calculateChunkSize` bytes, produced by
    /// `LevelChunkSection.write` in `rivet-world`).
    buffer: Vec<u8>,
    /// `BlockEntityInfo` list.
    block_entities: Vec<BlockEntityInfo>,
}

impl LevelChunkPacketData {
    /// `new ClientboundLevelChunkPacketData(...)` — the packet-body value
    /// constructor. `heightmaps` must already be in ascending type id order
    /// with distinct ids (the `EnumMap` order the server produces); debug
    /// builds enforce that contract so a non-canonical `Vec` cannot silently
    /// emit a non-canonical wire.
    pub fn new(
        heightmaps: Vec<(HeightmapType, Vec<i64>)>,
        buffer: Vec<u8>,
        block_entities: Vec<BlockEntityInfo>,
    ) -> Self {
        debug_assert!(
            heightmaps.windows(2).all(|w| w[0].0.id() < w[1].0.id()),
            "heightmaps must be in ascending, distinct type id order (EnumMap order)"
        );
        LevelChunkPacketData {
            heightmaps,
            buffer,
            block_entities,
        }
    }

    /// `getHeightmaps()`.
    pub fn heightmaps(&self) -> &[(HeightmapType, Vec<i64>)] {
        &self.heightmaps
    }

    /// `getReadBuffer()` — the raw sections buffer.
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// `getBlockEntitiesData()` — the block-entity list.
    pub fn block_entities(&self) -> &[BlockEntityInfo] {
        &self.block_entities
    }

    /// `HEIGHTMAPS_STREAM_CODEC` — `ByteBufCodecs.map(EnumMap::new,
    /// Heightmap.Types.STREAM_CODEC, LONG_ARRAY)`. Written in `EnumMap`
    /// iteration order (ascending type id). The count is unbounded in Java; the
    /// pre-allocation is capped at `MAX_INITIAL_COLLECTION_SIZE` like every
    /// other collection decode, so a hostile count cannot pre-size a huge
    /// allocation before the decode loop runs out of buffer. Decode replaces on
    /// a duplicate type id (`EnumMap.put`), then sorts to the ascending order.
    fn heightmaps_stream_codec() -> StreamCodec<FriendlyByteBuf, Vec<(HeightmapType, Vec<i64>)>> {
        let type_codec = HeightmapType::stream_codec();
        let type_codec_decode = type_codec.clone();
        let long_codec = crate::codec::byte_buf_codecs::long_array();
        let long_codec_decode = long_codec.clone();
        crate::codec::of(
            move |output: &mut FriendlyByteBuf, heightmaps: &Vec<(HeightmapType, Vec<i64>)>| {
                output.write_var_int(heightmaps.len() as i32);
                for (ty, raw) in heightmaps {
                    type_codec.encode(output, ty)?;
                    long_codec.encode(output, raw)?;
                }
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                let count = input.read_var_int();
                let mut out = Vec::with_capacity(
                    (count.max(0) as usize).min(MAX_INITIAL_COLLECTION_SIZE as usize),
                );
                for _ in 0..count {
                    let ty = type_codec_decode.decode(input)?;
                    let raw = long_codec_decode.decode(input)?;
                    // `EnumMap.put`: a duplicate type id overwrites the earlier
                    // entry (last wins), so replace any existing key.
                    if let Some(entry) = out.iter_mut().find(|(t, _)| *t == ty) {
                        entry.1 = raw;
                    } else {
                        out.push((ty, raw));
                    }
                }
                // `EnumMap` iteration order: ascending type id.
                out.sort_by_key(|(ty, _)| ty.id());
                Ok(out)
            },
        )
    }

    /// `STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, LevelChunkPacketData> {
        let heightmaps_codec = Self::heightmaps_stream_codec();
        let heightmaps_codec_decode = heightmaps_codec.clone();
        codec(
            move |value: &LevelChunkPacketData, output: &mut RegistryFriendlyByteBuf| {
                heightmaps_codec.encode(output.inner_mut(), &value.heightmaps)?;
                // `writeVarInt(buffer.length)` then the raw bytes.
                output.inner_mut().write_var_int(value.buffer.len() as i32);
                output.inner_mut().write_bytes(&value.buffer);
                // `BlockEntityInfo.LIST_STREAM_CODEC` — varint count then the
                // infos (no cap, `ByteBufCodecs.list()`).
                output
                    .inner_mut()
                    .write_var_int(value.block_entities.len() as i32);
                for info in &value.block_entities {
                    info.write(output)?;
                }
                Ok(())
            },
            move |input: &mut RegistryFriendlyByteBuf| {
                let heightmaps = heightmaps_codec_decode.decode(input.inner_mut())?;
                let size = input.inner_mut().read_var_int();
                if size > TWO_MEGABYTES {
                    return Err(CodecError::new(
                        "Chunk Packet trying to allocate too much memory on read.",
                    ));
                }
                if size < 0 {
                    // Java `NegativeArraySizeException` (message is the size).
                    panic!("{size}");
                }
                let readable = input.readable_bytes() as i32;
                if size > readable {
                    return Err(CodecError::new(format!(
                        "Chunk buffer of {size} bytes exceeds {readable} readable bytes"
                    )));
                }
                let buffer = input.inner_mut().read_slice(size);
                // `BlockEntityInfo.LIST_STREAM_CODEC` — `ByteBufCodecs.list()`
                // (`ArrayList::new`): a negative count throws Java's
                // `IllegalArgumentException("Illegal Capacity: -N")`.
                let count = input.inner_mut().read_var_int();
                if count < 0 {
                    panic!("Illegal Capacity: {count}");
                }
                let mut block_entities =
                    Vec::with_capacity((count as usize).min(MAX_INITIAL_COLLECTION_SIZE as usize));
                for _ in 0..count {
                    block_entities.push(BlockEntityInfo::read(input));
                }
                Ok(LevelChunkPacketData {
                    heightmaps,
                    buffer,
                    block_entities,
                })
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use rivet_registry::registry::RegistryKey;
    use rivet_registry::{Identifier, RegistryAccess, ResourceKey};

    fn registry_buf(bytes: Vec<u8>) -> RegistryFriendlyByteBuf {
        RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), RegistryAccess::empty())
    }

    /// The generated Minecraft 26.2 `BLOCK_ENTITY_TYPE` registry, plus two
    /// representative stored allocations for packet round trips.
    fn block_entity_registry() -> (RegistryAccess, Arc<BlockEntityType>, Arc<BlockEntityType>) {
        let key: RegistryKey<BlockEntityType> = ResourceKey::create_registry_key(
            Identifier::with_default_namespace("block_entity_type"),
        );
        let registry = BlockEntityType::built_in_registry();
        let access = RegistryAccess::from_single_registry(key.clone(), registry);
        let furnace = access.lookup(&key).unwrap().by_id_arc(0).unwrap().clone();
        let chest = access.lookup(&key).unwrap().by_id_arc(1).unwrap().clone();
        (access, furnace, chest)
    }

    #[test]
    fn buffer_oversize_decode_errors_with_java_message() {
        // A hostile `size` over TWO_MEGABYTES -> `Err` at the codec boundary
        // (Java RuntimeException). heightmaps count 0, then the oversized size.
        let mut input = registry_buf(Vec::new());
        input.inner_mut().write_var_int(0);
        input.inner_mut().write_var_int(TWO_MEGABYTES + 1);
        let err = LevelChunkPacketData::stream_codec()
            .decode(&mut input)
            .unwrap_err();
        assert_eq!(
            err.message,
            "Chunk Packet trying to allocate too much memory on read."
        );
    }

    #[test]
    fn buffer_negative_size_panics_like_java() {
        // A negative size passes the TWO_MEGABYTES guard and hits
        // `new byte[-1]` -> `NegativeArraySizeException`, whose message is the
        // size (the port panics with the same payload).
        let mut input = registry_buf(Vec::new());
        input.inner_mut().write_var_int(0);
        input.inner_mut().write_var_int(-3);
        let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = LevelChunkPacketData::stream_codec().decode(&mut input);
        }))
        .unwrap_err();
        assert_eq!(msg.downcast_ref::<String>().unwrap(), "-3");
    }

    #[test]
    fn heightmap_decode_normalizes_to_ascending_type_id() {
        // A hostile wire order (5,1,4) must decode to the EnumMap order
        // (1,4,5) so re-encode is byte-identical with Java. Each type carries a
        // distinct long so the value-to-key pairing is also verified, not just
        // the id order.
        let mut input = registry_buf(Vec::new());
        input.inner_mut().write_var_int(3);
        let values = [0x111i64, 0x222, 0x333];
        for (ty, value) in [
            HeightmapType::MotionBlockingNoLeaves,
            HeightmapType::WorldSurface,
            HeightmapType::MotionBlocking,
        ]
        .into_iter()
        .zip(values)
        {
            input.inner_mut().write_var_int(ty.id());
            input.inner_mut().write_var_int(1);
            input.inner_mut().write_long(value);
        }
        // buffer size 0, block-entity count 0
        input.inner_mut().write_var_int(0);
        input.inner_mut().write_var_int(0);
        let decoded = LevelChunkPacketData::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(
            decoded
                .heightmaps()
                .iter()
                .map(|(ty, _)| ty.id())
                .collect::<Vec<_>>(),
            vec![1, 4, 5]
        );
        // The values reorder with their keys: id 1 (WorldSurface) got 0x222,
        // id 4 (MotionBlocking) got 0x333, id 5 (NoLeaves) got 0x111.
        assert_eq!(
            decoded
                .heightmaps()
                .iter()
                .map(|(_, raw)| raw[0])
                .collect::<Vec<_>>(),
            vec![0x222, 0x333, 0x111]
        );
    }

    #[test]
    fn block_entity_info_round_trips_through_the_registry() {
        // A populated BLOCK_ENTITY_TYPE registry: decode a varint type id back
        // to the stored element, with an NBT tag alongside.
        let (access, furnace, _chest) = block_entity_registry();

        let mut tag = CompoundTag::new();
        tag.put(
            "Items".to_string(),
            Tag::Int(rivet_nbt::int_tag::IntTag::value_of(3)),
        );
        let info = BlockEntityInfo::new(0x57, -64, furnace.clone(), Some(tag));

        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
        BlockEntityInfo::stream_codec()
            .encode(&mut out, &info)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        // packedXZ byte, y short BE, registry varint id 0, then the NBT.
        assert_eq!(bytes[0], 0x57);
        assert_eq!(&bytes[1..3], &(-64i16).to_be_bytes());
        assert_eq!(bytes[3], 0);

        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access.clone());
        let decoded = BlockEntityInfo::stream_codec().decode(&mut input).unwrap();
        assert_eq!(decoded.packed_xz(), 0x57);
        assert_eq!(decoded.y(), -64);
        assert_eq!(input.readable_bytes(), 0);
        // The decoded `Arc` aliases the stored allocation (decode -> encode id).
        let mut re = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
        BlockEntityInfo::stream_codec()
            .encode(&mut re, &decoded)
            .unwrap();
        assert_eq!(re.into_inner().to_vec(), bytes);
        assert_eq!(
            decoded.tag().unwrap().get("Items").unwrap(),
            &Tag::Int(rivet_nbt::int_tag::IntTag::value_of(3))
        );
    }

    #[test]
    fn block_entity_info_null_tag_round_trips_and_consumes_end_byte() {
        // The `None`-tag half must round trip byte-identically (the trailing
        // EndTag 0x00 is written on encode and consumed on decode), so a
        // dropped/leaked EndTag would fail the re-encode identity and the
        // `readable_bytes() == 0` assert below.
        let (access, furnace, _chest) = block_entity_registry();
        let info = BlockEntityInfo::new(0x11, 0x7F, furnace.clone(), None);

        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
        BlockEntityInfo::stream_codec()
            .encode(&mut out, &info)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        // packedXZ 0x11, y 0x007F, registry id 0, then the EndTag byte 0x00.
        assert_eq!(&bytes[0..4], &[0x11, 0x00, 0x7F, 0x00]);
        assert_eq!(bytes.len(), 5);

        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access.clone());
        let decoded = BlockEntityInfo::stream_codec().decode(&mut input).unwrap();
        assert_eq!(decoded.packed_xz(), 0x11);
        assert_eq!(decoded.y(), 0x7F);
        assert!(decoded.tag().is_none());
        assert_eq!(input.readable_bytes(), 0);

        let mut re = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
        BlockEntityInfo::stream_codec()
            .encode(&mut re, &decoded)
            .unwrap();
        assert_eq!(re.into_inner().to_vec(), bytes);
    }

    #[test]
    fn block_entity_list_round_trips_with_multiple_infos() {
        // `BlockEntityInfo.LIST_STREAM_CODEC` with count > 0: a wrong loop
        // bound or a truncated list would fail the exact byte layout and the
        // `readable_bytes() == 0` assert.
        let (access, furnace, chest) = block_entity_registry();
        let mut tag = CompoundTag::new();
        tag.put_string("name", "chest");
        let infos = vec![
            BlockEntityInfo::new(0x57, -64, furnace.clone(), None),
            BlockEntityInfo::new(0x12, 64, chest.clone(), Some(tag)),
        ];
        let data = LevelChunkPacketData::new(vec![], vec![0xDE, 0xAD], infos);

        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
        LevelChunkPacketData::stream_codec()
            .encode(&mut out, &data)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        // heightmaps count 0, buffer size 2 + bytes, block-entity count 2, then
        // the infos. The second info's NBT is length-prefixed by `writeNbt`'s
        // unnamed compound: 0x0A id + 2-byte name len + name + 0x00 payload len
        // + name/value + EndTag.
        assert_eq!(bytes[0], 0); // heightmaps count
        assert_eq!(bytes[1], 2); // buffer size
        assert_eq!(&bytes[2..4], &[0xDE, 0xAD]);
        assert_eq!(bytes[4], 2); // block-entity count
        // First info (bytes 5..10): packedXZ 0x57, y -64 BE, registry id 0,
        // then the null-tag EndTag byte 0x00.
        assert_eq!(&bytes[5..10], &[0x57, 0xFF, 0xC0, 0x00, 0x00]);
        // Second info starts at byte 10: packedXZ 0x12, y 64 BE, registry id 1.
        assert_eq!(&bytes[10..14], &[0x12, 0x00, 0x40, 0x01]);

        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access.clone());
        let decoded = LevelChunkPacketData::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(input.readable_bytes(), 0);
        assert_eq!(decoded.block_entities().len(), 2);
        assert_eq!(decoded.block_entities()[0].entity_type(), &furnace);
        assert_eq!(decoded.block_entities()[1].entity_type(), &chest);
        assert!(decoded.block_entities()[0].tag().is_none());
        assert_eq!(
            decoded.block_entities()[1]
                .tag()
                .unwrap()
                .get_string("name")
                .unwrap(),
            "chest"
        );
    }

    #[test]
    fn negative_block_entity_count_panics_like_arraylist() {
        // `ByteBufCodecs.list()` -> `new ArrayList<>(count)` throws
        // `IllegalArgumentException("Illegal Capacity: -1")` on a negative
        // count — NOT silently accepted as empty. heightmaps 0, buffer size 0,
        // then the hostile count.
        let mut input = registry_buf(Vec::new());
        input.inner_mut().write_var_int(0);
        input.inner_mut().write_var_int(0);
        input.inner_mut().write_var_int(-1);
        let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = LevelChunkPacketData::stream_codec().decode(&mut input);
        }))
        .unwrap_err();
        assert_eq!(
            msg.downcast_ref::<String>().unwrap(),
            "Illegal Capacity: -1"
        );
    }

    #[test]
    fn heightmap_duplicate_type_ids_dedupe_last_wins_like_enum_map() {
        // A hostile wire repeating a type id: `EnumMap.put` overwrites, so the
        // last occurrence wins and re-encode writes one entry (count 1), not 2.
        let mut input = registry_buf(Vec::new());
        input.inner_mut().write_var_int(2);
        input.inner_mut().write_var_int(1); // WorldSurface
        input.inner_mut().write_var_int(1);
        input.inner_mut().write_long(0x111);
        input.inner_mut().write_var_int(1); // WorldSurface again
        input.inner_mut().write_var_int(1);
        input.inner_mut().write_long(0x222);
        input.inner_mut().write_var_int(0); // buffer size 0
        input.inner_mut().write_var_int(0); // block-entity count 0
        let decoded = LevelChunkPacketData::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded.heightmaps().len(), 1);
        assert_eq!(decoded.heightmaps()[0].0, HeightmapType::WorldSurface);
        assert_eq!(decoded.heightmaps()[0].1, vec![0x222]);

        // Re-encode writes a single heightmap entry.
        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
        LevelChunkPacketData::stream_codec()
            .encode(&mut out, &decoded)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes[0], 1); // heightmap count
    }
}
