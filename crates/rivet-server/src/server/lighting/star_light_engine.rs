//! `ca.spottedleaf.moonrise.patches.starlight.light.StarLightEngine` — the
//! Starlight flood-fill light engine core (MC 26.2, Paper).
//!
//! Java source: `working/Paper/.../starlight/light/StarLightEngine.java` and
//! `SkyStarLightEngine.java`. This module ports the engine's shared machinery —
//! the direction enum, the chunk/section/nibble caches, the queue encoding,
//! `light`, `handleEmptySectionChanges`, `lightChunk`, `performLightIncrease`,
//! `performLightDecrease`, `propagateNeighbourLevels` and the flag constants —
//! plus the skylight engine's overrides (`initNibble`, `setNibbleNull`,
//! `rewriteNibbleCacheForSkylight`, `checkNullSection`, `getLightLevelExtruded`,
//! `tryPropagateSkylight`, and the delayed-light-set write-backs
//! `processDelayedIncreases`/`processDelayedDecreases`).
//!
//! #184 (M2) slices the Starlight compute engine in on the light-chunk path:
//! the concrete engine is [`SkyStarLightEngine`] and the block-engine branches
//! of the shared methods are not reachable from it. The seams the base leaves
//! open (the abstract hooks `getEmptinessMap`/`setEmptinessMap`,
//! `getNibblesOnChunk`/`setNibbles`, `canUseChunk`) are implemented through the
//! provider's [`ChunkAccessor`] and the write-back hooks (`pending_nibbles` /
//! `pending_emptiness_map`), keeping the engine itself free of a concrete chunk
//! type. The visibility callback (`updateVisible`'s `onLightUpdate`) is not
//! ported: the engine publishes the *visible* state per section (via
//! `SwmrNibbleArray::update_visible`) and the provider owns the chunk, so there
//! is no external listener to notify.
//!
//! Fidelity notes (PORTING.md):
//! - All coordinate arithmetic uses Java's plain wrapping `+`/`-`/`*` (the port
//!   keeps `i32` and relies on Rust's two's-complement wrapping for the `&`
//!   masks), and the queue entries pack into `u64` exactly as Java's `long`.
//! - The face-occlusion branches (`isConditionallyFullOpaque` +
//!   `getFaceOcclusionShape` + `Shapes.faceShapeOccludes`) are NOT ported
//!   (`VoxelShape` is not). A conditionally-full-opaque *propagated-into* block
//!   is carried as the `FLAG_HAS_SIDED_TRANSPARENT_BLOCKS` flag and the light
//!   propagates through it unconditionally (set in both
//!   `perform_light_increase` branches), where Java tests the culling shape and
//!   skips a neighbour whose face occludes — the seam over-propagates there.
//!   The *source* (from-shape) and column-walk branches instead assume occlusion
//!   (`continue`/`break`), matching Java's skip when the face shape occludes.
//!   Neither deviation affects the superflat air + stone content (neither block
//!   sets the flag), so the byte-exact contract is unaffected (the tests use
//!   only air/stone).
//! - `checkChunkEdges` (the `needsEdgeChecks=true` branch's per-edge decrease)
//!   is not ported — `relightChunks`, `checkChunkEdges` and the client-side
//!   notify path (`isClientSide`) defer with the edge-check unit (RivetTodo
//!   #184 below); `lightChunk` on the edge-checks path runs the propagation that
//!   is shared with the no-edge-checks path and skips the per-edge decrease.
//!
//! RivetTodo(#184): `blockChange`/`sectionChange` live re-lighting, the block
//! engine, the face-occlusion shape test, `relightChunks`, `checkChunkEdges`
//! and the client-side notify path (`isClientSide`) are not ported. The
//! remaining `StarLightProvider` mutators (`block_change`, `section_change`,
//! `relight_chunks`, `check_chunk_edges`) are the no-ops the provider keeps for
//! the deferred paths, and this engine is exercised through the provider's
//! light-chunk path. The light-only branches of the shared methods are reachable
//! from `SkyLightProvider`; the block-engine branches and the deferred edges
//! still read as dead code to the compiler, so the allow stays.
#![allow(dead_code)]

use std::collections::HashMap;

use crate::server::level::level_chunk::{BiomeId as ServerBiomeId, StateId, StructureKey};
use rivet_registry::block_state::BlockState;
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::level_chunk_section::LevelChunkSection;
use rivet_world::level::height_accessor::LevelHeightAccessor;
use rivet_world::lighting::swmr_nibble_array::SwmrNibbleArray;

const QUEUE_COORDINATE_MASK: u64 = (1u64 << (6 + 6 + 16)) - 1;

#[inline]
fn cache_index(chunk_x: i32, chunk_y: i32, chunk_z: i32, offset: i32) -> usize {
    chunk_x
        .wrapping_add(chunk_z.wrapping_mul(5))
        .wrapping_add(chunk_y.wrapping_mul(25))
        .wrapping_add(offset) as usize
}

#[inline]
fn encode_queue_position(x: i32, y: i32, z: i32, offset: i32) -> u64 {
    x.wrapping_add(z.wrapping_shl(6))
        .wrapping_add(y.wrapping_shl(12))
        .wrapping_add(offset) as u64
        & QUEUE_COORDINATE_MASK
}

/// The six propagation directions, in Java declaration order (the ordinal is
/// the bit position in the propagation bitset). The opposite pairs differ only
/// in the low bit, so `ordinal ^ 1` flips to the opposite direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum AxisDirection {
    /// +X (EAST).
    PositiveX,
    /// -X (WEST).
    NegativeX,
    /// +Z (SOUTH).
    PositiveZ,
    /// -Z (NORTH).
    NegativeZ,
    /// +Y (UP).
    PositiveY,
    /// -Y (DOWN).
    NegativeY,
}

impl AxisDirection {
    const ALL: [AxisDirection; 6] = [
        AxisDirection::PositiveX,
        AxisDirection::NegativeX,
        AxisDirection::PositiveZ,
        AxisDirection::NegativeZ,
        AxisDirection::PositiveY,
        AxisDirection::NegativeY,
    ];

    const ONLY_HORIZONTAL: [AxisDirection; 4] = [
        AxisDirection::PositiveX,
        AxisDirection::NegativeX,
        AxisDirection::PositiveZ,
        AxisDirection::NegativeZ,
    ];

    /// `x` — the direction's X delta (`-1`, `0`, or `1`).
    const fn x(self) -> i32 {
        match self {
            AxisDirection::PositiveX => 1,
            AxisDirection::NegativeX => -1,
            _ => 0,
        }
    }

    /// `z` — the direction's Z delta.
    const fn z(self) -> i32 {
        match self {
            AxisDirection::PositiveZ => 1,
            AxisDirection::NegativeZ => -1,
            _ => 0,
        }
    }

    /// `y` — the direction's Y delta.
    const fn y(self) -> i32 {
        match self {
            AxisDirection::PositiveY => 1,
            AxisDirection::NegativeY => -1,
            _ => 0,
        }
    }

    /// `getOpposite()` — the direction's index (`ordinal ^ 1`).
    const fn opposite(self) -> usize {
        (self as usize) ^ 1
    }

    /// `everythingButThisDirection` — the direction bitset excluding this
    /// direction's bit (`ALL_DIRECTIONS_BITSET ^ (1 << ordinal)`).
    const fn everything_but_this_direction(self) -> u64 {
        (ALL_DIRECTIONS_BITSET ^ (1 << self as usize)) as u64
    }

    /// `everythingButTheOppositeDirection` — the bitset excluding the opposite
    /// direction's bit (propagation in this direction's "forward" sense stops
    /// at the boundary the light came from).
    const fn everything_but_the_opposite_direction(self) -> u64 {
        (ALL_DIRECTIONS_BITSET ^ (1 << self.opposite())) as u64
    }

    /// The single-bit set containing only this direction's bit.
    const fn as_single_bit(self) -> u64 {
        1u64 << (self as usize)
    }
}

/// `ALL_DIRECTIONS_BITSET` — all six direction bits set.
pub(crate) const ALL_DIRECTIONS_BITSET: usize = (1 << 6) - 1;

/// Queue flag: the increase propagator must write the propagated level to the
/// position (restores block sources after a decrease).
const FLAG_WRITE_LEVEL: u64 = i64::MIN as u64 >> 2;
/// Queue flag: the increase propagator must re-check the position's current
/// level equals the expected level before writing.
const FLAG_RECHECK_LEVEL: u64 = i64::MIN as u64 >> 1;
/// Queue flag: the position has sided-transparent (conditionally full-opaque)
/// blocks, so the propagator must consult the directional shape test.
const FLAG_HAS_SIDED_TRANSPARENT_BLOCKS: u64 = i64::MIN as u64;

/// `OLD_CHECK_DIRECTIONS[bitset]` — the directions whose bits are set in
/// `bitset`, in ascending ordinal order (Java's `IntegerUtil.trailingZeros`
/// iteration). Compiled once for every `bitset` in `0..64`.
fn old_check_directions(bitset: usize) -> &'static [AxisDirection] {
    const LEN: [u8; 64] = {
        let mut len = [0u8; 64];
        let mut i = 0usize;
        while i < 64 {
            len[i] = i.count_ones() as u8;
            i += 1;
        }
        len
    };
    const TABLE: [[AxisDirection; 6]; 64] = {
        let mut table = [[AxisDirection::PositiveX; 6]; 64];
        let mut bitset = 0usize;
        while bitset < 64 {
            let mut remaining = bitset;
            let mut index = 0usize;
            while remaining != 0 {
                let trailing = remaining.trailing_zeros() as usize;
                table[bitset][index] = AxisDirection::ALL[trailing];
                index += 1;
                remaining &= remaining - 1;
            }
            bitset += 1;
        }
        table
    };
    &TABLE[bitset][..LEN[bitset] as usize]
}

/// The per-state light-property accessors the engine needs, resolved through
/// [`BlockState`] (which reads the generated behavior table).
pub(crate) trait BlockStateLight {
    /// `state.getLightDampening()` in `0..=15`.
    fn light_dampening(&self) -> i32;
    /// `state.getLightEmission()` in `0..=15`.
    fn light_emission(&self) -> i32;
    /// `state.isConditionallyFullOpaque()` (the `Starlight` injected surface) —
    /// `canOcclude` AND `useShapeForLightOcclusion`. The shape test itself
    /// defers with #184, so this flag selects the flag-only propagation branch
    /// (exact for the superflat air + stone content, neither of which sets it).
    fn conditionally_full_opaque(&self) -> bool;
}

impl BlockStateLight for StateId {
    fn light_dampening(&self) -> i32 {
        BlockState::new(*self).light_dampening() as i32
    }

    fn light_emission(&self) -> i32 {
        BlockState::new(*self).light_emission() as i32
    }

    fn conditionally_full_opaque(&self) -> bool {
        let state = BlockState::new(*self);
        state.can_occlude() && state.use_shape_for_light_occlusion()
    }
}

/// The chunk-access seam the engine uses to resolve the radius-1/2 neighbour
/// chunks during `setupCaches` (Java's `LightChunkGetter.getChunkForLighting`).
/// The provider implements this; the engine never stores the returned chunk
/// (the caches hold owned nibbles/sections/emptiness snapshots), so there is
/// no lifetime coupling between a cached surface and the resolved chunk.
pub(crate) trait ChunkAccessor {
    /// `LightChunkGetter.getChunkForLighting(chunkX, chunkZ)` — the chunk at
    /// the given chunk coordinates, or `None` when it is not loaded/usable.
    fn get_chunk_for_lighting(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Option<&ChunkAccess<StateId, ServerBiomeId, StructureKey>>;
}

/// A section-content snapshot the engine reads block states from. Java's
/// `sectionCache` holds references to the chunks' `LevelChunkSection`s; the
/// Rust cache cannot borrow across the [`ChunkAccessor`] seam, so it snapshots
/// exactly the two surfaces the engine reads during a run: the air flag and the
/// raw block-state ids (`getStates().get(index)`).
#[derive(Clone, Debug)]
struct SectionSnapshot {
    /// `LevelChunkSection.hasOnlyAir()`.
    has_only_air: bool,
    /// The raw block-state id for every local index; empty when
    /// `has_only_air` (the engine never reads the states of an air section —
    /// `getBlockState` returns AIR for it).
    states: Vec<StateId>,
}

impl SectionSnapshot {
    fn of(section: &LevelChunkSection<StateId, ServerBiomeId>) -> Self {
        let has_only_air = section.has_only_air();
        let states = if has_only_air {
            Vec::new()
        } else {
            (0..(16 * 16 * 16))
                .map(|i| section.states().get_index(i))
                .collect()
        };
        SectionSnapshot {
            has_only_air,
            states,
        }
    }
}

/// The post-run light state for one cached neighbour. Java's cache entries
/// alias the neighbour's live nibbles, so `updateVisible` publishes mutations
/// to that chunk even though `light()` calls `setNibbles` only for the center.
/// Rust snapshots must carry those visible nibbles back explicitly before the
/// provider restores the owned runtime chunk.
#[derive(Default)]
pub(crate) struct NeighborLightUpdate {
    /// Only source-owned sections that still have a cache entry are returned.
    /// Skylight's rewrite intentionally removes null sections and null-section
    /// propagation can create scratch entries; neither should replace the
    /// neighbour's original nibble array.
    pub(crate) nibbles: Vec<(usize, SwmrNibbleArray)>,
    pub(crate) emptiness_map: Option<Vec<bool>>,
}

/// The sky light engine — `SkyStarLightEngine` on the light-chunk path. Owns
/// the per-run scratch caches (chunk/section/nibble/emptiness arrays, the two
/// FIFO queues, the null-propagation check) exactly as the Java instance does;
/// each [`SkyStarLightEngine::light`] run re-`setupCaches`s them around the
/// center chunk.
///
/// The engine is confined to a single run (the provider's `light_chunk`): it is
/// not `Send`-shared, matching OWNERSHIP.md.
pub(crate) struct SkyStarLightEngine {
    /// `minLightSection` — `WorldUtil.getMinLightSection(world)`.
    min_light_section: i32,
    /// `maxLightSection` — `WorldUtil.getMaxLightSection(world)` (inclusive).
    max_light_section: i32,
    /// `minSection` — `WorldUtil.getMinSection(world)`.
    min_section: i32,
    /// `maxSection` — `WorldUtil.getMaxSection(world)`.
    max_section: i32,

