//! Port of `net.minecraft.server.level.WorldGenRegion` (MC 26.2, Paper) — the
//! `mc.server.level.pipeline.region` unit value layer.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/server/level/WorldGenRegion.java`
//! (584 lines). `WorldGenRegion` is the worldgen chunk-view container: a
//! `StaticCache2D<GenerationChunkHolder>` square centered on the generating
//! chunk, with the per-ring status/distance contract (`getChunk(x, z, status,
//! loadOrGenerate)`), the write-radius gate (`ensureCanWrite` /
//! `isWithinWriteZone`), and the `WorldGenLevel` read facade the feature
//! placement stack runs against.
//!
//! ## Value-layer scope
//!
//! This slice ports the value layer only: the `StaticCache2D` chunk view, the
//! ring/status/distance contract, biome access, write-radius gating, and the
//! minimal `WorldGenLevel` facade. It does NOT port the scheduler, the
//! `ChunkPyramid` tables, server production generation, or generator
//! realization — those defer with their owning units (#185).
//!
//! ## The two typed seams
//!
//! Two upstream types the region consumes are not ported yet, so the region
//! reads them through the smallest typed contract it needs instead of
//! fabricating their internals:
//!
//! - [`GenerationChunkHolderView`] — the `mc.server.level.pipeline.holder`
//!   `GenerationChunkHolder` surface (`getChunkIfPresentUnchecked` /
//!   `getPersistedStatus`, plus the Rust-only mutable half for `setBlock`).
//!   The real holder (futures, scheduling, status ladder) lands with the
//!   holder unit; the region only needs a holder that can hand back a chunk
//!   completed to a given status.
//! - [`ChunkStepView`] — the `net.minecraft.world.level.chunk.status.ChunkStep`
//!   surface the region reads (`directDependencies` / `targetStatus` /
//!   `blockStateWriteRadius`). The real `ChunkStep`/`ChunkDependencies` land
//!   with the chunk.status unit; the region only needs the per-ring dependency
//!   list and the write radius.
//!
//! ## The `ServerLevel` seam
//!
//! Java's `WorldGenRegion(ServerLevel, StaticCache2D, ChunkStep, ChunkAccess)`
//! reads `seed`/`levelData`/`random`/`dimensionType`/`minY`/`height`/`seaLevel`
//! and the `getUncachedNoiseBiome`/POI/light/difficulty/border surface off the
//! `ServerLevel`. The M2 STUB seam (MANIFEST) absorbs that residual
//! `ServerLevel` reference as stubs; this value layer decomposes it into the
//! scalar values the region actually reads (`seed`/`min_y`/`height`/`sea_level`)
//! plus the injected [`NoiseBiomeSource`] for `getUncachedNoiseBiome`. The
//! heavy reads (POI update on `setBlock`, block-entity creation, light engine,
//! difficulty, world border, entity/player collections, registry access) are
//! not ported and fail or no-op explicitly rather than fabricating access —
//! each with a `RivetTodo` pointing at the owning unit.
//!
//! ## Biome access
//!
//! Java constructs `biomeManager = new BiomeManager(this, obfuscateSeed(seed))`
//! where `this` is the region as a `NoiseBiomeSource` (the `LevelReader`
//! default `getNoiseBiome` reads a cached chunk, falling back to
//! `getUncachedNoiseBiome`). The port cannot hold `Arc<Self>` (the ownership
//! model forbids a self-referential worldgen view), so the region's
//! `BiomeManager` is constructed over the same injected uncached source
//! `getUncachedNoiseBiome` delegates to, and the chunk-cached read defers
//! (RivetTodo #185 holder). The fiddled-distance corner interpolation itself is
//! faithfully the `BiomeManager` the region returns from `getBiomeManager`.

use std::sync::Arc;

use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};
use rivet_registry::fluid_id::FluidId;
use rivet_registry::generated::block_behaviors::{
    BEHAVIOR_FLAG_FLUID_EMPTY, BEHAVIOR_FLAG_RANDOM_TICKING, behavior_of,
};
use rivet_registry::generated::block_states::StateId;
use rivet_registry::holder::Holder;
use rivet_util::StaticCache2D;
use rivet_util::mth;
use rivet_util::util::log_and_pause_if_in_ide;
use rivet_world::biome::biome_manager::{BiomeManager, NoiseBiomeSource};
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::status::ChunkStatus;
use rivet_world::level::WorldGenLevel;
use rivet_world::level::height_accessor::LevelHeightAccessor;
use rivet_world::levelgen::heightmap::Types;

use crate::server::level::level_chunk::{BiomeId as ServerBiomeId, StructureKey, state_flags};

/// `Block.UPDATE_ALL` — `UPDATE_NEIGHBORS | UPDATE_CLIENTS` (1 | 2), the flag
/// `removeBlock`/`destroyBlock` pass to `setBlock`.
const UPDATE_ALL: i32 = 3;

/// `mc.server.level.pipeline.holder` STUB — the `GenerationChunkHolder` read
/// surface `WorldGenRegion` consumes.
///
/// Java `GenerationChunkHolder` (owned by the pending holder unit) exposes
/// `getChunkIfPresentUnchecked(ChunkStatus)` (the chunk stored for a completed
/// status, or null) and `getPersistedStatus()` (the held chunk's status, or
/// null). The region reads exactly those two. The real holder adds the
/// scheduling/future machinery the region never touches; this trait is the
/// smallest contract the region needs so it type-checks before the holder unit
/// lands. [`get_chunk_if_present_unchecked_mut`](Self::get_chunk_if_present_unchecked_mut)
/// is the Rust-only mutable half: Java's `setBlock` writes through the shared
/// `ChunkAccess` reference the holder returned, which Rust cannot express
/// without the mutable accessor.
pub trait GenerationChunkHolderView: Send {
    /// `GenerationChunkHolder.getChunkIfPresentUnchecked(ChunkStatus)` — the
    /// held chunk completed to at least `status`, if any.
    fn get_chunk_if_present_unchecked(
        &self,
        status: ChunkStatus,
    ) -> Option<&ChunkAccess<StateId, ServerBiomeId, StructureKey>>;

    /// `GenerationChunkHolder.getPersistedStatus()` — the held chunk's status,
    /// or `None` for a holder with no chunk (Java null).
    fn get_persisted_status(&self) -> Option<ChunkStatus>;

    /// Rust-only mutable half of
    /// [`get_chunk_if_present_unchecked`](Self::get_chunk_if_present_unchecked)
    /// for the region's `setBlock` chunk write (see the trait doc).
    fn get_chunk_if_present_unchecked_mut(
        &mut self,
        status: ChunkStatus,
    ) -> Option<&mut ChunkAccess<StateId, ServerBiomeId, StructureKey>>;
}

