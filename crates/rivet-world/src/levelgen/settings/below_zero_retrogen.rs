//! Port of `net.minecraft.world.level.levelgen.BelowZeroRetrogen` (26.2) — the
//! `mc.world.level.levelgen.settings` unit.
//!
//! The below-zero retrogen settings: a `ChunkStatus` target plus the
//! `BitSet` bedrock-hole mask, serialized as `"target_status"` (a
//! non-empty chunk status by name) and a lenient `"missing_bedrock"` long-word
//! stream. The class also carries the `UPGRADE_HEIGHT_ACCESSOR` (the `[-64,
//! 0)` window below-zero chunks are re-generated into), the bedrock-mask
//! writes, and the `getBiomeResolver` retrogen biome override.
//!
//! ### The `java.util.BitSet` seam
//!
//! Java's `java.util.BitSet` is not ported anywhere in the workspace (no
//! `bitset` crate is approved in `CRATES.md` either). The class needs the word
//! array subset — `valueOf(long[])` / `toLongArray()` / `isEmpty()` / `get(i)`
//! — so this module ports that subset as [`BitSet`] with Java's exact 64-bit
//! word layout (little-endian bit order, trailing zero words trimmed). When a
//! shared `java.util.BitSet` port lands (the region/upgrade storage units) it
//! should replace this local type.
//!
//! ### The `ChunkStatus` by-name codec
//!
//! Java's `BuiltInRegistries.CHUNK_STATUS.byNameCodec()` resolves the registry
//! name through `Identifier.CODEC.comapFlatMap(name -> registry.get(name))`:
//! the `Identifier` parse defaults a bare path to the `minecraft` namespace, so
//! both the short (`"empty"`..`"full"`) and full (`"minecraft:empty"`..
//! `"minecraft:full"`) forms decode. The ported `ChunkStatus` enum has
//! `serialization_name()` and is a `Copy` value, so the codec composes
//! `identifier_codec` with a full-name lookup over the ladder — no local
//! `StringRepresentable`/`Display` impls (the `mc.world.level.chunk.status`
//! unit stays un-pre-empted).
//!
//! ### The chunk-write seam
//!
//! `replaceOldBedrock`/`applyBedrockMask` mutate a `ProtoChunk` through Java's
//! `chunk.setBlockState`. The ported `ProtoChunk` defers `setBlockState`'s
//! block-mutation half to the #216 section write, so the writes here route
//! through [`ProtoChunk::write_worldgen_block`] (the real worldgen block write
//! the noisegen unit uses) with the two `Usage.WORLDGEN` heightmaps created up
//! front — the `EMPTY`-status `heightmapsAfter` set, which is the set Java's
//! `setBlockState` primes for a generation-phase proto chunk. The non-worldgen
//! heightmaps, post-processing, and light defer with the owning chunk units
//! (RivetTodo #216/#183).

use crate::biome::biome_resolver::BiomeResolver;
use crate::biome::biomes;
use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::proto_chunk::ProtoChunk;
use crate::chunk::status::ChunkStatus;
use crate::chunk::storage::chunk_reconstruction::block_state_predicates;
use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor};
use crate::levelgen::heightmap::Types;
use rivet_registry::ResourceKey;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::holder::Holder;
use rivet_registry::identifier::{Identifier, identifier_codec};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::{Arc, LazyLock};

use crate::block::blocks::Blocks;

/// `BelowZeroRetrogen.UPGRADE_HEIGHT_ACCESSOR` — the `LevelHeightAccessor`
/// with `getHeight() = 64`, `getMinY() = -64` the below-zero retrogen writes
/// into.
pub static UPGRADE_HEIGHT_ACCESSOR: LazyLock<SimpleLevelHeightAccessor> =
    LazyLock::new(|| crate::level::height_accessor::create(-64, 64));

/// `BelowZeroRetrogen.RETAINED_RETROGEN_BIOMES` — the biome keys the retrogen
/// keeps from the noise resolver (`Set.of(LUSH_CAVES, DRIPSTONE_CAVES,
/// DEEP_DARK)`).
///
/// A function (not a `const`): the `LazyLock` deref that yields the
/// `&'static ResourceKey` is not a `const` operation.
pub fn retained_retrogen_biomes() -> [&'static ResourceKey<BiomeId>; 3] {
    [
        &biomes::LUSH_CAVES,
        &biomes::DRIPSTONE_CAVES,
        &biomes::DEEP_DARK,
    ]
}

