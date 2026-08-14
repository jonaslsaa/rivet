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
//! `tryPropagateSkylight`).
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
//!   (`VoxelShape` is not). A conditionally-full-opaque block is carried as the
//!   `FLAG_HAS_SIDED_TRANSPARENT_BLOCKS` flag and, where Java would test the
//!   shape, the seam assumes it occludes (`continue`/`break`). For the superflat
//!   air + stone content neither block sets the flag, so the byte-exact contract
//!   is unaffected (the tests use only air/stone).
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
//! `relight_chunks`, `check_chunk_edges`) are the phase-A no-ops, and this
//! engine is exercised only through the light-chunk path.
//!
//! The engine is not yet wired into the provider (see the RivetTodo above), so
//! its public surface is unreachable from the lib target and reads as dead code;
//! the internal call graph between these items is live and compiles.
#![allow(dead_code)]

use crate::server::level::level_chunk::{BiomeId as ServerBiomeId, StateId, StructureKey};
use rivet_registry::block_state::BlockState;
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::level_chunk_section::LevelChunkSection;
use rivet_world::level::height_accessor::LevelHeightAccessor;
use rivet_world::lighting::swmr_nibble_array::SwmrNibbleArray;

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
    /// `chunkCache`, indexed `x + 5*z + chunkIndexOffset`. Java holds the
    /// chunk reference; the only engine read is the presence check in
    /// `initNibble`, so the cache is a membership flag.
    chunk_cache: Vec<bool>,
    /// `emptinessMapCache`, indexed like `chunkCache`.
    emptiness_map_cache: Vec<Option<Vec<bool>>>,
    /// `nullPropagationCheckCache` — per-light-section flag: whether this
    /// section's null-propagation check already ran this run.
    null_propagation_check_cache: Vec<bool>,

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
}

impl SkyStarLightEngine {
    /// `new SkyStarLightEngine(); setWorld(world)` — built with the world's
    /// vertical extent (`SimpleLevelHeightAccessor`), so the engine's section
    /// bounds match the chunk's.
    pub(crate) fn new(accessor: &dyn LevelHeightAccessor) -> Self {
        let min_section = accessor.get_min_section_y();
        let max_section = accessor.get_max_section_y();
        let min_light_section = min_section - 1;
        let max_light_section = max_section + 1;
        let light_section_count = (max_light_section - min_light_section + 1) as usize;
        let min_array_size = 5 * 5 * (light_section_count + 2);
        SkyStarLightEngine {
            min_light_section,
            max_light_section,
            min_section,
            max_section,
            section_cache: (0..min_array_size).map(|_| None).collect(),
            nibble_cache: (0..min_array_size).map(|_| None).collect(),
            chunk_cache: vec![false; 25],
            emptiness_map_cache: (0..25).map(|_| None).collect(),
            null_propagation_check_cache: vec![false; light_section_count],
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
        }
    }