    // --- the Java caches, sized per `setWorld`/`setupCaches` ---
    /// `sectionCache`, indexed `x + 5*z + 25*y + chunkSectionIndexOffset`.
    /// Owned [`SectionSnapshot`]s (see its doc for why the cache does not hold
    /// the chunk's live sections).
    section_cache: Vec<Option<SectionSnapshot>>,
    /// `nibbleCache`, indexed like `sectionCache`.
    nibble_cache: Vec<Option<SwmrNibbleArray>>,
    /// Whether each cached nibble originated from an array supplied by its
    /// source chunk. `initNibble(..., initRemovedNibbles=true)` creates a
    /// scratch array when a null entry is needed for propagation; Java never
    /// writes that replacement back to a neighbour, so those entries must not
    /// be published during the provider's explicit write-back.
    nibble_cache_writeback: Vec<bool>,
    /// A source nibble removed by `rewriteNibbleCacheForSkylight`. Java calls
    /// `updateVisible` on the source object before dropping its cache entry, so
    /// retain that post-publication value for the provider's neighbour write-back.
    dropped_nibble_writeback: Vec<Option<SwmrNibbleArray>>,
    /// `chunkCache`, indexed `x + 5*z + chunkIndexOffset`. Java holds the
    /// chunk reference; the only engine read is the presence check in
    /// `initNibble`, so the cache is a membership flag.
    chunk_cache: Vec<bool>,
    /// `emptinessMapCache`, indexed like `chunkCache`.
    emptiness_map_cache: Vec<Option<Vec<bool>>>,
    /// `nullPropagationCheckCache` — per-light-section flag: whether this
    /// section's null-propagation check already ran this run.
    null_propagation_check_cache: Vec<bool>,
    /// `rewriteNibbleCacheForSkylight`'s effect on the center chunk: which
    /// light sections the rewrite nulled out of the cache. Java writes the
    /// *original* array back to the chunk, so a section the rewrite nulled (and
    /// `checkNullSection` may have re-created as a fresh scratch nibble) keeps
    /// the untouched original `Null` nibble; only sections the cache still
    /// aliases carry the in-place mutations. Indexed by `chunkY -
    /// minLightSection`, reset per `light` run.
    nulled_sections: Vec<bool>,

    // --- encode offsets, recomputed per `setupEncodeOffset` ---
    encode_offset_x: i32,
    encode_offset_y: i32,
    encode_offset_z: i32,
    coordinate_offset: i32,
    chunk_offset_x: i32,
    chunk_offset_y: i32,
    chunk_offset_z: i32,
    chunk_index_offset: i32,
    chunk_section_index_offset: i32,

    /// `increaseQueue` + `increaseQueueInitialLength` (Java's `long[]` + tail).
    increase_queue: Vec<u64>,
    increase_queue_initial_length: usize,
    /// `decreaseQueue` + `decreaseQueueInitialLength`.
    decrease_queue: Vec<u64>,
    decrease_queue_initial_length: usize,

    /// The sections-per-chunk the caches are sized to hold (Java's
    /// `ySections + 2 + 2` = `maxLightSection - minLightSection + 1`).
    light_section_count: usize,
    /// `isClientSide` — always false (server); the client notify path defers.
    is_client_side: bool,

    /// `setNibbles(chunk, to)` write-back — the computed center-chunk nibbles,
    /// surfaced for the provider to publish onto the chunk.
    pending_nibbles: Option<Vec<SwmrNibbleArray>>,
    /// `setEmptinessMap(chunk, to)` write-back — the recomputed sky-emptiness
    /// map, surfaced for the provider to publish onto the chunk.
    pending_emptiness_map: Option<Vec<bool>>,
    /// Cache mutations for owned neighbours. Java's nibble cache aliases the
    /// live neighbour arrays; the Rust cache owns clones, so the provider must
    /// publish these states before it restores the taken chunks.
    pending_neighbor_updates: HashMap<(i32, i32), NeighborLightUpdate>,
}

impl SkyStarLightEngine {
    /// `new SkyStarLightEngine(); setWorld(world)` — built with the world's
    /// vertical extent (`SimpleLevelHeightAccessor`), so the engine's section
    /// bounds match the chunk's.
    pub(crate) fn new(accessor: &dyn LevelHeightAccessor) -> Self {
        let min_section = accessor.get_min_section_y();
        let max_section = accessor.get_max_section_y();
        let min_light_section = min_section.wrapping_sub(1);
        let max_light_section = max_section.wrapping_add(1);
        let light_section_count = (max_light_section
            .wrapping_sub(min_light_section)
            .wrapping_add(1)) as usize;
        let min_array_size = 5 * 5 * (light_section_count + 2);
        SkyStarLightEngine {
            min_light_section,
            max_light_section,
            min_section,
            max_section,
            section_cache: (0..min_array_size).map(|_| None).collect(),
            nibble_cache: (0..min_array_size).map(|_| None).collect(),
            nibble_cache_writeback: vec![false; min_array_size],
            dropped_nibble_writeback: (0..min_array_size).map(|_| None).collect(),
            chunk_cache: vec![false; 25],
            emptiness_map_cache: (0..25).map(|_| None).collect(),
            null_propagation_check_cache: vec![false; light_section_count],
            nulled_sections: vec![false; light_section_count],
            encode_offset_x: 0,
            encode_offset_y: 0,
            encode_offset_z: 0,
            coordinate_offset: 0,
            chunk_offset_x: 0,
            chunk_offset_y: 0,
            chunk_offset_z: 0,
            chunk_index_offset: 0,
            chunk_section_index_offset: 0,
            increase_queue: vec![0u64; 16 * 16 * 16],
            increase_queue_initial_length: 0,
            decrease_queue: vec![0u64; 16 * 16 * 16],
            decrease_queue_initial_length: 0,
            light_section_count,
            is_client_side: false,
            pending_nibbles: None,
            pending_emptiness_map: None,
            pending_neighbor_updates: HashMap::new(),
        }
    }

    /// `setupEncodeOffset(centerX, centerY, centerZ)` — Java's per-run center
    /// encoding; the offsets make the center chunk's blocks encode into the
    /// queue's 28-bit coordinate space.
    fn setup_encode_offset(&mut self, center_x: i32, _center_y: i32, center_z: i32) {
        self.encode_offset_x = 31i32.wrapping_sub(center_x);
        self.encode_offset_y = self
            .min_light_section
            .wrapping_sub(1)
            .wrapping_neg()
            .wrapping_shl(4);
        self.encode_offset_z = 31i32.wrapping_sub(center_z);
        self.coordinate_offset = self
            .encode_offset_x
            .wrapping_add(self.encode_offset_z.wrapping_shl(6))
            .wrapping_add(self.encode_offset_y.wrapping_shl(12));
        self.chunk_offset_x = 2i32.wrapping_sub(center_x >> 4);
        self.chunk_offset_y = self.min_light_section.wrapping_sub(1).wrapping_neg();
        self.chunk_offset_z = 2i32.wrapping_sub(center_z >> 4);
        self.chunk_index_offset = self
            .chunk_offset_x
            .wrapping_add(self.chunk_offset_z.wrapping_mul(5));
        self.chunk_section_index_offset = self
            .chunk_index_offset
            .wrapping_add(self.chunk_offset_y.wrapping_mul(25));
    }