/// `net.minecraft.world.level.chunk.status.ChunkStep` STUB — the generating-step
/// surface `WorldGenRegion` reads.
///
/// Java `ChunkStep` (owned by the pending chunk.status unit) carries the
/// per-ring `ChunkDependencies` (`directDependencies` — a status per chessboard
/// distance ring), the `targetStatus`, and the `blockStateWriteRadius`. The
/// region reads exactly those three; the step's `task`/scheduler surface defers
/// with #185. A `ChunkStepView` is the smallest contract the region needs, so
/// the real `ChunkStep` can implement it when it lands.
pub trait ChunkStepView: Send {
    /// `ChunkStep.directDependencies()` — the per-ring dependency statuses, by
    /// chessboard distance from the generating chunk (`size()` / `get(int)` on
    /// Java's `ChunkDependencies`).
    fn direct_dependencies(&self) -> &[ChunkStatus];

    /// `ChunkStep.targetStatus()` — the status the step is generating toward.
    fn target_status(&self) -> ChunkStatus;

    /// `ChunkStep.blockStateWriteRadius()` — the write-zone radius (in chunks)
    /// around the generating chunk.
    fn block_state_write_radius(&self) -> i32;
}

/// Why a `getChunk(x, z, status, loadOrGenerate)` request failed during world
/// generation — the typed form of Java's `ReportedException(CrashReport)`
/// "Requested chunk unavailable during world generation" diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableChunkDiagnostic {
    /// The requested chunk's x (chunk coordinate).
    pub chunk_x: i32,
    /// The requested chunk's z (chunk coordinate).
    pub chunk_z: i32,
    /// `generatingStep.targetStatus()` — the status being generated toward.
    pub generating_status: ChunkStatus,
    /// The requested target status.
    pub requested_status: ChunkStatus,
    /// The held chunk's status, or `None` for a holder outside the cache.
    pub actual_status: Option<ChunkStatus>,
    /// The status allowed at this ring, or `None` beyond the dependency list.
    pub max_allowed_status: Option<ChunkStatus>,
    /// `generatingStep.directDependencies()` — the per-ring status list.
    pub dependencies: Vec<ChunkStatus>,
    /// The chessboard distance of the request from the generating chunk.
    pub distance: i32,
    /// The generating (center) chunk.
    pub generating_chunk: ChunkPos,
}

impl std::fmt::Display for UnavailableChunkDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Java renders "[out of cache bounds]" only when the request is beyond
        // the dependency list (`chunkHolder == null`). A request inside the
        // list whose holder holds no chunk yet would NPE in Java's crash-report
        // supplier; the port renders that distinct case honestly instead.
        let actual = if self.max_allowed_status.is_none() {
            "[out of cache bounds]".to_string()
        } else {
            self.actual_status
                .map_or_else(|| "[no chunk held]".to_string(), |s| s.name().to_string())
        };
        let max_allowed = self
            .max_allowed_status
            .map_or_else(|| "null".to_string(), |s| s.name().to_string());
        let deps = self
            .dependencies
            .iter()
            .map(|s| s.name())
            .collect::<Vec<_>>()
            .join(", ");
        write!(
            f,
            "Requested chunk unavailable during world generation: requesting chunk [{}, {}] while generating chunk [{}, {}] (distance: {}, generating status: {}, requested status: {}, actual status: {}, maximum allowed status: {}, dependencies: [{}])",
            self.chunk_x,
            self.chunk_z,
            self.generating_chunk.x(),
            self.generating_chunk.z(),
            self.distance,
            self.generating_status.name(),
            self.requested_status.name(),
            actual,
            max_allowed,
            deps,
        )
    }
}

/// `net.minecraft.server.level.WorldGenRegion` — the worldgen chunk-view
/// container.
///
/// Owns a [`StaticCache2D`] square of [`GenerationChunkHolderView`] references
/// (the chunk view), the generating [`ChunkStepView`] (per-ring dependencies +
/// write radius), the center chunk position, and the scalar `ServerLevel` seam
/// values. The value-layer slice implements the ring/status/distance contract,
/// the write-radius gate, and the minimal [`WorldGenLevel`] facade; the heavy
/// server reads defer (see the module doc).
pub struct WorldGenRegion {
    /// `cache` — the `StaticCache2D<GenerationChunkHolder>` chunk view.
    cache: StaticCache2D<Box<dyn GenerationChunkHolderView>>,
    /// `center` (as `getPos()`) — the generating chunk's position.
    center_pos: ChunkPos,
    /// `centerChunkX` — the center chunk's x.
    center_chunk_x: i32,
    /// `centerChunkZ` — the center chunk's z.
    center_chunk_z: i32,
    /// `generatingStep` — the step whose per-ring dependencies bound chunk
    /// availability and whose `blockStateWriteRadius` bounds writes.
    generating_step: Box<dyn ChunkStepView>,
    /// `writeRadius` — `generatingStep.blockStateWriteRadius()`.
    write_radius: i32,
    /// `seed` — `level.getSeed()`.
    seed: i64,
    /// `level.getMinY()`.
    min_y: i32,
    /// `level.getHeight()`.
    height: i32,
    /// `level.getSeaLevel()`.
    sea_level: i32,
    /// `biomeManager` — `new BiomeManager(this, obfuscateSeed(seed))`, the
    /// source routed to the injected uncached source (see the module doc).
    biome_manager: BiomeManager,
    /// The `ServerLevel.getUncachedNoiseBiome` seam — the injected noise-biome
    /// source `getUncachedNoiseBiome` delegates to (the generator realization
    /// defers with its owning unit).
    uncached_biome_source: Arc<dyn NoiseBiomeSource>,
}

impl WorldGenRegion {
    /// `new WorldGenRegion(ServerLevel, StaticCache2D, ChunkStep, ChunkAccess)`.
    ///
    /// The `ServerLevel` seam is decomposed into the scalar values the region
    /// reads (`seed`/`min_y`/`height`/`sea_level`) and the injected
    /// `uncached_biome_source` (the `getUncachedNoiseBiome` seam); the `center`
    /// `ChunkAccess` is decomposed into its `ChunkPos` (the region reads the
    /// cached chunks through the holder view, never a separate center
    /// reference).
    #[allow(clippy::too_many_arguments)] // mirrors the Java constructor's parameter surface.
    pub fn new(
        cache: StaticCache2D<Box<dyn GenerationChunkHolderView>>,
        center_pos: ChunkPos,
        generating_step: Box<dyn ChunkStepView>,
        seed: i64,
        min_y: i32,
        height: i32,
        sea_level: i32,
        uncached_biome_source: Arc<dyn NoiseBiomeSource>,
    ) -> Self {
        let write_radius = generating_step.block_state_write_radius();
        let biome_manager = BiomeManager::new(
            uncached_biome_source.clone(),
            BiomeManager::obfuscate_seed(seed),
        );
        WorldGenRegion {
            center_chunk_x: center_pos.x(),
            center_chunk_z: center_pos.z(),
            cache,
            center_pos,
            generating_step,
            write_radius,
            seed,
            min_y,
            height,
            sea_level,
            biome_manager,
            uncached_biome_source,
        }
    }