/// Java `java.util.BitSet` word-array subset (see the module doc).
///
/// `words` holds the 64-bit words little-endian (bit `i` is word `i >> 6`, bit
/// `i & 63`); trailing zero words are trimmed, matching Java's `wordsInUse`
/// (so `toLongArray()` never has trailing zeros and `isEmpty()` is
/// `words.is_empty()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    /// `new BitSet(0)` — the empty set.
    pub fn empty() -> BitSet {
        BitSet { words: Vec::new() }
    }

    /// `BitSet.valueOf(long[])` — builds the set from little-endian 64-bit
    /// words, trimming trailing zero words (Java's `wordsInUse` fold).
    pub fn value_of(words: &[i64]) -> BitSet {
        let mut words: Vec<u64> = words.iter().map(|w| *w as u64).collect();
        while words.last() == Some(&0) {
            words.pop();
        }
        BitSet { words }
    }

    /// `BitSet.toLongArray()` — the words up to the highest set bit.
    pub fn to_long_array(&self) -> Vec<i64> {
        self.words.iter().map(|w| *w as i64).collect()
    }

    /// `BitSet.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// `BitSet.get(int)` — `(words[wordIndex(bitIndex)] & (1L << bitIndex)) !=
    /// 0`; out-of-range words read `false`.
    pub fn get(&self, bit_index: usize) -> bool {
        let word = bit_index >> 6;
        if word >= self.words.len() {
            return false;
        }
        (self.words[word] & (1u64 << (bit_index & 63))) != 0
    }
}

/// `BelowZeroRetrogen.BITSET_CODEC` — `Codec.LONG_STREAM.xmap(stream ->
/// BitSet.valueOf(stream.toArray()), bitSet -> LongStream.of(bitSet
/// .toLongArray()))`.
fn bitset_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BitSet, Ops>> {
    codec::xmap(
        codec::long_stream_codec::<Ops>(),
        Arc::new(|words: &Vec<i64>| BitSet::value_of(words)),
        Arc::new(|bitset: &BitSet| bitset.to_long_array()),
    )
}

/// `BelowZeroRetrogen.NON_EMPTY_CHUNK_STATUS` — the by-name `ChunkStatus` codec
/// that rejects `EMPTY` (`"target_status cannot be empty"`).
///
/// Composed like Java's `BuiltInRegistries.CHUNK_STATUS.byNameCodec()` (an
/// `Identifier.CODEC.comapFlatMap(name -> registry.get(name), ...)`), so the
/// `Identifier` parse defaults a bare path to the `minecraft` namespace: the
/// short `"empty"`/`"features"` forms decode exactly like the full
/// `"minecraft:empty"`/`"minecraft:features"` forms.
fn non_empty_chunk_status_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ChunkStatus, Ops>> {
    let by_name = codec::comap_flat_map(
        identifier_codec::<Ops>(),
        Arc::new(|identifier: &Identifier| {
            let name = identifier.to_string();
            match ChunkStatus::ALL
                .iter()
                .find(|status| status.serialization_name() == name)
            {
                Some(status) => DataResult::success(*status),
                None => DataResult::error(format!(
                    "Unknown registry key in minecraft:chunk_status: {name}"
                )),
            }
        }),
        Arc::new(|status: &ChunkStatus| Identifier::parse(status.serialization_name())),
    );
    // Java: `.comapFlatMap(status -> status == EMPTY ? error : success,
    // Function.identity())`.
    codec::comap_flat_map(
        by_name,
        Arc::new(|status: &ChunkStatus| {
            if *status == ChunkStatus::Empty {
                DataResult::error("target_status cannot be empty")
            } else {
                DataResult::success(*status)
            }
        }),
        Arc::new(|status: &ChunkStatus| *status),
    )
}

/// `net.minecraft.world.level.levelgen.BelowZeroRetrogen`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BelowZeroRetrogen {
    /// `targetStatus` — the chunk status the retrogen lifts the chunk to.
    target_status: ChunkStatus,
    /// `missingBedrock` — the bedrock-hole mask (bit `(z & 15) * 16 + (x & 15)`).
    missing_bedrock: BitSet,
}