    /// `setupEncodeOffset(centerX, centerY, centerZ)` — Java's per-run center
    /// encoding; the offsets make the center chunk's blocks encode into the
    /// queue's 28-bit coordinate space.
    fn setup_encode_offset(&mut self, center_x: i32, _center_y: i32, center_z: i32) {
        self.encode_offset_x = 31 - center_x;
        self.encode_offset_y = -(self.min_light_section - 1) << 4;
        self.encode_offset_z = 31 - center_z;
        self.coordinate_offset =
            self.encode_offset_x + (self.encode_offset_z << 6) + (self.encode_offset_y << 12);
        self.chunk_offset_x = 2 - (center_x >> 4);
        self.chunk_offset_y = -(self.min_light_section - 1);
        self.chunk_offset_z = 2 - (center_z >> 4);
        self.chunk_index_offset = self.chunk_offset_x + 5 * self.chunk_offset_z;
        self.chunk_section_index_offset = self.chunk_index_offset + 25 * self.chunk_offset_y;
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
            (center_x >> 4) * 16 + 7,
            (center_y >> 4) * 16 + 7,
            (center_z >> 4) * 16 + 7,
        );
        let radius = if try_to_load_chunks_for_2_radius {
            2
        } else {
            1
        };
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let cx = (center_x >> 4) + dx;
                let cz = (center_z >> 4) + dz;
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
        self.chunk_cache[(chunk_x + 5 * chunk_z + self.chunk_index_offset) as usize]
    }

    fn set_chunk_in_cache(&mut self, chunk_x: i32, chunk_z: i32) {
        let idx = (chunk_x + 5 * chunk_z + self.chunk_index_offset) as usize;
        self.chunk_cache[idx] = true;
    }

    fn set_emptiness_map_cache(&mut self, chunk_x: i32, chunk_z: i32, map: Option<Vec<bool>>) {
        let idx = (chunk_x + 5 * chunk_z + self.chunk_index_offset) as usize;
        self.emptiness_map_cache[idx] = map;
    }

    fn get_emptiness_map(&self, chunk_x: i32, chunk_z: i32) -> Option<&Vec<bool>> {
        self.emptiness_map_cache[(chunk_x + 5 * chunk_z + self.chunk_index_offset) as usize]
            .as_ref()
    }

    fn get_chunk_section(
        &self,
        chunk_x: i32,
        chunk_y: i32,
        chunk_z: i32,
    ) -> Option<&SectionSnapshot> {
        let idx = (chunk_x + 5 * chunk_z + 25 * chunk_y + self.chunk_section_index_offset) as usize;
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
                    .get((cy - self.min_section) as usize)
                    .map(SectionSnapshot::of)
            } else {
                None
            };
            let idx = (chunk_x + 5 * chunk_z + 25 * cy + self.chunk_section_index_offset) as usize;
            self.section_cache[idx] = section;
        }
    }

    fn get_nibble_from_cache(
        &self,
        chunk_x: i32,
        chunk_y: i32,
        chunk_z: i32,
    ) -> Option<&SwmrNibbleArray> {
        let idx = (chunk_x + 5 * chunk_z + 25 * chunk_y + self.chunk_section_index_offset) as usize;
        self.nibble_cache[idx].as_ref()
    }

    fn get_nibble_from_cache_mut(
        &mut self,
        chunk_x: i32,
        chunk_y: i32,
        chunk_z: i32,
    ) -> Option<&mut SwmrNibbleArray> {
        let idx = (chunk_x + 5 * chunk_z + 25 * chunk_y + self.chunk_section_index_offset) as usize;
        self.nibble_cache[idx].as_mut()
    }

    fn set_nibble_in_cache(
        &mut self,
        chunk_x: i32,
        chunk_y: i32,
        chunk_z: i32,
        nibble: Option<SwmrNibbleArray>,
    ) {
        let idx = (chunk_x + 5 * chunk_z + 25 * chunk_y + self.chunk_section_index_offset) as usize;
        self.nibble_cache[idx] = nibble;
    }

    fn get_nibbles_for_chunk_from_cache(&self, chunk_x: i32, chunk_z: i32) -> Vec<SwmrNibbleArray> {
        (self.min_light_section..=self.max_light_section)
            .map(|cy| {
                self.get_nibble_from_cache(chunk_x, cy, chunk_z)
                    .cloned()
                    .unwrap_or_else(|| SwmrNibbleArray::new_with_bytes_and_null(None, true))
            })
            .collect()
    }

    fn set_nibbles_for_chunk_in_cache(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        nibbles: &[SwmrNibbleArray],
    ) {
        for (index, cy) in (self.min_light_section..=self.max_light_section).enumerate() {
            let nibble = nibbles.get(index).cloned();
            self.set_nibble_in_cache(chunk_x, cy, chunk_z, nibble);
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
        let index = ((world_x >> 4)
            + 5 * (world_z >> 4)
            + 25 * (world_y >> 4)
            + self.chunk_section_index_offset) as usize;
        self.get_block_state(
            index,
            ((world_x & 15) | ((world_z & 15) << 4) | ((world_y & 15) << 8)) as usize,
        )
    }

    /// `getLightLevel(worldX, worldY, worldZ)` — the updating (writer-side)
    /// light at the position, 0 when the section nibble is null.
    fn get_light_level(&self, world_x: i32, world_y: i32, world_z: i32) -> i32 {
        let index = ((world_x >> 4)
            + 5 * (world_z >> 4)
            + 25 * (world_y >> 4)
            + self.chunk_section_index_offset) as usize;
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
        let index = ((world_x >> 4)
            + 5 * (world_z >> 4)
            + 25 * (world_y >> 4)
            + self.chunk_section_index_offset) as usize;
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

    /// `getEmptinessMap(chunk)` — the chunk's stored sky emptiness map. The
    /// phase-A `ChunkAccess` has no sky-emptiness field, so this returns `None`
    /// and the engine recomputes the map from the empty-section mask in
    /// `handle_empty_section_changes`, surfacing it via
    /// `set_emptiness_map_on_surface` for the provider to publish.
    fn get_emptiness_map_from_chunk(
        &self,
        _chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
    ) -> Option<Vec<bool>> {
        None
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
        let mut lowest_y = self.min_light_section - 1;
        let emptiness_map = self.get_emptiness_map(chunk_x, chunk_z);
        for curr_y in (self.min_section..=self.max_section).rev() {
            let empty = match emptiness_map {
                Some(map) => map[(curr_y - self.min_section) as usize],
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
            for curr_y in (chunk_y + 1)..=self.max_light_section {
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
    /// sections by dropping them from the cache. The dropped nibbles are null
    /// (updating state), so `updateVisible` would publish a null visible state;
    /// dropping the cache entry has the same effect for this run.
    fn rewrite_nibble_cache_for_skylight(&mut self) {
        for index in 0..self.nibble_cache.len() {
            let is_null = match self.nibble_cache[index].as_ref() {
                Some(nibble) => nibble.is_null_nibble_updating(),
                None => false,
            };
            if is_null {
                self.nibble_cache[index] = None;
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
            || self.null_propagation_check_cache[(chunk_y - self.min_light_section) as usize]
        {
            return false;
        }
        self.null_propagation_check_cache[(chunk_y - self.min_light_section) as usize] = true;
        let mut need_init_neighbours = false;
        'search: for dz in -1..=1 {
            for dx in -1..=1 {
                if let Some(nibble) =
                    self.get_nibble_from_cache(dx + chunk_x, chunk_y, dz + chunk_z)
                    && !nibble.is_null_nibble_updating()
                {
                    need_init_neighbours = true;
                    break 'search;
                }
            }
        }
        if need_init_neighbours {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    // the centre gets the caller's extrude flag, the edges always
                    // extrude (they're guaranteed to have light above)
                    let extrude = if (dx | dz) == 0 {
                        extrude_initialised
                    } else {
                        true
                    };
                    self.init_nibble(dx + chunk_x, chunk_y, dz + chunk_z, extrude, true);
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
            chunk_y += 1;
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
        if self.get_light_level_extruded(world_x, start_y + 1, world_z) != 15 {
            return start_y;
        }
        self.check_null_section(
            world_x >> 4,
            start_y >> 4,
            world_z >> 4,
            extrude_initialised,
        );
        let mut above = self.get_block_state_at(world_x, start_y + 1, world_z);
        while start_y >= (self.min_light_section << 4) {
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
                ((world_x + (world_z << 6) + (start_y << 12) + encode_offset) as u64
                    & ((1u64 << (6 + 6 + 16)) - 1))
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
            start_y -= 1;
        }
        start_y
    }

    /// `processDelayedIncreases` — write the queued increase levels (the light
    /// set deferred by tryPropagateSkylight's delayLightSet).
    fn process_delayed_increases(&mut self) {
        let decode_offset_x = -self.encode_offset_x;
        let decode_offset_y = -self.encode_offset_y;
        let decode_offset_z = -self.encode_offset_z;
        let queue = self.increase_queue.clone();
        let len = self.increase_queue_initial_length;
        for &value in &queue[..len] {
            let pos_x = ((value as i32) & 63) + decode_offset_x;
            let pos_z = (((value >> 6) as i32) & 63) + decode_offset_z;
            let pos_y = (((value >> 12) as i32) & ((1 << 16) - 1)) + decode_offset_y;
            let level = ((value >> (6 + 6 + 16)) & 0xF) as i32;
            self.set_light_level(pos_x, pos_y, pos_z, level);
        }
    }

    /// `processDelayedDecreases` — write 0 to the queued decrease positions.
    fn process_delayed_decreases(&mut self) {
        let decode_offset_x = -self.encode_offset_x;
        let decode_offset_y = -self.encode_offset_y;
        let decode_offset_z = -self.encode_offset_z;
        let queue = self.decrease_queue.clone();
        let len = self.decrease_queue_initial_length;
        for &value in &queue[..len] {
            let pos_x = ((value as i32) & 63) + decode_offset_x;
            let pos_z = (((value >> 6) as i32) & 63) + decode_offset_z;
            let pos_y = (((value >> 12) as i32) & ((1 << 16) - 1)) + decode_offset_y;
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
                        chunk_x + neighbour_off_x,
                        curr_section_y,
                        chunk_z + neighbour_off_z,
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
                            (chunk_x << 4) - 1
                        } else {
                            (chunk_x << 4) + 16
                        },
                        chunk_z << 4,
                    )
                } else {
                    (
                        1,
                        0,
                        chunk_x << 4,
                        if direction.z() < 0 {
                            (chunk_z << 4) - 1
                        } else {
                            (chunk_z << 4) + 16
                        },
                    )
                };
                let propagate_direction = 1u64 << direction.opposite();
                let encode_offset = self.coordinate_offset;
                for curr_y in (curr_section_y << 4)..=((curr_section_y << 4) | 15) {
                    let (mut curr_x, mut curr_z) = (start_x, start_z);
                    for _ in 0..16 {
                        let index =
                            ((curr_x & 15) | ((curr_z & 15) << 4) | ((curr_y & 15) << 8)) as usize;
                        let level = neighbour_nibble.get_updating_index(index);
                        if level <= 1 {
                            // nothing to propagate
                        } else {
                            self.append_to_increase_queue(
                                ((curr_x + (curr_z << 6) + (curr_y << 12) + encode_offset) as u64
                                    & ((1u64 << (6 + 6 + 16)) - 1))
                                    | ((level as u64 & 0xF) << (6 + 6 + 16))
                                    | (propagate_direction << (6 + 6 + 16 + 4))
                                    | FLAG_HAS_SIDED_TRANSPARENT_BLOCKS,
                            );
                        }
                        curr_x += inc_x;
                        curr_z += inc_z;
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
        let decode_offset_x = -self.encode_offset_x;
        let decode_offset_y = -self.encode_offset_y;
        let decode_offset_z = -self.encode_offset_z;
        let encode_offset = self.coordinate_offset;
        let section_offset = self.chunk_section_index_offset;

        while queue_read_index < queue_length {
            let queue_value = self.increase_queue[queue_read_index];
            queue_read_index += 1;
            let pos_x = ((queue_value as i32) & 63) + decode_offset_x;
            let pos_z = (((queue_value >> 6) as i32) & 63) + decode_offset_z;
            let pos_y = (((queue_value >> 12) as i32) & ((1 << 16) - 1)) + decode_offset_y;
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
                    let off_x = pos_x + propagate.x();
                    let off_y = pos_y + propagate.y();
                    let off_z = pos_z + propagate.z();
                    let section_index =
                        ((off_x >> 4) + 5 * (off_z >> 4) + 25 * (off_y >> 4) + section_offset)
                            as usize;
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
                            ((off_x + (off_z << 6) + (off_y << 12) + encode_offset) as u64
                                & ((1u64 << (6 + 6 + 16)) - 1))
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
                    let off_x = pos_x + propagate.x();
                    let off_y = pos_y + propagate.y();
                    let off_z = pos_z + propagate.z();
                    if from_shape_blocked {
                        // the seam treats a conditionally full-opaque source as
                        // occluding in this direction
                        continue;
                    }
                    let section_index =
                        ((off_x >> 4) + 5 * (off_z >> 4) + 25 * (off_y >> 4) + section_offset)
                            as usize;
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
                            ((off_x + (off_z << 6) + (off_y << 12) + encode_offset) as u64
                                & ((1u64 << (6 + 6 + 16)) - 1))
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
        let decode_offset_x = -self.encode_offset_x;
        let decode_offset_y = -self.encode_offset_y;
        let decode_offset_z = -self.encode_offset_z;
        let encode_offset = self.coordinate_offset;
        let section_offset = self.chunk_section_index_offset;
        // `emittedLightMask = skylightPropagator ? 0 : 0xF` — the sky engine
        // never re-propagates emitted block light.
        let emitted_mask = 0;

        while queue_read_index < queue_length {
            let queue_value = self.decrease_queue[queue_read_index];
            queue_read_index += 1;
            let pos_x = ((queue_value as i32) & 63) + decode_offset_x;
            let pos_z = (((queue_value >> 6) as i32) & 63) + decode_offset_z;
            let pos_y = (((queue_value >> 12) as i32) & ((1 << 16) - 1)) + decode_offset_y;
            let propagated_light_level = ((queue_value >> (6 + 6 + 16)) & 0xF) as i32;
            let check_directions =
                old_check_directions(((queue_value >> (6 + 6 + 16 + 4)) & 63) as usize);

            if queue_value & FLAG_HAS_SIDED_TRANSPARENT_BLOCKS == 0 {
                // we don't need to worry about our state here
                for &propagate in check_directions {
                    let off_x = pos_x + propagate.x();
                    let off_y = pos_y + propagate.y();
                    let off_z = pos_z + propagate.z();
                    let section_index =
                        ((off_x >> 4) + 5 * (off_z >> 4) + 25 * (off_y >> 4) + section_offset)
                            as usize;
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
                            ((off_x + (off_z << 6) + (off_y << 12) + encode_offset) as u64
                                & ((1u64 << (6 + 6 + 16)) - 1))
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
                            ((off_x + (off_z << 6) + (off_y << 12) + encode_offset) as u64
                                & ((1u64 << (6 + 6 + 16)) - 1))
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
                            ((off_x + (off_z << 6) + (off_y << 12) + encode_offset) as u64
                                & ((1u64 << (6 + 6 + 16)) - 1))
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
                    let off_x = pos_x + propagate.x();
                    let off_y = pos_y + propagate.y();
                    let off_z = pos_z + propagate.z();
                    if from_shape_blocked {
                        // the seam treats a conditionally full-opaque source as
                        // occluding in this direction
                        continue;
                    }
                    let section_index =
                        ((off_x >> 4) + 5 * (off_z >> 4) + 25 * (off_y >> 4) + section_offset)
                            as usize;
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
                            ((off_x + (off_z << 6) + (off_y << 12) + encode_offset) as u64
                                & ((1u64 << (6 + 6 + 16)) - 1))
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
                            ((off_x + (off_z << 6) + (off_y << 12) + encode_offset) as u64
                                & ((1u64 << (6 + 6 + 16)) - 1))
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
                            ((off_x + (off_z << 6) + (off_y << 12) + encode_offset) as u64
                                & ((1u64 << (6 + 6 + 16)) - 1))
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
        let total_sections = (self.max_section - self.min_section + 1) as usize;

        let mut chunk_emptiness_map = self.get_emptiness_map(chunk_x, chunk_z).cloned();
        let mut ret = None;
        let needs_init = unlit || chunk_emptiness_map.is_none();
        if needs_init {
            let fresh = vec![false; total_sections];
            self.set_emptiness_map_cache(chunk_x, chunk_z, Some(fresh.clone()));
            chunk_emptiness_map = Some(fresh.clone());
            ret = Some(fresh);
        }
        let emptiness_map = chunk_emptiness_map.as_mut().expect("map set above");

        // update emptiness map
        for section_index in (0..emptiness_changes.len()).rev() {
            let mut value_boxed = emptiness_changes[section_index];
            if value_boxed.is_none() {
                if !needs_init {
                    continue;
                }
                let section = self.get_chunk_section(
                    chunk_x,
                    section_index as i32 + self.min_section,
                    chunk_z,
                );
                value_boxed = Some(match section {
                    None => true,
                    Some(section) => section.has_only_air,
                });
            }
            if let Some(v) = value_boxed {
                emptiness_map[section_index] = v;
            }
        }

        // now init neighbour nibbles
        for section_index in (0..emptiness_changes.len()).rev() {
            let value_boxed = emptiness_changes[section_index];
            let section_y = section_index as i32 + self.min_section;
            let Some(empty) = value_boxed else { continue };
            if empty {
                continue;
            }
            for dz in -1..=1 {
                for dx in -1..=1 {
                    // if we're not empty, we also need to initialise nibbles
                    // note: if we're unlit, we absolutely do not want to
                    // extrude, as light data isn't set up
                    let extrude = (dx | dz) != 0 || !unlit;
                    for dy in (-1..=1).rev() {
                        self.init_nibble(
                            dx + chunk_x,
                            dy + section_y,
                            dz + chunk_z,
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
        for dz in -1..=1 {
            for dx in -1..=1 {
                // does this neighbour have 1 radius loaded?
                let mut neighbours_loaded = true;
                'neighbour_loaded_search: for dz2 in -1..=1 {
                    for dx2 in -1..=1 {
                        if self
                            .get_emptiness_map(dx + dx2 + chunk_x, dz + dz2 + chunk_z)
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
                    'neighbour_search: for dy2 in -1..=1 {
                        for dz2 in -1..=1 {
                            for dx2 in -1..=1 {
                                let y = section_y + dy2;
                                if y < self.min_section || y > self.max_section {
                                    // empty
                                    continue;
                                }
                                if let Some(emptiness_map) =
                                    self.get_emptiness_map(dx + dx2 + chunk_x, dz + dz2 + chunk_z)
                                {
                                    if !emptiness_map[(y - self.min_section) as usize] {
                                        all_empty = false;
                                        break 'neighbour_search;
                                    }
                                } else {
                                    let section = self.get_chunk_section(
                                        dx + dx2 + chunk_x,
                                        y,
                                        dz + dz2 + chunk_z,
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
                        self.set_nibble_null(dx + chunk_x, section_y, dz + chunk_z);
                    } else if !all_empty {
                        // must init
                        let extrude = (dx | dz) != 0 || !unlit;
                        self.init_nibble(dx + chunk_x, section_y, dz + chunk_z, extrude, false);
                    }
                }
            }
        }

        ret
    }

    /// `light(lightAccess, chunk, emptySections)` — the entry point the
    /// provider's `light_chunk` drives. Setup caches, force the chunk into the
    /// cache with fresh filled-empty nibbles, run the emptiness changes, light
    /// the chunk, publish the computed nibbles, and update the visible state.
    /// The computed nibbles and emptiness map are surfaced through
    /// `pending_nibbles` / `pending_emptiness_map` for the provider to write
    /// onto the chunk.
    pub(crate) fn light(
        &mut self,
        provider: &mut dyn ChunkAccessor,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        empty_sections: &[Option<bool>],
    ) {
        let chunk_x = chunk.get_pos().x();
        let chunk_z = chunk.get_pos().z();
        self.setup_caches(
            provider,
            chunk_x * 16 + 7,
            128,
            chunk_z * 16 + 7,
            true,
            true,
        );

        let nibbles =
            get_filled_empty_light((self.max_light_section - self.min_light_section + 1) as usize);
        // force current chunk into cache
        self.set_chunk_in_cache(chunk_x, chunk_z);
        self.set_blocks_for_chunk_in_cache(chunk_x, chunk_z, chunk);
        self.set_nibbles_for_chunk_in_cache(chunk_x, chunk_z, &nibbles);
        self.set_emptiness_map_cache(chunk_x, chunk_z, self.get_emptiness_map_from_chunk(chunk));

        let ret = self.handle_empty_section_changes(chunk, empty_sections, true);
        if let Some(map) = ret {
            self.set_emptiness_map_on_surface(map);
        }
        self.light_chunk_impl(chunk, true);
        // `setNibbles(chunk, nibbles)` — the cache now holds the computed
        // center-chunk nibbles; hand them to the provider.
        let computed = self.get_nibbles_for_chunk_from_cache(chunk_x, chunk_z);
        self.set_nibbles_on_surface(computed);
        self.update_visible();
        self.destroy_caches();
    }

    /// `SkyStarLightEngine.lightChunk(lightAccess, chunk, needsEdgeChecks)`.
    fn light_chunk_impl(
        &mut self,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        needs_edge_checks: bool,
    ) {
        self.rewrite_nibble_cache_for_skylight();
        self.null_propagation_check_cache
            .iter_mut()
            .for_each(|b| *b = false);

        let chunk_x = chunk.get_pos().x();
        let chunk_z = chunk.get_pos().z();
        let sections = chunk.get_sections();

        let mut highest_non_empty_section = self.max_section;
        // Walk empty sections from the top down, propagating FULL to horizontal
        // neighbours.
        while highest_non_empty_section == (self.min_section - 1)
            || sections
                .get((highest_non_empty_section - self.min_section) as usize)
                .is_none_or(|s| s.has_only_air())
        {
            self.check_null_section(chunk_x, highest_non_empty_section, chunk_z, false);
            for direction in AxisDirection::ONLY_HORIZONTAL {
                let neighbour_x = chunk_x + direction.x();
                let neighbour_z = chunk_z + direction.z();
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
                            chunk_x << 4
                        } else {
                            chunk_x << 4 | 15
                        },
                        chunk_z << 4,
                    )
                } else {
                    (
                        1,
                        0,
                        chunk_x << 4,
                        if direction.z() < 0 {
                            chunk_z << 4
                        } else {
                            chunk_z << 4 | 15
                        },
                    )
                };
                let encode_offset = self.coordinate_offset;
                let propagate_direction = direction.as_single_bit();
                for curr_y in
                    (highest_non_empty_section << 4)..=((highest_non_empty_section << 4) | 15)
                {
                    let (mut curr_x, mut curr_z) = (start_x, start_z);
                    for _ in 0..16 {
                        self.append_to_increase_queue(
                            ((curr_x + (curr_z << 6) + (curr_y << 12) + encode_offset) as u64
                                & ((1u64 << (6 + 6 + 16)) - 1))
                                | (15u64 << (6 + 6 + 16))
                                | (propagate_direction << (6 + 6 + 16 + 4)),
                        );
                        curr_x += inc_x;
                        curr_z += inc_z;
                    }
                }
            }
            if highest_non_empty_section == (self.min_section - 1) {
                break;
            }
            highest_non_empty_section -= 1;
        }

        if highest_non_empty_section >= self.min_section {
            let min_x = chunk_x << 4;
            let max_x = chunk_x << 4 | 15;
            let min_z = chunk_z << 4;
            let max_z = chunk_z << 4 | 15;
            let start_y = highest_non_empty_section << 4 | 15;
            for curr_z in min_z..=max_z {
                for curr_x in min_x..=max_x {
                    self.try_propagate_skylight(curr_x, start_y + 1, curr_z, false, false);
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

    /// `destroyCaches()` — Java's finally-clear of every cache between runs.
    fn destroy_caches(&mut self) {
        self.section_cache.iter_mut().for_each(|s| *s = None);
        self.nibble_cache.iter_mut().for_each(|s| *s = None);
        self.chunk_cache.iter_mut().for_each(|c| *c = false);
        self.emptiness_map_cache.iter_mut().for_each(|s| *s = None);
    }
}

/// `StarLightEngine.getFilledEmptyLight(totalLightSections)` — an array of
/// null `SWMRNibbleArray`s (state `Null`, no backing).
fn get_filled_empty_light(total_light_sections: usize) -> Vec<SwmrNibbleArray> {
    (0..total_light_sections)
        .map(|_| SwmrNibbleArray::new_with_bytes_and_null(None, true))
        .collect()
}