    /// `setupCaches(lightAccess, centerX, centerY, centerZ, relaxed,
    /// tryToLoadChunksFor2Radius)` — populate the chunk/section/nibble caches
    /// for the 1- (or 2-) radius neighbourhood around the center chunk. The
    /// provider resolves chunks through [`ChunkAccessor`], and `canUseChunk`
    /// filters (a chunk at LIGHT status + light-correct).
    fn setup_caches(
        &mut self,
        provider: &mut dyn ChunkAccessor,
        center_x: i32,
        center_y: i32,
        center_z: i32,
        relaxed: bool,
        try_to_load_chunks_for_2_radius: bool,
    ) {
        self.setup_encode_offset(
            (center_x >> 4).wrapping_mul(16).wrapping_add(7),
            (center_y >> 4).wrapping_mul(16).wrapping_add(7),
            (center_z >> 4).wrapping_mul(16).wrapping_add(7),
        );
        let radius = if try_to_load_chunks_for_2_radius {
            2
        } else {
            1
        };
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let cx = (center_x >> 4).wrapping_add(dx);
                let cz = (center_z >> 4).wrapping_add(dz);
                let is_two_radius = dx.abs().max(dz.abs()) == 2;
                let Some(chunk) = provider.get_chunk_for_lighting(cx, cz) else {
                    if relaxed | is_two_radius {
                        continue;
                    }
                    panic!("Trying to propagate light update before 1 radius neighbours ready");
                };
                if !self.can_use_chunk(chunk) {
                    continue;
                }
                self.set_chunk_in_cache(cx, cz);
                self.set_emptiness_map_cache(cx, cz, self.get_emptiness_map_from_chunk(chunk));
                if !is_two_radius {
                    self.set_blocks_for_chunk_in_cache(cx, cz, chunk);
                    let nibbles = self.get_nibbles_on_chunk(chunk);
                    self.set_nibbles_for_chunk_in_cache(cx, cz, &nibbles);
                }
            }
        }
    }

    /// `SkyStarLightEngine.canUseChunk(chunk)` — `status.isOrAfter(LIGHT) &&
    /// (isClientSide || isLightCorrect())` on the server. The light-chunk run
    /// itself is exempt because the pipeline calls `lightChunk` before the
    /// chunk is light-correct; the provider drives `light` on the center chunk
    /// regardless of this gate (Java forces it into the cache) and
    /// `setup_caches` filters only neighbours.
    fn can_use_chunk(&self, chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>) -> bool {
        // The generated pipeline does not stamp persisted status yet (#185);
        // neighbours are trusted when light-correct, matching Java's server
        // `canUseChunk` (status LIGHT is the only non-light-correct status the
        // engine trusts for neighbours, and the pipeline has none of those yet).
        chunk.is_light_correct()
    }

    // --- chunk / section / nibble cache accessors (indexed like Java) ---

    fn is_chunk_in_cache(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.chunk_cache[cache_index(chunk_x, 0, chunk_z, self.chunk_index_offset)]
    }

    fn set_chunk_in_cache(&mut self, chunk_x: i32, chunk_z: i32) {
        let idx = cache_index(chunk_x, 0, chunk_z, self.chunk_index_offset);
        self.chunk_cache[idx] = true;
    }

    fn set_emptiness_map_cache(&mut self, chunk_x: i32, chunk_z: i32, map: Option<Vec<bool>>) {
        let idx = cache_index(chunk_x, 0, chunk_z, self.chunk_index_offset);
        self.emptiness_map_cache[idx] = map;
    }

    fn get_emptiness_map(&self, chunk_x: i32, chunk_z: i32) -> Option<&Vec<bool>> {
        self.emptiness_map_cache[cache_index(chunk_x, 0, chunk_z, self.chunk_index_offset)].as_ref()
    }

    fn get_chunk_section(
        &self,
        chunk_x: i32,
        chunk_y: i32,
        chunk_z: i32,
    ) -> Option<&SectionSnapshot> {
        let idx = cache_index(chunk_x, chunk_y, chunk_z, self.chunk_section_index_offset);
        self.section_cache[idx].as_ref()
    }

    fn set_blocks_for_chunk_in_cache(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
    ) {
        for cy in self.min_light_section..=self.max_light_section {
            let section = if cy >= self.min_section && cy <= self.max_section {
                chunk
                    .get_sections()
                    .get((cy.wrapping_sub(self.min_section)) as usize)
                    .map(SectionSnapshot::of)
            } else {
                None
            };
            let idx = cache_index(chunk_x, cy, chunk_z, self.chunk_section_index_offset);
            self.section_cache[idx] = section;
        }
    }

    fn get_nibble_from_cache(
        &self,
        chunk_x: i32,
        chunk_y: i32,
        chunk_z: i32,
    ) -> Option<&SwmrNibbleArray> {
        let idx = cache_index(chunk_x, chunk_y, chunk_z, self.chunk_section_index_offset);
        self.nibble_cache[idx].as_ref()
    }

    fn get_nibble_from_cache_mut(
        &mut self,
        chunk_x: i32,
        chunk_y: i32,
        chunk_z: i32,
    ) -> Option<&mut SwmrNibbleArray> {
        let idx = cache_index(chunk_x, chunk_y, chunk_z, self.chunk_section_index_offset);
        self.nibble_cache[idx].as_mut()
    }

    fn set_nibble_in_cache(
        &mut self,
        chunk_x: i32,
        chunk_y: i32,
        chunk_z: i32,
        nibble: Option<SwmrNibbleArray>,
    ) {
        let idx = cache_index(chunk_x, chunk_y, chunk_z, self.chunk_section_index_offset);
        self.nibble_cache[idx] = nibble;
        // This setter is also used for scratch arrays created by
        // `initNibble`. Those arrays do not belong to the source chunk and
        // therefore are not eligible for neighbour write-back. The initial
        // source population below marks its aliases explicitly.
        self.nibble_cache_writeback[idx] = false;
    }

    fn set_nibbles_for_chunk_in_cache(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        nibbles: &[SwmrNibbleArray],
    ) {
        for (index, cy) in (self.min_light_section..=self.max_light_section).enumerate() {
            let idx = cache_index(chunk_x, cy, chunk_z, self.chunk_section_index_offset);
            self.nibble_cache[idx] = nibbles.get(index).cloned();
            self.dropped_nibble_writeback[idx] = None;
            // A cached source nibble can be published back to that chunk. A
            // missing source section is represented by no alias, even when a
            // later null-propagation pass creates a scratch nibble.
            self.nibble_cache_writeback[idx] = self.nibble_cache[idx].is_some();
        }
    }

    // --- block state / light level accessors ---

    /// `getBlockState(sectionIndex, localIndex)` — the cached section's state,
    /// `AIR` for a null (unloaded / out-of-bounds) section or an air-only one.
    /// `StateId(0)` is air.
    fn get_block_state(&self, section_index: usize, local_index: usize) -> StateId {
        match self.section_cache[section_index].as_ref() {
            Some(section) if !section.has_only_air => section.states[local_index],
            _ => StateId(0),
        }
    }

    /// `getBlockState(worldX, worldY, worldZ)`.
    fn get_block_state_at(&self, world_x: i32, world_y: i32, world_z: i32) -> StateId {
        let index = cache_index(
            world_x >> 4,
            world_y >> 4,
            world_z >> 4,
            self.chunk_section_index_offset,
        );
        self.get_block_state(
            index,
            ((world_x & 15) | ((world_z & 15) << 4) | ((world_y & 15) << 8)) as usize,
        )
    }

    /// `getLightLevel(worldX, worldY, worldZ)` — the updating (writer-side)
    /// light at the position, 0 when the section nibble is null.
    fn get_light_level(&self, world_x: i32, world_y: i32, world_z: i32) -> i32 {
        let index = cache_index(
            world_x >> 4,
            world_y >> 4,
            world_z >> 4,
            self.chunk_section_index_offset,
        );
        self.get_light_level_index(
            index,
            ((world_x & 15) | ((world_z & 15) << 4) | ((world_y & 15) << 8)) as usize,
        )
    }

    fn get_light_level_index(&self, section_index: usize, local_index: usize) -> i32 {
        match self.nibble_cache[section_index].as_ref() {
            Some(nibble) => nibble.get_updating_index(local_index),
            None => 0,
        }
    }

    /// `setLightLevel(worldX, worldY, worldZ, level)` — CoW-write the updating
    /// nibble. The server has no client notify path.
    fn set_light_level(&mut self, world_x: i32, world_y: i32, world_z: i32, level: i32) {
        let index = cache_index(
            world_x >> 4,
            world_y >> 4,
            world_z >> 4,
            self.chunk_section_index_offset,
        );
        if let Some(nibble) = self.nibble_cache[index].as_mut() {
            nibble.set(world_x, world_y, world_z, level);
        }
    }

    fn set_light_level_index(&mut self, section_index: usize, local_index: usize, level: i32) {
        if let Some(nibble) = self.nibble_cache[section_index].as_mut() {
            nibble.set_index(local_index, level);
        }
    }

    // --- queues ---

    /// `resizeIncreaseQueue` — Java's `Math.max(4, len + (len >>> 1))` growth.
    fn resize_increase_queue(&mut self) {
        let len = self.increase_queue.len();
        let new_len = (len + (len >> 1)).max(4);
        self.increase_queue.resize(new_len, 0);
    }

    /// `resizeDecreaseQueue`.
    fn resize_decrease_queue(&mut self) {
        let len = self.decrease_queue.len();
        let new_len = (len + (len >> 1)).max(4);
        self.decrease_queue.resize(new_len, 0);
    }

    /// `appendToIncreaseQueue`.
    fn append_to_increase_queue(&mut self, value: u64) {
        let idx = self.increase_queue_initial_length;
        if idx >= self.increase_queue.len() {
            self.resize_increase_queue();
        }
        self.increase_queue[idx] = value;
        self.increase_queue_initial_length += 1;
    }

    /// `appendToDecreaseQueue`.
    fn append_to_decrease_queue(&mut self, value: u64) {
        let idx = self.decrease_queue_initial_length;
        if idx >= self.decrease_queue.len() {
            self.resize_decrease_queue();
        }
        self.decrease_queue[idx] = value;
        self.decrease_queue_initial_length += 1;
    }

    // --- the abstract hooks the sky engine implements (inline dispatch keeps
    // the shared core faithful to Java's protected methods) ---

    /// `getEmptinessMap(chunk)` — the chunk's stored sky emptiness map
    /// (`StarlightChunk.starlight$getSkyEmptinessMap`). `None` before the sky
    /// engine has computed it; `handle_empty_section_changes` then derives the
    /// map from the empty-section mask, surfacing it via
    /// `set_emptiness_map_on_surface` for the provider to publish.
    fn get_emptiness_map_from_chunk(
        &self,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
    ) -> Option<Vec<bool>> {
        chunk.sky_emptiness_map().map(ToOwned::to_owned)
    }

    /// `setEmptinessMap(chunk, to)` — publish the recomputed sky emptiness map
    /// back onto the chunk. The engine itself cannot mutate the borrowed chunk,
    /// so the provider (which owns the `&mut` chunk) does the write; this hook
    /// carries the value out of the engine.
    fn set_emptiness_map_on_surface(&mut self, to: Vec<bool>) {
        self.pending_emptiness_map = Some(to);
    }

    /// `getNibblesOnChunk(chunk)` — `starlight$getSkyNibbles()`.
    fn get_nibbles_on_chunk(
        &self,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
    ) -> Vec<SwmrNibbleArray> {
        chunk.sky_nibbles().to_vec()
    }

    /// `setNibbles(chunk, to)` — `starlight$setSkyNibbles(to)`. The engine
    /// writes the final nibbles into the chunk through this hook (the provider
    /// owns the `&mut` chunk); Java writes through `ChunkLightTask`.
    fn set_nibbles_on_surface(&mut self, to: Vec<SwmrNibbleArray>) {
        self.pending_nibbles = Some(to);
    }

    /// The provider's read of `pending_nibbles` — take (rather than borrow) so
    /// the next run starts clean, mirroring Java where each `lightChunk` hands
    /// a fresh `setNibbles` array to the chunk.
    pub(crate) fn take_pending_nibbles(&mut self) -> Option<Vec<SwmrNibbleArray>> {
        self.pending_nibbles.take()
    }

    /// The provider's read of `pending_emptiness_map` — take (rather than
    /// borrow) so the next run starts clean.
    pub(crate) fn take_pending_emptiness_map(&mut self) -> Option<Vec<bool>> {
        self.pending_emptiness_map.take()
    }

    /// Take the visible light mutations for cached neighbours. The provider
    /// applies these while the neighbours are still in its transactional take
    /// set, before returning ownership to the caller's storage.
    pub(crate) fn take_pending_neighbor_updates(
        &mut self,
    ) -> HashMap<(i32, i32), NeighborLightUpdate> {
        std::mem::take(&mut self.pending_neighbor_updates)
    }

    /// `initNibble(chunkX, chunkY, chunkZ, extrude, initRemovedNibbles)` —
    /// `SkyStarLightEngine.initNibble`.
    fn init_nibble(
        &mut self,
        chunk_x: i32,
        chunk_y: i32,
        chunk_z: i32,
        extrude: bool,
        init_removed_nibbles: bool,
    ) {
        if chunk_y < self.min_light_section
            || chunk_y > self.max_light_section
            || !self.is_chunk_in_cache(chunk_x, chunk_z)
        {
            return;
        }
        match self.get_nibble_from_cache(chunk_x, chunk_y, chunk_z) {
            None => {
                if !init_removed_nibbles {
                    panic!("nibble removed while not requested");
                }
                // create a fresh null nibble and let the init below fill it
                self.set_nibble_in_cache(
                    chunk_x,
                    chunk_y,
                    chunk_z,
                    Some(SwmrNibbleArray::new_with_bytes_and_null(None, true)),
                );
            }
            Some(nibble) => {
                if !nibble.is_null_nibble_updating() {
                    // already initialised
                    return;
                }
            }
        }
        self.init_nibble_impl(chunk_x, chunk_y, chunk_z, extrude);
    }

    /// `initNibble(SWMRNibbleArray, chunkX, chunkY, chunkZ, extrude)` — the
    /// skylight-specific init: either set the section fully lit (above the
    /// lowest non-empty section) or extrude the light from the first non-null
    /// section above.
    fn init_nibble_impl(&mut self, chunk_x: i32, chunk_y: i32, chunk_z: i32, extrude: bool) {
        let is_null = match self.get_nibble_from_cache(chunk_x, chunk_y, chunk_z) {
            Some(nibble) => nibble.is_null_nibble_updating(),
            None => return,
        };
        if !is_null {
            // already initialised
            return;
        }

        // Determine the lowest non-empty world section of this column. The
        // emptiness map decides when it is populated; otherwise (a neighbour
        // chunk that has not run the light stage) fall back to the section
        // content.
        let mut lowest_y = self.min_light_section.wrapping_sub(1);
        let emptiness_map = self.get_emptiness_map(chunk_x, chunk_z);
        for curr_y in (self.min_section..=self.max_section).rev() {
            let empty = match emptiness_map {
                Some(map) => map[(curr_y.wrapping_sub(self.min_section)) as usize],
                None => self
                    .get_chunk_section(chunk_x, curr_y, chunk_z)
                    .is_none_or(|s| s.has_only_air),
            };
            if empty {
                continue;
            }
            lowest_y = curr_y;
            break;
        }
        if chunk_y > lowest_y {
            // above the lowest non-empty section: fully lit
            let nibble = self
                .get_nibble_from_cache_mut(chunk_x, chunk_y, chunk_z)
                .unwrap();
            nibble.set_non_null();
            nibble.set_full();
            return;
        }
        if extrude {
            // copy the first non-null section's y=0 layer down into this one
            for curr_y in (chunk_y.wrapping_add(1))..=self.max_light_section {
                let above_is_null = match self.get_nibble_from_cache(chunk_x, curr_y, chunk_z) {
                    Some(above) => above.is_null_nibble_updating(),
                    None => true,
                };
                if above_is_null {
                    continue;
                }
                let other = self
                    .get_nibble_from_cache(chunk_x, curr_y, chunk_z)
                    .unwrap()
                    .clone();
                let curr = self
                    .get_nibble_from_cache_mut(chunk_x, chunk_y, chunk_z)
                    .unwrap();
                curr.set_non_null();
                curr.extrude_lower(&other);
                break;
            }
        } else {
            let nibble = self
                .get_nibble_from_cache_mut(chunk_x, chunk_y, chunk_z)
                .unwrap();
            nibble.set_non_null();
        }
    }

    /// `setNibbleNull(chunkX, chunkY, chunkZ)` — `SkyStarLightEngine.setNibbleNull`.
    fn set_nibble_null(&mut self, chunk_x: i32, chunk_y: i32, chunk_z: i32) {
        if let Some(nibble) = self.get_nibble_from_cache_mut(chunk_x, chunk_y, chunk_z) {
            nibble.set_null();
        }
    }

    /// `rewriteNibbleCacheForSkylight(chunk)` — stop propagation through null
    /// sections by dropping them from the cache. Java first calls
    /// `updateVisible` on every dropped source object, so retain that published
    /// value for neighbour write-back; a scratch nibble created later by
    /// `initNibble(..., initRemovedNibbles=true)` still has no source owner and
    /// is never published. The center chunk's dropped light sections are
    /// recorded in `nulled_sections` — the write-back in `light` substitutes
    /// the original null nibble for them (Java's `setNibbles` writes the
    /// original array, so a section the rewrite nulled and `checkNullSection`
    /// re-created never reaches the chunk).
    fn rewrite_nibble_cache_for_skylight(&mut self, chunk_x: i32, chunk_z: i32) {
        let y_divisor = self.light_section_count + 2;
        for index in 0..self.nibble_cache.len() {
            let Some(mut nibble) = self.nibble_cache[index].take() else {
                continue;
            };
            if !nibble.is_null_nibble_updating() {
                self.nibble_cache[index] = Some(nibble);
                continue;
            }
            nibble.update_visible();
            if self.nibble_cache_writeback[index] {
                self.dropped_nibble_writeback[index] = Some(nibble.clone());
            }
            self.nibble_cache_writeback[index] = false;
            let cx = ((index % 5) as i32).wrapping_sub(self.chunk_offset_x);
            let cz = (((index / 5) % 5) as i32).wrapping_sub(self.chunk_offset_z);
            let cy = (((index / 25) % y_divisor) as i32).wrapping_sub(self.chunk_offset_y);
            if cx == chunk_x && cz == chunk_z {
                let rel = (cy.wrapping_sub(self.min_light_section)) as usize;
                if rel < self.nulled_sections.len() {
                    self.nulled_sections[rel] = true;
                }
            }
        }
    }

    /// `checkNullSection(chunkX, chunkY, chunkZ, extrudeInitialised)` — ensure
    /// the null (empty) section's horizontal neighbours that carry light get
    /// their nibbles init'd so propagation can cross the empty section. Rets
    /// whether neighbours were init'd.
    fn check_null_section(
        &mut self,
        chunk_x: i32,
        chunk_y: i32,
        chunk_z: i32,
        extrude_initialised: bool,
    ) -> bool {
        if chunk_y < self.min_light_section
            || chunk_y > self.max_light_section
            || self.null_propagation_check_cache
                [(chunk_y.wrapping_sub(self.min_light_section)) as usize]
        {
            return false;
        }
        self.null_propagation_check_cache
            [(chunk_y.wrapping_sub(self.min_light_section)) as usize] = true;
        let mut need_init_neighbours = false;
        'search: for dz in -1i32..=1 {
            for dx in -1i32..=1 {
                if let Some(nibble) = self.get_nibble_from_cache(
                    dx.wrapping_add(chunk_x),
                    chunk_y,
                    dz.wrapping_add(chunk_z),
                ) && !nibble.is_null_nibble_updating()
                {
                    need_init_neighbours = true;
                    break 'search;
                }
            }
        }
        if need_init_neighbours {
            for dz in -1i32..=1 {
                for dx in -1i32..=1 {
                    // the centre gets the caller's extrude flag, the edges always
                    // extrude (they're guaranteed to have light above)
                    let extrude = if (dx | dz) == 0 {
                        extrude_initialised
                    } else {
                        true
                    };
                    self.init_nibble(
                        dx.wrapping_add(chunk_x),
                        chunk_y,
                        dz.wrapping_add(chunk_z),
                        extrude,
                        true,
                    );
                }
            }
        }
        need_init_neighbours
    }

    /// `getLightLevelExtruded(worldX, worldY, worldZ)` — the sky light at a
    /// position, extruding upward from the first non-null section (the column
    /// above the sky-fill boundary is full 15).
    fn get_light_level_extruded(&self, world_x: i32, world_y: i32, world_z: i32) -> i32 {
        let chunk_x = world_x >> 4;
        let mut chunk_y = world_y >> 4;
        let chunk_z = world_z >> 4;
        if let Some(nibble) = self.get_nibble_from_cache(chunk_x, chunk_y, chunk_z) {
            return nibble.get_updating(world_x, world_y, world_z);
        }
        loop {
            chunk_y = chunk_y.wrapping_add(1);
            if chunk_y > self.max_light_section {
                return 15;
            }
            if let Some(nibble) = self.get_nibble_from_cache(chunk_x, chunk_y, chunk_z) {
                return nibble.get_updating(world_x, 0, world_z);
            }
        }
    }

    /// `tryPropagateSkylight(world, worldX, startY, worldZ, extrudeInitialised,
    /// delayLightSet)` — walk a column from `startY` downward queueing 15 into
    /// every light-transparent block; returns the highest y that could NOT be
    /// propagated to.
    fn try_propagate_skylight(
        &mut self,
        world_x: i32,
        mut start_y: i32,
        world_z: i32,
        extrude_initialised: bool,
        delay_light_set: bool,
    ) -> i32 {
        let encode_offset = self.coordinate_offset;
        let propagate_direction = AxisDirection::PositiveY.everything_but_this_direction();
        if self.get_light_level_extruded(world_x, start_y.wrapping_add(1), world_z) != 15 {
            return start_y;
        }
        self.check_null_section(
            world_x >> 4,
            start_y >> 4,
            world_z >> 4,
            extrude_initialised,
        );
        let mut above = self.get_block_state_at(world_x, start_y.wrapping_add(1), world_z);
        while start_y >= self.min_light_section.wrapping_shl(4) {
            if (start_y & 15) == 15 {
                self.check_null_section(
                    world_x >> 4,
                    start_y >> 4,
                    world_z >> 4,
                    extrude_initialised,
                );
            }
            let current = self.get_block_state_at(world_x, start_y, world_z);
            if above.conditionally_full_opaque() {
                // the face-occlusion test is not ported; a conditionally
                // full-opaque above block always occludes (the seam would test
                // the shape here).
                break;
            }
            let flags = 0u64;
            if current.conditionally_full_opaque() {
                // the seam breaks rather than testing the culling shape, so no
                // flag-only propagation path is entered from a column walk.
                break;
            }
            let opacity = current.light_dampening();
            if opacity > 0 {
                // the queued value (if any) handles it from here
                break;
            }
            self.append_to_increase_queue(
                encode_queue_position(world_x, start_y, world_z, encode_offset)
                    | (15u64 << (6 + 6 + 16))
                    | (propagate_direction << (6 + 6 + 16 + 4))
                    | flags,
            );
            above = current;
            if self
                .get_nibble_from_cache(world_x >> 4, start_y >> 4, world_z >> 4)
                .is_none()
            {
                // skip empty sections: the above block propagates through air
                self.increase_queue_initial_length -= 1;
                start_y &= !15;
                above = StateId(0);
            } else if !delay_light_set {
                self.set_light_level(world_x, start_y, world_z, 15);
            }
            start_y = start_y.wrapping_sub(1);
        }
        start_y
    }

    /// `processDelayedIncreases` — write the queued increase levels (the light
    /// set deferred by tryPropagateSkylight's delayLightSet).
    fn process_delayed_increases(&mut self) {
        let decode_offset_x = self.encode_offset_x.wrapping_neg();
        let decode_offset_y = self.encode_offset_y.wrapping_neg();
        let decode_offset_z = self.encode_offset_z.wrapping_neg();
        let queue = self.increase_queue.clone();
        let len = self.increase_queue_initial_length;
        for &value in &queue[..len] {
            let pos_x = ((value as i32) & 63).wrapping_add(decode_offset_x);
            let pos_z = (((value >> 6) as i32) & 63).wrapping_add(decode_offset_z);
            let pos_y = (((value >> 12) as i32) & ((1 << 16) - 1)).wrapping_add(decode_offset_y);
            let level = ((value >> (6 + 6 + 16)) & 0xF) as i32;
            self.set_light_level(pos_x, pos_y, pos_z, level);
        }
    }

    /// `processDelayedDecreases` — write 0 to the queued decrease positions.
    fn process_delayed_decreases(&mut self) {
        let decode_offset_x = self.encode_offset_x.wrapping_neg();
        let decode_offset_y = self.encode_offset_y.wrapping_neg();
        let decode_offset_z = self.encode_offset_z.wrapping_neg();
        let queue = self.decrease_queue.clone();
        let len = self.decrease_queue_initial_length;
        for &value in &queue[..len] {
            let pos_x = ((value as i32) & 63).wrapping_add(decode_offset_x);
            let pos_z = (((value >> 6) as i32) & 63).wrapping_add(decode_offset_z);
            let pos_y = (((value >> 12) as i32) & ((1 << 16) - 1)).wrapping_add(decode_offset_y);
            self.set_light_level(pos_x, pos_y, pos_z, 0);
        }
    }

    /// `propagateNeighbourLevels(lightAccess, chunk, fromSection, toSection)` —
    /// pull the 1-radius neighbours' horizontal edge light into the increase
    /// queue (used on the no-edge-checks light path).
    fn propagate_neighbour_levels(
        &mut self,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        from_section: i32,
        to_section: i32,
    ) {
        let chunk_x = chunk.get_pos().x();
        let chunk_z = chunk.get_pos().z();
        for curr_section_y in (from_section..=to_section).rev() {
            let Some(_curr_nibble) = self.get_nibble_from_cache(chunk_x, curr_section_y, chunk_z)
            else {
                continue;
            };
            for direction in AxisDirection::ONLY_HORIZONTAL {
                let neighbour_off_x = direction.x();
                let neighbour_off_z = direction.z();
                let neighbour_nibble = self
                    .get_nibble_from_cache(
                        chunk_x.wrapping_add(neighbour_off_x),
                        curr_section_y,
                        chunk_z.wrapping_add(neighbour_off_z),
                    )
                    .cloned();
                let Some(neighbour_nibble) = neighbour_nibble else {
                    continue;
                };
                if !neighbour_nibble.is_initialised_updating() {
                    // can't pull from 0
                    continue;
                }
                let (inc_x, inc_z, start_x, start_z) = if neighbour_off_x != 0 {
                    (
                        0,
                        1,
                        if direction.x() < 0 {
                            (chunk_x.wrapping_shl(4)).wrapping_sub(1)
                        } else {
                            (chunk_x.wrapping_shl(4)).wrapping_add(16)
                        },
                        chunk_z.wrapping_shl(4),
                    )
                } else {
                    (
                        1,
                        0,
                        chunk_x.wrapping_shl(4),
                        if direction.z() < 0 {
                            (chunk_z.wrapping_shl(4)).wrapping_sub(1)
                        } else {
                            (chunk_z.wrapping_shl(4)).wrapping_add(16)
                        },
                    )
                };
                let propagate_direction = 1u64 << direction.opposite();
                let encode_offset = self.coordinate_offset;
                for curr_y in
                    (curr_section_y.wrapping_shl(4))..=((curr_section_y.wrapping_shl(4)) | 15)
                {
                    let (mut curr_x, mut curr_z) = (start_x, start_z);
                    for _ in 0..16 {
                        let index =
                            ((curr_x & 15) | ((curr_z & 15) << 4) | ((curr_y & 15) << 8)) as usize;
                        let level = neighbour_nibble.get_updating_index(index);
                        if level <= 1 {
                            // nothing to propagate
                        } else {
                            self.append_to_increase_queue(
                                encode_queue_position(curr_x, curr_y, curr_z, encode_offset)
                                    | ((level as u64 & 0xF) << (6 + 6 + 16))
                                    | (propagate_direction << (6 + 6 + 16 + 4))
                                    | FLAG_HAS_SIDED_TRANSPARENT_BLOCKS,
                            );
                        }
                        curr_x = curr_x.wrapping_add(inc_x);
                        curr_z = curr_z.wrapping_add(inc_z);
                    }
                }
            }
        }
    }

    /// `performLightIncrease` — the flood-fill BFS. Drain the increase queue,
    /// propagating each entry's light into the 6 (or fewer) neighbours.
    fn perform_light_increase(&mut self) {
        let mut queue_read_index = 0usize;
        let mut queue_length = self.increase_queue_initial_length;
        self.increase_queue_initial_length = 0;
        let decode_offset_x = self.encode_offset_x.wrapping_neg();
        let decode_offset_y = self.encode_offset_y.wrapping_neg();
        let decode_offset_z = self.encode_offset_z.wrapping_neg();
        let encode_offset = self.coordinate_offset;
        let section_offset = self.chunk_section_index_offset;

        while queue_read_index < queue_length {
            let queue_value = self.increase_queue[queue_read_index];
            queue_read_index += 1;
            let pos_x = ((queue_value as i32) & 63).wrapping_add(decode_offset_x);
            let pos_z = (((queue_value >> 6) as i32) & 63).wrapping_add(decode_offset_z);
            let pos_y =
                (((queue_value >> 12) as i32) & ((1 << 16) - 1)).wrapping_add(decode_offset_y);
            let propagated_light_level = ((queue_value >> (6 + 6 + 16)) & 0xF) as i32;
            let check_directions =
                old_check_directions(((queue_value >> (6 + 6 + 16 + 4)) & 63) as usize);

            if queue_value & FLAG_RECHECK_LEVEL != 0 {
                if self.get_light_level(pos_x, pos_y, pos_z) != propagated_light_level {
                    // not at the level we expect, so something changed
                    continue;
                }
            } else if queue_value & FLAG_WRITE_LEVEL != 0 {
                // these are used to restore block sources after a propagation decrease
                self.set_light_level(pos_x, pos_y, pos_z, propagated_light_level);
            }

            if queue_value & FLAG_HAS_SIDED_TRANSPARENT_BLOCKS == 0 {
                // we don't need to worry about our state here
                for &propagate in check_directions {
                    let off_x = pos_x.wrapping_add(propagate.x());
                    let off_y = pos_y.wrapping_add(propagate.y());
                    let off_z = pos_z.wrapping_add(propagate.z());
                    let section_index =
                        cache_index(off_x >> 4, off_y >> 4, off_z >> 4, section_offset);
                    let local_index =
                        ((off_x & 15) | ((off_z & 15) << 4) | ((off_y & 15) << 8)) as usize;

                    let current_level = match self.nibble_cache[section_index].as_ref() {
                        Some(nibble) => nibble.get_updating_index(local_index),
                        None => continue, // unloaded
                    };
                    if current_level >= (propagated_light_level - 1) {
                        continue; // already at the level we want
                    }
                    let block_state = self.get_block_state(section_index, local_index);
                    let mut flags = 0u64;
                    if block_state.conditionally_full_opaque() {
                        // the shape test would decide; the seam keeps the flag
                        flags |= FLAG_HAS_SIDED_TRANSPARENT_BLOCKS;
                    }
                    let opacity = block_state.light_dampening();
                    let target_level = propagated_light_level - opacity.max(1);
                    if target_level <= current_level {
                        continue;
                    }
                    if let Some(nibble) = self.nibble_cache[section_index].as_mut() {
                        nibble.set_index(local_index, target_level);
                    }
                    if target_level > 1 {
                        if queue_length >= self.increase_queue.len() {
                            self.resize_increase_queue();
                        }
                        self.increase_queue[queue_length] =
                            encode_queue_position(off_x, off_y, off_z, encode_offset)
                                | (((target_level as u64) & 0xF) << (6 + 6 + 16))
                                | (propagate.everything_but_the_opposite_direction()
                                    << (6 + 6 + 16 + 4))
                                | flags;
                        queue_length += 1;
                    }
                }
            } else {
                // we actually need to worry about our state here
                let from_block = self.get_block_state_at(pos_x, pos_y, pos_z);
                let from_shape_blocked = from_block.conditionally_full_opaque();
                for &propagate in check_directions {
                    let off_x = pos_x.wrapping_add(propagate.x());
                    let off_y = pos_y.wrapping_add(propagate.y());
                    let off_z = pos_z.wrapping_add(propagate.z());
                    if from_shape_blocked {
                        // the seam treats a conditionally full-opaque source as
                        // occluding in this direction
                        continue;
                    }
                    let section_index =
                        cache_index(off_x >> 4, off_y >> 4, off_z >> 4, section_offset);
                    let local_index =
                        ((off_x & 15) | ((off_z & 15) << 4) | ((off_y & 15) << 8)) as usize;

                    let current_level = match self.nibble_cache[section_index].as_ref() {
                        Some(nibble) => nibble.get_updating_index(local_index),
                        None => continue, // unloaded
                    };
                    if current_level >= (propagated_light_level - 1) {
                        continue; // already at the level we want
                    }
                    let block_state = self.get_block_state(section_index, local_index);
                    let mut flags = 0u64;
                    if block_state.conditionally_full_opaque() {
                        // the shape test would decide; the seam keeps the flag
                        flags |= FLAG_HAS_SIDED_TRANSPARENT_BLOCKS;
                    }
                    let opacity = block_state.light_dampening();
                    let target_level = propagated_light_level - opacity.max(1);
                    if target_level <= current_level {
                        continue;
                    }
                    if let Some(nibble) = self.nibble_cache[section_index].as_mut() {
                        nibble.set_index(local_index, target_level);
                    }
                    if target_level > 1 {
                        if queue_length >= self.increase_queue.len() {
                            self.resize_increase_queue();
                        }
                        self.increase_queue[queue_length] =
                            encode_queue_position(off_x, off_y, off_z, encode_offset)
                                | (((target_level as u64) & 0xF) << (6 + 6 + 16))
                                | (propagate.everything_but_the_opposite_direction()
                                    << (6 + 6 + 16 + 4))
                                | flags;
                        queue_length += 1;
                    }
                }
            }
        }
    }

    /// `performLightDecrease` — the flood-fill decrease BFS, then re-propagate
    /// any sources that were clobbered (the light set + emission path).
    fn perform_light_decrease(&mut self) {
        let mut queue_read_index = 0usize;
        let mut queue_length = self.decrease_queue_initial_length;
        self.decrease_queue_initial_length = 0;
        let mut increase_queue_length = self.increase_queue_initial_length;
        let decode_offset_x = self.encode_offset_x.wrapping_neg();
        let decode_offset_y = self.encode_offset_y.wrapping_neg();
        let decode_offset_z = self.encode_offset_z.wrapping_neg();
        let encode_offset = self.coordinate_offset;
        let section_offset = self.chunk_section_index_offset;
        // `emittedLightMask = skylightPropagator ? 0 : 0xF` — the sky engine
        // never re-propagates emitted block light.
        let emitted_mask = 0;

        while queue_read_index < queue_length {
            let queue_value = self.decrease_queue[queue_read_index];
            queue_read_index += 1;
            let pos_x = ((queue_value as i32) & 63).wrapping_add(decode_offset_x);
            let pos_z = (((queue_value >> 6) as i32) & 63).wrapping_add(decode_offset_z);
            let pos_y =
                (((queue_value >> 12) as i32) & ((1 << 16) - 1)).wrapping_add(decode_offset_y);
            let propagated_light_level = ((queue_value >> (6 + 6 + 16)) & 0xF) as i32;
            let check_directions =
                old_check_directions(((queue_value >> (6 + 6 + 16 + 4)) & 63) as usize);

            if queue_value & FLAG_HAS_SIDED_TRANSPARENT_BLOCKS == 0 {
                // we don't need to worry about our state here
                for &propagate in check_directions {
                    let off_x = pos_x.wrapping_add(propagate.x());
                    let off_y = pos_y.wrapping_add(propagate.y());
                    let off_z = pos_z.wrapping_add(propagate.z());
                    let section_index =
                        cache_index(off_x >> 4, off_y >> 4, off_z >> 4, section_offset);
                    let local_index =
                        ((off_x & 15) | ((off_z & 15) << 4) | ((off_y & 15) << 8)) as usize;

                    let light_level = match self.nibble_cache[section_index].as_ref() {
                        Some(nibble) => nibble.get_updating_index(local_index),
                        None => continue, // unloaded
                    };
                    if light_level == 0 {
                        continue; // already at lowest, nothing we can do
                    }
                    let block_state = self.get_block_state(section_index, local_index);
                    let mut flags = 0u64;
                    if block_state.conditionally_full_opaque() {
                        // the shape test would decide; the seam keeps the flag
                        flags |= FLAG_HAS_SIDED_TRANSPARENT_BLOCKS;
                    }
                    let opacity = block_state.light_dampening();
                    let target_level = (propagated_light_level - opacity.max(1)).max(0);
                    if light_level > target_level {
                        // another source propagated here, so re-propagate it
                        if increase_queue_length >= self.increase_queue.len() {
                            self.resize_increase_queue();
                        }
                        self.increase_queue[increase_queue_length] =
                            encode_queue_position(off_x, off_y, off_z, encode_offset)
                                | (((light_level as u64) & 0xF) << (6 + 6 + 16))
                                | ((ALL_DIRECTIONS_BITSET as u64) << (6 + 6 + 16 + 4))
                                | (FLAG_RECHECK_LEVEL | flags);
                        increase_queue_length += 1;
                        continue;
                    }
                    let emitted_light = block_state.light_emission() & emitted_mask;
                    if emitted_light != 0 {
                        // re-propagate source; no recheck or the propagation fails
                        if increase_queue_length >= self.increase_queue.len() {
                            self.resize_increase_queue();
                        }
                        self.increase_queue[increase_queue_length] =
                            encode_queue_position(off_x, off_y, off_z, encode_offset)
                                | (((emitted_light as u64) & 0xF) << (6 + 6 + 16))
                                | ((ALL_DIRECTIONS_BITSET as u64) << (6 + 6 + 16 + 4))
                                | (flags | FLAG_WRITE_LEVEL);
                        increase_queue_length += 1;
                    }
                    if let Some(nibble) = self.nibble_cache[section_index].as_mut() {
                        nibble.set_index(local_index, 0);
                    }
                    if target_level > 0 {
                        if queue_length >= self.decrease_queue.len() {
                            self.resize_decrease_queue();
                        }
                        self.decrease_queue[queue_length] =
                            encode_queue_position(off_x, off_y, off_z, encode_offset)
                                | (((target_level as u64) & 0xF) << (6 + 6 + 16))
                                | (propagate.everything_but_the_opposite_direction()
                                    << (6 + 6 + 16 + 4))
                                | flags;
                        queue_length += 1;
                    }
                }
            } else {
                // we actually need to worry about our state here
                let from_block = self.get_block_state_at(pos_x, pos_y, pos_z);
                let from_shape_blocked = from_block.conditionally_full_opaque();
                for &propagate in check_directions {
                    let off_x = pos_x.wrapping_add(propagate.x());
                    let off_y = pos_y.wrapping_add(propagate.y());
                    let off_z = pos_z.wrapping_add(propagate.z());
                    if from_shape_blocked {
                        // the seam treats a conditionally full-opaque source as
                        // occluding in this direction
                        continue;
                    }
                    let section_index =
                        cache_index(off_x >> 4, off_y >> 4, off_z >> 4, section_offset);
                    let local_index =
                        ((off_x & 15) | ((off_z & 15) << 4) | ((off_y & 15) << 8)) as usize;

                    let light_level = match self.nibble_cache[section_index].as_ref() {
                        Some(nibble) => nibble.get_updating_index(local_index),
                        None => continue, // unloaded
                    };
                    if light_level == 0 {
                        continue; // already at lowest, nothing we can do
                    }
                    let block_state = self.get_block_state(section_index, local_index);
                    let mut flags = 0u64;
                    if block_state.conditionally_full_opaque() {
                        // the shape test would decide; the seam keeps the flag
                        flags |= FLAG_HAS_SIDED_TRANSPARENT_BLOCKS;
                    }
                    let opacity = block_state.light_dampening();
                    let target_level = (propagated_light_level - opacity.max(1)).max(0);
                    if light_level > target_level {
                        // another source propagated here, so re-propagate it
                        if increase_queue_length >= self.increase_queue.len() {
                            self.resize_increase_queue();
                        }
                        self.increase_queue[increase_queue_length] =
                            encode_queue_position(off_x, off_y, off_z, encode_offset)
                                | (((light_level as u64) & 0xF) << (6 + 6 + 16))
                                | ((ALL_DIRECTIONS_BITSET as u64) << (6 + 6 + 16 + 4))
                                | (FLAG_RECHECK_LEVEL | flags);
                        increase_queue_length += 1;
                        continue;
                    }
                    let emitted_light = block_state.light_emission() & emitted_mask;
                    if emitted_light != 0 {
                        // re-propagate source; no recheck or the propagation fails
                        if increase_queue_length >= self.increase_queue.len() {
                            self.resize_increase_queue();
                        }
                        self.increase_queue[increase_queue_length] =
                            encode_queue_position(off_x, off_y, off_z, encode_offset)
                                | (((emitted_light as u64) & 0xF) << (6 + 6 + 16))
                                | ((ALL_DIRECTIONS_BITSET as u64) << (6 + 6 + 16 + 4))
                                | (flags | FLAG_WRITE_LEVEL);
                        increase_queue_length += 1;
                    }
                    if let Some(nibble) = self.nibble_cache[section_index].as_mut() {
                        nibble.set_index(local_index, 0);
                    }
                    if target_level > 0 {
                        if queue_length >= self.decrease_queue.len() {
                            self.resize_decrease_queue();
                        }
                        self.decrease_queue[queue_length] =
                            encode_queue_position(off_x, off_y, off_z, encode_offset)
                                | (((target_level as u64) & 0xF) << (6 + 6 + 16))
                                | (propagate.everything_but_the_opposite_direction()
                                    << (6 + 6 + 16 + 4))
                                | flags;
                        queue_length += 1;
                    }
                }
            }
        }
        self.increase_queue_initial_length = increase_queue_length;
        self.perform_light_increase();
    }

    /// `handleEmptySectionChanges(lightAccess, chunk, emptinessChanges, unlit)`
    /// — the protected helper that computes the chunk's emptiness map from the
    /// caller's `empty_sections` (null entries derived from section content on
    /// first load) and inits/de-inits the neighbour nibbles. `ret` is `Some`
    /// when the map was freshly allocated (the caller must publish it).
    fn handle_empty_section_changes(
        &mut self,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        emptiness_changes: &[Option<bool>],
        unlit: bool,
    ) -> Option<Vec<bool>> {
        let chunk_x = chunk.get_pos().x();
        let chunk_z = chunk.get_pos().z();
        let total_sections = self
            .max_section
            .wrapping_sub(self.min_section)
            .wrapping_add(1) as usize;

        // Java's `getEmptinessMap(chunkX, chunkZ)` returns the live array from
        // the cache; the port's cache owns a value copy, so the entry is cloned
        // here and written back once the update loop below runs (see below).
        let mut chunk_emptiness_map = self.get_emptiness_map(chunk_x, chunk_z).cloned();
        let needs_init = unlit || chunk_emptiness_map.is_none();
        if needs_init {
            chunk_emptiness_map = Some(vec![false; total_sections]);
        }

        // Java's `Boolean[] emptinessChanges` is mutated in place by the
        // derivation below (the `null -> derived` write-back); the port keeps a
        // local copy so the second loop observes the derived values exactly
        // like Java.
        let mut changes: Vec<Option<bool>> = emptiness_changes.to_vec();
        {
            let emptiness_map = chunk_emptiness_map.as_mut().expect("map set above");

            // update emptiness map
            for section_index in (0..changes.len()).rev() {
                let mut value_boxed = changes[section_index];
                if value_boxed.is_none() {
                    if !needs_init {
                        continue;
                    }
                    let section = self.get_chunk_section(
                        chunk_x,
                        (section_index as i32).wrapping_add(self.min_section),
                        chunk_z,
                    );
                    value_boxed = Some(match section {
                        None => true,
                        Some(section) => section.has_only_air,
                    });
                    changes[section_index] = value_boxed;
                }
                if let Some(v) = value_boxed {
                    emptiness_map[section_index] = v;
                }
            }
        }
        // `initNibble` reads the map through the cache, and `ret` aliases the
        // same array as the cache in Java — publish the updated map to both
        // before the init loops below run.
        let updated = chunk_emptiness_map.expect("map set above");
        let ret = if needs_init {
            Some(updated.clone())
        } else {
            None
        };
        self.set_emptiness_map_cache(chunk_x, chunk_z, Some(updated));

        // now init neighbour nibbles
        for section_index in (0..changes.len()).rev() {
            let value_boxed = changes[section_index];
            let section_y = (section_index as i32).wrapping_add(self.min_section);
            let Some(empty) = value_boxed else { continue };
            if empty {
                continue;
            }
            for dz in -1i32..=1 {
                for dx in -1i32..=1 {
                    // if we're not empty, we also need to initialise nibbles
                    // note: if we're unlit, we absolutely do not want to
                    // extrude, as light data isn't set up
                    let extrude = (dx | dz) != 0 || !unlit;
                    for dy in (-1i32..=1).rev() {
                        self.init_nibble(
                            dx.wrapping_add(chunk_x),
                            dy.wrapping_add(section_y),
                            dz.wrapping_add(chunk_z),
                            extrude,
                            false,
                        );
                    }
                }
            }
        }

        // check for de-init and lazy-init
        // lazy init is when chunks are being lit, so at the time they weren't
        // loaded when their neighbours were running init checks.
        for dz in -1i32..=1 {
            for dx in -1i32..=1 {
                // does this neighbour have 1 radius loaded?
                let mut neighbours_loaded = true;
                'neighbour_loaded_search: for dz2 in -1i32..=1 {
                    for dx2 in -1i32..=1 {
                        if self
                            .get_emptiness_map(
                                dx.wrapping_add(dx2).wrapping_add(chunk_x),
                                dz.wrapping_add(dz2).wrapping_add(chunk_z),
                            )
                            .is_none()
                        {
                            neighbours_loaded = false;
                            break 'neighbour_loaded_search;
                        }
                    }
                }

                for section_y in (self.min_light_section..=self.max_light_section).rev() {
                    // check neighbours to see if we need to de-init this one
                    let mut all_empty = true;
                    'neighbour_search: for dy2 in -1i32..=1 {
                        for dz2 in -1i32..=1 {
                            for dx2 in -1i32..=1 {
                                let y = section_y.wrapping_add(dy2);
                                if y < self.min_section || y > self.max_section {
                                    // empty
                                    continue;
                                }
                                if let Some(emptiness_map) = self.get_emptiness_map(
                                    dx.wrapping_add(dx2).wrapping_add(chunk_x),
                                    dz.wrapping_add(dz2).wrapping_add(chunk_z),
                                ) {
                                    if !emptiness_map[(y.wrapping_sub(self.min_section)) as usize] {
                                        all_empty = false;
                                        break 'neighbour_search;
                                    }
                                } else {
                                    let section = self.get_chunk_section(
                                        dx.wrapping_add(dx2).wrapping_add(chunk_x),
                                        y,
                                        dz.wrapping_add(dz2).wrapping_add(chunk_z),
                                    );
                                    if section.is_some() && !section.unwrap().has_only_air {
                                        all_empty = false;
                                        break 'neighbour_search;
                                    }
                                }
                            }
                        }
                    }

                    if all_empty && neighbours_loaded {
                        // can only de-init when neighbours are loaded
                        // de-init is fine to delay, as de-init is just an
                        // optimisation - it's not required for lighting to be
                        // correct
                        self.set_nibble_null(
                            dx.wrapping_add(chunk_x),
                            section_y,
                            dz.wrapping_add(chunk_z),
                        );
                    } else if !all_empty {
                        // must init
                        let extrude = (dx | dz) != 0 || !unlit;
                        self.init_nibble(
                            dx.wrapping_add(chunk_x),
                            section_y,
                            dz.wrapping_add(chunk_z),
                            extrude,
                            false,
                        );
                    }
                }
            }
        }

        ret
    }

    /// `light(lightAccess, chunk, emptySections)` — the entry point the
    /// provider's `light_chunk` drives. Setup caches, force the chunk into the
    /// cache with fresh filled-empty nibbles, run the emptiness changes, light
    /// the chunk, write back the computed nibbles, and update the visible
    /// state. The computed nibbles and emptiness map are surfaced through
    /// `pending_nibbles` / `pending_emptiness_map` for the provider to write
    /// onto the chunk.
    ///
    /// The write-back is `setNibbles(chunk, nibbles)` then `updateVisible`:
    /// Java hands the chunk the *original* array — the same objects the cache
    /// aliased and mutated in place — and publishes the visible state of the
    /// very objects it handed out. The port reproduces both: it publishes the
    /// cache clones, then writes back the mutated clone for each section the
    /// cache still holds, substituting the untouched original `Null` nibble
    /// for the sections `rewrite_nibble_cache_for_skylight` nulled out (Java's
    /// re-created scratch nibbles never reach the chunk).
    /// `StarLightEngine.light(lightAccess, chunk, emptySections)` — the
    /// generation entry: `lightChunk(chunk, true)` (edge-checks).
    pub(crate) fn light(
        &mut self,
        provider: &mut dyn ChunkAccessor,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        empty_sections: &[Option<bool>],
    ) {
        self.light_impl(provider, chunk, empty_sections, true);
    }

    /// The per-neighbour `lightChunk(lightAccess, chunk, false)` path from
    /// `relightChunks` — the idempotent re-light. The only difference from
    /// [`Self::light`] is `needsEdgeChecks == false`, so the neighbour-light
    /// pull (`propagateNeighbourLevels`) runs instead of the edge-decrease
    /// pass. The differential test lights committed chunks against committed
    /// neighbours through this path.
    pub(crate) fn relight(
        &mut self,
        provider: &mut dyn ChunkAccessor,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        empty_sections: &[Option<bool>],
    ) {
        self.light_impl(provider, chunk, empty_sections, false);
    }

    fn light_impl(
        &mut self,
        provider: &mut dyn ChunkAccessor,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        empty_sections: &[Option<bool>],
        needs_edge_checks: bool,
    ) {
        let chunk_x = chunk.get_pos().x();
        let chunk_z = chunk.get_pos().z();
        // A previous successful call normally has already handed these values
        // to the provider. Clear them defensively so a setup panic or a caller
        // retry can never publish an older run's result.
        self.pending_nibbles = None;
        self.pending_emptiness_map = None;
        self.pending_neighbor_updates.clear();

        // Java's `try { ... } finally { destroyCaches(); }` covers the entire
        // light operation. In particular, a storage callback can panic while
        // `setupCaches` is resolving a neighbour; that path must not leave the
        // old run's cache membership visible to the next attempt.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.nulled_sections.fill(false);
            self.setup_caches(
                provider,
                chunk_x.wrapping_mul(16).wrapping_add(7),
                128,
                chunk_z.wrapping_mul(16).wrapping_add(7),
                true,
                true,
            );

            let nibbles = get_filled_empty_light(
                (self
                    .max_light_section
                    .wrapping_sub(self.min_light_section)
                    .wrapping_add(1)) as usize,
            );
            // force current chunk into cache
            self.set_chunk_in_cache(chunk_x, chunk_z);
            self.set_blocks_for_chunk_in_cache(chunk_x, chunk_z, chunk);
            self.set_nibbles_for_chunk_in_cache(chunk_x, chunk_z, &nibbles);
            self.set_emptiness_map_cache(
                chunk_x,
                chunk_z,
                self.get_emptiness_map_from_chunk(chunk),
            );

            let ret = self.handle_empty_section_changes(chunk, empty_sections, true);
            if let Some(map) = ret {
                self.set_emptiness_map_on_surface(map);
            }
            self.light_chunk_impl(chunk, needs_edge_checks);
            // `setNibbles(chunk, nibbles)` then `updateVisible(lightAccess)`:
            // publish every cache clone first. Center nibbles are returned to
            // the caller, while neighbour clones are returned separately so
            // the provider can write them back before restoration.
            self.update_visible();
            self.pending_neighbor_updates = self.collect_neighbor_updates(chunk_x, chunk_z);
            let mut computed = Vec::with_capacity(nibbles.len());
            for (index, cy) in (self.min_light_section..=self.max_light_section).enumerate() {
                if self.nulled_sections[index] {
                    computed.push(nibbles[index].clone());
                } else {
                    computed.push(
                        self.get_nibble_from_cache(chunk_x, cy, chunk_z)
                            .cloned()
                            .unwrap_or_else(|| nibbles[index].clone()),
                    );
                }
            }
            self.set_nibbles_on_surface(computed);
        }));
        if result.is_err() {
            // A failed run has no publishable output. The cache cleanup below
            // is still performed before the original panic resumes.
            self.pending_nibbles = None;
            self.pending_emptiness_map = None;
            self.pending_neighbor_updates.clear();
        }
        self.destroy_caches();
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }

    /// `SkyStarLightEngine.lightChunk(lightAccess, chunk, needsEdgeChecks)`.
    fn light_chunk_impl(
        &mut self,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        needs_edge_checks: bool,
    ) {
        let chunk_x = chunk.get_pos().x();
        let chunk_z = chunk.get_pos().z();
        self.rewrite_nibble_cache_for_skylight(chunk_x, chunk_z);
        self.null_propagation_check_cache
            .iter_mut()
            .for_each(|b| *b = false);

        let sections = chunk.get_sections();

        let mut highest_non_empty_section = self.max_section;
        // Walk empty sections from the top down, propagating FULL to horizontal
        // neighbours.
        while highest_non_empty_section == (self.min_section.wrapping_sub(1))
            || sections
                .get(highest_non_empty_section.wrapping_sub(self.min_section) as usize)
                .is_none_or(|s| s.has_only_air())
        {
            self.check_null_section(chunk_x, highest_non_empty_section, chunk_z, false);
            for direction in AxisDirection::ONLY_HORIZONTAL {
                let neighbour_x = chunk_x.wrapping_add(direction.x());
                let neighbour_z = chunk_z.wrapping_add(direction.z());
                let Some(_neighbour_nibble) =
                    self.get_nibble_from_cache(neighbour_x, highest_non_empty_section, neighbour_z)
                else {
                    // unloaded neighbour
                    continue;
                };
                let (inc_x, inc_z, start_x, start_z) = if direction.x() != 0 {
                    (
                        0,
                        1,
                        if direction.x() < 0 {
                            chunk_x.wrapping_shl(4)
                        } else {
                            chunk_x.wrapping_shl(4) | 15
                        },
                        chunk_z.wrapping_shl(4),
                    )
                } else {
                    (
                        1,
                        0,
                        chunk_x.wrapping_shl(4),
                        if direction.z() < 0 {
                            chunk_z.wrapping_shl(4)
                        } else {
                            chunk_z.wrapping_shl(4) | 15
                        },
                    )
                };
                let encode_offset = self.coordinate_offset;
                let propagate_direction = direction.as_single_bit();
                for curr_y in (highest_non_empty_section.wrapping_shl(4))
                    ..=((highest_non_empty_section.wrapping_shl(4)) | 15)
                {
                    let (mut curr_x, mut curr_z) = (start_x, start_z);
                    for _ in 0..16 {
                        self.append_to_increase_queue(
                            encode_queue_position(curr_x, curr_y, curr_z, encode_offset)
                                | (15u64 << (6 + 6 + 16))
                                | (propagate_direction << (6 + 6 + 16 + 4)),
                        );
                        curr_x = curr_x.wrapping_add(inc_x);
                        curr_z = curr_z.wrapping_add(inc_z);
                    }
                }
            }
            if highest_non_empty_section == (self.min_section.wrapping_sub(1)) {
                break;
            }
            highest_non_empty_section = highest_non_empty_section.wrapping_sub(1);
        }

        if highest_non_empty_section >= self.min_section {
            let min_x = chunk_x.wrapping_shl(4);
            let max_x = chunk_x.wrapping_shl(4) | 15;
            let min_z = chunk_z.wrapping_shl(4);
            let max_z = chunk_z.wrapping_shl(4) | 15;
            let start_y = highest_non_empty_section.wrapping_shl(4) | 15;
            for curr_z in min_z..=max_z {
                for curr_x in min_x..=max_x {
                    self.try_propagate_skylight(
                        curr_x,
                        start_y.wrapping_add(1),
                        curr_z,
                        false,
                        false,
                    );
                }
            }
        }

        if needs_edge_checks {
            self.perform_light_increase();
            for y in (self.min_light_section..=highest_non_empty_section).rev() {
                self.check_null_section(chunk_x, y, chunk_z, false);
            }
            // Java then runs super.checkChunkEdges (edge checks deferred: the
            // per-edge decrease defers with the edge-check unit, #184).
        } else {
            for y in (self.min_light_section..=highest_non_empty_section).rev() {
                self.check_null_section(chunk_x, y, chunk_z, false);
            }
            self.propagate_neighbour_levels(
                chunk,
                self.min_light_section,
                highest_non_empty_section,
            );
            self.perform_light_increase();
        }
    }

    /// `updateVisible(lightAccess)` — publish every dirty nibble's updating
    /// state into its visible state. Java also fires `onLightUpdate` to the
    /// client; the server-side notify path defers (#184), so only the SWMR
    /// copy-on-write publication happens here.
    fn update_visible(&mut self) {
        for nibble in self.nibble_cache.iter_mut().flatten() {
            nibble.update_visible();
        }
    }

    /// Snapshot the post-`updateVisible` state of every cached neighbour. A
    /// radius-two cache has only an emptiness map; a radius-one cache has the
    /// complete nibble array as well. Missing cache sections are deliberately
    /// not published, preserving the malformed-input panic/retry boundary.
    fn collect_neighbor_updates(
        &self,
        center_x: i32,
        center_z: i32,
    ) -> HashMap<(i32, i32), NeighborLightUpdate> {
        let mut updates = HashMap::new();
        for dz in -2i32..=2 {
            for dx in -2i32..=2 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let chunk_x = center_x.wrapping_add(dx);
                let chunk_z = center_z.wrapping_add(dz);
                if !self.is_chunk_in_cache(chunk_x, chunk_z) {
                    continue;
                }

                let nibbles = (self.min_light_section..=self.max_light_section)
                    .enumerate()
                    .filter_map(|(index, chunk_y)| {
                        let cache_slot =
                            cache_index(chunk_x, chunk_y, chunk_z, self.chunk_section_index_offset);
                        if !self.nibble_cache_writeback[cache_slot] {
                            return self.dropped_nibble_writeback[cache_slot]
                                .as_ref()
                                .cloned()
                                .map(|nibble| (index, nibble));
                        }
                        self.get_nibble_from_cache(chunk_x, chunk_y, chunk_z)
                            .or(self.dropped_nibble_writeback[cache_slot].as_ref())
                            .cloned()
                            .map(|nibble| (index, nibble))
                    })
                    .collect::<Vec<_>>();
                let emptiness_map = self.get_emptiness_map(chunk_x, chunk_z).cloned();
                if !nibbles.is_empty() || emptiness_map.is_some() {
                    updates.insert(
                        (chunk_x, chunk_z),
                        NeighborLightUpdate {
                            nibbles,
                            emptiness_map,
                        },
                    );
                }
            }
        }
        updates
    }

    /// `destroyCaches()` — Java's finally-clear of every cache between runs.
    fn destroy_caches(&mut self) {
        self.section_cache.iter_mut().for_each(|s| *s = None);
        self.nibble_cache.iter_mut().for_each(|s| *s = None);
        self.nibble_cache_writeback.fill(false);
        self.dropped_nibble_writeback
            .iter_mut()
            .for_each(|s| *s = None);
        self.chunk_cache.iter_mut().for_each(|c| *c = false);
        self.emptiness_map_cache.iter_mut().for_each(|s| *s = None);
        self.null_propagation_check_cache.fill(false);
        self.nulled_sections.fill(false);
        self.increase_queue_initial_length = 0;
        self.decrease_queue_initial_length = 0;
    }

    /// `#[cfg(test)]` probe for the provider's panic-path tests: whether the
    /// per-run caches are in the post-`destroyCaches` state. Java's finally-clear
    /// runs even when the `light` body unwinds, so the caches must be gone
    /// whether the run completed or panicked.
    #[cfg(test)]
    pub(crate) fn per_run_caches_are_clear(&self) -> bool {
        self.section_cache.iter().all(Option::is_none)
            && self.nibble_cache.iter().all(Option::is_none)
            && self
                .nibble_cache_writeback
                .iter()
                .all(|&writeback| !writeback)
            && self.dropped_nibble_writeback.iter().all(Option::is_none)
            && self.chunk_cache.iter().all(|&present| !present)
            && self.emptiness_map_cache.iter().all(Option::is_none)
            && self
                .null_propagation_check_cache
                .iter()
                .all(|&checked| !checked)
            && self.nulled_sections.iter().all(|&nulled| !nulled)
            && self.increase_queue_initial_length == 0
            && self.decrease_queue_initial_length == 0
    }
}

