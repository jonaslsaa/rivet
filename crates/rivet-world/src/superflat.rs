//! Deterministic superflat chunk content (issue #100) — the single-spawn-chunk
//! filler whose wire bytes byte-compare against the committed #153 capture
//! fixtures in `rivet-protocol/tests/fixtures/`.
//!
//! Java ground truth: `FlatLevelSource.fillFromNoise` for a 1-layer stone
//! superflat preset (`stone, layer, 1, minecraft:bedrock`'s `minecraft:air`
//! fill is the empty preset; the captured spawn chunk has exactly one stone
//! layer). This module builds the *content* — the 24 `LevelChunkSection`s, the
//! three client heightmaps, and the deterministic full-sky light layers — as
//! pure values generic over the block-state/biome value types. The caller
//! supplies the strategies (which carry the global id maps) and the per-state
//! predicates; `rivet-registry`'s generated block-state table is a dev
//! dependency, so the builder itself stays feature-free.
//!
//! Verified geometry (independent fixture decode; the earlier 4-section/
//! 42-byte-biome interpretation was wrong):
//! - 24 sections (`384 / 16`), min_y -64.
//! - Section index 0 (Y=-4) holds 256 stone blocks at absolute y=-64 (section
//!   y=0) and air everywhere else. Its block container is a 4-bit
//!   `LinearPalette [air, stone]` (the Strategy ladder collapses bits 1..4 to
//!   `FOUR_BITS_LINEAR`), 256 storage longs.
//! - Sections 1..23 are all-air (`00 00` single-value) with plains biome.
//! - Every biome container is a two-byte `SingleValuePalette` with global id 40
//!   (plains, alphabetical biome id).
//! - The three client heightmaps are 9-bit `SimpleBitStorage` (37 longs), all
//!   256 stored values = 1 (`y + 1 - minY` = `-63 + 64`).
//! - Light: min light section -5, 26 light sections; sky layer index 0 is
//!   empty (below the floor), indices 1 and 2 are sky updates (the floor layer
//!   `128 zero bytes then 1920 FF`, then all-FF), block layers 0..2 are empty.

use bytes::BytesMut;

use crate::chunk::data_layer::DataLayer;
use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::chunk::paletted_container::PalettedContainer;
use crate::chunk::strategy::Strategy;
use crate::levelgen::heightmap::{Heightmap, prime_heightmaps};
use crate::lighting::light_update_data::build_light_update_data;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::protocol::game::heightmap_types::HeightmapType;
use rivet_protocol::protocol::game::level_chunk_packet_data::LevelChunkPacketData;
use rivet_protocol::protocol::game::light_update_packet_data::LightUpdatePacketData;

/// `LevelHeightAccessor.getMinY()` for the overworld (superflat).
pub const SUPERFLAT_MIN_Y: i32 = -64;
/// `LevelHeightAccessor.getHeight()` for the overworld (superflat).
pub const SUPERFLAT_HEIGHT: i32 = 384;
/// `getSectionsCount()` — `height / 16`.
pub const SECTION_COUNT: usize = (SUPERFLAT_HEIGHT / 16) as usize;
/// `LevelLightEngine.getMinLightSection()` — `getMinSectionY() - 1`.
pub const MIN_LIGHT_SECTION: i32 = SUPERFLAT_MIN_Y / 16 - 1;
/// `LevelLightEngine.getLightSectionCount()` — `getSectionsCount() + 2`.
pub const LIGHT_SECTION_COUNT: usize = SECTION_COUNT + 2;

/// The superflat chunk content: sections + heightmaps + light, ready to hand
/// to the #94 packet bodies.
pub struct SuperflatChunkContent<
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
> {
    /// The 24 `LevelChunkSection`s, lowest Y first.
    pub sections: Vec<LevelChunkSection<T, B>>,
    /// The three client heightmaps (`(HeightmapType, raw long[])`), ascending
    /// type id (the `EnumMap` order `LevelChunkPacketData` writes).
    pub heightmaps: Vec<(HeightmapType, Vec<i64>)>,
    /// The deterministic full-sky light payload.
    pub light_data: LightUpdatePacketData,
}