    /// `WorldGenRegion.getCenter()`.
    pub fn get_center(&self) -> ChunkPos {
        self.center_pos
    }

    /// `WorldGenRegion.hasChunk(int, int)` — whether the chessboard distance of
    /// the chunk from the generating chunk is within the dependency ring
    /// (`distance < directDependencies().size()`).
    pub fn has_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        let distance = self
            .center_pos
            .get_chessboard_distance_coords(chunk_x, chunk_z);
        distance < self.generating_step.direct_dependencies().len() as i32
    }

    /// `WorldGenRegion.getChunk(int, int)` — the 2-arg form, targeting
    /// `ChunkStatus.EMPTY`. Panics with the unavailable-chunk diagnostic when
    /// the chunk is not available, exactly as Java throws `ReportedException`.
    pub fn get_chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
    ) -> &ChunkAccess<StateId, ServerBiomeId, StructureKey> {
        self.try_get_chunk(chunk_x, chunk_z, ChunkStatus::Empty, true)
            .unwrap_or_else(|diagnostic| panic!("{}", diagnostic))
    }

    /// `WorldGenRegion.getChunk(int, int, ChunkStatus, boolean)` — the
    /// ring/status/distance contract, as a `Result` instead of Java's thrown
    /// `ReportedException`.
    ///
    /// The chessboard distance picks the ring's maximum allowed status from
    /// `directDependencies()`; a request whose target is at or before that
    /// status returns the holder's chunk completed to it. Anything else —
    /// beyond the dependency list, a target after the ring's allowed status, or
    /// a holder without a chunk at the allowed status — yields the
    /// [`UnavailableChunkDiagnostic`]. `loadOrGenerate` is unused (Java's body
    /// never reads it).
    pub fn try_get_chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        target_status: ChunkStatus,
        _load_or_generate: bool,
    ) -> Result<&ChunkAccess<StateId, ServerBiomeId, StructureKey>, UnavailableChunkDiagnostic>
    {
        let distance = self
            .center_pos
            .get_chessboard_distance_coords(chunk_x, chunk_z);
        // The per-ring dependency slice is only materialized for the error
        // diagnostic; the happy path reads it by index (no per-access Vec).
        let dependencies = self.generating_step.direct_dependencies();
        let max_allowed_status = if distance >= dependencies.len() as i32 {
            None
        } else {
            Some(dependencies[distance as usize])
        };

        let actual_status = if let Some(max_allowed) = max_allowed_status {
            let holder = self.cache.get(chunk_x, chunk_z);
            if target_status.is_or_before(max_allowed)
                && let Some(chunk) = holder.get_chunk_if_present_unchecked(max_allowed)
            {
                return Ok(chunk);
            }
            holder.get_persisted_status()
        } else {
            None
        };

        Err(UnavailableChunkDiagnostic {
            chunk_x,
            chunk_z,
            generating_status: self.generating_step.target_status(),
            requested_status: target_status,
            actual_status,
            max_allowed_status,
            dependencies: dependencies.to_vec(),
            distance,
            generating_chunk: self.center_pos,
        })
    }

    /// Rust-only mutable half of [`try_get_chunk`](Self::try_get_chunk) for the
    /// `setBlock` chunk write (Java's shared-reference aliasing).
    fn try_get_chunk_mut(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        target_status: ChunkStatus,
        _load_or_generate: bool,
    ) -> Result<&mut ChunkAccess<StateId, ServerBiomeId, StructureKey>, UnavailableChunkDiagnostic>
    {
        let distance = self
            .center_pos
            .get_chessboard_distance_coords(chunk_x, chunk_z);
        // The per-ring dependency slice is scoped so its immutable borrow of
        // `self` ends before the mutable `cache` access below (no per-access
        // Vec on the happy path; the diagnostic re-fetches it).
        let max_allowed_status = {
            let dependencies = self.generating_step.direct_dependencies();
            if distance >= dependencies.len() as i32 {
                None
            } else {
                Some(dependencies[distance as usize])
            }
        };

        let actual_status = if let Some(max_allowed) = max_allowed_status {
            let holder = self.cache.get_mut(chunk_x, chunk_z);
            // Read the persisted status before the mutable accessor so the
            // diagnostic never holds both the mutable chunk borrow and an
            // immutable holder borrow at once.
            let persisted = holder.get_persisted_status();
            if target_status.is_or_before(max_allowed)
                && let Some(chunk) = holder.get_chunk_if_present_unchecked_mut(max_allowed)
            {
                return Ok(chunk);
            }
            persisted
        } else {
            None
        };

        Err(UnavailableChunkDiagnostic {
            chunk_x,
            chunk_z,
            generating_status: self.generating_step.target_status(),
            requested_status: target_status,
            actual_status,
            max_allowed_status,
            dependencies: self.generating_step.direct_dependencies().to_vec(),
            distance,
            generating_chunk: self.center_pos,
        })
    }

    /// `WorldGenRegion.getChunk(int, int)` mutable half — the 2-arg contract
    /// for `setBlock`.
    fn get_chunk_mut(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
    ) -> &mut ChunkAccess<StateId, ServerBiomeId, StructureKey> {
        self.try_get_chunk_mut(chunk_x, chunk_z, ChunkStatus::Empty, true)
            .unwrap_or_else(|diagnostic| panic!("{}", diagnostic))
    }

    /// `WorldGenRegion.getBiomeManager()`.
    pub fn get_biome_manager(&self) -> &BiomeManager {
        &self.biome_manager
    }

    /// `WorldGenRegion.getUncachedNoiseBiome(int, int, int)` — the
    /// `ServerLevel.getUncachedNoiseBiome` seam, delegated to the injected
    /// uncached source.
    pub fn get_uncached_noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
    ) -> Holder<BiomeId> {
        self.uncached_biome_source
            .get_noise_biome(quart_x, quart_y, quart_z)
    }

    /// `WorldGenRegion.getFluidState(BlockPos)` — the block's fluid id, with
    /// the same outside-write-zone warning as `getBlockState`.
    ///
    /// Java returns a `FluidState`; the port's fluid value is the [`FluidId`]
    /// handle (OWNERSHIP — no `FluidState` value type yet), so the read is the
    /// state's fluid registry id.
    pub fn get_fluid_state(&self, pos: &BlockPos) -> FluidId {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.warn_if_read_outside_write_zone(chunk_x, chunk_z);
        let chunk = self.get_chunk(chunk_x, chunk_z);
        let state = chunk_block_state(chunk, pos);
        FluidId(state.fluid_id())
    }

    /// `WorldGenRegion.isStateAtPosition(BlockPos, Predicate<BlockState>)`.
    pub fn is_state_at_position(
        &self,
        pos: &BlockPos,
        predicate: impl Fn(BlockState) -> bool,
    ) -> bool {
        predicate(self.get_block_state(pos))
    }

    /// `WorldGenRegion.isFluidAtPosition(BlockPos, Predicate<FluidState>)` —
    /// over the port's [`FluidId`] handle (see [`get_fluid_state`](Self::get_fluid_state)).
    pub fn is_fluid_at_position(
        &self,
        pos: &BlockPos,
        predicate: impl Fn(FluidId) -> bool,
    ) -> bool {
        predicate(self.get_fluid_state(pos))
    }

    /// `WorldGenRegion.setBlock(BlockPos, BlockState, int updateFlags, int
    /// updateLimit)` — the write-radius-gated block write.
    ///
    /// Outside the write zone `ensureCanWrite` returns false and the write is
    /// dropped (Java logs and returns false). Inside, the block is written
    /// through the holder's chunk section. The side-effects Java gates on the
    /// `updateFlags` all defer with their owning units — the POI update
    /// (`level.updatePOIOnBlockStateChange` on `(flags & UPDATE_SKIP_POI) == 0`,
    /// where `UPDATE_SKIP_POI = 4096`, #185), the block-entity create/remove
    /// (the `hasBlockEntity()` DUMMY proto vs `EntityBlock` level paths and
    /// the `oldState.hasBlockEntity()` removal, block-entity unit), and the
    /// shape post-process mark (`getPostProcessPos` on
    /// `(flags & UPDATE_KNOWN_SHAPE) == 0`, where `UPDATE_KNOWN_SHAPE = 16`,
    /// #228) — so the value layer does not consume the flags at all (it never
    /// fabricates the deferred side-effects). The `updateLimit` is likewise
    /// unread by the ported surface.
    pub fn set_block(
        &mut self,
        pos: &BlockPos,
        block_state: BlockState,
        _update_flags: i32,
        _update_limit: i32,
    ) -> bool {
        if !self.ensure_can_write(pos) {
            return false;
        }
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        // The base `ChunkAccess` carries no persisted status (the concrete chunk
        // types do), so the `heightmapsAfter()` set the write must update is
        // threaded from the holder seam (Java's `ProtoChunk.setBlockState` reads
        // `getPersistedStatus().heightmapsAfter()`).
        let persisted_status = self.cache.get(chunk_x, chunk_z).get_persisted_status();
        let chunk = self.get_chunk_mut(chunk_x, chunk_z);
        // `oldState` — the previous state `chunk.setBlockState` returns; the
        // block-entity removal (`oldState.hasBlockEntity()`) and POI update
        // read it, so the write retains it for those deferred seams (#185).
        let _old_state = write_block(chunk, pos, block_state, persisted_status);
        true
    }

    /// `WorldGenRegion.removeBlock(BlockPos, boolean)` —
    /// `setBlock(pos, Blocks.AIR.defaultBlockState(), Block.UPDATE_ALL)` (with
    /// the default `updateLimit = 0`).
    pub fn remove_block(&mut self, pos: &BlockPos, _moved_by_piston: bool) -> bool {
        self.set_block(pos, BlockState::new(StateId(0)), UPDATE_ALL, 0)
    }

    /// `WorldGenRegion.ensureCanWrite(BlockPos)` — the writability gate every
    /// write checks first.
    ///
    /// Inside the write zone the gate is open; Java's upgrade branch
    /// (`center.isUpgrading()` → the generation height-accessor check) never
    /// runs here because `BelowZeroRetrogen` is always null in the port, so
    /// `isUpgrading()` is always false (RivetTodo #185).
    pub fn ensure_can_write(&self, pos: &BlockPos) -> bool {
        if !self.is_within_write_zone(pos) {
            let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
            let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
            // Java logs + IDE-pauses once (`hasSetFarWarned`) and thread-dumps
            // when debugging; the value layer logs through the shared
            // `logAndPauseIfInIde` seam and defers the one-time flag + dump.
            log_and_pause_if_in_ide(&format!(
                "Detected setBlock in a far chunk [{}, {}], pos: {:?}, status: {}",
                chunk_x,
                chunk_z,
                pos,
                self.generating_step.target_status().name()
            ));
            return false;
        }
        true
    }

    /// `WorldGenRegion.isWithinWriteZone(BlockPos)`.
    pub fn is_within_write_zone(&self, pos: &BlockPos) -> bool {
        self.is_within_write_zone_coords(
            SectionPos::block_to_section_coord(pos.get_x()),
            SectionPos::block_to_section_coord(pos.get_z()),
        )
    }

    /// The private `isWithinWriteZone(int, int)` half.
    fn is_within_write_zone_coords(&self, chunk_x: i32, chunk_z: i32) -> bool {
        mth::abs_i32(self.center_chunk_x.wrapping_sub(chunk_x)) <= self.write_radius
            && mth::abs_i32(self.center_chunk_z.wrapping_sub(chunk_z)) <= self.write_radius
    }

    /// `warnIfReadOutsideWriteZone(int, int)` — the unsafe-read warning for a
    /// non-center chunk outside the write zone (Java still performs the read).
    fn warn_if_read_outside_write_zone(&self, chunk_x: i32, chunk_z: i32) {
        if (self.center_chunk_x != chunk_x || self.center_chunk_z != chunk_z)
            && !self.is_within_write_zone_coords(chunk_x, chunk_z)
        {
            let read_distance = mth::abs_max(
                mth::abs_i32(self.center_chunk_x.wrapping_sub(chunk_x)),
                mth::abs_i32(self.center_chunk_z.wrapping_sub(chunk_z)),
            );
            // Java appends the `currentlyGenerating` narration when set
            // (RivetTodo #232); the value layer omits it.
            log_and_pause_if_in_ide(&format!(
                "Detected unsafe terrain read during worldgen: reading from chunk [{}, {}] while generating chunk [{}, {}] (distance: {}, write radius: {}), step: {}",
                chunk_x,
                chunk_z,
                self.center_chunk_x,
                self.center_chunk_z,
                read_distance,
                self.write_radius,
                self.generating_step.target_status().name()
            ));
        }
    }

    /// `WorldGenRegion.getSkyDarken()` — 0 during worldgen.
    pub fn get_sky_darken(&self) -> i32 {
        0
    }

    /// `WorldGenRegion.isClientSide()` — false.
    pub fn is_client_side(&self) -> bool {
        false
    }

    /// `WorldGenRegion.getSeaLevel()` — `level.getSeaLevel()`.
    pub fn get_sea_level(&self) -> i32 {
        self.sea_level
    }
}