/// `StarLightEngine.getFilledEmptyLight(totalLightSections)` — an array of
/// null `SWMRNibbleArray`s (state `Null`, no backing).
fn get_filled_empty_light(total_light_sections: usize) -> Vec<SwmrNibbleArray> {
    (0..total_light_sections)
        .map(|_| SwmrNibbleArray::new_with_bytes_and_null(None, true))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::level_chunk::{
        BiomeId as ServerBiomeId, StateId, StructureKey, container_factory, state_flags,
        superflat_content,
    };
    use rivet_registry::core::ChunkPos;
    use rivet_world::chunk::upgrade_data::UpgradeData;
    use rivet_world::level::height_accessor::create as create_accessor;
    use rivet_world::superflat::{SECTION_COUNT, SUPERFLAT_HEIGHT, SUPERFLAT_MIN_Y};

    /// The overworld superflat vertical extent (minY -64, height 384): sections
    /// -4..19, light sections -5..20.
    fn overworld_accessor() -> Box<dyn rivet_world::level::LevelHeightAccessor + Send> {
        Box::new(create_accessor(SUPERFLAT_MIN_Y, SUPERFLAT_HEIGHT))
    }

    /// The server's superflat chunk at `pos` — a single stone layer at block
    /// y=-64, air everywhere above. The runtime `StateId` resolver classifies
    /// the states (air = 0, everything else opaque).
    fn superflat_chunk(pos: ChunkPos) -> ChunkAccess<StateId, ServerBiomeId, StructureKey> {
        let content = superflat_content();
        let height_accessor = create_accessor(SUPERFLAT_MIN_Y, SUPERFLAT_HEIGHT);
        ChunkAccess::new(
            pos,
            UpgradeData::empty(height_accessor.get_sections_count() as usize),
            height_accessor,
            &container_factory(),
            0,
            Some(content.sections),
            &|state: &StateId| state_flags(*state),
        )
    }

    /// An all-air chunk at `pos` — every section air (the superflat preset's
    /// `minecraft:air` fill).
    fn all_air_chunk(pos: ChunkPos) -> ChunkAccess<StateId, ServerBiomeId, StructureKey> {
        let height_accessor = create_accessor(SUPERFLAT_MIN_Y, SUPERFLAT_HEIGHT);
        ChunkAccess::new(
            pos,
            UpgradeData::empty(height_accessor.get_sections_count() as usize),
            height_accessor,
            &container_factory(),
            0,
            None, // the `ChunkAccess::new` default: every section all-air
            &|state: &StateId| state_flags(*state),
        )
    }

    /// The `emptySections` argument for a superflat chunk: the stone section
    /// (index 0, world section -4) is non-empty, every other section derives
    /// from the chunk content (the `None` entries exercise the `needsInit`
    /// derivation branch of `handleEmptySectionChanges`).
    fn superflat_empty_sections() -> Vec<Option<bool>> {
        let mut empty = vec![None; SECTION_COUNT];
        empty[0] = Some(false);
        empty
    }

    /// The `emptySections` argument for an all-air chunk: every section empty.
    fn all_air_empty_sections() -> Vec<Option<bool>> {
        vec![Some(true); SECTION_COUNT]
    }

    /// A provider with no loaded neighbours. `light()` sets up its caches with
    /// `relaxed = true` (and radius 2), so a missing neighbour is tolerated;
    /// only the center chunk (forced in by `light`) participates.
    struct EmptyProvider;

    impl ChunkAccessor for EmptyProvider {
        fn get_chunk_for_lighting(
            &mut self,
            _chunk_x: i32,
            _chunk_z: i32,
        ) -> Option<&ChunkAccess<StateId, ServerBiomeId, StructureKey>> {
            None
        }
    }

    /// A provider that resolves exactly one loaded, light-correct neighbour
    /// chunk (at `(1, 0)`) and nothing else — the multi-chunk neighbour path
    /// the [`EmptyProvider`] tests never exercise.
    struct SingleNeighbourProvider(ChunkAccess<StateId, ServerBiomeId, StructureKey>);

    impl ChunkAccessor for SingleNeighbourProvider {
        fn get_chunk_for_lighting(
            &mut self,
            chunk_x: i32,
            chunk_z: i32,
        ) -> Option<&ChunkAccess<StateId, ServerBiomeId, StructureKey>> {
            if chunk_x == 1 && chunk_z == 0 {
                Some(&self.0)
            } else {
                None
            }
        }
    }

    /// Encode a queue entry exactly as Java's `appendToIncreaseQueue`/
    /// `appendToDecreaseQueue` do — the 28-bit packed coordinate, the 4-bit
    /// level, and the 6-bit direction bitset.
    fn encode_entry(
        engine: &SkyStarLightEngine,
        x: i32,
        y: i32,
        z: i32,
        level: i32,
        dirs: usize,
    ) -> u64 {
        let eo = engine.coordinate_offset;
        (x.wrapping_add(z.wrapping_shl(6))
            .wrapping_add(y.wrapping_shl(12))
            .wrapping_add(eo) as u64)
            & ((1u64 << (6 + 6 + 16)) - 1)
            | (((level as u64) & 0xF) << (6 + 6 + 16))
            | (((dirs as u64) & 63) << (6 + 6 + 16 + 4))
    }

    /// The `OLD_CHECK_DIRECTIONS` table must decode every one of the 64
    /// propagation bitsets to exactly the set directions, in ascending ordinal
    /// order (Java's `IntegerUtil.trailingZeros` iteration). A wrong bit
    /// mapping, a dropped direction, or a non-ascending order fails here.
    #[test]
    fn old_check_directions_covers_all_64_bitsets() {
        for bitset in 0u32..64 {
            let dirs = old_check_directions(bitset as usize);
            assert_eq!(
                dirs.len(),
                bitset.count_ones() as usize,
                "popcount of bitset {bitset}"
            );
            let mut mask = 0u64;
            let mut prev = -1i32;
            for &d in dirs {
                let ordinal = d as usize;
                assert!(
                    ordinal as i32 > prev,
                    "directions must be ascending ordinal for bitset {bitset}"
                );
                prev = ordinal as i32;
                mask |= 1u64 << ordinal;
            }
            assert_eq!(mask, bitset as u64, "exact bit coverage of bitset {bitset}");
        }
    }

    /// A few hand-written rows pin the ordinal mapping and the boundary cases:
    /// the empty bitset, single bits, adjacent pairs, and the full six bits.
    #[test]
    fn old_check_directions_pins_exact_rows() {
        assert_eq!(old_check_directions(0), &[]);
        assert_eq!(old_check_directions(1), &[AxisDirection::PositiveX]);
        assert_eq!(
            old_check_directions(0b000011),
            &[AxisDirection::PositiveX, AxisDirection::NegativeX]
        );
        assert_eq!(
            old_check_directions(0b010001),
            &[AxisDirection::PositiveX, AxisDirection::PositiveY]
        );
        assert_eq!(
            old_check_directions(0b111111),
            &[
                AxisDirection::PositiveX,
                AxisDirection::NegativeX,
                AxisDirection::PositiveZ,
                AxisDirection::NegativeZ,
                AxisDirection::PositiveY,
                AxisDirection::NegativeY,
            ]
        );
    }

    /// The end-to-end light-chunk run on the flat air + stone superflat chunk.
    /// The expected sky light is the established M1 superflat sky contract: the
    /// floor light section (block y -64..-49) is byte-exact `128 zeros then 1920
    /// `0xFF`` (stone at y=-64 blocks sky, the 15 air levels above are full),
    /// and the section above (block y -48..-33) is uniformly full. The section
    /// below the world is untouched (0).
    ///
    /// This is the "mutated nibbles actually written back" assertion: the
    /// computed sky light must survive the run into `pending_nibbles`, not be
    /// discarded in favor of the initial all-null `getFilledEmptyLight` array.
    #[test]
    fn flat_superflat_chunk_lights_sky_full_above_and_zero_at_the_floor() {
        let mut engine = SkyStarLightEngine::new(&*overworld_accessor());
        let chunk = superflat_chunk(ChunkPos::new(0, 0));
        let mut provider = EmptyProvider;
        engine.light(&mut provider, &chunk, &superflat_empty_sections());

        let nibbles = engine
            .pending_nibbles
            .as_ref()
            .expect("sky nibbles written back");
        // 26 light sections (-5..=20), indexed from the min light section.
        assert_eq!(nibbles.len(), 26);

        // Light section index 1 = block y -64..-49: the stone floor plane (y=0)
        // is 0, the air above it (y=1..15) is fully lit.
        let floor = &nibbles[1];
        for x in 0..16 {
            for z in 0..16 {
                assert_eq!(
                    floor.get_updating(x, 0, z),
                    0,
                    "stone at y=-64 blocks sky at ({x}, {z})"
                );
            }
        }
        for y in 1..16 {
            for x in 0..16 {
                for z in 0..16 {
                    assert_eq!(
                        floor.get_updating(x, y, z),
                        15,
                        "air above the floor at ({x}, {y}, {z})"
                    );
                }
            }
        }

        // Light section index 2 = block y -48..-33: fully lit throughout.
        let above = &nibbles[2];
        for y in 0..16 {
            for x in 0..16 {
                for z in 0..16 {
                    assert_eq!(
                        above.get_updating(x, y, z),
                        15,
                        "open sky at ({x}, {y}, {z})"
                    );
                }
            }
        }

        // Light section index 0 = block y -80..-65 (below the world floor):
        // untouched, all 0.
        let below = &nibbles[0];
        for y in 0..16 {
            for x in 0..16 {
                for z in 0..16 {
                    assert_eq!(
                        below.get_updating(x, y, z),
                        0,
                        "below the floor at ({x}, {y}, {z})"
                    );
                }
            }
        }
    }

    /// The same run, asserted at the byte level against the established M1
    /// superflat sky contract (`superflat::superflat_sky_layers`): the floor
    /// layer is exactly `128 zeros + 1920 0xFF`, the above layer is all `0xFF`.
    ///
    /// The write-back publishes the visible state (Java's `updateVisible` after
    /// `setNibbles`), so the handed-out clones convert directly — no manual
    /// `update_visible` on a captured copy.
    #[test]
    fn flat_superflat_sky_bytes_match_the_paper_fixture() {
        let mut engine = SkyStarLightEngine::new(&*overworld_accessor());
        let chunk = superflat_chunk(ChunkPos::new(0, 0));
        let mut provider = EmptyProvider;
        engine.light(&mut provider, &chunk, &superflat_empty_sections());

        let nibbles = engine
            .pending_nibbles
            .as_ref()
            .expect("sky nibbles written back");

        let floor_data = nibbles[1]
            .to_vanilla_nibble()
            .expect("floor initialised")
            .get_data();
        assert_eq!(floor_data.len(), 2048);
        assert_eq!(&floor_data[..128], &[0u8; 128][..]);
        assert_eq!(&floor_data[128..], &[0xFFu8; 1920][..]);

        let above_data = nibbles[2]
            .to_vanilla_nibble()
            .expect("above initialised")
            .get_data();
        assert_eq!(above_data, vec![0xFFu8; 2048]);
    }

    /// The write-back hooks must surface the recomputed state, not the initial
    /// empties: `pending_nibbles` carries the engine's mutations (the floor
    /// section is `Initialised`, not the original `Null`), and
    /// `pending_emptiness_map` carries the freshly derived sky-emptiness map
    /// (stone section non-empty, every air section empty).
    #[test]
    fn light_surfaces_computed_nibbles_and_emptiness_map() {
        let mut engine = SkyStarLightEngine::new(&*overworld_accessor());
        let chunk = superflat_chunk(ChunkPos::new(0, 0));
        let mut provider = EmptyProvider;
        engine.light(&mut provider, &chunk, &superflat_empty_sections());

        let nibbles = engine
            .pending_nibbles
            .as_ref()
            .expect("sky nibbles written back");
        // The floor and above sections must have been initialised during the
        // run — a write-back that discarded the cache mutations would leave
        // them `Null` (the `getFilledEmptyLight` originals).
        assert!(
            nibbles[1].is_initialised_updating(),
            "floor section mutation must be written back"
        );
        assert!(
            nibbles[2].is_initialised_updating(),
            "above section mutation must be written back"
        );
        // The light-less sections stay null (the faithful empty-chunk state).
        assert!(nibbles[3].is_null_nibble_updating());

        let map = engine
            .pending_emptiness_map
            .as_ref()
            .expect("sky emptiness map written back");
        assert_eq!(map.len(), SECTION_COUNT);
        assert!(!map[0], "stone section is non-empty");
        assert!(
            map[1..].iter().all(|&empty| empty),
            "every air section is empty"
        );
    }

    /// A fully-empty chunk produces no light: all sky nibbles stay `Null` and
    /// the emptiness map is uniformly empty. (The fully-exposed sky is
    /// represented by null sections — `getLightLevelExtruded` reads 15 above
    /// them — so the engine correctly materializes nothing.)
    #[test]
    fn all_air_chunk_produces_no_light_and_all_empty_map() {
        let mut engine = SkyStarLightEngine::new(&*overworld_accessor());
        let chunk = all_air_chunk(ChunkPos::new(0, 0));
        let mut provider = EmptyProvider;
        engine.light(&mut provider, &chunk, &all_air_empty_sections());

        let nibbles = engine
            .pending_nibbles
            .as_ref()
            .expect("sky nibbles written back");
        for nibble in nibbles {
            assert!(
                nibble.is_null_nibble_updating(),
                "an all-air chunk must not materialize sky light"
            );
        }
        let map = engine
            .pending_emptiness_map
            .as_ref()
            .expect("sky emptiness map written back");
        assert!(map.iter().all(|&empty| empty));
    }

    /// With a loaded, light-correct neighbour the engine runs the multi-chunk
    /// path the [`EmptyProvider`] tests never exercise. The centre chunk is
    /// entirely air, and the neighbour (at chunk `(1, 0)`) is also all-air — so
    /// `handleEmptySectionChanges` inits nothing in the centre — but carries a
    /// pre-existing non-null sky nibble at light section -5, the same height as
    /// a centre section. `checkNullSection(0, -5, 0)` therefore sees a lit
    /// neighbour and re-creates the centre's nulled section as a scratch
    /// (Java's `initNibble` with `initRemovedNibbles`), which must not leak
    /// into the write-back: `setNibbles(chunk, nibbles)` hands the chunk the
    /// original array, and a nulled section keeps its untouched original `Null`.
    #[test]
    fn loaded_neighbour_recreated_centre_section_stays_null_in_writeback() {
        let mut engine = SkyStarLightEngine::new(&*overworld_accessor());
        let chunk = all_air_chunk(ChunkPos::new(0, 0));
        // All-air, light-correct neighbour at (1, 0) with its light-section -5
        // (index 0) already initialised to a distinctive non-null level — the
        // only lit nibble in the centre's neighbourhood at that height.
        let mut neighbour = all_air_chunk(ChunkPos::new(1, 0));
        let mut neighbour_nibbles: Vec<SwmrNibbleArray> = neighbour.sky_nibbles().to_vec();
        neighbour_nibbles[0] = SwmrNibbleArray::new_with_bytes(vec![0x0Fu8; 2048]);
        neighbour.set_sky_nibbles(neighbour_nibbles);
        neighbour.set_light_correct(true);
        let mut provider = SingleNeighbourProvider(neighbour);
        engine.light(&mut provider, &chunk, &all_air_empty_sections());

        let nibbles = engine
            .pending_nibbles
            .as_ref()
            .expect("sky nibbles written back");
        // The all-air centre materializes no stored light: every section the
        // rewrite nulled (all of them) is written back as its original `Null`,
        // including the index-0 section `checkNullSection` re-created as a
        // scratch (Paper's re-created nibbles never reach the chunk).
        for (index, nibble) in nibbles.iter().enumerate() {
            assert!(
                nibble.is_null_nibble_updating(),
                "a nulled centre section keeps its original null, not a re-created scratch (index {index})"
            );
        }
    }

    /// `performLightIncrease` spills a level-15 source into every direction the
    /// queue entry's bitset selects. With the four horizontal bits set the light
    /// reaches all four horizontal neighbours at level 14 (15 − 1 through air)
    /// while the source keeps 15 — the multi-direction horizontal spill.
    #[test]
    fn increase_spills_into_all_four_horizontal_neighbours() {
        let mut engine = SkyStarLightEngine::new(&*overworld_accessor());
        engine.setup_encode_offset(7, 128, 7);
        engine.set_chunk_in_cache(0, 0);

        // Light section -4 (block y -64..-49): a 15 source at (7, -50, 7).
        let mut nibble = SwmrNibbleArray::new_with_bytes(vec![0u8; 2048]);
        nibble.set(7, 14, 7, 15);
        engine.set_nibble_in_cache(0, -4, 0, Some(nibble));

        // The four horizontal directions: +X, -X, +Z, -Z.
        let horizontal = 0b001111;
        engine.append_to_increase_queue(encode_entry(&engine, 7, -50, 7, 15, horizontal));
        engine.perform_light_increase();

        let result = engine
            .get_nibble_from_cache(0, -4, 0)
            .expect("section nibble");
        assert_eq!(
            result.get_updating(7, 14, 7),
            15,
            "source retains its level"
        );
        assert_eq!(result.get_updating(8, 14, 7), 14, "+X neighbour");
        assert_eq!(result.get_updating(6, 14, 7), 14, "-X neighbour");
        assert_eq!(result.get_updating(7, 14, 8), 14, "+Z neighbour");
        assert_eq!(result.get_updating(7, 14, 6), 14, "-Z neighbour");
    }

    /// `performLightDecrease` zeroes every neighbour the decrease entry's
    /// bitset selects — the source is zeroed first (`SkyStarLightEngine.
    /// checkBlock` sets the position to 0 before queueing), then the decrease
    /// propagates through the four horizontal directions, cascading each 14 to 0
    /// and re-queueing the next level down.
    #[test]
    fn decrease_zeroes_all_four_horizontal_neighbours() {
        let mut engine = SkyStarLightEngine::new(&*overworld_accessor());
        engine.setup_encode_offset(7, 128, 7);
        engine.set_chunk_in_cache(0, 0);

        // A 15 source lit to 14 in its four horizontal neighbours.
        let mut nibble = SwmrNibbleArray::new_with_bytes(vec![0u8; 2048]);
        nibble.set(7, 14, 7, 15);
        nibble.set(8, 14, 7, 14);
        nibble.set(6, 14, 7, 14);
        nibble.set(7, 14, 8, 14);
        nibble.set(7, 14, 6, 14);
        engine.set_nibble_in_cache(0, -4, 0, Some(nibble));

        // `checkBlock` zeroes the source before queueing the decrease.
        engine.set_light_level(7, -50, 7, 0);
        let horizontal = 0b001111;
        engine.append_to_decrease_queue(encode_entry(&engine, 7, -50, 7, 15, horizontal));
        engine.perform_light_decrease();

        let result = engine
            .get_nibble_from_cache(0, -4, 0)
            .expect("section nibble");
        assert_eq!(result.get_updating(7, 14, 7), 0, "source stays removed");
        assert_eq!(result.get_updating(8, 14, 7), 0, "+X neighbour");
        assert_eq!(result.get_updating(6, 14, 7), 0, "-X neighbour");
        assert_eq!(result.get_updating(7, 14, 8), 0, "+Z neighbour");
        assert_eq!(result.get_updating(7, 14, 6), 0, "-Z neighbour");
    }

    /// `initNibble` with `extrude=true` copies the y=0 plane of the first
    /// non-null section above down into every y-layer of the section below the
    /// lowest non-empty section. A wrong extrusion (copying the wrong layer,
    /// only one layer, or leaving the section null) fails the byte assertion.
    #[test]
    fn init_nibble_extrudes_the_floor_layer_from_the_section_above() {
        let mut engine = SkyStarLightEngine::new(&*overworld_accessor());
        engine.setup_encode_offset(7, 128, 7);
        engine.set_chunk_in_cache(0, 0);

        // Emptiness: the lowest non-empty world section is 0 (block y -64..-49).
        let mut map = vec![true; SECTION_COUNT];
        map[0] = false;
        engine.set_emptiness_map_cache(0, 0, Some(map));

        // The section above (light section -4) is non-null with a distinctive
        // y=0 plane.
        let mut above = SwmrNibbleArray::new_with_bytes(vec![0u8; 2048]);
        for x in 0..16 {
            for z in 0..16 {
                above.set(x, 0, z, x.wrapping_add(z) % 16);
            }
        }
        engine.set_nibble_in_cache(0, -4, 0, Some(above));

        // The section below (light section -5, below the world floor) starts
        // null.
        engine.set_nibble_in_cache(
            0,
            -5,
            0,
            Some(SwmrNibbleArray::new_with_bytes_and_null(None, true)),
        );

        engine.init_nibble(0, -5, 0, true, false);

        let below = engine
            .get_nibble_from_cache(0, -5, 0)
            .expect("extruded nibble");
        assert!(
            !below.is_null_nibble_updating(),
            "extrusion must init the section"
        );
        for y in 0..16 {
            for x in 0..16 {
                for z in 0..16 {
                    assert_eq!(
                        below.get_updating(x, y, z),
                        x.wrapping_add(z) % 16,
                        "y-layer {y} at ({x}, {z}) must carry the above section's y=0 plane"
                    );
                }
            }
        }
    }

    /// `initNibble` without `extrude` below the lowest non-empty section only
    /// de-nulls the nibble (Java's `setNonNull`); it must stay uninitialised
    /// with no data (reads 0), never extruded.
    #[test]
    fn init_nibble_without_extrude_stays_uninitialised() {
        let mut engine = SkyStarLightEngine::new(&*overworld_accessor());
        engine.setup_encode_offset(7, 128, 7);
        engine.set_chunk_in_cache(0, 0);

        let mut map = vec![true; SECTION_COUNT];
        map[0] = false;
        engine.set_emptiness_map_cache(0, 0, Some(map));

        engine.set_nibble_in_cache(
            0,
            -5,
            0,
            Some(SwmrNibbleArray::new_with_bytes_and_null(None, true)),
        );

        engine.init_nibble(0, -5, 0, false, false);

        let below = engine.get_nibble_from_cache(0, -5, 0).expect("nibble");
        assert!(!below.is_null_nibble_updating(), "must be de-nulled");
        assert!(below.is_uninitialised_updating(), "must not be extruded");
        assert_eq!(below.get_updating(0, 0, 0), 0, "no data, reads 0");
    }
}