impl<
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
> SuperflatChunkContent<T, B>
{
    /// `calculateChunkSize` + `extractChunkData` — the opaque sections buffer
    /// the `LevelChunkPacketData` carries. Java sizes the byte array from the
    /// per-section serialized sizes, then each section writes exactly its
    /// share; the port asserts the same invariant after writing.
    pub fn sections_buffer(&self) -> Vec<u8> {
        let mut buf = FriendlyByteBuf::new(BytesMut::new());
        for section in &self.sections {
            section.write(&mut buf);
        }
        let bytes = buf.into_inner().to_vec();
        let expected: i32 = self.sections.iter().map(|s| s.get_serialized_size()).sum();
        assert_eq!(
            bytes.len() as i32,
            expected,
            "section buffer must be exactly the sum of getSerializedSize()"
        );
        bytes
    }

    /// `new ClientboundLevelChunkPacketData(levelChunk, null)` — the superflat
    /// send payload (no block entities).
    pub fn chunk_packet_data(&self) -> LevelChunkPacketData {
        LevelChunkPacketData::new(self.heightmaps.clone(), self.sections_buffer(), vec![])
    }
}

/// The `BlockBehaviour` flag predicates the superflat build resolves per state
/// (the content is air + stone, so they are exact for it; the owning world
/// units replace them with real behavior). Grouped so `build_superflat` takes
/// one parameter instead of four predicates.
pub struct BlockFlags<T: 'static> {
    /// `state.isAir()`.
    pub is_air: &'static dyn Fn(&T) -> bool,
    /// `state.blocksMotion()`.
    pub blocks_motion: &'static dyn Fn(&T) -> bool,
    /// `!state.getFluidState().isEmpty()` — true when the state has a non-empty
    /// fluid state (the `MotionBlocking` heightmap predicate's second disjunct).
    pub has_fluid: &'static dyn Fn(&T) -> bool,
    /// `state.is(BlockTags.LEAVES)`.
    pub is_leaves: &'static dyn Fn(&T) -> bool,
}

/// Builds the deterministic single-stone superflat chunk content.
///
/// `block_strategy`/`biome_strategy` are the `Strategy::create_for_*` values
/// carrying the caller's global id maps; `air`/`stone`/`plains` are the values
/// to place; `flags` resolves the per-state predicates for the heightmap/recalc
/// walks.
pub fn build_superflat<T, B>(
    block_strategy: Strategy<T>,
    biome_strategy: Strategy<B>,
    air: T,
    stone: T,
    plains: B,
    flags: BlockFlags<T>,
) -> SuperflatChunkContent<T, B>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
{
    let mut sections = Vec::with_capacity(SECTION_COUNT);
    // Section index 0 (Y=-4): the single stone layer at absolute y=-64.
    let mut states = PalettedContainer::new(air.clone(), block_strategy.clone());
    for z in 0..16 {
        for x in 0..16 {
            states.set(x, 0, z, stone.clone());
        }
    }
    let biomes = PalettedContainer::new(plains.clone(), biome_strategy.clone());
    sections.push(LevelChunkSection::new(states, biomes, flags.is_air));
    // Sections 1..23: all air, plains biome.
    for _ in 1..SECTION_COUNT {
        let states = PalettedContainer::new(air.clone(), block_strategy.clone());
        let biomes = PalettedContainer::new(plains.clone(), biome_strategy.clone());
        sections.push(LevelChunkSection::new(states, biomes, flags.is_air));
    }

    let heightmaps = heightmaps_for_sections(&sections, SUPERFLAT_MIN_Y, SUPERFLAT_HEIGHT, &flags);

    let light_data = superflat_light_data();

    SuperflatChunkContent {
        sections,
        heightmaps,
        light_data,
    }
}