impl LevelHeightAccessor for WorldGenRegion {
    fn get_height(&self) -> i32 {
        self.height
    }

    fn get_min_y(&self) -> i32 {
        self.min_y
    }
}

impl WorldGenLevel for WorldGenRegion {
    /// `WorldGenLevel.getSeed()`.
    fn get_seed(&self) -> i64 {
        self.seed
    }

    /// `WorldGenLevel.ensureCanWrite(BlockPos)` — the write-radius gate.
    fn ensure_can_write(&self, pos: &BlockPos) -> bool {
        WorldGenRegion::ensure_can_write(self, pos)
    }

    /// `BlockGetter.getBlockState(BlockPos)` — the gated chunk block read.
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.warn_if_read_outside_write_zone(chunk_x, chunk_z);
        let chunk = self.get_chunk(chunk_x, chunk_z);
        chunk_block_state(chunk, pos)
    }

    /// `LevelReader.getBiome(BlockPos)` — `getBiomeManager().getBiome(pos)`
    /// (the fiddled-distance read through the injected uncached source; see the
    /// module doc).
    fn get_biome(&self, pos: &BlockPos) -> Holder<BiomeId> {
        self.get_biome_manager().get_biome(pos)
    }

    /// `LevelReader.getHeight(Heightmap.Types, int, int)` — the gated heightmap
    /// read.
    ///
    /// [`Heightmap::get_height_at`] is the Java `ChunkAccess.getHeight` value
    /// (`getFirstAvailable(x, z) - 1` — the topmost opaque block's Y; a
    /// never-set entry reads `minY - 1`), so a primed entry is returned
    /// directly. When the entry is absent the port cannot prime it here —
    /// `ChunkAccess::prime_heightmaps` takes `&mut` (`ChunkAccess::get_height_at`
    /// is the `&mut`-typed half) — and returns the superflat floor's height
    /// `minY` (the topmost block sits at `minY`, so Java's primed `getHeight` is
    /// `minY`; a genuinely all-air column would be `minY - 1`, deferred with the
    /// `&mut` seam, RivetTodo #228). Since `write_block` primes and updates the
    /// `heightmapsAfter()` entries on every write, the None branch is only a
    /// never-written chunk; written chunks return the real post-write height.
    fn get_height_at(&self, ty: Types, x: i32, z: i32) -> i32 {
        let chunk_x = SectionPos::block_to_section_coord(x);
        let chunk_z = SectionPos::block_to_section_coord(z);
        self.warn_if_read_outside_write_zone(chunk_x, chunk_z);
        let chunk = self.get_chunk(chunk_x, chunk_z);
        match chunk.heightmaps()[ty as usize].as_ref() {
            Some(heightmap) => heightmap.get_height_at(x & 15, z & 15, chunk.get_min_y()),
            None => chunk.get_min_y(),
        }
    }
}