impl BelowZeroRetrogen {
    /// The private constructor — `missingBedrock.orElse(EMPTY)`.
    pub(crate) fn new(target_status: ChunkStatus, missing_bedrock: Option<BitSet>) -> Self {
        BelowZeroRetrogen {
            target_status,
            missing_bedrock: missing_bedrock.unwrap_or_else(BitSet::empty),
        }
    }

    /// `targetStatus()`.
    pub fn target_status(&self) -> ChunkStatus {
        self.target_status
    }

    /// `hasBedrockHoles()` — `!this.missingBedrock.isEmpty()`.
    pub fn has_bedrock_holes(&self) -> bool {
        !self.missing_bedrock.is_empty()
    }

    /// `hasBedrockHole(int x, int z)` — `this.missingBedrock.get((z & 15) * 16
    /// + (x & 15))`.
    pub fn has_bedrock_hole(&self, x: i32, z: i32) -> bool {
        self.missing_bedrock
            .get(((z & 15).wrapping_mul(16).wrapping_add(x & 15)) as usize)
    }

    /// `applyBedrockMask(ProtoChunk)` — clears the bedrock-hole columns (air
    /// where the mask has a hole).
    ///
    /// The height window is `chunk.getHeightAccessorForGeneration()`. At the
    /// only call site (the noise step of an upgrading chunk in
    /// `ChunkStatusTasks`) the chunk is upgrading (`getBelowZeroRetrogen() !=
    /// null`), so that accessor always resolves to `UPGRADE_HEIGHT_ACCESSOR` —
    /// the `[-64, 64)` below-zero window — and the mask clears only that
    /// window, never the terrain above. The two worldgen heightmaps are created
    /// once up front (the `fill_from_noise` pattern) before the write loop.
    pub fn apply_bedrock_mask<B, S>(&self, chunk: &mut ProtoChunk<BlockState, B, S>)
    where
        B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
        S: Eq + std::hash::Hash,
    {
        let height_accessor = &*UPGRADE_HEIGHT_ACCESSOR;
        let min_y = height_accessor.get_min_y();
        let max_y = height_accessor.get_max_y();

        chunk.get_or_create_heightmap_unprimed(Types::OceanFloorWg);
        chunk.get_or_create_heightmap_unprimed(Types::WorldSurfaceWg);

        for x in 0..16 {
            for z in 0..16 {
                if self.has_bedrock_hole(x, z) {
                    for pos in BlockPos::between_closed(x, min_y, z, x, max_y, z) {
                        write_block(chunk, &pos, Blocks::AIR.default_block_state());
                    }
                }
            }
        }
    }
}

/// `BelowZeroRetrogen.CODEC` — the ops-generic
/// `below_zero_retrogen_codec::<Ops>()` factory.
pub fn below_zero_retrogen_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<BelowZeroRetrogen, Ops>> {
    let bitset = bitset_codec::<Ops>();
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|b: &BelowZeroRetrogen| b.target_status),
                codec::field_of(
                    non_empty_chunk_status_codec::<Ops>(),
                    "target_status".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|b: &BelowZeroRetrogen| {
                    if b.missing_bedrock.is_empty() {
                        None
                    } else {
                        Some(b.missing_bedrock.clone())
                    }
                }),
                codec::optional_field("missing_bedrock".to_string(), bitset, true),
            ))
            .apply(instance, Arc::new(BelowZeroRetrogen::new))
    })
}

/// `BelowZeroRetrogen.replaceOldBedrock(ProtoChunk)` (static) — replaces the
/// old bedrock roof (`y` in `[0, 4]`) with deepslate. The write routes through
/// the worldgen block write (see the module doc).
pub fn replace_old_bedrock<B, S>(chunk: &mut ProtoChunk<BlockState, B, S>)
where
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    // The two worldgen heightmaps are created once up front (the
    // `fill_from_noise` pattern) before the write loop.
    chunk.get_or_create_heightmap_unprimed(Types::OceanFloorWg);
    chunk.get_or_create_heightmap_unprimed(Types::WorldSurfaceWg);
    for pos in BlockPos::between_closed(0, 0, 0, 15, 4, 15) {
        let state = chunk.get_block_state(pos.get_x(), pos.get_y(), pos.get_z());
        if state.block() == Blocks::BEDROCK.id() {
            write_block(chunk, &pos, Blocks::DEEPSLATE.default_block_state());
        }
    }
}