/// The deterministic full-sky light payload for a superflat chunk — the light
/// the `ClientboundLevelChunkWithLightPacket` body carries (Java queries the
/// `LevelLightEngine`; the engine is not ported, so the M1 send path uses the
/// fixed superflat light).
///
/// RivetTodo(#184): the real `LevelLightEngine` replaces this filler when the
/// lighting engine unit lands.
pub fn superflat_light_data() -> LightUpdatePacketData {
    build_light_update_data(&superflat_sky_layers(), &superflat_block_layers())
}

/// `primeHeightmaps` for a superflat chunk: the topmost opaque block per column
/// drives each of the three client heightmaps. The stone layer at y=-64 is the
/// only non-air block, so every column stores height `-63` (offset 1).
fn heightmaps_for_sections<T, B>(
    sections: &[LevelChunkSection<T, B>],
    min_y: i32,
    height: i32,
    flags: &BlockFlags<T>,
) -> Vec<(HeightmapType, Vec<i64>)>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
{
    // `getHighestFilledSectionIndex` -> `getHighestSectionPosition()`: the top
    // block coordinate of the highest non-air section (Java: `-1` -> minY,
    // else `sectionToBlockCoord(sectionY)`).
    let highest_filled = sections.iter().rposition(|s| !s.has_only_air());
    let highest_section_position = match highest_filled {
        None => min_y,
        Some(index) => min_y + (index as i32) * 16,
    };
    // Java's `primeHeightmaps` walks `y` from `getHighestSectionPosition() + 16
    // - 1` down to `getMinY()`. For the superflat stone section at index 0 the
    // scan starts at -49 and reaches the stone at y=-64 inclusive.
    let scan_top = highest_section_position + 16 - 1;
    prime_heightmaps(height, min_y, |ty, x, z| {
        for y in (min_y..=scan_top).rev() {
            let state = block_state_at(sections, min_y, x, y, z);
            if Heightmap::is_opaque(
                ty,
                (flags.is_air)(&state),
                (flags.blocks_motion)(&state),
                (flags.has_fluid)(&state),
                (flags.is_leaves)(&state),
            ) {
                return Some(y);
            }
        }
        None
    })
}

/// `ChunkAccess.getBlockState(BlockPos)` for an absolute y, resolved through
/// the section array (`sectionIndex = (y - minY) >> 4`).
fn block_state_at<T, B>(
    sections: &[LevelChunkSection<T, B>],
    min_y: i32,
    x: i32,
    y: i32,
    z: i32,
) -> T
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
{
    let rel = y - min_y;
    let index = (rel / 16) as usize;
    let rel_y = rel % 16;
    sections[index].get_block_state(x, rel_y, z)
}

/// The 26 sky light layers, indexed by light-section index (`minLightSection`
/// plus `index`). Index 0 (below the world floor) is empty (uniform 0). Index 1
/// is the floor layer: the y=-64 stone floor blocks sky light, so the first 128
/// bytes (one 16×16 y-level) are zero and everything above is fully lit (1920
/// `0xFF` bytes). Index 2 (the next section, all air) is fully lit. All higher
/// layers are absent (`None`), matching the captured fixture.
fn superflat_sky_layers() -> Vec<Option<DataLayer>> {
    let mut floor = vec![0u8; 128];
    floor.extend(vec![0xFFu8; 1920]);
    let mut layers = vec![None; LIGHT_SECTION_COUNT];
    layers[0] = Some(DataLayer::new(0));
    layers[1] = Some(DataLayer::with_data(floor));
    layers[2] = Some(DataLayer::new(15));
    layers
}

/// The 26 block light layers: indices 0..2 exist and are empty (uniform 0),
/// everything else absent — a fresh superflat chunk has no block light.
fn superflat_block_layers() -> Vec<Option<DataLayer>> {
    let mut layers = vec![None; LIGHT_SECTION_COUNT];
    layers[0] = Some(DataLayer::new(0));
    layers[1] = Some(DataLayer::new(0));
    layers[2] = Some(DataLayer::new(0));
    layers
}