/// The region's block-state read — the `ChunkAccess` block-state spine: air for
/// an out-of-build-height or all-air position, else the section storage read.
///
/// Java `ProtoChunk.getBlockState` returns `Blocks.VOID_AIR` for an
/// out-of-build-height read (a distinct block from `AIR`); `LevelChunk` returns
/// `AIR` (`getBlockStateFinal`'s empty/out-of-range section). The region reads
/// the base `ChunkAccess`, which carries no `void_air` value — the concrete
/// chunk types own theirs — so the port reads the server's dense `StateId`
/// where air is id 0 for both. That matches the `LevelChunk` read; the
/// `ProtoChunk` `VOID_AIR` (block id 794) divergence is block-identity only —
/// both states are `isAir()` with an empty fluid, which is all the value-layer
/// consumers observe.
fn chunk_block_state(
    chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
    pos: &BlockPos,
) -> BlockState {
    let y = pos.get_y();
    if chunk.is_outside_build_height(y) {
        return BlockState::new(StateId(0));
    }
    let section_index = chunk.get_section_index(y);
    let section = chunk.get_section(section_index as usize);
    if section.non_empty_block_count() == 0 {
        return BlockState::new(StateId(0));
    }
    BlockState::new(section.get_block_state(pos.get_x() & 15, y & 15, pos.get_z() & 15))
}

/// The region's block-state write — the section-level `setBlockState` with the
/// server `StateId` behavior predicates, then the `heightmapsAfter()` update.
///
/// This mirrors the core of Java `ProtoChunk.setBlockState` /
/// `LevelChunk.setBlockState` (the region's chunks are the generic
/// `ChunkAccess` base, and during worldgen they are `ProtoChunk`s until FULL).
/// Out-of-build-height positions return air — Java returns
/// `Blocks.VOID_AIR.defaultBlockState()` (block 794; both air) — without
/// writing, matching Java's early return.
///
/// For an in-build-height write the section write is followed by Java's
/// unconditional `getPersistedStatus().heightmapsAfter()` update loop (prime
/// missing entries, then `update` per type). The base `ChunkAccess` carries no
/// persisted status — the concrete chunk types do — so the caller threads it in
/// from the holder seam (`persisted_status`); `None` skips the heightmap update
/// (a holder with no chunk is unreachable for an in-ring write, so this only
/// guards the free function's contract).
///
/// Java's `setBlockState` also runs, past `INITIALIZE_LIGHT`, the light-engine
/// update; the value layer defers that — the light engine is not on `ChunkAccess`
/// (#185) — as it defers the `UPDATE_SKIP_POI` POI update and the
/// `UPDATE_KNOWN_SHAPE` post-process mark (see [`set_block`](Self::set_block)).
/// The section write itself — the paletted-container set plus the
/// `BlockBehaviour` count and ticking bookkeeping — is faithful.
fn write_block(
    chunk: &mut ChunkAccess<StateId, ServerBiomeId, StructureKey>,
    pos: &BlockPos,
    block_state: BlockState,
    persisted_status: Option<ChunkStatus>,
) -> StateId {
    let y = pos.get_y();
    if chunk.is_outside_build_height(y) {
        return StateId(0);
    }
    let section_index = chunk.get_section_index(y);
    let section = chunk.get_section_mut(section_index as usize);
    let old_state = section.set_block_state(
        pos.get_x() & 15,
        y & 15,
        pos.get_z() & 15,
        block_state.id(),
        &state_is_air,
        &state_is_randomly_ticking,
        &fluid_is_empty,
        &fluid_is_randomly_ticking,
        &state_is_special_colliding,
    );
    // Java `ProtoChunk.setBlockState`: `getPersistedStatus().heightmapsAfter()`
    // — primed by `update_heightmaps_after` — updated with `localX, y, localZ`
    // and the placed state, unconditionally for every in-build-height write.
    if let Some(status) = persisted_status {
        chunk.update_heightmaps_after(
            status.heightmaps_after(),
            pos.get_x() & 15,
            y,
            pos.get_z() & 15,
            state_flags(block_state.id()),
        );
    }
    old_state
}