/// The `ProtoChunk.setBlockState` write seam — Java's `setBlockState` prologue
/// primes the persisted-status heightmaps before the section write; the
/// worldgen write (`write_worldgen_block`) does the same for the two
/// `Usage.WORLDGEN` heightmaps (the `EMPTY`-status set). See the module doc.
///
/// The heightmaps must exist before the first write: every caller (the two
/// `replaceOldBedrock`/`applyBedrockMask` paths) creates them once up front,
/// mirroring `flat_level_source.fill_from_noise`.
fn write_block<B, S>(chunk: &mut ProtoChunk<BlockState, B, S>, pos: &BlockPos, state: BlockState)
where
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    let section_index = chunk.get_section_index(pos.get_y());
    let predicates = block_state_predicates();
    chunk.write_worldgen_block(
        section_index,
        pos.get_x() & 15,
        pos.get_y() & 15,
        pos.get_z() & 15,
        pos.get_y(),
        state,
        &predicates.is_air,
        &predicates.is_randomly_ticking,
        &predicates.fluid_is_empty,
        &predicates.fluid_is_randomly_ticking,
        &predicates.is_special_colliding,
    );
}

/// `BelowZeroRetrogen.getBiomeResolver(BiomeResolver, ChunkAccess)` — the
/// retrogen biome override: for an upgrading chunk, non-retained noise biomes
/// are replaced by the chunk's stored `getNoiseBiome(quartX, 0, quartZ)`.
///
/// RivetTodo(#183): the retrogen branch is a typed seam — `ChunkAccess
/// .isUpgrading()`/`getBelowZeroRetrogen()` defer with the chunk-access unit,
/// and the `Holder.is(Set<ResourceKey<Biome>>)` check needs a
/// `HolderLookup<BiomeId>` to resolve the reference holder (the port's
/// `Holder` back-reference rule). The method fails loudly rather than
/// fabricate a biome resolver.
pub fn get_biome_resolver<S: Eq + std::hash::Hash>(
    _biome_resolver: &dyn BiomeResolver,
    _proto_chunk: &ChunkAccess<BlockState, Holder<BiomeId>, S>,
) -> Arc<dyn BiomeResolver> {
    panic!(
        "BelowZeroRetrogen.getBiomeResolver is not implemented (RivetTodo #183): requires ChunkAccess.isUpgrading()/getBelowZeroRetrogen() and a HolderLookup<BiomeId>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn bitset_value_of_and_to_long_array_round_trip() {
        // Bit 0 in word 0 = 1; bit 63 = the sign bit; bit 65 = word 1 bit 1.
        let words = [1i64 | (1i64 << 63), 2i64];
        let set = BitSet::value_of(&words);
        assert!(set.get(0));
        assert!(set.get(63));
        assert!(set.get(65));
        assert!(!set.get(1));
        assert!(!set.get(64));
        assert!(!set.get(128));
        assert_eq!(set.to_long_array(), vec![words[0], words[1]]);
    }

    #[test]
    fn bitset_trims_trailing_zero_words() {
        let set = BitSet::value_of(&[0, 5, 0, 0]);
        // `wordsInUse` = 2 (the trailing zeros are trimmed).
        assert_eq!(set.to_long_array(), vec![0, 5]);
        assert!(!set.is_empty());
        assert!(BitSet::value_of(&[0, 0, 0]).is_empty());
        assert!(BitSet::value_of(&[]).is_empty());
        // `new BitSet(0)`.
        assert!(BitSet::empty().is_empty());
        assert_eq!(BitSet::empty().to_long_array(), Vec::<i64>::new());
    }

    #[test]
    fn codec_round_trips_and_rejects_empty_status() {
        let ops = JsonOps::INSTANCE;
        let codec = below_zero_retrogen_codec::<JsonOps>();
        let retrogen = BelowZeroRetrogen::new(ChunkStatus::Features, Some(BitSet::value_of(&[1])));
        let encoded = codec
            .encode_start(&ops, &retrogen)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"target_status": "minecraft:features", "missing_bedrock": [1]})
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, retrogen);
        assert!(decoded.has_bedrock_holes());
        assert!(decoded.has_bedrock_hole(0, 0));
        assert!(!decoded.has_bedrock_hole(1, 0));
    }

    #[test]
    fn codec_omits_empty_missing_bedrock() {
        let ops = JsonOps::INSTANCE;
        let codec = below_zero_retrogen_codec::<JsonOps>();
        let retrogen = BelowZeroRetrogen::new(ChunkStatus::Surface, None);
        let encoded = codec
            .encode_start(&ops, &retrogen)
            .result()
            .expect("encode")
            .clone();
        // The empty mask is omitted (the `Optional.empty()` getter).
        assert_eq!(encoded, json!({"target_status": "minecraft:surface"}));
        assert!(!retrogen.has_bedrock_holes());
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, retrogen);
    }

    #[test]
    fn codec_rejects_empty_target_status() {
        let ops = JsonOps::INSTANCE;
        let codec = below_zero_retrogen_codec::<JsonOps>();
        assert!(
            codec
                .parse(&ops, &json!({"target_status": "minecraft:empty"}))
                .result()
                .is_none()
        );
        assert!(
            codec
                .parse(&ops, &json!({"target_status": "minecraft:not_a_status"}))
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_accepts_short_default_namespace_status() {
        let ops = JsonOps::INSTANCE;
        let codec = below_zero_retrogen_codec::<JsonOps>();
        // Java's `Identifier` parse defaults a bare path to the `minecraft`
        // namespace (`byNameCodec`), so the short form decodes exactly like the
        // full form.
        let decoded = codec
            .parse(&ops, &json!({"target_status": "features"}))
            .result()
            .expect("short 'features' must decode like 'minecraft:features'")
            .clone();
        assert_eq!(decoded.target_status(), ChunkStatus::Features);
        // The default-namespace `"empty"` still resolves to `EMPTY` and is
        // rejected by `NON_EMPTY_CHUNK_STATUS`.
        assert!(
            codec
                .parse(&ops, &json!({"target_status": "empty"}))
                .result()
                .is_none()
        );
        // A foreign namespace is not a registry status (unknown-key error).
        assert!(
            codec
                .parse(&ops, &json!({"target_status": "foreign:features"}))
                .result()
                .is_none()
        );
    }

    #[test]
    fn has_bedrock_hole_indexes_like_java() {
        let mask = BitSet::value_of(&[1i64 << ((3 * 16 + 5) as u64)]);
        let retrogen = BelowZeroRetrogen::new(ChunkStatus::Full, Some(mask));
        // `(z & 15) * 16 + (x & 15)` — x=5, z=3.
        assert!(retrogen.has_bedrock_hole(5, 3));
        assert!(!retrogen.has_bedrock_hole(5, 2));
        assert!(!retrogen.has_bedrock_hole(4, 3));
    }

    #[test]
    fn upgrade_height_accessor_matches_java() {
        let accessor = &*UPGRADE_HEIGHT_ACCESSOR;
        assert_eq!(accessor.get_min_y(), -64);
        assert_eq!(accessor.get_height(), 64);
    }

    #[test]
    fn retained_retrogen_biomes_are_the_three_keys() {
        assert_eq!(
            retained_retrogen_biomes(),
            [
                &*biomes::LUSH_CAVES,
                &*biomes::DRIPSTONE_CAVES,
                &*biomes::DEEP_DARK
            ]
        );
    }

    /// The overworld chunk shape the below-zero writes target (the noisegen
    /// `worldgen_proto` pattern): 24 all-air sections over `-64..=319`.
    fn worldgen_proto()
    -> ProtoChunk<BlockState, crate::chunk::storage::section_reconstruction::BiomeId, &'static str>
    {
        use crate::chunk::level_chunk_section::LevelChunkSection;
        use crate::chunk::storage::chunk_reconstruction::resolve_state_flags;
        use crate::chunk::storage::section_reconstruction::{
            BiomeId as SectionBiomeId, current_version_container_factory,
        };
        use crate::chunk::upgrade_data::UpgradeData;
        use crate::level::height_accessor::create as create_accessor;
        use rivet_registry::core::ChunkPos;

        let factory = current_version_container_factory();
        let air = Blocks::AIR.default_block_state();
        let sections: Vec<LevelChunkSection<BlockState, SectionBiomeId>> = (0..24)
            .map(|_| {
                LevelChunkSection::new_all_air(
                    factory.create_for_block_states(),
                    factory.create_for_biomes(),
                )
            })
            .collect();
        ProtoChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            create_accessor(-64, 384),
            &factory,
            Some(sections),
            air,
            air,
            &resolve_state_flags,
        )
    }

    #[test]
    fn replace_old_bedrock_writes_deepslate_through_the_real_chunk() {
        // Fill the old bedrock roof `[0, 4]` (the below-zero upgrade source),
        // then `replaceOldBedrock` swaps it for deepslate via the real worldgen
        // block write (section write + worldgen heightmap updates).
        let bedrock = Blocks::BEDROCK.default_block_state();
        let mut proto = worldgen_proto();
        // `write_block` requires the worldgen heightmaps (the caller primes
        // them once, as `applyBedrockMask`/`replaceOldBedrock` do).
        proto.get_or_create_heightmap_unprimed(Types::OceanFloorWg);
        proto.get_or_create_heightmap_unprimed(Types::WorldSurfaceWg);
        for y in 0..=4 {
            for x in 0..16 {
                for z in 0..16 {
                    write_block(&mut proto, &BlockPos::new(x, y, z), bedrock);
                }
            }
        }
        assert_eq!(proto.get_block_state(0, 2, 0), bedrock);

        replace_old_bedrock(&mut proto);
        let deepslate = Blocks::DEEPSLATE.default_block_state();
        assert_eq!(proto.get_block_state(0, 2, 0), deepslate);
        assert_eq!(proto.get_block_state(15, 0, 15), deepslate);
        assert_eq!(proto.get_block_state(7, 4, 7), deepslate);
        // The block just below the roof is untouched (still air).
        assert_eq!(
            proto.get_block_state(0, -1, 0),
            Blocks::AIR.default_block_state()
        );
        // The worldgen heightmaps tracked the write (topmost non-air = 4).
        let min_y = -64;
        assert_eq!(
            proto
                .get_or_create_heightmap_unprimed(Types::OceanFloorWg)
                .get_height_at(0, 0, min_y),
            4
        );
    }

    #[test]
    fn apply_bedrock_mask_clears_the_hole_column_within_the_upgrade_window() {
        // A below-zero bedrock layer plus a mask hole at (x=0, z=0):
        // `applyBedrockMask` clears the hole column within the
        // `UPGRADE_HEIGHT_ACCESSOR` window `[-64, -1]`, leaving the old roof
        // above and the neighboring columns intact.
        let bedrock = Blocks::BEDROCK.default_block_state();
        let mut proto = worldgen_proto();
        // `write_block` requires the worldgen heightmaps (the caller primes
        // them once, as `applyBedrockMask`/`replaceOldBedrock` do).
        proto.get_or_create_heightmap_unprimed(Types::OceanFloorWg);
        proto.get_or_create_heightmap_unprimed(Types::WorldSurfaceWg);
        for y in -64..=4 {
            for x in 0..16 {
                for z in 0..16 {
                    write_block(&mut proto, &BlockPos::new(x, y, z), bedrock);
                }
            }
        }
        // Bit `(z & 15) * 16 + (x & 15)` = bit 0 for (x=0, z=0).
        let mask = BitSet::value_of(&[1i64]);
        let retrogen = BelowZeroRetrogen::new(ChunkStatus::Full, Some(mask));
        retrogen.apply_bedrock_mask(&mut proto);
        // The hole column is air within the upgrade window `[-64, -1]`...
        assert_eq!(
            proto.get_block_state(0, -64, 0),
            Blocks::AIR.default_block_state()
        );
        assert_eq!(
            proto.get_block_state(0, -1, 0),
            Blocks::AIR.default_block_state()
        );
        // ...and untouched above it (the old roof survives).
        assert_eq!(proto.get_block_state(0, 0, 0), bedrock);
        assert_eq!(proto.get_block_state(0, 2, 0), bedrock);
        assert_eq!(proto.get_block_state(0, 4, 0), bedrock);
        // The neighboring column is untouched throughout.
        assert_eq!(proto.get_block_state(1, -64, 0), bedrock);
        assert_eq!(proto.get_block_state(1, 2, 0), bedrock);
        assert_eq!(proto.get_block_state(1, 4, 0), bedrock);
    }
}