// The `BlockBehaviour` predicate set `LevelChunkSection.setBlockState` needs —
// the generated behavior-table flags (`is_air`/`is_randomly_ticking`/
// `fluid_is_empty`) and the two flags the table does not carry
// (`fluid_is_randomly_ticking`/`is_special_colliding`), conservatively false
// (exact for the air/stone superflat content and the no-fluid value layer;
// the real `FluidState.isRandomlyTicking`/`CollisionUtil.isSpecialCollidingBlock`
// defer with the fluid/block-behavior units).

/// `BlockBehaviour.isAir(state)`.
fn state_is_air(state: &StateId) -> bool {
    BlockState::new(*state).is_air()
}

/// `BlockBehaviour.isRandomlyTicking(state)` — the behavior-table flag.
fn state_is_randomly_ticking(state: &StateId) -> bool {
    behavior_of(*state) & BEHAVIOR_FLAG_RANDOM_TICKING != 0
}

/// `BlockBehaviour.getFluidState(state).isEmpty()` — the behavior-table flag.
fn fluid_is_empty(state: &StateId) -> bool {
    behavior_of(*state) & BEHAVIOR_FLAG_FLUID_EMPTY != 0
}

/// `getFluidState().isRandomlyTicking()` — false (no fluid-random-tick flag in
/// the generated table; exact for the no-fluid value layer).
fn fluid_is_randomly_ticking(_state: &StateId) -> bool {
    false
}

/// `CollisionUtil.isSpecialCollidingBlock(state)` — false (no special-colliding
/// flag in the generated table; exact for the superflat content).
fn state_is_special_colliding(_state: &StateId) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::core::QuartPos;
    use rivet_util::StaticCache2D;
    use rivet_world::chunk::upgrade_data::UpgradeData;
    use rivet_world::level::height_accessor::create as create_accessor;

    use crate::server::level::level_chunk::{
        BiomeId as ServerBiomeId, container_factory, superflat_content,
    };
    use rivet_world::superflat::{SUPERFLAT_HEIGHT, SUPERFLAT_MIN_Y};

    /// A test chunk — a superflat content chunk at `pos` with `sections_count`
    /// world sections, classified with the server's air/motion predicates.
    fn test_chunk(pos: ChunkPos) -> ChunkAccess<StateId, ServerBiomeId, StructureKey> {
        let content = superflat_content();
        let height_accessor = create_accessor(SUPERFLAT_MIN_Y, SUPERFLAT_HEIGHT);
        ChunkAccess::new(
            pos,
            UpgradeData::empty(height_accessor.get_sections_count() as usize),
            height_accessor,
            &container_factory(),
            0,
            Some(content.sections),
            &|s: &StateId| rivet_world::levelgen::heightmap::StateFlags {
                is_air: s.0 == 0,
                blocks_motion: s.0 != 0,
                has_fluid: false,
                is_leaves: false,
            },
        )
    }

    /// A test holder — a single chunk at a persisted status.
    struct TestHolder {
        chunk: ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        status: ChunkStatus,
    }

    impl TestHolder {
        fn new(
            chunk: ChunkAccess<StateId, ServerBiomeId, StructureKey>,
            status: ChunkStatus,
        ) -> Self {
            TestHolder { chunk, status }
        }
    }

    impl GenerationChunkHolderView for TestHolder {
        fn get_chunk_if_present_unchecked(
            &self,
            status: ChunkStatus,
        ) -> Option<&ChunkAccess<StateId, ServerBiomeId, StructureKey>> {
            self.status.is_or_after(status).then_some(&self.chunk)
        }

        fn get_persisted_status(&self) -> Option<ChunkStatus> {
            Some(self.status)
        }

        fn get_chunk_if_present_unchecked_mut(
            &mut self,
            status: ChunkStatus,
        ) -> Option<&mut ChunkAccess<StateId, ServerBiomeId, StructureKey>> {
            self.status.is_or_after(status).then_some(&mut self.chunk)
        }
    }

    /// A test holder with no chunk held (Java `GenerationChunkHolder` whose
    /// `getPersistedStatus()` returns null) — exercises the in-ring diagnostic
    /// branch that has no actual status to report.
    struct TestEmptyHolder;

    impl GenerationChunkHolderView for TestEmptyHolder {
        fn get_chunk_if_present_unchecked(
            &self,
            _status: ChunkStatus,
        ) -> Option<&ChunkAccess<StateId, ServerBiomeId, StructureKey>> {
            None
        }

        fn get_persisted_status(&self) -> Option<ChunkStatus> {
            None
        }

        fn get_chunk_if_present_unchecked_mut(
            &mut self,
            _status: ChunkStatus,
        ) -> Option<&mut ChunkAccess<StateId, ServerBiomeId, StructureKey>> {
            None
        }
    }

    /// A test step — a fixed per-ring dependency list, target, and write radius.
    struct TestStep {
        deps: Vec<ChunkStatus>,
        target: ChunkStatus,
        write_radius: i32,
    }

    impl TestStep {
        fn new(deps: Vec<ChunkStatus>, target: ChunkStatus, write_radius: i32) -> Self {
            TestStep {
                deps,
                target,
                write_radius,
            }
        }
    }

    impl ChunkStepView for TestStep {
        fn direct_dependencies(&self) -> &[ChunkStatus] {
            &self.deps
        }
        fn target_status(&self) -> ChunkStatus {
            self.target
        }
        fn block_state_write_radius(&self) -> i32 {
            self.write_radius
        }
    }

    /// A test noise source — a fixed biome at every quart.
    struct TestBiomeSource {
        biome: BiomeId,
    }

    impl NoiseBiomeSource for TestBiomeSource {
        fn get_noise_biome(&self, _quart_x: i32, _quart_y: i32, _quart_z: i32) -> Holder<BiomeId> {
            Holder::direct(self.biome)
        }
    }

    /// A feature-step region: `directDependencies = [BIOMES, NOISE, FEATURES,
    /// FULL]` (rings 0..3), write radius 1, every ring chunk present at its
    /// ring's allowed status. `cache` is a `2 * 3 + 1 = 7`-square centered on
    /// (0, 0), covering all four dependency rings.
    fn feature_region() -> WorldGenRegion {
        let deps = vec![
            ChunkStatus::Biomes,
            ChunkStatus::Noise,
            ChunkStatus::Features,
            ChunkStatus::Full,
        ];
        let step = TestStep::new(deps.clone(), ChunkStatus::Features, 1);
        let cache = StaticCache2D::create(0, 0, 3, &|x, z| {
            let distance = ChunkPos::new(0, 0).get_chessboard_distance_coords(x, z);
            let status = deps[distance.min(deps.len() as i32 - 1) as usize];
            Box::new(TestHolder::new(test_chunk(ChunkPos::new(x, z)), status))
                as Box<dyn GenerationChunkHolderView>
        });
        WorldGenRegion::new(
            cache,
            ChunkPos::new(0, 0),
            Box::new(step),
            0,
            SUPERFLAT_MIN_Y,
            SUPERFLAT_HEIGHT,
            -63,
            Arc::new(TestBiomeSource {
                biome: BiomeId::from_id(40),
            }),
        )
    }

    /// The center chunk's position, for the per-ring contract tests.
    fn center() -> ChunkPos {
        ChunkPos::new(0, 0)
    }

    // -----------------------------------------------------------------------
    // Ring / status / distance contract
    // -----------------------------------------------------------------------

    /// Requesting a chunk at distance 0 (the center) with a target at or before
    /// the ring's `BIOMES` allowed status returns it.
    #[test]
    fn center_ring_returns_the_chunk_for_allowed_status() {
        let region = feature_region();
        assert_eq!(
            region
                .try_get_chunk(0, 0, ChunkStatus::Empty, true)
                .expect("center at BIOMES serves EMPTY")
                .get_pos(),
            center()
        );
        assert_eq!(
            region
                .try_get_chunk(0, 0, ChunkStatus::Biomes, true)
                .expect("center at BIOMES serves BIOMES")
                .get_pos(),
            center()
        );
    }

    /// The per-ring allowed status: ring 1 allows `NOISE`, ring 2 allows
    /// `FEATURES`, ring 3 allows `FULL`; each returns the chunk for a target at
    /// or before the ring's status and diagnoses a target after it.
    #[test]
    fn per_ring_allowed_status_bounds_the_contract() {
        let region = feature_region();
        // Ring 1: NOISE.
        assert!(region.try_get_chunk(1, 0, ChunkStatus::Noise, true).is_ok());
        let diagnostic = region
            .try_get_chunk(1, 0, ChunkStatus::Features, true)
            .err()
            .expect("target after ring-1 NOISE is unavailable");
        assert_eq!(diagnostic.distance, 1);
        assert_eq!(diagnostic.max_allowed_status, Some(ChunkStatus::Noise));
        assert_eq!(diagnostic.requested_status, ChunkStatus::Features);

        // Ring 2: FEATURES.
        assert!(
            region
                .try_get_chunk(2, 0, ChunkStatus::Features, true)
                .is_ok()
        );
        let diagnostic = region
            .try_get_chunk(2, 0, ChunkStatus::Full, true)
            .err()
            .expect("target after ring-2 FEATURES is unavailable");
        assert_eq!(diagnostic.distance, 2);
        assert_eq!(diagnostic.max_allowed_status, Some(ChunkStatus::Features));
        assert_eq!(diagnostic.requested_status, ChunkStatus::Full);

        // Ring 3: FULL (chunk (3, 0) is chessboard distance 3).
        assert!(region.try_get_chunk(3, 0, ChunkStatus::Full, true).is_ok());
        assert_eq!(
            region
                .try_get_chunk(3, 0, ChunkStatus::Full, true)
                .expect("ring-3 FULL serves FULL")
                .get_pos(),
            ChunkPos::new(3, 0)
        );
    }

    /// The unavailable-chunk diagnostic: a request beyond the dependency list
    /// (distance 4, outside the 4-ring `[BIOMES, NOISE, FEATURES, FULL]`) yields
    /// `max_allowed_status = None` and the "out of cache bounds" actual status;
    /// a request at distance 1 for a status after the ring carries the full
    /// crash-report surface.
    #[test]
    fn unavailable_chunk_diagnostic_carries_the_request_details() {
        let region = feature_region();

        // Beyond the dependency list: the ring has no allowed status at all.
        let beyond = region
            .try_get_chunk(4, 0, ChunkStatus::Empty, true)
            .err()
            .expect("distance 4 is beyond the 4-ring dependency list");
        assert_eq!(beyond.chunk_x, 4);
        assert_eq!(beyond.chunk_z, 0);
        assert_eq!(beyond.distance, 4);
        assert_eq!(beyond.max_allowed_status, None);
        assert_eq!(beyond.actual_status, None);
        assert_eq!(beyond.generating_status, ChunkStatus::Features);
        assert_eq!(beyond.generating_chunk, center());
        assert_eq!(
            beyond.dependencies,
            vec![
                ChunkStatus::Biomes,
                ChunkStatus::Noise,
                ChunkStatus::Features,
                ChunkStatus::Full,
            ]
        );

        // A ring-1 request past the allowed status: actual status is the held
        // chunk's NOISE, the max allowed is NOISE.
        let too_far = region
            .try_get_chunk(1, 0, ChunkStatus::Full, true)
            .err()
            .expect("target after the ring-1 allowed status is unavailable");
        assert_eq!(too_far.actual_status, Some(ChunkStatus::Noise));
        assert_eq!(too_far.max_allowed_status, Some(ChunkStatus::Noise));
        assert_eq!(too_far.requested_status, ChunkStatus::Full);

        // The Display message mirrors the crash-report surface (generating
        // status, requested, actual, max allowed, dependencies, distance).
        let message = beyond.to_string();
        assert!(message.contains("Requested chunk unavailable during world generation"));
        assert!(message.contains("requesting chunk [4, 0]"));
        assert!(message.contains("distance: 4"));
        assert!(message.contains("generating status: minecraft:features"));
        assert!(message.contains("requested status: minecraft:empty"));
        assert!(message.contains("actual status: [out of cache bounds]"));
        assert!(message.contains("maximum allowed status: null"));
        assert!(
            message
                .contains("minecraft:biomes, minecraft:noise, minecraft:features, minecraft:full")
        );
    }

    /// An in-ring request whose holder holds no chunk yet renders "[no chunk
    /// held]" — the branch Java would NPE on (its `getPersistedStatus().getName()`
    /// supplier), rendered honestly instead of conflated with out-of-cache.
    #[test]
    fn in_ring_diagnostic_distinguishes_no_chunk_held_from_out_of_cache() {
        let deps = vec![
            ChunkStatus::Biomes,
            ChunkStatus::Noise,
            ChunkStatus::Features,
            ChunkStatus::Full,
        ];
        let step = TestStep::new(deps.clone(), ChunkStatus::Features, 1);
        let cache = StaticCache2D::create(0, 0, 1, &|_x, _z| {
            Box::new(TestEmptyHolder) as Box<dyn GenerationChunkHolderView>
        });
        let region = WorldGenRegion::new(
            cache,
            ChunkPos::new(0, 0),
            Box::new(step),
            0,
            SUPERFLAT_MIN_Y,
            SUPERFLAT_HEIGHT,
            -63,
            Arc::new(TestBiomeSource {
                biome: BiomeId::from_id(40),
            }),
        );

        // Ring 1 (chunk (1,0)) allows NOISE; the holder has no chunk, so a
        // request for NOISE fails with no actual status to report.
        let diagnostic = region
            .try_get_chunk(1, 0, ChunkStatus::Noise, true)
            .err()
            .expect("in-ring empty holder cannot serve the request");
        assert_eq!(diagnostic.max_allowed_status, Some(ChunkStatus::Noise));
        assert_eq!(diagnostic.actual_status, None);
        let message = diagnostic.to_string();
        assert!(message.contains("actual status: [no chunk held]"));
        assert!(message.contains("maximum allowed status: minecraft:noise"));
    }

    /// `hasChunk` is the same distance bound as the ring contract.
    #[test]
    fn has_chunk_matches_the_ring_bound() {
        let region = feature_region();
        for distance in 0..4 {
            // A chunk at (distance, 0) is within the 4-ring dependency list.
            assert!(region.has_chunk(distance, 0), "ring {distance} has a chunk");
        }
        assert!(!region.has_chunk(4, 0));
        assert!(!region.has_chunk(-4, 0));
    }

    // -----------------------------------------------------------------------
    // Write-radius gating
    // -----------------------------------------------------------------------

    /// A block write inside the write radius writes through to the cached
    /// chunk; a write outside the radius is gated (returns false) and leaves
    /// the chunk untouched.
    #[test]
    fn write_inside_the_radius_writes_and_outside_is_gated() {
        let mut region = feature_region();

        // Inside the radius: the center chunk, written with a non-air state.
        let inside = BlockPos::new(1, 64, 2);
        assert!(region.is_within_write_zone(&inside));
        assert!(region.ensure_can_write(&inside));
        assert_eq!(
            region.get_block_state(&inside),
            BlockState::new(StateId(0)),
            "the superflat chunk is air before the write"
        );
        assert!(region.set_block(&inside, BlockState::new(StateId(1)), UPDATE_ALL, 0));
        assert_eq!(
            region.get_block_state(&inside),
            BlockState::new(StateId(1)),
            "the write landed inside the radius"
        );

        // Outside the write radius but inside the cache ring (distance 2, write
        // radius 1): the write is gated and the chunk stays air.
        let outside = BlockPos::new(33, 64, 0); // chunk (2, 0)
        assert!(!region.is_within_write_zone(&outside));
        assert!(!region.ensure_can_write(&outside));
        assert!(!region.set_block(&outside, BlockState::new(StateId(1)), UPDATE_ALL, 0));
        assert_eq!(
            region.get_block_state(&outside),
            BlockState::new(StateId(0)),
            "the gated write must not land"
        );
    }

    /// `removeBlock` routes through `setBlock(AIR, UPDATE_ALL)`: gated outside
    /// the radius, effective inside.
    #[test]
    fn remove_block_is_gated_like_set_block() {
        let mut region = feature_region();
        let inside = BlockPos::new(0, 64, 1);
        assert!(region.set_block(&inside, BlockState::new(StateId(1)), UPDATE_ALL, 0));
        assert_eq!(region.get_block_state(&inside), BlockState::new(StateId(1)));

        assert!(region.remove_block(&inside, false));
        assert_eq!(region.get_block_state(&inside), BlockState::new(StateId(0)));

        let outside = BlockPos::new(33, 64, 0);
        assert!(!region.remove_block(&outside, false));
    }

    /// A block read outside the write radius (but inside the cache ring) still
    /// reads the chunk — Java warns and proceeds; the gating is write-only.
    #[test]
    fn read_outside_the_write_radius_still_reads() {
        let region = feature_region();
        let outside = BlockPos::new(33, 64, 0); // chunk (2, 0), distance 2
        assert!(!region.is_within_write_zone(&outside));
        // The read is served from the cached chunk (air in the superflat
        // content) rather than being blocked.
        assert_eq!(
            region.get_block_state(&outside),
            BlockState::new(StateId(0))
        );
    }

    // -----------------------------------------------------------------------
    // Biome access
    // -----------------------------------------------------------------------

    /// `getBiome` routes through the injected uncached source via the region's
    /// `BiomeManager` (the fiddled-distance interpolation).
    #[test]
    fn get_biome_routes_through_the_injected_source() {
        let region = feature_region();
        // The test source returns plains (id 40) at every quart; the fiddled
        // corner read resolves through it.
        let biome = region.get_biome(&BlockPos::new(0, 64, 0));
        assert_eq!(biome, Holder::direct(BiomeId::from_id(40)));
        assert_eq!(
            region.get_uncached_noise_biome(
                QuartPos::from_block(0),
                QuartPos::from_block(64),
                QuartPos::from_block(0),
            ),
            Holder::direct(BiomeId::from_id(40)),
        );
    }

    // -----------------------------------------------------------------------
    // Minimal WorldGenLevel facade
    // -----------------------------------------------------------------------

    /// The scalar facade values the region exposes.
    #[test]
    fn facade_exposes_the_scalar_level_values() {
        let region = feature_region();
        assert_eq!(region.get_seed(), 0);
        assert_eq!(region.get_min_y(), SUPERFLAT_MIN_Y);
        assert_eq!(region.get_height(), SUPERFLAT_HEIGHT);
        assert_eq!(region.get_sea_level(), -63);
        assert_eq!(region.get_sky_darken(), 0);
        assert!(!region.is_client_side());
        assert_eq!(region.get_center(), center());
        // `getHeight` of the superflat content: `WorldSurface` is a
        // FINAL_HEIGHTMAPS entry never primed by a BIOMES-persisted chunk, so
        // the None fallback returns `minY` — Java's primed `getHeight` for the
        // stone floor whose topmost block sits at `minY`.
        assert_eq!(
            region.get_height_at(Types::WorldSurface, 0, 0),
            SUPERFLAT_MIN_Y
        );
    }

    /// `set_block` primes and updates the `heightmapsAfter()` entries, so a
    /// written block moves `getHeight` to its Y (Java `ProtoChunk.setBlockState`
    /// runs the update unconditionally after every in-build-height write).
    #[test]
    fn set_block_updates_the_worldgen_heightmap() {
        let mut region = feature_region();
        // Write stone at block (0, 0, 0) — chunk (0, 0), inside the write radius.
        let pos = BlockPos::new(0, 0, 0);
        assert!(region.set_block(&pos, BlockState::new(StateId(1)), UPDATE_ALL, 0));
        // The center chunk's persisted status is BIOMES (< CARVERS), so the
        // WORLDGEN_HEIGHTMAPS types are maintained. `getHeight` is the topmost
        // block's Y — the written stone at 0 (the floor at -64 is below it).
        assert_eq!(region.get_height_at(Types::WorldSurfaceWg, 0, 0), 0);
        // `OceanFloorWg` (blocks-motion) tracks the same column.
        assert_eq!(region.get_height_at(Types::OceanFloorWg, 0, 0), 0);
        // The block itself reads back as non-air.
        assert!(!region.get_block_state(&pos).is_air());
        // A column that was never written still reads the floor's `minY`.
        assert_eq!(
            region.get_height_at(Types::WorldSurfaceWg, 15, 15),
            SUPERFLAT_MIN_Y
        );
    }
}
