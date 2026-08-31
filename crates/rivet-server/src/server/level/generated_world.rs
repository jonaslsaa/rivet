//! The generated-world pipeline spine (issue #185) — the server-side
//! realization that replaces the superflat fixture with real generated-world
//! ownership, without ever serving a ProtoChunk as FULL.
//!
//! ## The spine
//!
//! Given a seed, [`OverworldGenerator`] realizes the OVERWORLD
//! `NoiseGeneratorSettings`/`NoiseBasedChunkGenerator` from the merged worldgen
//! registries (one leak of the immutable `RegistryAccess` + `RandomState` per
//! world/seed — see [`RandomState`]'s borrow), and [`OverworldNoiseBiomeSource`]
//! adapts the overworld `MultiNoiseBiomeSource` over the realized climate
//! sampler. [`GenerationChunkHolder`] owns a real `ProtoChunk` and drives it
//! BIOMES→NOISE→SURFACE→CARVERS through the `GENERATION_PYRAMID` executor.
//!
//! **This is the standalone foundation, not yet the live ticket path.** No
//! production caller (ticket, `ChunkMap`, or boot) creates a holder yet — the
//! `RivetTodo(#185)`s mark the wiring that lands with the `.chunk.generator`
//! pipeline unit. The leak of the per-world registries/`RandomState` is the
//! mechanism that keeps the worldgen objects `'static` for the holder closures;
//! RivetTodo(#185): the world-level registry-ownership unit replaces it with a
//! reclaimable per-world owner before this is wired to the live path.
//!
//! ## The typed downstream boundary
//!
//! `WorldGenContext::generate_through` supports generation through `CARVERS` —
//! the BIOMES→NOISE→SURFACE→CARVERS task bodies are wired to the real Paper
//! drivers (`fillFromNoise` / `buildSurface` / `applyCarvers`), so an EMPTY
//! chunk can reach CARVERS. The FEATURES task body is also wired: the
//! caller-supplied [`GenerationChunkHolder::new`] features closure runs Java's
//! `ChunkStatusTasks.generateFeatures` — the `Heightmap.primeHeightmaps(chunk,
//! FINAL_HEIGHTMAPS)` priming and the `addVanillaDecorations` prologue over a
//! bounded region-backed FEATURES dependency window — the decoration-seed
//! derivation (`SectionPos.of(centerPos, level.getMinSectionY()).origin()` fed
//! to `setDecorationSeed`), a `WorldGenRegion` that borrows the center chunk
//! and owns the complete 17x17 dependency cache (CARVERS at distances 0/1,
//! STRUCTURE_STARTS through distance 8), and the Paper-order 3x3 biome-union
//! gather + `retainAll`. It then resolves generation settings for the FULL
//! `biomeSource.possibleBiomes()` list in source order and builds the
//! FeatureSorter once from it (Paper's `ChunkGenerator.featuresPerStep`,
//! `ChunkGenerator.java` 97-100 — the 3x3 union only picks which feature
//! indices execute per step). The generated feature tables cover EVERY
//! overworld possible biome (55 — the full list, not the reachable subset),
//! so the full list resolves. FEATURES only proceeds when the caller has
//! supplied a proven structure-decoration capability; otherwise it returns
//! `GenError::StructureDecorationIndexUnavailable` before feature RNG or
//! placement. With that capability, the lake, amethyst-geode, monster-room,
//! and the Batch 2/3/4 dispatch leaves (ore,
//! disk, spring, simple_block, block_column, vines, seagrass, freeze_top_layer,
//! underwater_magma, multiface_growth) are decoded from the generated JSON and
//! run with their exact feature seeds. The generated placed/configured closure
//! resolves the random-selector root through the shared registries; Paper's
//! seed-42 draw misses the 0.025 and 0.05 branches, then selects
//! `minecraft:dark_oak_leaf_litter` at the 0.6666667 branch. Its supported
//! placement chain then reaches the next honest boundary, the
//! `minecraft:freeze_top_layer` world-state seam at step 10/global index 0.
//! The chunk stays CARVERS. The INITIALIZE_LIGHT/
//! LIGHT steps are executor-wired but engine-gated (the holder wires no light
//! engine, so it cannot reach LIGHT). The public
//! [`GenerationChunkHolder::generate_spawn_with_region`] API is the standalone
//! SPAWN foundation: it requires the scheduler-owned radius-one
//! [`SpawnRegionProtos`] workspace, so it is intentionally not attached to the
//! center-only executor yet. No live ticket/ChunkMap caller reaches SPAWN or
//! FULL in this slice; ordinary `generate_through(SPAWN)` therefore refuses at
//! the missing-workspace seam rather than fabricating a detached region.
//! Everything the value layer does not wire is refused *before* running work: a
//! path through a light step with no engine is refused as
//! `GenError::LightEngineMissing`, and a target past LIGHT (FULL) is out
//! of range (`GenError::UnsupportedStatus`). The holder's
//! [`GenerationChunkHolder::generate_through`] surfaces these as typed
//! [`GeneratedChunkError::Generation`] / [`GeneratedChunkError::UnsupportedStatus`]
//! rather than stamping a status that was never generated. A generated chunk
//! enters the server authority only through the consuming
//! [`GenerationChunkHolder::into_level_chunk`] FULL promotion, which moves the
//! `ProtoChunk` out and calls [`LevelChunk::from_generated_spawn_proto`]; every
//! non-SPAWN generated parent status is refused atomically with
//! [`GeneratedChunkError::Convert`] (carrying [`LevelChunkBridgeError::GeneratedStatusNotSpawn`])
//! — no sub-FULL proto is ever fabricated into a FULL chunk or falls back to
//! superflat.
//!
//! ## The `GenerationChunkHolderView` seam
//!
//! The `WorldGenRegion` view contract is generic over the chunk value types
//! (`GenerationChunkHolderView<T, B, S>`), so the worldgen executor's region
//! (`BlockState`/`section_reconstruction::BiomeId`) uses the generic chunk-view
//! methods while the dense server region keeps its block-state `WorldGenLevel`
//! facade on the `StateId`/`ServerBiomeId`/`StructureKey` specialization. The
//! FEATURES body composes its full dependency window through [`CenterHolder`]
//! (which borrows the center chunk's base) and [`OwnedHolder`] (which owns the
//! ring chunks) — see [`compose_feature_region`]. A sub-FULL `ProtoChunk` still
//! cannot be converted into the dense server chunk (only an exact SPAWN-parent proto is
//! promoted by [`GenerationChunkHolder::into_level_chunk`] → [`LevelChunk::from_generated_spawn_proto`]),
//! so a pre-FULL generated chunk never enters the ChunkMap authority — the
//! refusal is atomic and typed. The holder hands out the chunk's status and
//! typed generation results too.
//!
//! Ownership follows OWNERSHIP.md: the generator/biome source are immutable
//! per-world config shared by `Arc` (no `Arc<RwLock>` game state — the only
//! interior mutability is `RandomState`'s own uncontended noise cache), the
//! holder and its `ProtoChunk` live on the sync tick thread by value.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use serde_json::Value;

use rivet_registry::Registry;
use rivet_registry::access::RegistryAccess;
use rivet_registry::access::{LayeredRegistryAccess, RegistryLayer};
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::PropertyValue;
use rivet_registry::builder::RegistryBuilder;
use rivet_registry::core::BlockPos;
use rivet_registry::core::ChunkPos;
use rivet_registry::core::SectionPos;
use rivet_registry::generated::biomes::BIOME_BY_ID;
use rivet_registry::generated::block_behaviors::{StaticCollisionBox, static_collision_shape_of};
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::generated::feature_data::{
    BIOME_GENERATION_SETTINGS_BY_NAME, CONFIGURED_FEATURE_BY_NAME, ConfiguredFeatureEntry,
    MOB_SPAWN_SETTINGS_BY_NAME, PLACED_FEATURE_BY_NAME, PlacedFeatureEntry,
};
use rivet_registry::holder::Holder;
use rivet_registry::holder::RegistryId;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registry_ops::RegistryOps;
use rivet_registry::{Identifier, RegistrationInfo, ResourceKey};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::json_ops::JsonOps;
use rivet_util::StaticCache2D;
use rivet_util::WorldgenRandom;
use rivet_util::random::LegacyRandomSource;
use rivet_util::random_source::XoroshiroRandomSource;
use rivet_util::random_source::random_support;
use rivet_util::weighted::{Weighted, WeightedList, WeightedRandom};
use rivet_util::{PositionalRandomFactory, RandomSource};
use rivet_world::biome::BiomeManager;
use rivet_world::biome::BiomeResolver;
use rivet_world::biome::BiomeSource;
use rivet_world::biome::biome_generation_settings::{BiomeGenerationSettings, PlainBuilder};
use rivet_world::biome::biome_manager::NoiseBiomeSource;
use rivet_world::biome::climate::Sampler;
use rivet_world::biome::feature_sorter::{StepFeatureData, build_features_per_step};
use rivet_world::biome::generated_biome_source::{dense_biome_id, overworld_biome_source};
use rivet_world::biome::multi_noise_biome_source::MultiNoiseBiomeSource;
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::chunk_generator::ChunkGenerator;
use rivet_world::chunk::proto_chunk::ProtoChunk;
#[cfg(test)]
use rivet_world::chunk::status::GeneratedLightTask;
use rivet_world::chunk::status::{ChunkStatus, GENERATION_PYRAMID, GenError, WorldGenContext};
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::chunk::storage::section_reconstruction::{
    BiomeId as WorldgenBiomeId, current_version_container_factory,
};
use rivet_world::chunk::upgrade_data::UpgradeData;
use rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries;
use rivet_world::level::height_accessor::LevelHeightAccessor;
use rivet_world::level::height_accessor::create as create_height_accessor;
use rivet_world::level::{WorldBorderSettings, WorldGenLevel};
use rivet_world::levelgen::blending::blender::Blender;
use rivet_world::levelgen::feature::configurations::block_column_configuration::block_column_configuration_codec;
use rivet_world::levelgen::feature::configurations::disk_configuration::disk_configuration_codec;
use rivet_world::levelgen::feature::configurations::geode_configuration::geode_configuration_codec;
use rivet_world::levelgen::feature::configurations::huge_mushroom_feature_configuration::huge_mushroom_feature_configuration_codec;
use rivet_world::levelgen::feature::configurations::multiface_growth_configuration::multiface_growth_configuration_codec;
use rivet_world::levelgen::feature::configurations::ore_configuration::ore_configuration_codec;
use rivet_world::levelgen::feature::configurations::probability_feature_configuration::probability_feature_configuration_codec;
use rivet_world::levelgen::feature::configurations::simple_block_configuration::simple_block_configuration_codec;
use rivet_world::levelgen::feature::configurations::spring_configuration::spring_configuration_codec;
use rivet_world::levelgen::feature::configurations::underwater_magma_configuration::underwater_magma_configuration_codec;
use rivet_world::levelgen::feature::configurations::weighted_random_feature_configuration::WeightedRandomFeatureConfiguration;
use rivet_world::levelgen::feature::configurations::{
    CompositeFeatureConfiguration, FeatureConfiguration, NoneFeatureConfiguration,
    RandomBooleanFeatureConfiguration, RandomFeatureConfiguration,
};
use rivet_world::levelgen::feature::lake_feature::lake_configuration_codec;
use rivet_world::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
use rivet_world::levelgen::feature::weighted_placed_feature::WeightedPlacedFeature;
use rivet_world::levelgen::feature::{
    ConfiguredFeatureErased, FeatureId, feature_id_from_registry_name,
};
use rivet_world::levelgen::generation_step::Decoration;
use rivet_world::levelgen::heightmap::{FINAL_HEIGHTMAPS, Types};
use rivet_world::levelgen::noise::registry_keys::NOISE_SETTINGS;
use rivet_world::levelgen::noisegen::noise_based_chunk_generator::NoiseBasedChunkGenerator;
use rivet_world::levelgen::noisegen::noise_generator_settings::OVERWORLD;
use rivet_world::levelgen::noisegen::random_state::RandomState;
use rivet_world::levelgen::placement::{
    ErasedPlacementModifier, PlacedFeature, biome_filter_codec, block_predicate_filter_codec,
    count_on_every_layer_placement_codec, count_placement_codec, environment_scan_placement_codec,
    fixed_placement_codec, height_range_placement_codec, heightmap_placement_codec,
    in_square_placement_codec, noise_based_count_placement_codec,
    noise_threshold_count_placement_codec, random_offset_placement_codec, rarity_filter_codec,
    surface_relative_threshold_filter_codec, surface_water_depth_filter_codec,
};
use rivet_world::levelgen::world_generation_context::WorldGenerationContext;

use crate::server::level::level_chunk::{LevelChunk, LevelChunkBridgeError, StructureKey};
use crate::server::level::world_gen_region::{
    CenterHolder, GenerationChunkHolderView, OwnedProtoHolder, WorldGenRegion,
};
use crate::server::lighting::{GeneratedLightStorage, GeneratedLightWorkspace};

/// The overworld generated-chunk error surface — every failure is typed, never
/// a silent fallback.
pub enum GeneratedChunkError {
    /// The status executor refused the promotion: a missing data prerequisite
    /// (`GenError::BiomesNotGenerated`/`DataNotCarried`), a demotion, or a
    /// wired-task mismatch. The chunk is left untouched.
    Generation(GenError),
    /// A target past `LIGHT` — the executor rejected it before running any
    /// work. Naming the requested status makes the downstream boundary explicit.
    /// (A target through a light step with no engine is instead refused as
    /// `GenError::LightEngineMissing`, and the wired FEATURES rung stops at
    /// its first typed generation boundary — see [`GenerationChunkHolder::new`].)
    UnsupportedStatus(ChunkStatus),
    /// The FULL conversion refused: the `LevelChunk` bridge rejected the proto.
    /// [`LevelChunkBridgeError::GeneratedStatusNotSpawn`] fires before the proto is consumed
    /// for any non-SPAWN generated parent status; `UnsupportedLightState` fires
    /// before the value transform, and `PaletteMap` arises from the `map_values`
    /// re-encode itself. A refusal never produces a partial `LevelChunk` or an
    /// install — the holder is consumed on every outcome (it is a self-taking
    /// API), and any generated-light workspace is returned in the error instead
    /// of being dropped with the consumed holder.
    Convert {
        error: LevelChunkBridgeError,
        generated_light_storage: Option<GeneratedLightStorage>,
    },
    /// The radius-one generated SPAWN workspace or the next entity capability
    /// refused population. The center remains below SPAWN and no entity NBT is
    /// fabricated.
    SpawnRegion(SpawnRegionError),
}

impl fmt::Debug for GeneratedChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generation(error) => f.debug_tuple("Generation").field(error).finish(),
            Self::UnsupportedStatus(status) => {
                f.debug_tuple("UnsupportedStatus").field(status).finish()
            }
            Self::Convert {
                error,
                generated_light_storage,
            } => f
                .debug_struct("Convert")
                .field("error", error)
                .field(
                    "generated_light_storage_len",
                    &generated_light_storage.as_ref().map(HashMap::len),
                )
                .finish(),
            Self::SpawnRegion(inner) => f.debug_tuple("SpawnRegion").field(inner).finish(),
        }
    }
}

impl fmt::Display for GeneratedChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeneratedChunkError::Generation(inner) => {
                write!(f, "chunk generation failed: {inner}")
            }
            GeneratedChunkError::UnsupportedStatus(status) => write!(
                f,
                "generating to {status:?} is unsupported: FULL is a consuming promotion boundary"
            ),
            GeneratedChunkError::Convert { error, .. } => write!(
                f,
                "a generated chunk could not be promoted to a FULL LevelChunk: {error}"
            ),
            GeneratedChunkError::SpawnRegion(inner) => {
                write!(f, "generated SPAWN population failed: {inner}")
            }
        }
    }
}

impl std::error::Error for GeneratedChunkError {}

/// Errors while extracting the immutable generated FEATURES workspace.
#[derive(Debug, thiserror::Error)]
pub enum GeneratedWorkspaceError {
    /// A generated registry/settings boundary refused the workspace.
    #[error(transparent)]
    Generation(#[from] GenError),
}

impl From<GeneratedWorkspaceError> for GenError {
    fn from(error: GeneratedWorkspaceError) -> Self {
        match error {
            GeneratedWorkspaceError::Generation(error) => error,
        }
    }
}

/// Typed failures at the generated SPAWN region/entity boundary. The region
/// itself is fully backed by tick-thread-owned `ProtoChunk`s; a failure means
/// the input workspace or the next entity capability is unavailable, never an
/// implicit no-op or a fabricated entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnRegionError {
    /// The cache must contain exactly the eight horizontal neighbours.
    MissingNeighbour { pos: ChunkPos },
    /// A supplied chunk is not one of the eight neighbours of the center.
    UnexpectedChunk { pos: ChunkPos },
    /// A supplied position occurred more than once.
    DuplicateChunk { pos: ChunkPos },
    /// A center/ring proto does not carry the status required by the SPAWN
    /// step's direct dependency table (`LIGHT` at distance zero and `BIOMES` at
    /// distance one).
    InsufficientStatus {
        pos: ChunkPos,
        actual: ChunkStatus,
        required: ChunkStatus,
    },
    /// The biome resolved by the shared region has no generated mob-settings
    /// entry. This is a data/registry boundary, not an entity-construction
    /// boundary, so it must not be reported as a rejected entity candidate.
    MissingBiomeSettings {
        chunk_pos: ChunkPos,
        biome: Option<&'static str>,
    },
    /// Placement gates passed, but the entity registry/constructor is not yet
    /// ported, so the population must stop before writing an entity tag.
    UnsupportedEntity {
        chunk_pos: ChunkPos,
        biome: &'static str,
        entity_type: &'static str,
        position: BlockPos,
    },
}

impl fmt::Display for SpawnRegionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpawnRegionError::MissingNeighbour { pos } => {
                write!(f, "generated SPAWN region is missing neighbour {pos}")
            }
            SpawnRegionError::UnexpectedChunk { pos } => {
                write!(
                    f,
                    "generated SPAWN region received non-adjacent chunk {pos}"
                )
            }
            SpawnRegionError::DuplicateChunk { pos } => {
                write!(f, "generated SPAWN region received duplicate chunk {pos}")
            }
            SpawnRegionError::InsufficientStatus {
                pos,
                actual,
                required,
            } => write!(
                f,
                "generated SPAWN region chunk {pos} is {actual:?}, requires {required:?}"
            ),
            SpawnRegionError::MissingBiomeSettings { chunk_pos, biome } => write!(
                f,
                "cannot populate generated SPAWN chunk {chunk_pos}: no generated mob settings for biome {biome:?}"
            ),
            SpawnRegionError::UnsupportedEntity {
                chunk_pos,
                biome,
                entity_type,
                position,
            } => write!(
                f,
                "cannot spawn {entity_type} in biome {biome} for chunk {chunk_pos} at {position}: entity construction is unavailable"
            ),
        }
    }
}

impl std::error::Error for SpawnRegionError {}

/// The immutable decoration plan shared by every holder in one world.
///
/// Paper memoizes `ChunkGenerator.featuresPerStep` from the full possible-biome
/// list. Keep the same settings sources, holder-id diagnostics, and sorter in
/// the per-world generator instead of rebuilding them for every FEATURES target.
struct FeaturePlan {
    placed_by_id: HashMap<u32, &'static str>,
    settings_sources: Vec<(BiomeGenerationSettings, &'static str)>,
    feature_list: Vec<StepFeatureData>,
}

/// The caller-owned radius-one SPAWN workspace. The center remains in the
/// [`GenerationChunkHolder`]; this value owns only the eight neighbouring
/// protos, all on the same tick thread. No `Arc<RwLock>` or clone-backed cache
/// is used. Each proto must already carry at least `BIOMES`; the center must be
/// at least `LIGHT` when [`GenerationChunkHolder::generate_spawn_with_region`]
/// consumes it.
pub struct SpawnRegionProtos {
    center: ChunkPos,
    /// Canonical x-then-z order, matching `StaticCache2D::from_entries`.
    neighbours: Vec<(
        ChunkPos,
        ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    )>,
    /// Owned current-bounds snapshot for the production SPAWN region. This is
    /// a value copy, never a borrow or moved live `WorldBorder`; the snapshot
    /// is normalized to a stationary extent at construction time.
    world_border_settings: WorldBorderSettings,
}

impl SpawnRegionProtos {
    /// Build the exact 3x3 cache around `center` from eight owned neighbours.
    /// Input order is irrelevant; cache iteration is canonicalized by chunk
    /// coordinates when the Paper `WorldGenRegion` is composed. This explicit
    /// default-border constructor is only for callers that truly have Paper's
    /// default border; a world-aware scheduler must use
    /// [`Self::new_with_world_border_settings`] so its current-bounds snapshot
    /// cannot be silently replaced.
    pub fn new(
        center: ChunkPos,
        neighbours: impl IntoIterator<Item = ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>>,
    ) -> Result<Self, SpawnRegionError> {
        Self::new_with_world_border_settings(
            center,
            neighbours,
            WorldBorderSettings::default_settings(),
        )
    }

    /// Build a radius-one workspace with an owned current-bounds WorldBorder
    /// snapshot. The supplied settings may describe an active interpolation;
    /// only its current center/current size are retained and the resulting
    /// region is stationary (`lerp_time = 0`).
    pub fn new_with_world_border_settings(
        center: ChunkPos,
        neighbours: impl IntoIterator<Item = ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>>,
        settings: WorldBorderSettings,
    ) -> Result<Self, SpawnRegionError> {
        let mut by_pos = HashMap::with_capacity(8);
        for chunk in neighbours {
            let pos = chunk.get_pos();
            if pos == center || center.get_chessboard_distance(&pos) > 1 {
                return Err(SpawnRegionError::UnexpectedChunk { pos });
            }
            if by_pos.insert(pos, chunk).is_some() {
                return Err(SpawnRegionError::DuplicateChunk { pos });
            }
        }
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let pos = ChunkPos::new(center.x().wrapping_add(dx), center.z().wrapping_add(dz));
                if !by_pos.contains_key(&pos) {
                    return Err(SpawnRegionError::MissingNeighbour { pos });
                }
            }
        }
        let mut neighbours: Vec<_> = by_pos.into_iter().collect();
        neighbours.sort_by_key(|(pos, _)| (pos.x(), pos.z()));
        Ok(Self {
            center,
            neighbours,
            world_border_settings: settings.current_bounds_snapshot(),
        })
    }

    /// The center this workspace surrounds.
    pub fn center(&self) -> ChunkPos {
        self.center
    }

    /// The stationary current-bounds snapshot used when composing the
    /// production-capable `WorldGenRegion`.
    pub fn world_border_settings(&self) -> WorldBorderSettings {
        self.world_border_settings
    }

    /// Return the eight neighbour protos to the tick-thread scheduler after
    /// the synchronous SPAWN view is dropped. The canonical x-then-z order is
    /// preserved; each proto remains the same owned value that was supplied to
    /// [`Self::new`], including any heightmap writes made through the region.
    pub fn into_neighbours(self) -> Vec<ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>> {
        self.neighbours
            .into_iter()
            .map(|(_, chunk)| chunk)
            .collect()
    }
}

pub type FeatureWritebacks = Vec<(
    ChunkPos,
    ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
)>;

/// The caller-owned FEATURES dependency workspace.
///
/// Paper's `WorldGenRegion` borrows scheduler-owned `GenerationChunkHolder`s;
/// this standalone value layer has no live scheduler yet, so the caller can
/// provide that ownership explicitly through this sync-thread workspace. A
/// decoration pass reads snapshots of its ring chunks, then replaces every
/// owned dependency entry atomically after the region completes. This preserves
/// heightmap, post-processing, block-entity, and tick mutations even when they
/// occur outside the block-state write radius. Failed passes never mutate this
/// map.
type OwnedDependencyChunks =
    Rc<RefCell<HashMap<ChunkPos, ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>>>>;

#[derive(Clone, Default)]
pub struct FeatureWorkspace {
    chunks: OwnedDependencyChunks,
}

impl FeatureWorkspace {
    /// Create an empty scheduler-owned dependency workspace.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace an authoritative generated dependency chunk.
    pub fn insert(&self, chunk: ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>) {
        let pos = chunk.get_pos();
        self.chunks.borrow_mut().insert(pos, chunk);
    }

    /// Inspect one authoritative dependency chunk without exposing its map
    /// borrow beyond the synchronous callback.
    pub fn with_chunk<R>(
        &self,
        pos: ChunkPos,
        f: impl FnOnce(Option<&ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>>) -> R,
    ) -> R {
        f(self.chunks.borrow().get(&pos))
    }

    /// Number of authoritative dependency chunks currently retained.
    pub fn len(&self) -> usize {
        self.chunks.borrow().len()
    }

    /// Whether this workspace has no authoritative dependency chunks.
    pub fn is_empty(&self) -> bool {
        self.chunks.borrow().is_empty()
    }

    /// Snapshot every retained dependency chunk in deterministic coordinate
    /// order. The shared G4 scheduler uses this value-only view to publish
    /// cross-chunk FEATURES writes back to its holder arena without exposing
    /// the interior map borrow.
    pub(crate) fn snapshot_chunks(
        &self,
    ) -> Vec<ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>> {
        let mut chunks: Vec<_> = self
            .chunks
            .borrow()
            .values()
            .map(snapshot_generated_chunk)
            .collect();
        chunks.sort_by_key(|chunk| (chunk.get_pos().x(), chunk.get_pos().z()));
        chunks
    }
}

/// The registry-derived structure decoration index consumed by Paper's
/// `ChunkGenerator.addVanillaDecorations` prelude. The structure manager and
/// placement bodies are still outside this value slice, but the registry's
/// per-step cardinalities are real inputs to the decoration boundary rather
/// than a fixture `Some(0)` or an invented seed offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructureFeatureIndex {
    counts_by_step: [usize; Decoration::VALUES.len()],
}

impl StructureFeatureIndex {
    /// Derive the index from the exact 26.2 vanilla `Registries.STRUCTURE`
    /// entries registered by `net.minecraft.data.worldgen.Structures`.
    fn from_registry(entries: &[(&str, Decoration)]) -> Self {
        let mut counts_by_step = [0; Decoration::VALUES.len()];
        for &(_, step) in entries {
            counts_by_step[decoration_index(step)] += 1;
        }
        Self { counts_by_step }
    }

    /// The normal overworld registry-derived structure index.
    pub(crate) fn vanilla_registry() -> Self {
        Self::from_registry(VANILLA_STRUCTURE_REGISTRY)
    }

    /// Adapt the focused omitted-structure-loop fixture into the index shape.
    /// Production uses the registry-derived index above.
    #[cfg(test)]
    fn explicit_count(count: usize) -> Self {
        Self {
            counts_by_step: [count; Decoration::VALUES.len()],
        }
    }

    /// The number of registry structures at a Paper decoration step.
    pub(crate) fn count_for_step(&self, step: Decoration) -> usize {
        self.counts_by_step[decoration_index(step)]
    }

    /// The number of registry structures at a decoration ordinal. Steps past
    /// the vanilla enum are valid for a larger generated feature list and carry
    /// no vanilla structure entries.
    pub(crate) fn count_for_step_index(&self, step_index: usize) -> usize {
        Decoration::VALUES
            .get(step_index)
            .map_or(0, |step| self.count_for_step(*step))
    }

    /// The total number of structure registry entries.
    #[cfg(test)]
    fn total(&self) -> usize {
        self.counts_by_step.iter().sum()
    }
}

fn decoration_index(step: Decoration) -> usize {
    Decoration::VALUES
        .iter()
        .position(|candidate| *candidate == step)
        .expect("every decoration step is in Decoration::VALUES")
}

/// Paper's vanilla structure registry in bootstrap registration order. The
/// default `StructureSettings` step is `SURFACE_STRUCTURES`; only entries with
/// an explicit `.generationStep(...)` use another step. Keeping the registry
/// entries as data and deriving the counts above mirrors Java's
/// `structuresRegistry.stream().collect(groupingBy(structure -> structure.step().ordinal()))`.
const VANILLA_STRUCTURE_REGISTRY: &[(&str, Decoration)] = &[
    ("minecraft:pillager_outpost", Decoration::SurfaceStructures),
    ("minecraft:mineshaft", Decoration::UndergroundStructures),
    (
        "minecraft:mineshaft_mesa",
        Decoration::UndergroundStructures,
    ),
    ("minecraft:mansion", Decoration::SurfaceStructures),
    ("minecraft:jungle_pyramid", Decoration::SurfaceStructures),
    ("minecraft:desert_pyramid", Decoration::SurfaceStructures),
    ("minecraft:igloo", Decoration::SurfaceStructures),
    ("minecraft:shipwreck", Decoration::SurfaceStructures),
    ("minecraft:shipwreck_beached", Decoration::SurfaceStructures),
    ("minecraft:swamp_hut", Decoration::SurfaceStructures),
    ("minecraft:stronghold", Decoration::SurfaceStructures),
    ("minecraft:monument", Decoration::SurfaceStructures),
    ("minecraft:ocean_ruin_cold", Decoration::SurfaceStructures),
    ("minecraft:ocean_ruin_warm", Decoration::SurfaceStructures),
    ("minecraft:fortress", Decoration::UndergroundDecoration),
    ("minecraft:nether_fossil", Decoration::UndergroundDecoration),
    ("minecraft:end_city", Decoration::SurfaceStructures),
    (
        "minecraft:buried_treasure",
        Decoration::UndergroundStructures,
    ),
    ("minecraft:bastion_remnant", Decoration::SurfaceStructures),
    ("minecraft:village_plains", Decoration::SurfaceStructures),
    ("minecraft:village_desert", Decoration::SurfaceStructures),
    ("minecraft:village_savanna", Decoration::SurfaceStructures),
    ("minecraft:village_snowy", Decoration::SurfaceStructures),
    ("minecraft:village_taiga", Decoration::SurfaceStructures),
    ("minecraft:ruined_portal", Decoration::SurfaceStructures),
    (
        "minecraft:ruined_portal_desert",
        Decoration::SurfaceStructures,
    ),
    (
        "minecraft:ruined_portal_jungle",
        Decoration::SurfaceStructures,
    ),
    (
        "minecraft:ruined_portal_swamp",
        Decoration::SurfaceStructures,
    ),
    (
        "minecraft:ruined_portal_mountain",
        Decoration::SurfaceStructures,
    ),
    (
        "minecraft:ruined_portal_ocean",
        Decoration::SurfaceStructures,
    ),
    (
        "minecraft:ruined_portal_nether",
        Decoration::SurfaceStructures,
    ),
    ("minecraft:ancient_city", Decoration::UndergroundDecoration),
    ("minecraft:trail_ruins", Decoration::UndergroundStructures),
    (
        "minecraft:trial_chambers",
        Decoration::UndergroundStructures,
    ),
];

/// The per-world OVERWORLD generator realization — `NoiseBasedChunkGenerator`
/// resolved from the merged worldgen registries for a seed, plus the realized
/// `RandomState` and overworld biome source.
///
/// `RandomState` borrows the registries it resolves, so the immutable worldgen
/// `RegistryAccess` and the `RandomState` are leaked once per world/seed
/// (`Box::leak` → `'static'`); the value shell's `NoiseBasedChunkGenerator`
/// holds its settings as a `Holder::Direct` (a `Reference` holder would panic
/// without a threaded `HolderLookup` — see `settings_value`).
pub struct OverworldGenerator {
    generator: NoiseBasedChunkGenerator,
    random_state: &'static RandomState<'static>,
    biome_source: OverworldNoiseBiomeSource,
    /// The leaked worldgen `RegistryAccess` — the `registryAccess()` back
    /// reference the FEATURES decoration body (and the `WorldGenRegion` it
    /// constructs) resolves the placed-feature registry through, and the
    /// `lookupOrThrow(Registries.STRUCTURE)`/`Registries.PLACED_FEATURE`
    /// lookups Java's `addVanillaDecorations` performs. Stored alongside the
    /// random state it already shares the leak of (see [`OverworldGenerator::new`]).
    access: &'static RegistryAccess,
    /// Paper's per-world `paperConfig().featureSeeds.features` overrides,
    /// keyed by the configured feature resource key (`PlacedFeature.feature()`),
    /// not by the surrounding placed-feature key.
    feature_seeds: HashMap<String, i64>,
    /// The leaked feature `RegistryAccess` — the worldgen access composed with
    /// the frozen placed/configured-feature registries the seed-42 decoder and
    /// the selector/composite features resolve their recursive `Holder`
    /// references through (the `worldgen/placed_feature` /
    /// `worldgen/configured_feature` back-reference the `#181` dispatch and the
    /// Batch 2 selector arms require). See [`build_feature_access`].
    feature_access: &'static RegistryAccess,
    /// The immutable structure-registry step index used by every production
    /// FEATURES holder. It is derived once from the exact vanilla structure
    /// registry metadata and shared by value with the tick-thread holders.
    structure_feature_index: StructureFeatureIndex,
    seed: i64,
    /// Lazily built once per immutable world/seed, matching Paper's memoized
    /// `featuresPerStep`; the typed error is cached too so retries do not repeat
    /// an invalid workspace extraction.
    feature_plan: OnceLock<Result<FeaturePlan, GeneratedWorkspaceError>>,
}

impl OverworldGenerator {
    /// Realize the OVERWORLD generator for `seed`.
    ///
    /// `build_worldgen_registries` bundles the NOISE/DENSITY_FUNCTION/BIOME/
    /// NOISE_SETTINGS registries; `RandomState::create_from_provider` resolves
    /// the `overworld` settings preset through `NOISE_SETTINGS` and wires the
    /// router/sampler/surface system. The generator's settings holder is the
    /// resolved `overworld` value (Direct), matching the shell's value model.
    pub fn new(seed: i64) -> Self {
        Self::new_with_feature_seeds(seed, HashMap::new())
    }

    /// Realize the OVERWORLD generator with Paper's per-configured-feature
    /// population-seed overrides. A value of `-1` has Paper's sentinel meaning:
    /// retain the ordinary decoration seed and consume no extra draws.
    pub fn new_with_feature_seeds(seed: i64, feature_seeds: HashMap<String, i64>) -> Self {
        let access: &'static RegistryAccess = Box::leak(Box::new(build_worldgen_registries()));
        let random_state: &'static RandomState<'static> = Box::leak(Box::new(
            RandomState::create_from_provider(access, &OVERWORLD, seed),
        ));
        // The settings the random state resolved through `NOISE_SETTINGS`; the
        // generator needs an owned `Holder::Direct` (see the module doc).
        let settings = {
            let settings_registry = access.lookup_or_throw(&NOISE_SETTINGS);
            let settings_holder = settings_registry.get_or_throw(&OVERWORLD);
            settings_holder.value(settings_registry).clone()
        };
        let generator = NoiseBasedChunkGenerator::new(Holder::Direct(settings));
        let feature_access: &'static RegistryAccess =
            Box::leak(Box::new(build_feature_access(access)));
        OverworldGenerator {
            generator,
            random_state,
            biome_source: OverworldNoiseBiomeSource::new(random_state),
            access,
            feature_seeds,
            feature_access,
            structure_feature_index: StructureFeatureIndex::vanilla_registry(),
            seed,
            feature_plan: OnceLock::new(),
        }
    }

    fn feature_plan(&self) -> Result<&FeaturePlan, GeneratedWorkspaceError> {
        self.feature_plan
            .get_or_init(|| {
                let placed_registry_id = RegistryBuilder::new(&*PLACED_FEATURE).registry_id();
                let mut placed_by_id = HashMap::new();
                let full_possible_biomes = self.biome_source.possible_biomes();
                let settings_sources = resolve_feature_settings(
                    &full_possible_biomes,
                    placed_registry_id,
                    &mut placed_by_id,
                )
                .map_err(GeneratedWorkspaceError::from)?;
                let feature_list = build_features_per_step(
                    &settings_sources,
                    |(settings, _)| settings.features(),
                    true,
                );
                Ok(FeaturePlan {
                    placed_by_id,
                    settings_sources,
                    feature_list,
                })
            })
            .as_ref()
            .map_err(|error| match error {
                GeneratedWorkspaceError::Generation(error) => {
                    GeneratedWorkspaceError::Generation(*error)
                }
            })
    }

    /// The seed this generator was realized for.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// The leaked worldgen `RegistryAccess` — the FEATURES decoration body's
    /// registry back-reference (`registryAccess()`, `lookupOrThrow`).
    pub fn registry_access(&self) -> &'static RegistryAccess {
        self.access
    }

    /// Paper's registry-derived per-step structure decoration index.
    pub(crate) fn structure_feature_index(&self) -> StructureFeatureIndex {
        self.structure_feature_index
    }

    /// The leaked feature `RegistryAccess` — the worldgen access composed with
    /// the frozen placed/configured-feature registries (see the struct field).
    pub fn feature_access(&self) -> &'static RegistryAccess {
        self.feature_access
    }

    /// The value shell — the source of truth for the real world-surface bodies
    /// ([`ChunkGenerator`] delegates to it below).
    pub fn generator(&self) -> &NoiseBasedChunkGenerator {
        &self.generator
    }

    /// The realized per-world random state (leaked `'static`, shared by all
    /// holders of this world).
    pub fn random_state(&self) -> &'static RandomState<'static> {
        self.random_state
    }

    /// The overworld biome source over this world's climate sampler.
    pub fn biome_source(&self) -> &OverworldNoiseBiomeSource {
        &self.biome_source
    }

    /// Create a generation holder for `pos`, wiring the BIOMES→NOISE executor
    /// closures over the shared worldgen objects (`self` is `Arc`-shared so the
    /// `'static` closures capture a cheap clone). Structure decoration is not
    /// available in this value layer, so FEATURES refuses before feature RNG
    /// unless the caller supplies the consumed per-step index explicitly.
    pub fn create_holder(self: &Arc<Self>, pos: ChunkPos) -> GenerationChunkHolder {
        GenerationChunkHolder::new(pos, Arc::clone(self))
    }

    /// Create a holder attached to caller-owned dependency chunks. Holders
    /// sharing one workspace observe successful FEATURES writes exactly as the
    /// scheduler's shared `GenerationChunkHolder`s would.
    pub fn create_holder_with_workspace(
        self: &Arc<Self>,
        pos: ChunkPos,
        feature_workspace: FeatureWorkspace,
    ) -> GenerationChunkHolder {
        GenerationChunkHolder::new_with_workspace(pos, Arc::clone(self), feature_workspace)
    }

    /// Create a workspace-backed holder with the registry-derived structure
    /// decoration index used by the production generated-world scheduler.
    pub(crate) fn create_holder_with_workspace_and_structure_feature_index(
        self: &Arc<Self>,
        pos: ChunkPos,
        feature_workspace: FeatureWorkspace,
        structure_feature_index: Option<StructureFeatureIndex>,
    ) -> GenerationChunkHolder {
        GenerationChunkHolder::new_with_workspace_and_structure_feature_index(
            pos,
            Arc::clone(self),
            feature_workspace,
            structure_feature_index,
        )
    }
}

/// The `ChunkGenerator` realization — delegates the abstract world-surface
/// reads to the noisegen value shell's real bodies, resolving the trait seams
/// the `.chunk.generator` module doc's `RivetTodo(#185)` reconciliation note
/// requires (no separate source of truth).
impl ChunkGenerator for OverworldGenerator {
    fn get_min_y(&self) -> i32 {
        self.generator.get_min_y()
    }

    fn get_gen_depth(&self) -> i32 {
        self.generator.get_gen_depth()
    }

    fn get_sea_level(&self) -> i32 {
        self.generator.get_sea_level()
    }

    fn get_base_height(
        &self,
        x: i32,
        z: i32,
        ty: Types,
        height_accessor: &dyn LevelHeightAccessor,
        random_state: &RandomState,
    ) -> i32 {
        self.generator
            .get_base_height(x, z, ty, height_accessor, random_state)
    }

    fn get_base_column(
        &self,
        x: i32,
        z: i32,
        height_accessor: &dyn LevelHeightAccessor,
        random_state: &RandomState,
    ) -> Option<(i32, Vec<BlockState>)> {
        self.generator
            .get_base_column(x, z, height_accessor, random_state)
    }

    fn add_debug_screen_info(
        &self,
        result: &mut Vec<String>,
        random_state: &RandomState,
        feet_pos: &BlockPos,
    ) {
        self.generator
            .add_debug_screen_info(result, random_state, feet_pos)
    }
}

/// The overworld `NoiseBiomeSource` adapter — the `MultiNoiseBiomeSource`
/// (Java's `biomeSource` field, built from the `overworld` preset table) over
/// this world's climate `Sampler` (Java's `randomState.sampler()`). Shared
/// immutably by `Arc`; also a `BiomeResolver` so the BIOMES step can drive
/// `fill_biomes_from_noise` with the same source Paper uses.
#[derive(Debug, Clone)]
pub struct OverworldNoiseBiomeSource {
    source: MultiNoiseBiomeSource,
    sampler: Sampler,
}

impl OverworldNoiseBiomeSource {
    /// Build the source over the random state's realized sampler.
    pub fn new(random_state: &RandomState) -> Self {
        OverworldNoiseBiomeSource {
            source: overworld_biome_source(),
            sampler: random_state.sampler().clone(),
        }
    }

    /// The climate sampler this source samples with.
    pub fn sampler(&self) -> &Sampler {
        &self.sampler
    }

    /// `BiomeSource.possibleBiomes()` — the overworld source's possible-biome
    /// set (the `retainAll` argument of Java's `addVanillaDecorations` biome
    /// union: `possibleBiomes.retainAll(this.biomeSource.possibleBiomes())`).
    pub fn possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        self.source.possible_biomes()
    }
}

impl NoiseBiomeSource for OverworldNoiseBiomeSource {
    /// `BiomeManager.NoiseBiomeSource.getNoiseBiome` — samples this world's
    /// sampler and resolves the biome through the overworld table.
    fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Holder<BiomeId> {
        self.source
            .get_noise_biome(quart_x, quart_y, quart_z, &self.sampler)
    }
}

impl BiomeResolver for OverworldNoiseBiomeSource {
    /// `BiomeResolver.getNoiseBiome(qx, qy, qz, sampler)` — the `BiomeSource`
    /// default (`getNoiseBiome(sampler.sample(...))`), with the resolver's own
    /// table.
    fn get_noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        sampler: &Sampler,
    ) -> Holder<BiomeId> {
        self.source
            .get_noise_biome(quart_x, quart_y, quart_z, sampler)
    }
}

/// A real generated chunk being driven through the pipeline — owns the
/// worldgen `ProtoChunk` (block element `BlockState`, biome element the
/// worldgen `section_reconstruction::BiomeId`, structure key the server
/// `StructureKey`) and the BIOMES→NOISE→SURFACE→CARVERS→FEATURES executor over
/// the shared worldgen objects.
pub struct GenerationChunkHolder {
    chunk: ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    context: WorldGenContext<BlockState, WorldgenBiomeId, StructureKey, GeneratedLightStorage>,
    /// A typed FEATURES boundary is deterministic for the immutable world plan.
    /// Cache it after the first partial attempt so retrying the holder is
    /// idempotent and never repeats feature placement against a partially
    /// mutated proto.
    features_failure: Option<GenError>,
    /// Immutable per-world generator configuration shared by the tick-thread
    /// holder. Keeping this on the holder gives the explicit SPAWN-region API
    /// the same source of truth as the executor closure, without reaching into
    /// the closure or introducing shared game-state locks.
    generator: Arc<OverworldGenerator>,
    /// Successful FEATURES decoration retains a diagnostic copy of the
    /// concrete distance-1 writebacks. The authoritative copies live in
    /// `feature_workspace` and therefore outlive the temporary region.
    feature_writebacks: FeatureWritebacks,
    pending_feature_writebacks: Rc<RefCell<Option<FeatureWritebacks>>>,
    feature_workspace: FeatureWorkspace,
}

impl GenerationChunkHolder {
    /// Create a holder for `pos` under `generator` and wire the executor.
    ///
    /// The `'static` closures capture the shared `Arc<OverworldGenerator>`: the
    /// BIOMES body runs `ChunkAccess.fillBiomesFromNoise` (Java's
    /// `createBiomes` default) over the overworld biome source and this world's
    /// sampler, mapping each `Holder<BiomeId>` to the dense worldgen biome id;
    /// the NOISE body runs the shell's real `fillFromNoise` block write over an
    /// empty blender; the SURFACE body runs the real `buildSurface`; the
    /// CARVERS body runs the real `applyCarvers` (the overworld-carvers
    /// center-chunk loop — see the noisegen driver's doc for the deferred
    /// `WorldGenRegion`/`StructureManager` seams); the FEATURES body starts
    /// Java's `ChunkStatusTasks.generateFeatures` — `run_biome_decoration`
    /// first requires the caller-proven structure-decoration capability and
    /// otherwise refuses before mutating the center or feature RNG. With that
    /// capability, it runs `addVanillaDecorations` faithfully: the
    /// `FINAL_HEIGHTMAPS`
    /// priming, the decoration-seed derivation, a dependency-window composition (`compose_feature_region`: a
    /// `WorldGenRegion` that borrows the center chunk and owns the 17x17
    /// FEATURES cache (288 ring holders, with CARVERS at distances 0/1 and
    /// STRUCTURE_STARTS through distance 8), and the Paper-order biome-union
    /// gather + `retainAll` — and then decodes and runs the registry-backed
    /// lake, amethyst-geode, monster-room, underwater_magma, and glow_lichen
    /// paths at their exact feature seeds before stopping at the first selected
    /// unsupported path (seed-42 chunk (0,0): the selector chooses
    /// `minecraft:freeze_top_layer` at step 10/global 0); the chunk stays
    /// CARVERS.
    pub fn new(pos: ChunkPos, generator: Arc<OverworldGenerator>) -> Self {
        Self::new_with_workspace_and_structure_feature_index(
            pos,
            generator,
            FeatureWorkspace::new(),
            None,
        )
    }

    /// Create a holder whose FEATURES dependency chunks are persisted in the
    /// caller-owned workspace after successful decoration.
    pub fn new_with_workspace(
        pos: ChunkPos,
        generator: Arc<OverworldGenerator>,
        feature_workspace: FeatureWorkspace,
    ) -> Self {
        Self::new_with_workspace_and_structure_feature_index(
            pos,
            generator,
            feature_workspace,
            None,
        )
    }

    /// Create a workspace-backed holder with a registry-derived structure
    /// decoration index. `None` intentionally retains the typed missing-index
    /// boundary used by direct holder tests and non-scheduler callers.
    pub(crate) fn new_with_workspace_and_structure_feature_index(
        pos: ChunkPos,
        generator: Arc<OverworldGenerator>,
        feature_workspace: FeatureWorkspace,
        structure_feature_index: Option<StructureFeatureIndex>,
    ) -> Self {
        let height_accessor = create_height_accessor(
            generator.generator().get_min_y(),
            generator.generator().get_gen_depth(),
        );
        let chunk = ProtoChunk::new(
            pos,
            UpgradeData::empty(height_accessor.get_sections_count() as usize),
            height_accessor,
            &current_version_container_factory(),
            None,
            Blocks::AIR.default_block_state(),
            // Paper: `ProtoChunk.getBlockState` returns `Blocks.VOID_AIR`
            // (`minecraft:void_air`, raw id 794) outside build height. The
            // named `Blocks` subset has no `VOID_AIR` constant, so resolve it
            // by raw id here — `BlockState::of` reads `BLOCK_STATE_BASES[794]`
            // (default state 15292). A wrong id silently resolves to another
            // block's default (830 is `minecraft:mud_brick_wall` → 18441), so
            // this must stay pinned to the generated registry.
            BlockState::of(BlockId(794)),
            &resolve_state_flags,
        );
        let pending_feature_writebacks = Rc::new(RefCell::new(None));
        let context = WorldGenContext::new(
            {
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    let source = &generator.biome_source;
                    chunk.fill_biomes_from_noise(source, &source.sampler, &|holder| {
                        WorldgenBiomeId(dense_biome_id(holder))
                    });
                }
            },
            {
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    generator.generator().fill_from_noise(
                        Blender::empty(),
                        generator.random_state(),
                        chunk,
                    );
                }
            },
            {
                // `ChunkStatusTasks.generateSurface` → the real
                // `NoiseBasedChunkGenerator.buildSurface` (the ported SURFACE
                // driver). The `BiomeManager` is built over the world's biome
                // source with the obfuscated seed and the generation context
                // over the generator + height accessor — the same arguments
                // Java's `NoiseBasedChunkGenerator.buildSurface` receives.
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    let height_accessor = chunk.height_accessor();
                    let biome_manager = Arc::new(BiomeManager::new(
                        Arc::new(generator.biome_source.clone()),
                        BiomeManager::obfuscate_seed(generator.seed()),
                    ));
                    let generation_context =
                        Arc::new(WorldGenerationContext::new(&*generator, &height_accessor));
                    generator.generator().build_surface(
                        generator.random_state(),
                        biome_manager,
                        generation_context,
                        chunk,
                        None,
                    );
                }
            },
            {
                // `ChunkStatusTasks.generateCarvers` → the real
                // `NoiseBasedChunkGenerator.applyCarvers` (the ported CARVERS
                // driver). Same `BiomeManager` argument the SURFACE closure
                // builds (Java's `applyCarvers` receives the same manager the
                // `buildSurface` call used and derives the
                // `withDifferentSource` corrected one inside), plus the
                // overworld biome source — the `biomeSource` field the Java
                // driver wraps.
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    let biome_manager = Arc::new(BiomeManager::new(
                        Arc::new(generator.biome_source.clone()),
                        BiomeManager::obfuscate_seed(generator.seed()),
                    ));
                    generator.generator().apply_carvers(
                        &*generator,
                        generator.seed(),
                        generator.random_state(),
                        &biome_manager,
                        Arc::new(generator.biome_source.clone()),
                        chunk,
                    );
                }
            },
            {
                // `ChunkStatusTasks.generateFeatures` (Java) → the caller-owned
                // decoration body, typed. The real body is
                // `NoiseBasedChunkGenerator.applyBiomeDecoration` over a bounded
                // `WorldGenRegion`; `run_biome_decoration` runs Java's
                // `ChunkStatusTasks.generateFeatures` + `addVanillaDecorations`
                // faithfully — the `FINAL_HEIGHTMAPS` priming, the
                // section-origin decoration-seed derivation, the complete
                // FEATURES dependency window (the borrowed center chunk plus
                // the 17x17 cache with CARVERS at distances 0/1 and
                // STRUCTURE_STARTS through distance 8), and the Paper-order
                // biome-union gather + `retainAll`, the FULL-source-list
                // settings resolution (`ChunkGenerator.featuresPerStep`,
                // `ChunkGenerator.java` 97-100) and FeatureSorter, and the
                // exact per-feature seeds — and then decodes and runs the
                // registry-backed lake, amethyst-geode, monster-room,
                // underwater_magma, and glow_lichen entries before failing
                // typed at the first selected unsupported path
                // (`minecraft:freeze_top_layer`, step 10/global 0 after selector dispatch).
                // It must never be "improved" into a silent skip or a blanket
                // UnsupportedTask.
                // The closure captures one generator clone; the holder keeps
                // one additional handle for the shared SPAWN-region API.
                let generator = Arc::clone(&generator);
                let writeback_sink = Rc::clone(&pending_feature_writebacks);
                let feature_workspace = feature_workspace.clone();
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    match run_biome_decoration(
                        chunk,
                        &generator,
                        &feature_workspace,
                        structure_feature_index,
                    ) {
                        Ok(writebacks) => {
                            *writeback_sink.borrow_mut() = Some(writebacks);
                            Ok(())
                        }
                        Err(error) => Err(error),
                    }
                }
            },
        );
        // SPAWN is deliberately not attached to this center-only executor.
        // Paper's `generateSpawn` constructs a radius-one `WorldGenRegion`, and
        // this holder owns only its center proto. The later scheduler/G4 owner
        // must call `generate_spawn_with_region` with its owned eight-neighbour
        // workspace; until that attachment exists, ordinary
        // `generate_through(ChunkStatus::Spawn)` remains a typed refusal rather
        // than silently reverting to a detached center-only spawn.
        GenerationChunkHolder {
            chunk,
            context,
            features_failure: None,
            generator,
            feature_writebacks: Vec::new(),
            pending_feature_writebacks,
            feature_workspace,
        }
    }

    /// Attach a finite generated-light workspace in place. This is the stable
    /// handoff for the owning G4 scheduler: normal construction remains
    /// provider-less and boot does not fabricate runtime chunks, while the
    /// owner can attach its already-composed tick-thread workspace. If this
    /// holder was
    /// already carrying a task, its complete owned runtime storage is detached
    /// and returned before the replacement is installed.
    #[allow(dead_code)]
    pub(crate) fn attach_generated_light_workspace(
        &mut self,
        workspace: GeneratedLightWorkspace,
    ) -> Option<GeneratedLightStorage> {
        self.context.attach_generated_light_task(workspace)
    }

    /// Attach a custom generated-light task for scheduler ownership tests.
    #[cfg(test)]
    pub(crate) fn attach_generated_light_task_for_test(
        &mut self,
        task: impl GeneratedLightTask<BlockState, WorldgenBiomeId, StructureKey, GeneratedLightStorage>
        + 'static,
    ) -> Option<GeneratedLightStorage> {
        self.context.attach_generated_light_task(task)
    }

    /// Detach the current generated-light workspace without consuming the
    /// holder. This is the G4 recovery path after success, a typed generation
    /// error, or a caught panic.
    #[allow(dead_code)]
    pub(crate) fn take_generated_light_storage(&mut self) -> Option<GeneratedLightStorage> {
        self.context.take_generated_light_storage()
    }

    /// Tear down the holder while explicitly recovering any runtime chunks
    /// still owned by its generated-light task. `Drop` cannot return values, so
    /// owners must use this method when abandoning a holder.
    #[allow(dead_code)]
    pub(crate) fn into_generated_light_storage(self) -> Option<GeneratedLightStorage> {
        self.context.into_generated_light_storage()
    }

    /// The chunk's persisted status — `EMPTY` before any step, `CARVERS` after a
    /// successful BIOMES→NOISE→SURFACE→CARVERS run, and never `FULL` (the
    /// executor refuses to stamp it). A FEATURES run primes the final heightmaps,
    /// drives the full 17x17 dependency-window region (the 3x3 window is only
    /// the biome union), resolves the FULL possible-biome settings and builds
    /// the FeatureSorter, decodes and runs the registry-backed lake, geode,
    /// monster-room, underwater_magma, and glow_lichen paths, and then fails
    /// typed at the first selected unsupported path (`FeaturePlacementDecode`,
    /// seed-42: `minecraft:freeze_top_layer` at step 10/global 0 after selector dispatch), so the
    /// chunk is never stamped FEATURES.
    pub fn status(&self) -> ChunkStatus {
        self.chunk.get_persisted_status()
    }

    /// Consume the successful FEATURES dependency writebacks in canonical
    /// distance-1 order. Failed or not-yet-run FEATURES passes return an empty
    /// vector; the center chunk remains owned by this holder.
    pub fn take_feature_writebacks(&mut self) -> FeatureWritebacks {
        std::mem::take(&mut self.feature_writebacks)
    }

    /// The caller-owned dependency workspace used by this holder's FEATURES
    /// pass. It can be shared with neighboring holders on the sync tick thread.
    pub fn feature_workspace(&self) -> FeatureWorkspace {
        self.feature_workspace.clone()
    }

    /// Snapshot the tick-thread-owned generated proto without exposing its
    /// representation. The shared FEATURES scheduler uses this to seed its
    /// temporary `WorldGenRegion` cache before a status task runs.
    pub(crate) fn snapshot_proto(&self) -> ProtoChunk<BlockState, WorldgenBiomeId, StructureKey> {
        snapshot_generated_chunk(&self.chunk)
    }

    /// Mutably expose the generated proto to the bounded tick-thread scheduler.
    pub(crate) fn proto_mut(
        &mut self,
    ) -> &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey> {
        &mut self.chunk
    }

    /// Rebuild the holder executor around an owned proto returned by a shared
    /// SPAWN region. The proto remains the sole mutable authority; the executor
    /// closures are recreated over the same immutable generator and FEATURES
    /// workspace.
    pub(crate) fn from_proto_with_workspace(
        chunk: ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
        generator: Arc<OverworldGenerator>,
        feature_workspace: FeatureWorkspace,
    ) -> Self {
        let pos = chunk.get_pos();
        let mut holder = Self::new_with_workspace(pos, generator, feature_workspace);
        holder.chunk = chunk;
        holder
    }

    /// Consume a holder's proto for a temporary shared status region. The
    /// scheduler only calls this for neighbours that have no attached runtime
    /// LIGHT task; a holder carrying such a task must be detached through the
    /// generated-light storage seam instead.
    pub(crate) fn into_proto(self) -> ProtoChunk<BlockState, WorldgenBiomeId, StructureKey> {
        self.chunk
    }

    /// Drive the chunk from its current persisted status through `target`
    /// (inclusive). The BIOMES→NOISE→SURFACE→CARVERS task bodies are wired (an
    /// EMPTY chunk can reach CARVERS); the FEATURES task body is wired (it runs
    /// Java's `ChunkStatusTasks.generateFeatures` + `addVanillaDecorations`'s
    /// full dependency-window composition, decodes and runs the lake, geode,
    /// and monster-room paths, and then fails typed at the first selected
    /// unsupported path — see [`GenerationChunkHolder::new`]). A
    /// target the value layer does not wire is rejected by the executor before
    /// any work with a typed error — a path through a light step with no
    /// attached generated workspace/engine is refused as
    /// `GenError::LightEngineMissing`, and a target past LIGHT
    /// (FULL) is out of range
    /// ([`GeneratedChunkError::UnsupportedStatus`]). The chunk is left
    /// untouched by every such refusal. (The wired FEATURES rung is the
    /// exception: it runs Java's priming prologue — heightmap priming, the
    /// decoration-seed derivation, the complete 17x17 dependency window, and
    /// the 3x3 biome union read — and then fails typed, so the center proto is
    /// rolled back to the status and data it had before this call; see
    /// [`GenerationChunkHolder::status`].)
    ///
    /// The SPAWN rung is intentionally absent from this center-only executor:
    /// Paper's `generateSpawn` needs the scheduler-owned radius-one cache, so
    /// [`Self::generate_spawn_with_region`] is the only SPAWN entry and a
    /// `generate_through(SPAWN)` request refuses in preflight as
    /// `GeneratedChunkError::UnsupportedStatus`. FULL is deliberately a separate
    /// consuming promotion after an exact SPAWN parent, not a borrowed executor
    /// rung.
    pub fn generate_through(&mut self, target: ChunkStatus) -> Result<(), GeneratedChunkError> {
        // FULL is always the consuming ProtoChunk → LevelChunk boundary. It
        // must win even after a cached FEATURES failure: callers must never
        // observe a historical generation error in place of the boundary's
        // stable UnsupportedStatus response.
        if target == ChunkStatus::Full {
            return Err(GeneratedChunkError::UnsupportedStatus(target));
        }
        if target.index() >= ChunkStatus::Features.index()
            && let Some(error) = self.features_failure
        {
            return Err(GeneratedChunkError::Generation(error));
        }
        let current = self.chunk.get_persisted_status();
        // A LIGHT or SPAWN request can enter the FEATURES body before a later
        // light prerequisite fails. Snapshot the whole proto before every
        // non-FULL path that reaches FEATURES, not only a direct FEATURES
        // request. The released generic boundary restores this value on every
        // typed error and panic, including all shared ChunkAccess writes and
        // the terminal failure cache.
        let can_enter_features = current.index() < ChunkStatus::Features.index()
            && target.index() >= ChunkStatus::Features.index();
        let features_snapshot = can_enter_features.then(|| snapshot_generated_chunk(&self.chunk));
        let prior_features_failure = self.features_failure;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.context
                .generate_through(&GENERATION_PYRAMID, &mut self.chunk, target)
        }));
        let result = match result {
            Ok(result) => result,
            Err(payload) => {
                // A FEATURES closure is caller-owned and may panic after it
                // has written blocks, heightmaps, or other ChunkAccess state.
                // Discard staged dependency writebacks and restore before
                // resuming the original panic so the holder is retryable and
                // its failure state is unchanged.
                self.pending_feature_writebacks.borrow_mut().take();
                if let Some(snapshot) = features_snapshot {
                    self.chunk = snapshot;
                    self.features_failure = prior_features_failure;
                }
                std::panic::resume_unwind(payload);
            }
        };
        match result {
            Ok(()) => {
                if features_snapshot.is_some() {
                    self.feature_writebacks = self
                        .pending_feature_writebacks
                        .borrow_mut()
                        .take()
                        .unwrap_or_default();
                }
                Ok(())
            }
            Err(error) => {
                self.pending_feature_writebacks.borrow_mut().take();
                // Any typed failure after entering this path rolls back the
                // complete pre-FEATURES value. A failure produced by the
                // FEATURES body remains the one cached terminal boundary; a
                // later LIGHT/SPAWN refusal does not poison that cache.
                let entered_features = can_enter_features
                    && (self.chunk.get_persisted_status().index() >= ChunkStatus::Features.index()
                        || is_features_failure(error));
                if let Some(snapshot) = features_snapshot {
                    self.chunk = snapshot;
                    self.features_failure = prior_features_failure;
                }
                if entered_features && is_features_failure(error) {
                    self.features_failure = Some(error);
                }
                match error {
                    GenError::UnsupportedStatus(status) => {
                        Err(GeneratedChunkError::UnsupportedStatus(status))
                    }
                    error => Err(GeneratedChunkError::Generation(error)),
                }
            }
        }
    }

    /// Run the Paper SPAWN body over a caller-owned radius-one cache. The
    /// caller supplies the eight neighbouring protos that the tick-thread
    /// scheduler already owns; this method borrows them for one synchronous
    /// `WorldGenRegion`. The workspace retains those owned protos and can be
    /// consumed with [`SpawnRegionProtos::into_neighbours`] after the call;
    /// heightmap reads may persist the same priming writes Paper performs. The
    /// center is stamped SPAWN only after the body succeeds (or after the
    /// faithful `isUpgrading` skip).
    pub fn generate_spawn_with_region(
        &mut self,
        workspace: &mut SpawnRegionProtos,
    ) -> Result<(), GeneratedChunkError> {
        self.generate_spawn_with_region_rule(workspace, true)
    }

    /// Variant of [`Self::generate_spawn_with_region`] with an explicit
    /// `SPAWN_MOBS` value. The no-argument integration API defaults this to
    /// `true`, matching a fresh Paper world's gamerule; callers with a real
    /// level overlay pass its actual value rather than disabling mobs as a
    /// shortcut.
    pub fn generate_spawn_with_region_rule(
        &mut self,
        workspace: &mut SpawnRegionProtos,
        spawn_mobs_rule: bool,
    ) -> Result<(), GeneratedChunkError> {
        let center = self.chunk.get_pos();
        // The scheduler invokes a status task only for a promotion. Mirror the
        // executor's idempotent target handling here so a retry cannot populate
        // the same chunk twice or consume another generation RNG stream.
        if self
            .chunk
            .get_persisted_status()
            .is_or_after(ChunkStatus::Spawn)
        {
            return Ok(());
        }
        // Paper's `ChunkStatusTasks.generateSpawn` skips the generator before
        // constructing `WorldGenRegion` when this is a below-zero retrogen
        // chunk. In particular, it must not validate or borrow the radius-one
        // cache: the scheduler may not have a ready neighbour ring yet.
        if self.chunk.is_upgrading() {
            if !self
                .chunk
                .get_persisted_status()
                .is_or_after(ChunkStatus::Light)
            {
                // The status executor normally invokes this body only after
                // LIGHT. Keep the direct API's prerequisite refusal for a
                // malformed caller while preserving the Paper skip for a
                // LIGHT-complete retrogen chunk.
                return Err(GeneratedChunkError::Generation(GenError::SpawnNotGenerated));
            }
            self.chunk.set_persisted_status(ChunkStatus::Spawn);
            return Ok(());
        }
        if !self
            .chunk
            .get_persisted_status()
            .is_or_after(ChunkStatus::Light)
        {
            return Err(GeneratedChunkError::Generation(GenError::SpawnNotGenerated));
        }
        if workspace.center() != center {
            return Err(GeneratedChunkError::SpawnRegion(
                SpawnRegionError::UnexpectedChunk { pos: center },
            ));
        }
        let mut region = compose_spawn_region(&mut self.chunk, workspace, &self.generator)
            .map_err(GeneratedChunkError::SpawnRegion)?;
        run_spawn_in_region(&mut region, &self.generator, spawn_mobs_rule)
            .map_err(GeneratedChunkError::SpawnRegion)?;
        drop(region);
        self.chunk.set_persisted_status(ChunkStatus::Spawn);
        Ok(())
    }

    /// Consume the holder and promote its chunk to a loaded `LevelChunk` — the
    /// FULL conversion (`ChunkFullTask.run`'s `new LevelChunk(level, protoChunk,
    /// postLoad)`, Paper `LevelChunk.java` 177). The chunk is moved out of the
    /// holder by value (tick-thread owned, never `Arc<RwLock>`) and
    /// [`LevelChunk::from_generated_spawn_proto`] consumes the `ProtoChunk`.
    ///
    /// The conversion is atomic and typed: a refusal produces no partial
    /// `LevelChunk`, no install, no clone, and no status fabrication. Every
    /// non-SPAWN generated parent status is rejected as
    /// [`GeneratedChunkError::Convert`] carrying [`LevelChunkBridgeError::GeneratedStatusNotSpawn`]
    /// before the `ProtoChunk` is consumed. A SPAWN-parent proto with a hostile
    /// persisted Starlight state is refused as `Convert(UnsupportedLightState)`
    /// before the value transform consumes it; a palette the server value pair
    /// cannot re-encode fails as `Convert(PaletteMap)` from the
    /// `map_values` re-encode itself (the proto is consumed in that hostile
    /// case, but no `LevelChunk` is ever produced).
    ///
    /// The generated-light workspace is detached before conversion and returned
    /// on both outcomes. On success the caller receives `(LevelChunk, storage)`
    /// and can reinsert all 24 runtime neighbours into its tick-thread owner;
    /// on conversion failure the same storage is carried by `Convert`. This is
    /// the consuming FULL boundary: the original `ProtoChunk` is not recoverable,
    /// but neither is the detached runtime workspace silently dropped.
    pub fn into_level_chunk(
        self,
    ) -> Result<(LevelChunk, Option<GeneratedLightStorage>), GeneratedChunkError> {
        let GenerationChunkHolder { chunk, context, .. } = self;
        let generated_light_storage = context.into_generated_light_storage();
        match LevelChunk::from_generated_spawn_proto(chunk) {
            Ok(level_chunk) => Ok((level_chunk, generated_light_storage)),
            Err(error) => Err(GeneratedChunkError::Convert {
                error,
                generated_light_storage,
            }),
        }
    }
}

/// Whether an error came from an actually-entered FEATURES body.
///
/// `generate_through` validates the entire requested path before running any
/// task. A later LIGHT/SPAWN refusal can therefore leave the holder at CARVERS
/// without ever entering FEATURES; only errors the FEATURES body itself can
/// produce are terminal for the holder's retry cache.
fn is_features_failure(error: GenError) -> bool {
    matches!(
        error,
        GenError::FeaturePlacementDecode { .. }
            | GenError::SettingsNotGenerated { .. }
            | GenError::StructureDecorationIndexUnavailable { .. }
    )
}

/// Clone a generated proto's owned representation before the FEATURES task.
///
/// `ProtoChunk` intentionally stays a value type without a blanket `Clone`
/// implementation, so this uses its transactional value-map seam with the same
/// worldgen strategies and identity mappers. The generated registry tables are
/// fixed and valid; a failure here is an invariant violation before any feature
/// body is allowed to run.
fn snapshot_generated_chunk(
    chunk: &ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
) -> ProtoChunk<BlockState, WorldgenBiomeId, StructureKey> {
    let factory = current_version_container_factory();
    chunk
        .map_values_ref(
            factory.block_states_strategy().clone(),
            factory.biome_strategy().clone(),
            *factory.default_block_state(),
            BlockState::of(BlockId(794)),
            *factory.default_biome(),
            &|state: &BlockState| *state,
            &|biome: &WorldgenBiomeId| *biome,
            &resolve_state_flags,
        )
        .expect("the generated proto must snapshot through its own value strategies")
}

/// A fresh EMPTY worldgen chunk — the same construction the holder uses for its
/// own chunk, so a ring chunk generated through CARVERS is built with the exact
/// worldgen element types and void-air out-of-height default.
fn fresh_worldgen_chunk(
    pos: ChunkPos,
    generator: &OverworldGenerator,
) -> ProtoChunk<BlockState, WorldgenBiomeId, StructureKey> {
    let height_accessor = create_height_accessor(
        generator.generator().get_min_y(),
        generator.generator().get_gen_depth(),
    );
    ProtoChunk::new(
        pos,
        UpgradeData::empty(height_accessor.get_sections_count() as usize),
        height_accessor,
        &current_version_container_factory(),
        None,
        Blocks::AIR.default_block_state(),
        BlockState::of(BlockId(794)),
        &resolve_state_flags,
    )
}

/// Drive a fresh EMPTY chunk through the BIOMES→NOISE→SURFACE→CARVERS rungs the
/// FEATURES ring reads — the same real bodies the holder's executor closures
/// wire (Java's `ChunkStatusTasks.generateBiomes/generateNoise/generateSurface/
/// generateCarvers` for a neighbor chunk Paper generates with the identical
/// shared worldgen config).
fn generate_ring_chunk(
    pos: ChunkPos,
    generator: &Arc<OverworldGenerator>,
) -> ProtoChunk<BlockState, WorldgenBiomeId, StructureKey> {
    let mut chunk = fresh_worldgen_chunk(pos, generator);
    let source = &generator.biome_source;
    chunk.fill_biomes_from_noise(source, &source.sampler, &|holder| {
        WorldgenBiomeId(dense_biome_id(holder))
    });
    chunk.set_persisted_status(ChunkStatus::Biomes);
    generator
        .generator()
        .fill_from_noise(Blender::empty(), generator.random_state(), &mut chunk);
    chunk.set_persisted_status(ChunkStatus::Noise);
    let height_accessor = chunk.height_accessor();
    let biome_manager = Arc::new(BiomeManager::new(
        Arc::new(generator.biome_source.clone()),
        BiomeManager::obfuscate_seed(generator.seed()),
    ));
    let generation_context = Arc::new(WorldGenerationContext::new(&**generator, &height_accessor));
    generator.generator().build_surface(
        generator.random_state(),
        biome_manager,
        generation_context,
        &mut chunk,
        None,
    );
    chunk.set_persisted_status(ChunkStatus::Surface);
    let biome_manager = Arc::new(BiomeManager::new(
        Arc::new(generator.biome_source.clone()),
        BiomeManager::obfuscate_seed(generator.seed()),
    ));
    generator.generator().apply_carvers(
        &**generator,
        generator.seed(),
        generator.random_state(),
        &biome_manager,
        Arc::new(generator.biome_source.clone()),
        &mut chunk,
    );
    chunk.set_persisted_status(ChunkStatus::Carvers);
    chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
    chunk
}

/// `ChunkGenerator.addVanillaDecorations` (Paper 26.2) over the complete 17x17
/// FEATURES dependency window — the body's real prologue, biome union, and
/// per-step loop, up to the first selected placed feature outside this slice.
///
/// In Java order:
///   1. `Heightmap.primeHeightmaps(chunk, FINAL_HEIGHTMAPS)` primes the four
///      final heightmaps the decoration bodies read (Java's
///      `ChunkStatusTasks.generateFeatures`, before `applyBiomeDecoration`);
///   2. `SectionPos.of(centerPos, level.getMinSectionY()).origin()` derives the
///      section-origin block position and `setDecorationSeed(seed, origin.x,
///      origin.z)` the decoration seed;
///   3. the region is composed: the center `ProtoChunk` (at CARVERS, the rung
///      the executor guarantees) is borrowed through a [`CenterHolder`], and
///      the 288 ring chunks are generated through CARVERS or initialized at
///      STRUCTURE_STARTS and owned by status-preserving holders — the
///      `StaticCache2D` the `WorldGenRegion` reads `level.getChunk` from;
///   4. the 3x3 biome union is gathered in Paper order (`ChunkPos.rangeClosed
///      (sectionPos.chunk(), 1)` → sections → `biomes().getAll`) and
///      `retainAll`-ed against the biome source's possible biomes;
///   5. the FULL `biomeSource.possibleBiomes()` list resolves its
///      `BiomeGenerationSettings` in source order (the exact argument Paper's
///      `ChunkGenerator.featuresPerStep` memoizes at construction,
///      `ChunkGenerator.java` 97-100) and `build_features_per_step` produces
///      the per-step data from that full list — the 3x3 union only picks which
///      global indices execute per step, exactly like Paper's
///      `addVanillaDecorations` (`generationSteps =
///      max(Decoration.values().length, featureStepCount)`).
///
/// The per-step loop runs the union's placed features in global-index order,
/// executing decoded lake, amethyst-geode, monster-room, underwater_magma, and
/// glow_lichen leaves with their exact feature seeds. On seed-42 chunk (0,0),
/// step 9/global index 17's `dark_forest_vegetation` parent selects no position:
/// all 16 count/in-square candidates fail its max-depth-zero water filter, so
/// the run reaches `minecraft:freeze_top_layer` at step 10/global index 0, whose
/// biome `shouldFreeze` world-state seam is not yet implemented. A separate
/// tree-bearing seed-42 chunk (4,4) does reach the same outer step-9/global-17
/// parent; its selector falls through to `oak_leaf_litter`, whose
/// `would_survive` state is the current unsupported `oak_sapling` seam. The
/// typed error names the outer placed feature in both cases. The generated
/// settings tables are the full 55-biome surface (no `SettingsNotGenerated`),
/// so these boundaries are reached without fabricated or silently skipped
/// features.
///
/// Compose the FEATURES `WorldGenRegion` over the complete accumulated
/// dependency window of the FEATURES step. Paper's direct dependencies are
/// `CARVERS` at distances 0 and 1, followed by `STRUCTURE_STARTS` through
/// distance 8, so the cache is 17x17. The decoration biome union reads only
/// the center 3x3, but placement and worldgen reads are bounded by the full
/// status contract and must not be backed by an undersized cache.
#[cfg_attr(not(test), allow(dead_code))]
fn compose_feature_region<'a>(
    chunk: &'a mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
) -> WorldGenRegion<'a, BlockState, WorldgenBiomeId, StructureKey> {
    compose_feature_region_with_workspace(chunk, generator, None)
}

/// Compose a FEATURES region over snapshots of the caller-owned dependency
/// workspace. The snapshots make the region transactional: no workspace entry
/// is changed until the decoration pass succeeds, while successful owned proto
/// values can be moved back into the authoritative workspace.
fn compose_feature_region_with_workspace<'a>(
    chunk: &'a mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
    workspace: Option<&FeatureWorkspace>,
) -> WorldGenRegion<'a, BlockState, WorldgenBiomeId, StructureKey> {
    let center_pos = chunk.get_pos();
    let center_status = chunk.get_persisted_status();
    let step = GENERATION_PYRAMID
        .get_step_to(ChunkStatus::Features)
        .clone();
    let dependencies = step.direct_dependencies();
    let radius = dependencies.size() as i32 - 1;
    let width = radius * 2 + 1;
    let mut holders: Vec<
        Box<dyn GenerationChunkHolderView<BlockState, WorldgenBiomeId, StructureKey> + 'a>,
    > = Vec::with_capacity((width * width) as usize);

    // `StaticCache2D::from_entries` stores X outer, Z inner — index
    // `(x - minX) * sizeZ + (z - minZ)`. Build in that order so every
    // `getChunk(x, z)` resolves the holder for its own coordinates.
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            let pos = ChunkPos::new(
                center_pos.x().wrapping_add(dx),
                center_pos.z().wrapping_add(dz),
            );
            if pos == center_pos {
                continue;
            }
            let distance = dx.abs().max(dz.abs()) as usize;
            let status = dependencies.get(distance);
            // A workspace entry is copied before the region can mutate it. The
            // copy preserves every represented ProtoChunk field, including
            // heightmaps, post-processing, block entities, and scheduled ticks.
            // Entries retained by a neighboring center may sit below this
            // slot's required status (a distance-two StructureStarts
            // placeholder can land in a distance-one Carvers slot); Paper's
            // scheduler never hands a decoration pass an under-generated
            // dependency, so stale lower-status entries are regenerated
            // exactly like absent ones instead of being silently reused.
            let existing = workspace.and_then(|workspace| {
                workspace.with_chunk(pos, |chunk| {
                    chunk
                        .filter(|chunk| chunk.get_persisted_status().is_or_after(status))
                        .map(snapshot_generated_chunk)
                })
            });
            match status {
                ChunkStatus::Carvers => {
                    holders.push(Box::new(OwnedProtoHolder::new(
                        existing.unwrap_or_else(|| generate_ring_chunk(pos, generator)),
                    )));
                }
                ChunkStatus::StructureStarts => {
                    let structure_chunk = existing.unwrap_or_else(|| {
                        let mut chunk = fresh_worldgen_chunk(pos, generator);
                        // `ChunkStatusTasks.generateFeatures` primes the final
                        // maps before decoration, and every dependency chunk
                        // must carry those persisted maps when the region reads it.
                        chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
                        chunk.set_persisted_status(ChunkStatus::StructureStarts);
                        chunk
                    });
                    holders.push(Box::new(OwnedProtoHolder::new(structure_chunk)));
                }
                other => {
                    panic!("unsupported FEATURES cache dependency {other:?} at distance {distance}")
                }
            }
        }
    }

    let center_index = (radius * width + radius) as usize;
    holders.insert(
        center_index,
        Box::new(CenterHolder::new(chunk.base_mut(), center_status)),
    );
    let cache = StaticCache2D::from_entries(
        center_pos.x().wrapping_sub(radius),
        center_pos.z().wrapping_sub(radius),
        width,
        width,
        holders,
    );
    WorldGenRegion::new(
        cache,
        center_pos,
        step,
        generator.seed(),
        generator.generator().get_min_y(),
        generator.generator().get_gen_depth(),
        generator.generator().get_sea_level(),
        Arc::new(generator.biome_source.clone()),
        generator.feature_access().clone(),
    )
}

/// `possibleBiomes` — the 3x3 biome union, gathered in Paper order and
/// `retainAll`-ed against the overworld source's possible biomes. The dense
/// worldgen biome id maps through `BIOME_BY_ID` (the registry-id-indexed
/// name table); ids outside the table are skipped (a hostile chunk can't
/// take the gather down).
fn gather_possible_biomes(
    region: &WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
) -> HashSet<&'static str> {
    let center_pos = region.get_center();
    let mut possible_biomes = HashSet::new();
    for pos in ChunkPos::range_closed(&center_pos, 1) {
        let chunk_in_range = region.get_chunk(pos.x(), pos.z());
        for section in chunk_in_range.get_sections() {
            section.biomes().get_all(|biome: WorldgenBiomeId| {
                if let Some(name) = BIOME_BY_ID.get(biome.0 as usize) {
                    possible_biomes.insert(*name);
                }
            });
        }
    }
    let possible = generator.biome_source().possible_biomes();
    possible_biomes.retain(|name| {
        possible.iter().any(|p| {
            BIOME_BY_ID
                .get(dense_biome_id(p) as usize)
                .is_some_and(|possible_name| *name == *possible_name)
        })
    });
    possible_biomes
}

/// Resolve one possible biome's `BiomeGenerationSettings` from the generated
/// feature tables.
///
/// The placed-feature holders are `Holder::Reference` over one fabricated
/// `PLACED_FEATURE` registry id (the generated tables are keyed by name; the
/// FeatureSorter keys on holder identity, so a single fabricated registry
/// collapses the biomes' shared steps exactly like Paper's registry does).
/// `placed_by_id` collects the reverse id → key map the typed error names.
///
/// A biome whose dense id is not in `BIOME_BY_ID`, or whose name has no
/// generated settings, fails typed (`GenError::SettingsNotGenerated`) — never
/// a phf panic, never a fabricated or silently-skipped biome.
fn resolve_biome_settings(
    name: &'static str,
    placed_registry_id: RegistryId,
    placed_by_id: &mut HashMap<u32, &'static str>,
) -> Result<BiomeGenerationSettings, GenError> {
    let table = BIOME_GENERATION_SETTINGS_BY_NAME
        .get(name)
        .ok_or(GenError::SettingsNotGenerated { biome: Some(name) })?;
    let mut builder = PlainBuilder::default();
    for (step, step_features) in table.features.iter().enumerate() {
        for feature_name in *step_features {
            let id = PLACED_FEATURE_BY_NAME
                .get(feature_name)
                .ok_or(GenError::SettingsNotGenerated { biome: Some(name) })?
                .id as u32;
            placed_by_id.entry(id).or_insert(*feature_name);
            builder =
                builder.add_feature_index(step as i32, Holder::reference(placed_registry_id, id));
        }
    }
    Ok(builder.build())
}

/// Resolve the FULL `biomeSource.possibleBiomes()` list in source order — the
/// exact argument Paper's `ChunkGenerator.featuresPerStep` memoizes
/// (`ChunkGenerator.java` 97-100: `FeatureSorter.buildFeaturesPerStep(List.
/// copyOf(biomeSource.possibleBiomes()), ...)`). The FeatureSorter must be
/// built once from this full list, not the per-chunk 3x3 union; the union only
/// decides which feature indices execute per step. The first possible biome
/// that cannot resolve its settings fails typed in source order.
///
/// Each resolved source is paired with its biome name (the `BIOME_BY_ID` name
/// at its full-list position) so the per-step loop can map a union biome back
/// to its full-list source by name — the generated table is name-keyed, so the
/// union and the full list resolve structurally identical `Reference` holders.
fn resolve_feature_settings(
    possible_biomes: &[Holder<BiomeId>],
    placed_registry_id: RegistryId,
    placed_by_id: &mut HashMap<u32, &'static str>,
) -> Result<Vec<(BiomeGenerationSettings, &'static str)>, GenError> {
    let mut settings_sources = Vec::with_capacity(possible_biomes.len());
    for holder in possible_biomes {
        let dense = dense_biome_id(holder) as usize;
        let name = *BIOME_BY_ID
            .get(dense)
            .ok_or(GenError::SettingsNotGenerated { biome: None })?;
        let settings = resolve_biome_settings(name, placed_registry_id, placed_by_id)?;
        settings_sources.push((settings, name));
    }
    Ok(settings_sources)
}

type FeatureOps = RegistryOps<Value, JsonOps>;

/// A generated configured feature whose registered feature type is known but
/// whose concrete configuration unit has not landed yet. It is a real registry
/// value, not a selector fallback: named and inline holders resolve normally,
/// and selecting this value refuses at the feature dispatch boundary.
#[derive(Debug)]
struct DeferredGeneratedFeatureConfiguration {
    configured_key: String,
}

impl FeatureConfiguration for DeferredGeneratedFeatureConfiguration {
    fn unavailable_feature(&self) -> Option<&str> {
        Some(&self.configured_key)
    }
}

/// The shared seed-42 feature `RegistryAccess` — the worldgen access composed
/// with the frozen placed/configured-feature registries the decoder and the
/// selector/composite features resolve their recursive `Holder` references
/// through.
///
/// The two feature registries are built as one generated closure, not as
/// empty placeholders. The generated ids are full registry ids (not PHF map
/// iteration order); synthetic gap values preserve those ids, and the final
/// values keep the same registry identities the temporary `RegistryOps` decode
/// used. One-owner transactions are necessary because placed and configured
/// features are mutually recursive; they are adopted only after the temporary
/// access is fully dropped, so a decode failure cannot publish a partially
/// populated access into the world.
fn build_feature_access(worldgen: &RegistryAccess) -> RegistryAccess {
    let (placed, configured) = build_generated_feature_closure(worldgen)
        .unwrap_or_else(|error| panic!("generated feature closure refused atomically: {error}"));
    let feature_layer = RegistryAccess::from_pairs(vec![
        (
            ResourceKey::create_registry_key(Identifier::with_default_namespace(
                "worldgen/placed_feature",
            )),
            Box::new(placed) as rivet_registry::root::AnyBox,
        ),
        (
            ResourceKey::create_registry_key(Identifier::with_default_namespace(
                "worldgen/configured_feature",
            )),
            Box::new(configured) as rivet_registry::root::AnyBox,
        ),
    ]);
    // Layer the feature registries (Static) over the worldgen registries
    // (Worldgen). The composite merges the disjoint key sets — the first layer
    // wins only on a key collision, of which there are none between
    // `worldgen/placed_feature`/`worldgen/configured_feature` and the worldgen
    // NOISE/DENSITY_FUNCTION/BIOME/NOISE_SETTINGS keys.
    LayeredRegistryAccess::new(vec![RegistryLayer::Static, RegistryLayer::Worldgen])
        .replace_from(RegistryLayer::Static, &[feature_layer])
        .replace_from(RegistryLayer::Worldgen, std::slice::from_ref(worldgen))
        .composite_access()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum GeneratedFeatureNode {
    Placed(&'static str),
    Configured(&'static str),
}

fn sorted_placed_feature_entries()
-> Result<Vec<(&'static str, &'static PlacedFeatureEntry)>, String> {
    let mut entries: Vec<_> = PLACED_FEATURE_BY_NAME
        .entries()
        .map(|(name, entry)| (*name, entry))
        .collect();
    entries.sort_unstable_by_key(|(_, entry)| entry.id);
    for ((previous_name, previous), (name, entry)) in entries.iter().zip(entries.iter().skip(1)) {
        if previous.id == entry.id {
            return Err(format!(
                "placed features {previous_name} and {name} share generated id {}",
                entry.id
            ));
        }
    }
    Ok(entries)
}

fn sorted_configured_feature_entries()
-> Result<Vec<(&'static str, &'static ConfiguredFeatureEntry)>, String> {
    let mut entries: Vec<_> = CONFIGURED_FEATURE_BY_NAME
        .entries()
        .map(|(name, entry)| (*name, entry))
        .collect();
    entries.sort_unstable_by_key(|(_, entry)| entry.id);
    for ((previous_name, previous), (name, entry)) in entries.iter().zip(entries.iter().skip(1)) {
        if previous.id == entry.id {
            return Err(format!(
                "configured features {previous_name} and {name} share generated id {}",
                entry.id
            ));
        }
    }
    Ok(entries)
}

fn placed_name_slots(
    entries: &[(&'static str, &'static PlacedFeatureEntry)],
) -> Vec<Option<&'static str>> {
    let mut slots = vec![None; entries.last().map_or(0, |(_, entry)| entry.id as usize + 1)];
    for &(name, entry) in entries {
        slots[entry.id as usize] = Some(name);
    }
    slots
}

fn configured_name_slots(
    entries: &[(&'static str, &'static ConfiguredFeatureEntry)],
) -> Vec<Option<&'static str>> {
    let mut slots = vec![None; entries.last().map_or(0, |(_, entry)| entry.id as usize + 1)];
    for &(name, entry) in entries {
        slots[entry.id as usize] = Some(name);
    }
    slots
}

#[derive(Clone, Copy, Debug)]
enum GeneratedHolderKind {
    Placed,
    Configured,
}

fn generated_placed_node(name: &str) -> Option<GeneratedFeatureNode> {
    PLACED_FEATURE_BY_NAME
        .entries()
        .find(|(candidate, _)| **candidate == name)
        .map(|(candidate, _)| GeneratedFeatureNode::Placed(candidate))
}

fn generated_configured_node(name: &str) -> Option<GeneratedFeatureNode> {
    CONFIGURED_FEATURE_BY_NAME
        .entries()
        .find(|(candidate, _)| **candidate == name)
        .map(|(candidate, _)| GeneratedFeatureNode::Configured(candidate))
}

/// Add a named holder edge, preserving the registry type encoded by the holder
/// position. Inline values are walked recursively instead of being treated as
/// registry names. This is deliberately not a generic JSON string walk: block
/// names, provider types, tags, placement metadata, and other resource strings
/// are not feature-holder references.
fn generated_named_holder(
    value: &Value,
    kind: GeneratedHolderKind,
    out: &mut Vec<GeneratedFeatureNode>,
    context: &str,
) -> Result<(), String> {
    let name = value
        .as_str()
        .ok_or_else(|| format!("{context} holder must be a name or inline object"))?;
    let node = match kind {
        GeneratedHolderKind::Placed => generated_placed_node(name),
        GeneratedHolderKind::Configured => generated_configured_node(name),
    };
    out.push(node.ok_or_else(|| {
        let registry = match kind {
            GeneratedHolderKind::Placed => "placed",
            GeneratedHolderKind::Configured => "configured",
        };
        format!("{context} references missing {registry} feature {name}")
    })?);
    Ok(())
}

/// Walk a `Holder<PlacedFeature>` encoded by `PlacedFeature.CODEC`. Its inline
/// form is `{feature: <Holder<ConfiguredFeature>>, placement: [...]}`. The
/// placement list is intentionally opaque; Paper's `ConfiguredFeature` closure
/// only follows the placed feature's configured-feature holder.
fn generated_placed_holder(
    value: &Value,
    out: &mut Vec<GeneratedFeatureNode>,
    context: &str,
) -> Result<(), String> {
    if value.is_string() {
        return generated_named_holder(value, GeneratedHolderKind::Placed, out, context);
    }
    let object = value
        .as_object()
        .ok_or_else(|| format!("{context} placed holder must be a name or object"))?;
    let feature = object
        .get("feature")
        .ok_or_else(|| format!("{context} inline placed holder has no feature"))?;
    if !object.contains_key("placement") {
        return Err(format!("{context} inline placed holder has no placement"));
    }
    generated_configured_holder(feature, out, &format!("{context}.feature"))
}

/// Walk a `Holder<ConfiguredFeature>` encoded by `ConfiguredFeature.CODEC`.
/// Named holders become graph nodes; inline configured values are traversed by
/// their concrete configuration shape without inventing a registry node.
fn generated_configured_holder(
    value: &Value,
    out: &mut Vec<GeneratedFeatureNode>,
    context: &str,
) -> Result<(), String> {
    if value.is_string() {
        return generated_named_holder(value, GeneratedHolderKind::Configured, out, context);
    }
    let object = value
        .as_object()
        .ok_or_else(|| format!("{context} configured holder must be a name or object"))?;
    generated_configured_object(object, out, context)
}

fn generated_feature_array<'a>(
    value: &'a Value,
    context: &str,
    field: &str,
) -> Result<&'a [Value], String> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{context} {field} must be an array"))
}

/// Follow exactly the generated configuration shapes whose Paper value types
/// expose configured-feature sub-features (the selector families), plus the
/// vegetation-patch and root-system holders used by the generated corpus. All
/// other config data is feature-local and must not be recursively interpreted
/// as holders.
///
/// This entry point receives the complete configured-feature object, including
/// its root `type` and `config` fields. Keeping that boundary explicit prevents
/// callers from accidentally walking an object while skipping its configuration.
fn generated_holder_refs(
    object: &serde_json::Map<String, Value>,
    out: &mut Vec<GeneratedFeatureNode>,
    context: &str,
) -> Result<(), String> {
    let feature_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{context} inline configured feature has no type"))?;
    let config = object
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{context} inline configured feature has no object config"))?;
    let mut holder = |value: &Value, field: &str| {
        generated_placed_holder(value, out, &format!("{context} {field}"))
    };
    match feature_type {
        "minecraft:random_selector" => {
            for (index, entry) in generated_feature_array(
                config
                    .get("features")
                    .ok_or_else(|| format!("{context} has no features list"))?,
                context,
                "features",
            )?
            .iter()
            .enumerate()
            {
                let feature = entry
                    .get("feature")
                    .ok_or_else(|| format!("{context} features[{index}] has no feature"))?;
                holder(feature, &format!("features[{index}].feature"))?;
            }
            holder(
                config
                    .get("default")
                    .ok_or_else(|| format!("{context} has no default"))?,
                "default",
            )?;
        }
        "minecraft:random_boolean_selector" => {
            holder(
                config
                    .get("feature_true")
                    .ok_or_else(|| format!("{context} has no feature_true"))?,
                "feature_true",
            )?;
            holder(
                config
                    .get("feature_false")
                    .ok_or_else(|| format!("{context} has no feature_false"))?,
                "feature_false",
            )?;
        }
        "minecraft:simple_random_selector" | "minecraft:sequence" => {
            for (index, feature) in generated_feature_array(
                config
                    .get("features")
                    .ok_or_else(|| format!("{context} has no features list"))?,
                context,
                "features",
            )?
            .iter()
            .enumerate()
            {
                holder(feature, &format!("features[{index}]"))?;
            }
        }
        "minecraft:weighted_random_selector" => {
            for (index, entry) in generated_feature_array(
                config
                    .get("features")
                    .ok_or_else(|| format!("{context} has no features list"))?,
                context,
                "features",
            )?
            .iter()
            .enumerate()
            {
                let data = entry
                    .get("data")
                    .ok_or_else(|| format!("{context} features[{index}] has no data"))?;
                holder(data, &format!("features[{index}].data"))?;
            }
        }
        "minecraft:vegetation_patch" | "minecraft:waterlogged_vegetation_patch" => {
            holder(
                config
                    .get("vegetation_feature")
                    .ok_or_else(|| format!("{context} has no vegetation_feature"))?,
                "vegetation_feature",
            )?;
        }
        "minecraft:root_system" => {
            holder(
                config
                    .get("feature")
                    .ok_or_else(|| format!("{context} has no feature"))?,
                "feature",
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Compatibility-named wrapper for inline configured holders. Both inline and
/// named registry entries use the same complete `{type, config}` traversal.
fn generated_configured_object(
    object: &serde_json::Map<String, Value>,
    out: &mut Vec<GeneratedFeatureNode>,
    context: &str,
) -> Result<(), String> {
    generated_holder_refs(object, out, context)
}

fn generated_feature_edges(
    node: GeneratedFeatureNode,
) -> Result<Vec<GeneratedFeatureNode>, String> {
    match node {
        GeneratedFeatureNode::Placed(name) => {
            let entry = PLACED_FEATURE_BY_NAME
                .get(name)
                .ok_or_else(|| format!("missing generated placed feature {name}"))?;
            let json: Value = serde_json::from_str(entry.json)
                .map_err(|error| format!("decode {name} JSON: {error}"))?;
            let object = json
                .as_object()
                .ok_or_else(|| format!("{name} JSON must be an object"))?;
            let configured = object
                .get("feature")
                .ok_or_else(|| format!("{name} JSON has no configured feature"))?;
            let mut refs = Vec::new();
            generated_configured_holder(configured, &mut refs, name)?;
            Ok(refs)
        }
        GeneratedFeatureNode::Configured(name) => {
            let entry = CONFIGURED_FEATURE_BY_NAME
                .get(name)
                .ok_or_else(|| format!("missing generated configured feature {name}"))?;
            let json: Value = serde_json::from_str(entry.json)
                .map_err(|error| format!("decode {name} JSON: {error}"))?;
            let object = json
                .as_object()
                .ok_or_else(|| format!("{name} JSON must be an object"))?;
            let mut refs = Vec::new();
            generated_holder_refs(object, &mut refs, name)?;
            Ok(refs)
        }
    }
}

fn validate_generated_feature_graph(
    roots: impl IntoIterator<Item = GeneratedFeatureNode>,
    edges: &mut dyn FnMut(GeneratedFeatureNode) -> Result<Vec<GeneratedFeatureNode>, String>,
) -> Result<(), String> {
    fn walk(
        node: GeneratedFeatureNode,
        state: &mut HashMap<GeneratedFeatureNode, u8>,
        path: &mut Vec<GeneratedFeatureNode>,
        edges: &mut dyn FnMut(GeneratedFeatureNode) -> Result<Vec<GeneratedFeatureNode>, String>,
    ) -> Result<(), String> {
        match state.get(&node).copied() {
            Some(2) => return Ok(()),
            Some(1) => {
                return Err(format!(
                    "generated feature closure cycle at {node:?} via {path:?}"
                ));
            }
            _ => {}
        }
        state.insert(node, 1);
        path.push(node);
        for edge in edges(node)? {
            walk(edge, state, path, edges)?;
        }
        path.pop();
        state.insert(node, 2);
        Ok(())
    }

    let mut state = HashMap::new();
    let mut path = Vec::new();
    for root in roots {
        walk(root, &mut state, &mut path, edges)?;
    }
    Ok(())
}

fn validate_generated_feature_closure(
    placed_entries: &[(&'static str, &'static PlacedFeatureEntry)],
    configured_entries: &[(&'static str, &'static ConfiguredFeatureEntry)],
) -> Result<(), String> {
    let roots = placed_entries
        .iter()
        .map(|&(name, _)| GeneratedFeatureNode::Placed(name))
        .chain(
            configured_entries
                .iter()
                .map(|&(name, _)| GeneratedFeatureNode::Configured(name)),
        );
    validate_generated_feature_graph(roots, &mut generated_feature_edges)
}

fn build_generated_feature_closure(
    worldgen: &RegistryAccess,
) -> Result<(Registry<PlacedFeature>, Registry<ConfiguredFeatureErased>), String> {
    let placed_entries = sorted_placed_feature_entries()?;
    let configured_entries = sorted_configured_feature_entries()?;
    validate_generated_feature_closure(&placed_entries, &configured_entries)?;

    // Build temporary lookup tables through one-owner transactions. Each
    // transaction owns the only builder for its identity; the access handoff
    // borrows those transactions while moving their frozen registries into the
    // temporary access, and its drop path adopts them back on every early exit.
    let mut placed_transaction = RegistryBuilder::new(&*PLACED_FEATURE).into_transaction();
    let mut configured_transaction = RegistryBuilder::new(&*CONFIGURED_FEATURE).into_transaction();
    let configured_registry_id = configured_transaction.registry_id();
    let placed_slots = placed_name_slots(&placed_entries);
    let configured_slots = configured_name_slots(&configured_entries);
    let configured_gap_id = configured_slots
        .iter()
        .position(Option::is_none)
        .ok_or_else(|| "generated closure has no configured gap sentinel".to_string())?
        as u32;
    for (id, &name) in configured_slots.iter().enumerate() {
        let identifier = name
            .map(Identifier::parse)
            .unwrap_or_else(|| Identifier::parse(&format!("rivet:generated_configured_gap_{id}")));
        let value = Arc::new(ConfiguredFeatureErased {
            feature: FeatureId::new(u32::MAX),
            config: Arc::new(DeferredGeneratedFeatureConfiguration {
                configured_key: format!("rivet:generated_configured_gap_{id}"),
            }),
        });
        configured_transaction.builder_mut().register(
            &ResourceKey::create(&*CONFIGURED_FEATURE, identifier),
            value,
            RegistrationInfo::BUILT_IN,
        );
    }
    for (id, &name) in placed_slots.iter().enumerate() {
        let identifier = name
            .map(Identifier::parse)
            .unwrap_or_else(|| Identifier::parse(&format!("rivet:generated_placed_gap_{id}")));
        let value = Arc::new(PlacedFeature::new(
            Holder::reference(configured_registry_id, configured_gap_id),
            Vec::new(),
        ));
        placed_transaction.builder_mut().register(
            &ResourceKey::create(&*PLACED_FEATURE, identifier),
            value,
            RegistrationInfo::BUILT_IN,
        );
    }
    let mut feature_handoff = placed_transaction.access_transaction();
    feature_handoff.add_transaction(&mut configured_transaction);
    let decode_access =
        LayeredRegistryAccess::new(vec![RegistryLayer::Static, RegistryLayer::Worldgen])
            .replace_from(
                RegistryLayer::Static,
                std::slice::from_ref(feature_handoff.access()),
            )
            .replace_from(RegistryLayer::Worldgen, std::slice::from_ref(worldgen))
            .composite_access();
    let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, decode_access.clone());

    let mut decoded_configured = HashMap::with_capacity(configured_entries.len());
    for &(name, _) in &configured_entries {
        let decoded = decode_configured_feature(name, &ops, &decode_access)?;
        decoded_configured.insert(name, Arc::new(decoded));
    }
    let mut decoded_placed = HashMap::with_capacity(placed_entries.len());
    for &(name, _) in &placed_entries {
        let entry = PLACED_FEATURE_BY_NAME
            .get(name)
            .ok_or_else(|| format!("missing generated {name} entry"))?;
        let json: Value = serde_json::from_str(entry.json)
            .map_err(|error| format!("decode {name} JSON: {error}"))?;
        let placed =
            decode_placed_feature_value(&json, &ops, &decode_access, &format!("decode {name}"))?;
        decoded_placed.insert(name, Arc::new(placed));
    }
    // Resolve every decoded value before taking either registry out of the
    // handoff. Thus every fallible path still drops the handoff with both
    // frozen registries present, and both borrowed transactions recover them.
    let configured_values: Vec<(Identifier, Arc<ConfiguredFeatureErased>)> = configured_slots
        .iter()
        .enumerate()
        .map(|(id, &name)| {
            let identifier = name.map(Identifier::parse).unwrap_or_else(|| {
                Identifier::parse(&format!("rivet:generated_configured_gap_{id}"))
            });
            let value = match name {
                Some(name) => Arc::clone(
                    decoded_configured
                        .get(name)
                        .ok_or_else(|| format!("decoded configured feature {name} is missing"))?,
                ),
                None => Arc::new(ConfiguredFeatureErased {
                    feature: FeatureId::new(u32::MAX),
                    config: Arc::new(DeferredGeneratedFeatureConfiguration {
                        configured_key: format!("rivet:generated_configured_gap_{id}"),
                    }),
                }),
            };
            Ok((identifier, value))
        })
        .collect::<Result<_, String>>()?;
    let placed_values: Vec<(Identifier, Arc<PlacedFeature>)> = placed_slots
        .iter()
        .enumerate()
        .map(|(id, &name)| {
            let identifier = name
                .map(Identifier::parse)
                .unwrap_or_else(|| Identifier::parse(&format!("rivet:generated_placed_gap_{id}")));
            let value = match name {
                Some(name) => Arc::clone(
                    decoded_placed
                        .get(name)
                        .ok_or_else(|| format!("decoded placed feature {name} is missing"))?,
                ),
                None => Arc::new(PlacedFeature::new(
                    Holder::reference(configured_registry_id, configured_gap_id),
                    Vec::new(),
                )),
            };
            Ok((identifier, value))
        })
        .collect::<Result<_, String>>()?;

    drop(ops);
    drop(decode_access);
    let mut placed_builder = feature_handoff
        .take_registry(&*PLACED_FEATURE)?
        .into_builder();
    let mut configured_builder = feature_handoff
        .take_registry(&*CONFIGURED_FEATURE)?
        .into_builder();
    drop(feature_handoff);

    for (identifier, value) in configured_values {
        configured_builder.replace_registered(
            &ResourceKey::create(&*CONFIGURED_FEATURE, identifier),
            value,
        );
    }
    for (identifier, value) in placed_values {
        placed_builder
            .replace_registered(&ResourceKey::create(&*PLACED_FEATURE, identifier), value);
    }
    Ok((placed_builder.freeze(), configured_builder.freeze()))
}

fn decode_value<T: Clone>(
    codec: Arc<dyn Codec<T, FeatureOps>>,
    ops: &FeatureOps,
    value: &Value,
    label: &str,
) -> Result<T, String> {
    let result = codec.parse(ops, value);
    match result.result() {
        Some(value) => Ok(value.clone()),
        None => Err(result
            .error_ref()
            .map(|error| format!("{label}: {}", error.message()))
            .unwrap_or_else(|| format!("{label}: codec returned no result"))),
    }
}

fn without_type(value: &Value, label: &str) -> Result<Value, String> {
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| format!("{label} must be an object"))?;
    object.remove("type");
    Ok(Value::Object(object))
}

fn decode_placement_modifier(
    value: &Value,
    ops: &FeatureOps,
    label: &str,
) -> Result<Arc<dyn ErasedPlacementModifier>, String> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} has no type"))?;
    let value_without_type = without_type(value, label)?;
    let modifier: Arc<dyn ErasedPlacementModifier> = match kind {
        "minecraft:block_predicate_filter" => Arc::new(decode_value(
            block_predicate_filter_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} block_predicate_filter"),
        )?),
        "minecraft:rarity_filter" => Arc::new(decode_value(
            rarity_filter_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} rarity_filter"),
        )?),
        "minecraft:in_square" => Arc::new(decode_value(
            in_square_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} in_square"),
        )?),
        "minecraft:height_range" => Arc::new(decode_value(
            height_range_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} height_range"),
        )?),
        "minecraft:environment_scan" => Arc::new(decode_value(
            environment_scan_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} environment_scan"),
        )?),
        "minecraft:surface_relative_threshold_filter" => Arc::new(decode_value(
            surface_relative_threshold_filter_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} surface_relative_threshold_filter"),
        )?),
        "minecraft:biome" => Arc::new(decode_value(
            biome_filter_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} biome"),
        )?),
        "minecraft:count" => Arc::new(decode_value(
            rivet_serialization::map_codec::codec_of(count_placement_codec::<FeatureOps>()),
            ops,
            &value_without_type,
            &format!("decode {label} count"),
        )?),
        "minecraft:count_on_every_layer" => Arc::new(decode_value(
            count_on_every_layer_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} count_on_every_layer"),
        )?),
        "minecraft:noise_based_count" => Arc::new(decode_value(
            rivet_serialization::map_codec::codec_of(
                noise_based_count_placement_codec::<FeatureOps>(),
            ),
            ops,
            &value_without_type,
            &format!("decode {label} noise_based_count"),
        )?),
        "minecraft:noise_threshold_count" => Arc::new(decode_value(
            rivet_serialization::map_codec::codec_of(noise_threshold_count_placement_codec::<
                FeatureOps,
            >()),
            ops,
            &value_without_type,
            &format!("decode {label} noise_threshold_count"),
        )?),
        "minecraft:heightmap" => Arc::new(decode_value(
            heightmap_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} heightmap"),
        )?),
        "minecraft:random_offset" => Arc::new(decode_value(
            random_offset_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} random_offset"),
        )?),
        "minecraft:surface_water_depth_filter" => Arc::new(decode_value(
            surface_water_depth_filter_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} surface_water_depth_filter"),
        )?),
        "minecraft:fixed_placement" => Arc::new(decode_value(
            fixed_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} fixed_placement"),
        )?),
        other => {
            return Err(format!(
                "{label} has unsupported placement modifier {other}"
            ));
        }
    };
    Ok(modifier)
}

fn decode_placement_list(
    placement: &Value,
    ops: &FeatureOps,
    label: &str,
) -> Result<Vec<Arc<dyn ErasedPlacementModifier>>, String> {
    placement
        .as_array()
        .ok_or_else(|| format!("{label} placement must be an array"))?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            decode_placement_modifier(value, ops, &format!("{label} placement {index}"))
        })
        .collect()
}

fn configured_holder_from_value(
    value: &Value,
    ops: &FeatureOps,
    access: &RegistryAccess,
    label: &str,
) -> Result<Holder<ConfiguredFeatureErased>, String> {
    match value {
        Value::String(name) => {
            let registry = access
                .lookup(&*CONFIGURED_FEATURE)
                .ok_or_else(|| format!("{label} configured-feature registry is missing"))?;
            let key = ResourceKey::create(&*CONFIGURED_FEATURE, Identifier::parse(name));
            registry
                .get(&key)
                .ok_or_else(|| format!("{label} references missing configured feature {name}"))
        }
        Value::Object(_) => Ok(Holder::direct(decode_configured_feature_value(
            label, value, ops, access,
        )?)),
        _ => Err(format!(
            "{label} configured feature must be a name or object"
        )),
    }
}

fn placed_holder_from_value(
    value: &Value,
    ops: &FeatureOps,
    access: &RegistryAccess,
    label: &str,
) -> Result<Holder<PlacedFeature>, String> {
    match value {
        Value::String(name) => {
            let registry = access
                .lookup(&*PLACED_FEATURE)
                .ok_or_else(|| format!("{label} placed-feature registry is missing"))?;
            let key = ResourceKey::create(&*PLACED_FEATURE, Identifier::parse(name));
            registry
                .get(&key)
                .ok_or_else(|| format!("{label} references missing placed feature {name}"))
        }
        Value::Object(object) => {
            let feature = object
                .get("feature")
                .ok_or_else(|| format!("{label} inline placed feature has no feature"))?;
            let configured = configured_holder_from_value(feature, ops, access, label)?;
            let placement = object
                .get("placement")
                .ok_or_else(|| format!("{label} inline placed feature has no placement"))?;
            Ok(Holder::direct(PlacedFeature::new(
                configured,
                decode_placement_list(placement, ops, label)?,
            )))
        }
        _ => Err(format!("{label} placed feature must be a name or object")),
    }
}

fn decode_placed_feature_value(
    value: &Value,
    ops: &FeatureOps,
    access: &RegistryAccess,
    label: &str,
) -> Result<PlacedFeature, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} placed feature must be an object"))?;
    let feature = object
        .get("feature")
        .ok_or_else(|| format!("{label} placed feature has no feature"))?;
    let configured = configured_holder_from_value(feature, ops, access, label)?;
    let placement = object
        .get("placement")
        .ok_or_else(|| format!("{label} placed feature has no placement"))?;
    Ok(PlacedFeature::new(
        configured,
        decode_placement_list(placement, ops, label)?,
    ))
}

fn decode_selector_placed_list(
    config: &Value,
    field: &str,
    ops: &FeatureOps,
    access: &RegistryAccess,
    label: &str,
) -> Result<Vec<Holder<PlacedFeature>>, String> {
    config
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label} config has no {field} list"))?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            placed_holder_from_value(value, ops, access, &format!("{label} {field}[{index}]"))
        })
        .collect()
}

fn decode_configured_feature_value(
    configured_key: &str,
    json: &Value,
    ops: &FeatureOps,
    access: &RegistryAccess,
) -> Result<ConfiguredFeatureErased, String> {
    let feature_type = json
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{configured_key} JSON has no feature type"))?;
    let feature = feature_id_from_registry_name(feature_type)
        .ok_or_else(|| format!("{configured_key} has unsupported feature type {feature_type}"))?;
    let config_value = json
        .get("config")
        .ok_or_else(|| format!("{configured_key} JSON has no config"))?;
    let config: Arc<dyn FeatureConfiguration> = match feature_type {
        "minecraft:lake" => Arc::new(decode_value(
            lake_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:monster_room" => Arc::new(NoneFeatureConfiguration),
        "minecraft:geode" => Arc::new(decode_value(
            geode_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:ore" => Arc::new(decode_value(
            ore_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:disk" => Arc::new(decode_value(
            disk_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:spring_feature" => Arc::new(decode_value(
            spring_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:simple_block" => Arc::new(decode_value(
            simple_block_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:block_column" => Arc::new(decode_value(
            block_column_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:vines" | "minecraft:freeze_top_layer" => Arc::new(NoneFeatureConfiguration),
        "minecraft:seagrass" => Arc::new(decode_value(
            probability_feature_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:underwater_magma" => Arc::new(decode_value(
            underwater_magma_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:multiface_growth" => Arc::new(decode_value(
            multiface_growth_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:huge_red_mushroom" | "minecraft:huge_brown_mushroom" => Arc::new(decode_value(
            huge_mushroom_feature_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:random_selector" => {
            let features = config_value
                .get("features")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("{configured_key} config has no features list"))?
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let chance = item.get("chance").ok_or_else(|| {
                        format!("{configured_key} features[{index}] has no chance")
                    })?;
                    let chance = decode_value(
                        codec::float_range::<FeatureOps>(0.0, 1.0),
                        ops,
                        chance,
                        &format!("{configured_key} features[{index}] chance"),
                    )?;
                    let feature = item.get("feature").ok_or_else(|| {
                        format!("{configured_key} features[{index}] has no feature")
                    })?;
                    Ok(WeightedPlacedFeature::new(
                        placed_holder_from_value(
                            feature,
                            ops,
                            access,
                            &format!("{configured_key} features[{index}]"),
                        )?,
                        chance,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let default = config_value
                .get("default")
                .ok_or_else(|| format!("{configured_key} config has no default"))?;
            Arc::new(RandomFeatureConfiguration::new(
                features,
                placed_holder_from_value(
                    default,
                    ops,
                    access,
                    &format!("{configured_key} default"),
                )?,
            ))
        }
        "minecraft:random_boolean_selector" => Arc::new(RandomBooleanFeatureConfiguration::new(
            placed_holder_from_value(
                config_value
                    .get("feature_true")
                    .ok_or_else(|| format!("{configured_key} config has no feature_true"))?,
                ops,
                access,
                &format!("{configured_key} feature_true"),
            )?,
            placed_holder_from_value(
                config_value
                    .get("feature_false")
                    .ok_or_else(|| format!("{configured_key} config has no feature_false"))?,
                ops,
                access,
                &format!("{configured_key} feature_false"),
            )?,
        )),
        "minecraft:simple_random_selector" | "minecraft:sequence" => {
            Arc::new(CompositeFeatureConfiguration::new(HolderSet::direct(
                decode_selector_placed_list(config_value, "features", ops, access, configured_key)?,
            )))
        }
        "minecraft:weighted_random_selector" => {
            let entries = config_value
                .get("features")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("{configured_key} config has no features list"))?;
            let weighted = entries
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let weight = item.get("weight").and_then(Value::as_i64).ok_or_else(|| {
                        format!("{configured_key} features[{index}] has no weight")
                    })?;
                    let weight = i32::try_from(weight).map_err(|_| {
                        format!("{configured_key} features[{index}] weight is out of range")
                    })?;
                    if weight < 1 {
                        return Err(format!(
                            "{configured_key} features[{index}] weight must be at least 1"
                        ));
                    }
                    let data = item
                        .get("data")
                        .ok_or_else(|| format!("{configured_key} features[{index}] has no data"))?;
                    Ok(Weighted::new(
                        placed_holder_from_value(
                            data,
                            ops,
                            access,
                            &format!("{configured_key} features[{index}]"),
                        )?,
                        weight,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Arc::new(WeightedRandomFeatureConfiguration::new(
                WeightedList::of_weighted_list(&weighted),
            ))
        }
        _ => Arc::new(DeferredGeneratedFeatureConfiguration {
            configured_key: configured_key.to_string(),
        }),
    };
    Ok(ConfiguredFeatureErased { feature, config })
}

fn decode_configured_feature(
    configured_key: &str,
    ops: &FeatureOps,
    access: &RegistryAccess,
) -> Result<ConfiguredFeatureErased, String> {
    let entry = CONFIGURED_FEATURE_BY_NAME
        .get(configured_key)
        .ok_or_else(|| format!("missing generated {configured_key} entry"))?;
    let json: Value = serde_json::from_str(entry.json)
        .map_err(|error| format!("decode {configured_key} JSON: {error}"))?;
    decode_configured_feature_value(configured_key, &json, ops, access)
}

fn decode_placement_modifiers(
    placed_key: &str,
    access: &RegistryAccess,
) -> Result<Vec<Arc<dyn ErasedPlacementModifier>>, String> {
    let entry = PLACED_FEATURE_BY_NAME
        .get(placed_key)
        .ok_or_else(|| format!("missing generated {placed_key} entry"))?;
    let json: Value = serde_json::from_str(entry.json)
        .map_err(|error| format!("decode {placed_key} JSON: {error}"))?;
    let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone());
    json.get("placement")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{placed_key} JSON has no placement list"))?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            decode_placement_modifier(value, &ops, &format!("{placed_key} placement {index}"))
        })
        .collect()
}

struct DecodedPlacedFeature {
    placed_registry: &'static Registry<PlacedFeature>,
    configured_registry: &'static Registry<ConfiguredFeatureErased>,
    placed_holder: Holder<PlacedFeature>,
}

impl DecodedPlacedFeature {
    fn place_with_biome_check(
        &self,
        level: &mut WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
        generator: &dyn ChunkGenerator,
        random: &mut WorldgenRandom<XoroshiroRandomSource>,
        origin: &BlockPos,
    ) {
        let placed = self.placed_holder.value(self.placed_registry);
        placed.place_with_biome_check(self.configured_registry, level, generator, random, origin);
    }
}

fn decode_placed_feature(
    placed_key: &str,
    generator: &OverworldGenerator,
) -> Result<DecodedPlacedFeature, String> {
    if !PLACED_FEATURE_BY_NAME.contains_key(placed_key) {
        return Err(format!("missing generated {placed_key} entry"));
    }
    let placed_registry = generator
        .feature_access()
        .lookup(&*PLACED_FEATURE)
        .ok_or_else(|| "generated placed-feature registry is missing".to_string())?;
    let configured_registry = generator
        .feature_access()
        .lookup(&*CONFIGURED_FEATURE)
        .ok_or_else(|| "generated configured-feature registry is missing".to_string())?;
    let key = ResourceKey::create(&*PLACED_FEATURE, Identifier::parse(placed_key));
    let placed_holder = placed_registry
        .get(&key)
        .ok_or_else(|| format!("generated placed-feature holder {placed_key} is missing"))?;
    Ok(DecodedPlacedFeature {
        placed_registry,
        configured_registry,
        placed_holder,
    })
}

fn configured_feature_key_for_placed(placed_key: &str) -> Result<String, String> {
    let placed_entry = PLACED_FEATURE_BY_NAME
        .get(placed_key)
        .ok_or_else(|| format!("missing generated {placed_key} entry"))?;
    let placed_json: Value = serde_json::from_str(placed_entry.json)
        .map_err(|error| format!("decode {placed_key} JSON: {error}"))?;
    placed_json
        .get("feature")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{placed_key} JSON has no configured feature"))
}

fn set_paper_feature_seed(
    random: &mut WorldgenRandom<XoroshiroRandomSource>,
    generator: &OverworldGenerator,
    decoration_seed: i64,
    origin: &BlockPos,
    placed_key: &str,
    global_feature_index: usize,
    step_index: usize,
) {
    let feature_population_seed = configured_feature_key_for_placed(placed_key)
        .ok()
        .and_then(|configured_key| generator.feature_seeds.get(&configured_key).copied())
        .filter(|seed| *seed != -1)
        .map(|seed| random.set_decoration_seed(seed, origin.get_x(), origin.get_z()))
        .unwrap_or(decoration_seed);
    random.set_feature_seed(
        feature_population_seed,
        global_feature_index as i32,
        step_index as i32,
    );
}

fn configured_feature_is_executable(placed_key: &str) -> Result<bool, String> {
    let placed_entry = PLACED_FEATURE_BY_NAME
        .get(placed_key)
        .ok_or_else(|| format!("missing generated {placed_key} entry"))?;
    let placed_json: Value = serde_json::from_str(placed_entry.json)
        .map_err(|error| format!("decode {placed_key} JSON: {error}"))?;
    let configured_key = placed_json
        .get("feature")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{placed_key} JSON has no configured feature"))?;
    let configured_entry = CONFIGURED_FEATURE_BY_NAME
        .get(configured_key)
        .ok_or_else(|| format!("missing generated {configured_key} entry"))?;
    let configured_json: Value = serde_json::from_str(configured_entry.json)
        .map_err(|error| format!("decode {configured_key} JSON: {error}"))?;
    Ok(matches!(
        configured_json.get("type").and_then(Value::as_str),
        Some("minecraft:lake")
            | Some("minecraft:monster_room")
            | Some("minecraft:geode")
            | Some("minecraft:ore")
            | Some("minecraft:disk")
            | Some("minecraft:spring_feature")
            | Some("minecraft:simple_block")
            | Some("minecraft:block_column")
            | Some("minecraft:vines")
            | Some("minecraft:seagrass")
            | Some("minecraft:freeze_top_layer")
            | Some("minecraft:underwater_magma")
            | Some("minecraft:multiface_growth")
            | Some("minecraft:huge_brown_mushroom")
            | Some("minecraft:huge_red_mushroom")
            | Some("minecraft:random_selector")
            | Some("minecraft:weighted_random_selector")
            | Some("minecraft:simple_random_selector")
            | Some("minecraft:random_boolean_selector")
            | Some("minecraft:sequence")
    ))
}

struct FeatureSelectionGenerator {
    generator: Arc<OverworldGenerator>,
    feature_key: &'static str,
}

impl ChunkGenerator for FeatureSelectionGenerator {
    fn get_min_y(&self) -> i32 {
        self.generator.get_min_y()
    }

    fn get_gen_depth(&self) -> i32 {
        self.generator.get_gen_depth()
    }

    fn get_biome_generation_settings_has_feature(
        &self,
        biome: &Holder<BiomeId>,
        _feature: &PlacedFeature,
    ) -> bool {
        let Holder::Direct(biome) = biome else {
            return false;
        };
        let Some(name) = BIOME_BY_ID.get(biome.0 as usize) else {
            return false;
        };
        BIOME_GENERATION_SETTINGS_BY_NAME
            .get(name)
            .is_some_and(|settings| {
                settings
                    .features
                    .iter()
                    .any(|step| step.contains(&self.feature_key))
            })
    }
}

fn placement_selects(
    region: &mut WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
    random: &mut WorldgenRandom<XoroshiroRandomSource>,
    origin: &BlockPos,
    feature_key: &'static str,
) -> Result<bool, String> {
    let modifiers = decode_placement_modifiers(feature_key, generator.feature_access())?;
    let selection_generator = FeatureSelectionGenerator {
        generator: Arc::clone(generator),
        feature_key,
    };
    let dummy_feature = ConfiguredFeatureErased {
        feature: FeatureId::new(u32::MAX),
        config: Arc::new(DeferredGeneratedFeatureConfiguration {
            configured_key: format!("{feature_key} placement selection"),
        }),
    };
    let placed = PlacedFeature::new(Holder::Direct(dummy_feature), modifiers);
    Ok(placed.has_placement_positions(region, &selection_generator, random, origin))
}

/// The single panic-payload classifier for generated-feature boundary panics.
/// Both `String` and `&'static str` payloads classify identically so the two
/// `panic!` payload shapes can never diverge in accepted messages.
fn generated_boundary_panic_message(payload: &(dyn std::any::Any + Send)) -> Option<&str> {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&'static str>().copied())
}

fn is_generated_feature_boundary_panic(payload: &(dyn std::any::Any + Send)) -> bool {
    generated_boundary_panic_message(payload).is_some_and(|message| {
        message.starts_with("generated feature ")
            || message.contains("BlockStateBase.canSurvive is not implemented")
            || message.contains("WorldGenRegion.canSurvive is not implemented")
            || message.contains("Biome.shouldFreeze is not implemented")
            || message.contains("Biome.shouldSnow is not implemented")
    })
}

fn generated_boundary_feature_key(
    payload: &(dyn std::any::Any + Send),
    fallback: &'static str,
) -> &'static str {
    let prefix = "generated feature ";
    let Some(name) = generated_boundary_panic_message(payload)
        .and_then(|message| message.strip_prefix(prefix))
        .and_then(|rest| rest.split(' ').next())
    else {
        return fallback;
    };
    // The panic payloads originate from `unavailable_feature`, whose values
    // are configured-feature keys (`DeferredGeneratedFeatureConfiguration`).
    // Validate against the configured table; 97 of the 170 generated
    // configured names also exist as unrelated placed entries, so a
    // placed-table search would misattribute instead of falling back.
    CONFIGURED_FEATURE_BY_NAME
        .entries()
        .find(|(candidate, _)| **candidate == name)
        .map(|(candidate, _)| *candidate)
        .unwrap_or(fallback)
}

fn run_biome_decoration(
    chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
    workspace: &FeatureWorkspace,
    structure_feature_index: Option<StructureFeatureIndex>,
) -> Result<FeatureWritebacks, GenError> {
    // Paper executes the structure loop before placed features whenever the
    // structure manager enables it. This value layer has no structure manager,
    // so only the registry-derived per-step index may cross the boundary. Its
    // counts are deliberately not added to placed-feature seeds: pinned Paper
    // 26.2 uses `globalIndexOfFeature` independently for that loop.
    let structure_feature_index =
        structure_feature_index.ok_or(GenError::StructureDecorationIndexUnavailable {
            chunk_pos: chunk.get_pos(),
        })?;
    run_biome_decoration_through(
        chunk,
        generator,
        workspace,
        structure_feature_index,
        Decoration::VALUES
            .len()
            .max(generator.feature_plan()?.feature_list.len()),
    )
}

/// The decoration body with a caller-chosen exclusive step bound. Production
/// runs every step; the seed-1 spring regression bounds execution at the
/// FLUID_SPRINGS step so the pass ends on a real spring placement instead of a
/// later unsupported seam.
fn run_biome_decoration_through(
    chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
    workspace: &FeatureWorkspace,
    structure_feature_index: StructureFeatureIndex,
    max_steps: usize,
) -> Result<FeatureWritebacks, GenError> {
    chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
    let center_pos = chunk.get_pos();
    let origin =
        SectionPos::of_chunk_pos(&center_pos, chunk.height_accessor().get_min_section_y()).origin();
    let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(
        random_support::generate_unique_seed(),
    ));
    let decoration_seed =
        random.set_decoration_seed(generator.seed(), origin.get_x(), origin.get_z());

    let mut region = compose_feature_region_with_workspace(chunk, generator, Some(workspace));
    let union_biomes = gather_possible_biomes(&region, generator);

    // Resolve the FULL `biomeSource.possibleBiomes()` list in source order and
    // build the FeatureSorter once from it — Paper's
    // `ChunkGenerator.featuresPerStep` (`ChunkGenerator.java` 97-100), NOT the
    // 3x3 union. The union only picks which global indices execute per step.
    // The placed-feature holders are `Holder::Reference` over one fabricated
    // `PLACED_FEATURE` registry id (the generated tables are keyed by name; the
    // FeatureSorter keys on holder identity, so a single fabricated registry
    // collapses the biomes' shared steps exactly like Paper's registry does).
    // The `features` lists and sorter are built once in the immutable per-world
    // plan (`OverworldGenerator::feature_plan`), matching Paper's memoized
    // `featuresPerStep` rather than rebuilding them for each target extraction.
    let plan = generator.feature_plan()?;
    let placed_by_id = &plan.placed_by_id;
    let settings_sources = &plan.settings_sources;
    let feature_list = &plan.feature_list;

    // The per-step loop — Paper's `addVanillaDecorations`. The structure body
    // remains deferred with the unported structure manager, but the capability
    // check above prevents this path from pretending that the structure loop
    // consumed no work. Once a caller proves the capability, placed-feature
    // seeds still use Paper's independent `globalIndexOfFeature`.
    let generation_steps = Decoration::VALUES.len().max(feature_list.len());
    // Paper walks steps in ascending order and, within a step, the sorted
    // global feature indices of the union biomes mapped through the full-list
    // sorter's `indexMapping`. Registry-backed configured features execute
    // through their exact placed-feature chains; unsupported selected leaves
    // stop the run with a typed boundary.
    for step_index in 0..max_steps.min(generation_steps) {
        // Paper gives structures their own zero-based index within each
        // decoration step. The structure manager/start lookup is outside this
        // value slice, but consuming the real registry cardinality preserves
        // the exact structure-loop seed calls without inventing an offset for
        // the independent placed-feature index below.
        for structure_index in 0..structure_feature_index.count_for_step_index(step_index) {
            random.set_feature_seed(decoration_seed, structure_index as i32, step_index as i32);
        }
        if step_index >= feature_list.len() {
            continue;
        }
        let step_feature_data = &feature_list[step_index];
        let mut possible_features_this_step = Vec::new();
        for name in &union_biomes {
            let Some(settings) = settings_sources
                .iter()
                .find(|(_, source_name)| *source_name == *name)
                .map(|(settings, _)| settings)
            else {
                continue;
            };
            if step_index < settings.features().len() {
                for holder in settings.features()[step_index].iter() {
                    if let Some(index) = step_feature_data.index_mapping(holder) {
                        possible_features_this_step.push(index);
                    }
                }
            }
        }
        possible_features_this_step.sort_unstable();
        possible_features_this_step.dedup();
        for global_feature_index in possible_features_this_step {
            let feature = &feature_list[step_index].features[global_feature_index];
            let feature_key = match feature {
                Holder::Reference { id, .. } => {
                    placed_by_id.get(id).copied().unwrap_or("minecraft:unknown")
                }
                Holder::Direct(_) => "minecraft:unknown",
            };
            // Paper's configurable population seed path uses the configured
            // feature holder (`PlacedFeature.feature()`), not the surrounding
            // placed-feature key. A configured seed other than `-1` reseeds via
            // `setDecorationSeed` first, consuming its two scale draws, then
            // `setFeatureSeed` receives that derived population seed.
            set_paper_feature_seed(
                &mut random,
                generator,
                decoration_seed,
                &origin,
                feature_key,
                global_feature_index,
                step_index,
            );
            let executable = configured_feature_is_executable(feature_key).map_err(|_| {
                GenError::FeaturePlacementDecode {
                    chunk_pos: center_pos,
                    step_index,
                    global_feature_index,
                    feature_key,
                }
            })?;
            if executable {
                let placed = decode_placed_feature(feature_key, generator).map_err(|_| {
                    GenError::FeaturePlacementDecode {
                        chunk_pos: center_pos,
                        step_index,
                        global_feature_index,
                        feature_key,
                    }
                })?;
                let dispatch_generator = FeatureSelectionGenerator {
                    generator: Arc::clone(generator),
                    feature_key,
                };
                let placement = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    placed.place_with_biome_check(
                        &mut region,
                        &dispatch_generator,
                        &mut random,
                        &origin,
                    );
                }));
                if let Err(payload) = placement {
                    if is_generated_feature_boundary_panic(payload.as_ref()) {
                        return Err(GenError::FeaturePlacementDecode {
                            chunk_pos: center_pos,
                            step_index,
                            global_feature_index,
                            feature_key: generated_boundary_feature_key(
                                payload.as_ref(),
                                feature_key,
                            ),
                        });
                    }
                    std::panic::resume_unwind(payload);
                }
                continue;
            }
            let selected = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                placement_selects(&mut region, generator, &mut random, &origin, feature_key)
            })) {
                Ok(Ok(selected)) => selected,
                Ok(Err(_)) => {
                    return Err(GenError::FeaturePlacementDecode {
                        chunk_pos: center_pos,
                        step_index,
                        global_feature_index,
                        feature_key,
                    });
                }
                Err(payload) if is_generated_feature_boundary_panic(payload.as_ref()) => {
                    return Err(GenError::FeaturePlacementDecode {
                        chunk_pos: center_pos,
                        step_index,
                        global_feature_index,
                        feature_key: generated_boundary_feature_key(payload.as_ref(), feature_key),
                    });
                }
                Err(payload) => std::panic::resume_unwind(payload),
            };
            if selected {
                return Err(GenError::FeaturePlacementDecode {
                    chunk_pos: center_pos,
                    step_index,
                    global_feature_index,
                    feature_key,
                });
            }
        }
    }
    let owned_entries = region.into_owned_proto_entries();
    let mut retained_writebacks = Vec::with_capacity(8);
    for (pos, chunk) in owned_entries {
        // Move every owned proto into the authoritative workspace. Paper's
        // region borrows these scheduler-owned chunks, so heightmaps,
        // post-processing, block entities, and scheduled ticks must survive
        // even outside the block-state write radius. Keep the public holder
        // diagnostic limited to the eight distance-one entries for the
        // FEATURES write zone.
        let distance_one = center_pos.get_chessboard_distance_coords(pos.x(), pos.z()) == 1;
        let diagnostic = distance_one.then(|| snapshot_generated_chunk(&chunk));
        workspace.insert(chunk);
        if let Some(diagnostic) = diagnostic {
            retained_writebacks.push((pos, diagnostic));
        }
    }
    Ok(retained_writebacks)
}

/// Compose the SPAWN step's exact radius-one `WorldGenRegion` cache. The
/// generation pyramid requires `LIGHT` at distance zero and `BIOMES` at
/// distance one; unlike the old detached seam this reads all eight neighbour
/// protos through the same cache contract Paper uses. Every holder is a
/// borrow-carrying tick-thread view, so no proto is cloned or moved into a
/// second authority.
fn compose_spawn_region<'a>(
    center: &'a mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    workspace: &'a mut SpawnRegionProtos,
    generator: &'a OverworldGenerator,
) -> Result<WorldGenRegion<'a, BlockState, WorldgenBiomeId, StructureKey>, SpawnRegionError> {
    let center_pos = center.get_pos();
    let step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Spawn).clone();
    let dependencies = step.direct_dependencies();
    for dx in -1..=1 {
        for dz in -1..=1 {
            let pos = ChunkPos::new(
                center_pos.x().wrapping_add(dx),
                center_pos.z().wrapping_add(dz),
            );
            let distance = dx.abs().max(dz.abs()) as usize;
            let required = dependencies.get(distance);
            let (actual, valid) = if pos == center_pos {
                let actual = center.get_persisted_status();
                (actual, actual.is_or_after(required))
            } else {
                let Some((_, neighbour)) = workspace
                    .neighbours
                    .iter()
                    .find(|(neighbour_pos, _)| *neighbour_pos == pos)
                else {
                    return Err(SpawnRegionError::MissingNeighbour { pos });
                };
                let actual = neighbour.get_persisted_status();
                (actual, actual.is_or_after(required))
            };
            if !valid {
                return Err(SpawnRegionError::InsufficientStatus {
                    pos,
                    actual,
                    required,
                });
            }
        }
    }

    let world_border_settings = workspace.world_border_settings();
    let center_status = center.get_persisted_status();
    let center_base = center.base_mut();
    let mut holders: Vec<
        Box<dyn GenerationChunkHolderView<BlockState, WorldgenBiomeId, StructureKey> + 'a>,
    > = Vec::with_capacity(9);
    for (_, neighbour) in workspace.neighbours.iter_mut() {
        let status = neighbour.get_persisted_status();
        holders.push(Box::new(CenterHolder::new(neighbour.base_mut(), status)));
    }
    // The eight neighbours were emitted in x-then-z order; inserting the
    // center at row-major slot 4 restores the complete 3x3 cache order.
    holders.insert(4, Box::new(CenterHolder::new(center_base, center_status)));

    Ok(WorldGenRegion::new_with_world_border_settings(
        StaticCache2D::from_entries(
            center_pos.x().wrapping_sub(1),
            center_pos.z().wrapping_sub(1),
            3,
            3,
            holders,
        ),
        center_pos,
        step,
        generator.seed(),
        generator.get_min_y(),
        generator.get_gen_depth(),
        generator.get_sea_level(),
        Arc::new(generator.biome_source.clone()),
        generator.registry_access().clone(),
        world_border_settings,
    ))
}

/// `ChunkStatusTasks.generateSpawn` → Java's `generator.spawnOriginalMobs`
/// (`NoiseBasedChunkGenerator.spawnOriginalMobs`, paper-server) over the SPAWN
/// step's `WorldGenRegion` — the G2 SPAWN seam body.
///
/// In Java order (`NoiseBasedChunkGenerator.spawnOriginalMobs`, 26.2):
///   1. `if (!this.settings.value().disableMobGeneration())` — the generator
///      settings' `disableMobGeneration` gate (the overworld preset sets it
///      `false`). When disabled, the spawn step is a faithful no-op (no RNG, no
///      population) and the caller advances to SPAWN.
///   2. `center = worldGenRegion.getCenter()`; `biome =
///      worldGenRegion.getBiome(center.getWorldPosition().atY(worldGenRegion.getMaxY()))`
///      — the biome at the chunk-minimum block coordinate and max build height
///      (`ChunkPos.getWorldPosition()` is `(minBlockX, 0, minBlockZ)`).
///   3. `random = new WorldgenRandom(new LegacyRandomSource(
///      RandomSupport.generateUniqueSeed()))`;
///      `random.setDecorationSeed(worldGenRegion.getSeed(), center.getMinBlockX(),
///      center.getMinBlockZ())` — the decoration seed overwrites the
///      unique seed (the seed that would have been consumed is never drawn).
///   4. `NaturalSpawner.spawnMobsForChunkGeneration(worldGenRegion, biome,
///      center, random)`:
///      - `mobSettings = biome.value().getMobSettings()`;
///        `mobs = mobSettings.getMobs(MobCategory.CREATURE)`.
///      - `if (!mobs.isEmpty() && level.getLevel().getGameRules().get(GameRules.SPAWN_MOBS))`
///        → the real empty/non-empty CREATURE gate AND the `SPAWN_MOBS` rule.
///        When either disqualifies, the body is a faithful no-op — no RNG draw,
///        no entity — and the caller advances to SPAWN.
///      - Non-empty + rule on: Java evaluates `while (random.nextFloat() <
///        mobSettings.getCreatureProbability())`. A failed first roll exits with
///        zero entities. The shared value layer uses the Paper `WeightedRandom`
///        selector and exact count/candidate draw order, including the
///        spawnable-block, empty-block, checkSpawnRules, no-collision, and
///        obstruction gates. It then fails typed only at unsupported entity
///        construction, before writing fabricated entity data. The chunk is
///        never stamped SPAWN on refusal.
///
/// The pinned seed-42 origin resolves `minecraft:dark_forest`, whose CREATURE
/// list is non-empty with probability 0.1. Its decoration-seeded first roll is
/// 0.7275637, so the exact while condition fails and Paper advances with zero
/// entities. This is never a fixture-specific shortcut: the list, rule, and RNG
/// condition are genuinely evaluated in Java order.
fn run_spawn_in_region(
    region: &mut WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    generator: &OverworldGenerator,
    spawn_mobs_rule: bool,
) -> Result<(), SpawnRegionError> {
    let center_pos = region.get_center();

    // `NoiseBasedChunkGenerator.spawnOriginalMobs` owns this gate. The
    // overworld preset is deliberately not changed to disable mobs: callers
    // that want a gamerule-off path pass `spawn_mobs_rule = false` below.
    if generator.generator().disable_mob_generation() {
        return Ok(());
    }

    let position = BlockPos::new(
        center_pos.get_min_block_x(),
        region.get_max_y(),
        center_pos.get_min_block_z(),
    );
    let biome = WorldGenLevel::get_biome(region, &position);
    let biome_name = BIOME_BY_ID.get(dense_biome_id(&biome) as usize).copied();
    // Java constructs and decoration-seeds the generation random before
    // entering `NaturalSpawner.spawnMobsForChunkGeneration`; `setDecorationSeed`
    // overwrites the unique seed before the empty-list/gamerule gate.
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(
        random_support::generate_unique_seed(),
    ));
    random.set_decoration_seed(
        region.get_seed(),
        center_pos.get_min_block_x(),
        center_pos.get_min_block_z(),
    );

    let Some(biome_name) = biome_name else {
        return Err(SpawnRegionError::MissingBiomeSettings {
            chunk_pos: center_pos,
            biome: None,
        });
    };
    let Some(mob_settings) = MOB_SPAWN_SETTINGS_BY_NAME.get(biome_name) else {
        return Err(SpawnRegionError::MissingBiomeSettings {
            chunk_pos: center_pos,
            biome: Some(biome_name),
        });
    };

    // Java evaluates the empty CREATURE list and gamerule before the first
    // random draw. This is the default-true path for a fresh world, not a
    // fixture-specific mob-disable shortcut.
    if mob_settings.creature.is_empty() || !spawn_mobs_rule {
        return Ok(());
    }

    let xo = center_pos.get_min_block_x();
    let zo = center_pos.get_min_block_z();
    let region_random_factory =
        generator
            .random_state()
            .get_or_create_random_factory(&Identifier::with_default_namespace(
                "worldgen_region_random",
            ));
    let mut level_random = region_random_factory.at(xo, 0, zo);

    // `NaturalSpawner.spawnMobsForChunkGeneration`. The loop retains Paper's
    // exact while/weighted/count/candidate/offset order. Placement and spawn
    // rule rejections continue to the next attempt; only a candidate which
    // passes every available gate reaches the genuinely unsupported entity
    // construction boundary.
    while random.next_float() < mob_settings.creature_probability {
        let Some(spawner) = WeightedRandom::get_random_item_from_total(
            &mut random,
            mob_settings.creature,
            |entry| i32::try_from(entry.weight).expect("generated spawn weight fits i32"),
        ) else {
            break;
        };
        let count = spawner.min
            + random.next_int_bound(spawner.max.wrapping_sub(spawner.min).wrapping_add(1) as i32)
                as u32;
        let mut x = xo.wrapping_add(random.next_int_bound(16));
        let mut z = zo.wrapping_add(random.next_int_bound(16));
        let start_x = x;
        let start_z = z;

        for _ in 0..count {
            for _ in 0..4 {
                let top_y = region.get_height_at(spawn_heightmap_type(spawner.ty), x, z);
                let y = adjust_spawn_y(region, spawner.ty, x, top_y, z);
                let spawn_pos = BlockPos::new(x, y, z);
                if region.is_inside_build_height(y)
                    && is_spawn_position_ok(region, spawner.ty, &spawn_pos)
                {
                    // Paper keeps EntityType dimensions as Java floats until the
                    // AABB constructor promotes them to doubles. Converting the
                    // exact f32 values here preserves both the clamp endpoints
                    // and the raw getSpawnAABB coordinates (not the rounded
                    // decimal literals' f64 approximations).
                    let (width, height) = spawn_dimensions(spawner.ty);
                    let width = f64::from(width);
                    let height = f64::from(height);
                    let fx = (x as f64).clamp(xo as f64 + width, xo as f64 + 16.0 - width);
                    let fz = (z as f64).clamp(zo as f64 + width, zo as f64 + 16.0 - width);
                    let entity_pos = BlockPos::containing(fx, y as f64, fz);
                    if no_collision(region, fx, y, fz, width, height)
                        && check_spawn_rules(
                            region,
                            spawner.ty,
                            biome_name,
                            &entity_pos,
                            &mut level_random,
                        )
                    {
                        // Entity construction and Entity.snapTo happen before
                        // Mob.checkSpawnObstruction in Paper. Consume the yaw
                        // draw for every candidate that reaches that boundary,
                        // including a candidate whose post-construction
                        // obstruction check rejects it.
                        consume_spawn_snap_yaw(&mut random);
                        if spawn_obstruction_ok(region, spawner.ty, fx, y, fz, width, height) {
                            // The current entity registry has no constructor or
                            // insertion surface for these mob types. Do not
                            // fabricate entity NBT at the intentional boundary.
                            return Err(SpawnRegionError::UnsupportedEntity {
                                chunk_pos: center_pos,
                                biome: biome_name,
                                entity_type: spawner.ty,
                                position: entity_pos,
                            });
                        }
                    }
                }

                advance_spawn_candidate(&mut random, &mut x, &mut z, start_x, start_z, xo, zo);
            }
        }
    }
    Ok(())
}

/// Consume the population RNG draw used by Paper's `Entity.snapTo` yaw during
/// the entity-construction boundary. Keeping it named makes the intentional
/// unsupported-entity stop auditable alongside the candidate offset draws.
fn consume_spawn_snap_yaw(random: &mut impl RandomSource) {
    let _ = random.next_float();
}

/// Java's four-attempt candidate offset block, including the retry loop that
/// re-seeds both coordinates from the original start whenever the offset
/// leaves the chunk. Keeping it isolated makes the draw order auditable and
/// testable independently of entity construction.
fn advance_spawn_candidate(
    random: &mut impl RandomSource,
    x: &mut i32,
    z: &mut i32,
    start_x: i32,
    start_z: i32,
    xo: i32,
    zo: i32,
) {
    *x = x
        .wrapping_add(random.next_int_bound(5))
        .wrapping_sub(random.next_int_bound(5));
    *z = z
        .wrapping_add(random.next_int_bound(5))
        .wrapping_sub(random.next_int_bound(5));
    while *x < xo || *x >= xo.wrapping_add(16) || *z < zo || *z >= zo.wrapping_add(16) {
        *x = start_x
            .wrapping_add(random.next_int_bound(5))
            .wrapping_sub(random.next_int_bound(5));
        *z = start_z
            .wrapping_add(random.next_int_bound(5))
            .wrapping_sub(random.next_int_bound(5));
    }
}

/// Paper's `SpawnPlacements.getHeightmapType` registration for the generated
/// CREATURE entries. Ocelots and parrots include leaves in their candidate
/// heightmap; all other entries in this slice use the no-leaves variant.
fn spawn_heightmap_type(entity_type: &str) -> Types {
    match entity_type {
        "minecraft:ocelot" | "minecraft:parrot" => Types::MotionBlocking,
        _ => Types::MotionBlockingNoLeaves,
    }
}

/// The generation-time `SpawnPlacements.ON_GROUND` predicate. Paper asks the
/// placement type to adjust a heightmap candidate down one block when the
/// block below is pathfindable by LAND. This is intentionally distinct from
/// `solid_render`: glass, for example, has a full collision shape but is not a
/// full render/occlusion block.
fn adjust_spawn_y(
    region: &WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    entity_type: &str,
    x: i32,
    y: i32,
    z: i32,
) -> i32 {
    if matches!(entity_type, "minecraft:fox" | "minecraft:panda") {
        return y;
    }
    let below = region.get_block_state(&BlockPos::new(x, y.wrapping_sub(1), z));
    if is_pathfindable_land(below) {
        y.wrapping_sub(1)
    } else {
        y
    }
}

/// `BlockState.isPathfindable(PathComputationType.LAND)`. The generated
/// collision-face table supplies the default full-shape answer. The explicit
/// class families below mirror Paper overrides; context-sensitive classes are
/// listed by their concrete registered block names rather than collapsing all
/// dynamic shapes into one answer.
fn is_pathfindable_land(state: BlockState) -> bool {
    let name = state.block().name();
    // PowderSnowBlock explicitly overrides LAND pathfinding to true even
    // though its collision shape is context-sensitive.
    if name == "minecraft:powder_snow" {
        return true;
    }
    // Scaffolding inherits BlockBehaviour.isPathfindable: its empty collision
    // context selects the stable, non-full shape, so LAND is true.
    if name == "minecraft:scaffolding" {
        return true;
    }
    match name {
        // These block classes override the default LAND predicate to false.
        "minecraft:soul_sand"
        | "minecraft:dirt_path"
        | "minecraft:farmland"
        | "minecraft:bamboo"
        | "minecraft:moving_piston"
        | "minecraft:pointed_dripstone"
        | "minecraft:sulfur_spike"
        | "minecraft:mud"
        | "minecraft:cactus"
        | "minecraft:iron_bars"
        | "minecraft:chain"
        | "minecraft:anvil"
        | "minecraft:chipped_anvil"
        | "minecraft:damaged_anvil"
        | "minecraft:azalea"
        | "minecraft:flowering_azalea"
        | "minecraft:bell"
        | "minecraft:brewing_stand"
        | "minecraft:cake"
        | "minecraft:campfire"
        | "minecraft:soul_campfire"
        | "minecraft:chest"
        | "minecraft:trapped_chest"
        | "minecraft:copper_chest"
        | "minecraft:ender_chest"
        | "minecraft:chorus_plant"
        | "minecraft:cocoa"
        | "minecraft:composter"
        | "minecraft:conduit"
        | "minecraft:decorated_pot"
        | "minecraft:dragon_egg"
        | "minecraft:dried_ghast"
        | "minecraft:enchanting_table"
        | "minecraft:end_portal_frame"
        | "minecraft:grindstone"
        | "minecraft:heavy_core"
        | "minecraft:hopper"
        | "minecraft:lantern"
        | "minecraft:soul_lantern"
        | "minecraft:copper_lantern"
        | "minecraft:lectern"
        | "minecraft:respawn_anchor"
        | "minecraft:end_rod"
        | "minecraft:lightning_rod"
        | "minecraft:sculk_sensor"
        | "minecraft:calibrated_sculk_sensor"
        | "minecraft:sea_pickle"
        | "minecraft:sniffer_egg"
        | "minecraft:stonecutter"
        | "minecraft:copper_golem_statue"
        | "minecraft:piston"
        | "minecraft:sticky_piston"
        | "minecraft:piston_head"
        | "minecraft:flower_pot"
        | "minecraft:water_cauldron"
        | "minecraft:lava_cauldron"
        | "minecraft:powder_snow_cauldron"
        | "minecraft:cauldron" => false,
        // The generated names for the remaining registered classes are
        // regular families: beds, candle cakes, skulls/heads, potted plants,
        // shelves, panes/bars, and wall-mounted hanging signs.
        name if name.ends_with("_bed")
            || name.ends_with("_candle_cake")
            || name.ends_with("_skull")
            || name.ends_with("_head")
            || name.starts_with("minecraft:potted_")
            || name.ends_with("_shelf")
            || name.ends_with("_pane")
            || name.ends_with("_bars")
            || name.ends_with("_chain")
            || name.ends_with("_lantern")
            || name.ends_with("_lightning_rod")
            || name.ends_with("_copper_chest")
            || name.ends_with("_copper_golem_statue")
            || name.ends_with("_wall_hanging_sign")
            || name == "minecraft:shulker_box"
            || name.ends_with("_shulker_box") =>
        {
            false
        }
        name if name.ends_with("_fence") || name.ends_with("_wall") => false,
        name if name.ends_with("_slab") || name.ends_with("_stairs") => false,
        name if name.ends_with("_door") || name.ends_with("_trapdoor") => state
            .get_value(BlockStateProperties::OPEN)
            .is_some_and(|value| matches!(value, PropertyValue::Bool(true))),
        name if name.ends_with("_fence_gate") => state
            .get_value(BlockStateProperties::OPEN)
            .is_some_and(|value| matches!(value, PropertyValue::Bool(true))),
        "minecraft:water" | "minecraft:bubble_column" => true,
        "minecraft:lava" => false,
        "minecraft:snow" => matches!(
            state.get_value(BlockStateProperties::LAYERS),
            Some(PropertyValue::Int(layers)) if layers < 5
        ),
        // The default BlockBehaviour predicate is `!isCollisionShapeFullBlock`.
        // Dynamic states still require a live collision context; an unknown
        // dynamic shape must not be inferred from its zero-context samples.
        // Static states can use the generated full-face mask for Paper's
        // empty-context full-block predicate.
        _ => !state.has_dynamic_shape() && state.collision_face_mask() != 0x3F,
    }
}

fn is_spawn_position_ok(
    region: &WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    entity_type: &str,
    pos: &BlockPos,
) -> bool {
    // `SpawnPlacements.NO_RESTRICTIONS` is the actual registration for foxes
    // and pandas. Their per-type `checkSpawnRules` still runs later, but the
    // placement predicate must not impose ON_GROUND's empty-block/floor gate.
    if matches!(entity_type, "minecraft:fox" | "minecraft:panda") {
        return true;
    }
    // `SpawnPlacementTypes.ON_GROUND` checks the candidate block against the
    // live `Level.getWorldBorder()` before any floor or empty-block predicates.
    // The region carries that same WorldBorder representation; NO_RESTRICTIONS
    // above intentionally remains border-independent like Paper.
    if !region.world_border().is_within_bounds(pos) {
        return false;
    }
    let below = pos.below();
    let below_state = region.get_block_state(&below);
    is_valid_spawn_floor(below_state, entity_type)
        && is_valid_empty_spawn_block(region.get_block_state(pos), entity_type, pos)
        && is_valid_empty_spawn_block(
            region.get_block_state(&pos.above()),
            entity_type,
            &pos.above(),
        )
}

/// `BlockState.isValidSpawn(level, pos, entityType)`, including the handful of
/// vanilla block-property overrides that matter to the generated creature
/// tables. The entity-specific spawnable-on tags are checked later by each
/// entity's `checkSpawnRules`, not by the placement type.
fn is_valid_spawn_floor(state: BlockState, entity_type: &str) -> bool {
    let name = state.block().name();
    // IceBlock and FrostedIceBlock replace the default sturdy-up predicate
    // with the entity-specific polar-bear check. Powder snow has no such
    // floor override: its dynamic collision affects obstruction only, while
    // isValidSpawn still uses the block's registered predicate.
    if entity_type == "minecraft:polar_bear"
        && matches!(name, "minecraft:ice" | "minecraft:frosted_ice")
    {
        return true;
    }
    if matches!(name, "minecraft:ice" | "minecraft:frosted_ice") {
        return false;
    }
    if state.has_dynamic_shape() {
        return false;
    }
    // Blocks.java installs these predicates on the block properties, replacing
    // the default sturdy-up + low-emission test. Keep the override order ahead
    // of the generic face query because glass, for example, is sturdy but has a
    // deliberate `never` predicate.
    if matches!(
        name,
        "minecraft:soul_sand"
            | "minecraft:carved_pumpkin"
            | "minecraft:jack_o_lantern"
            | "minecraft:redstone_lamp"
            | "minecraft:mud"
    ) {
        return true;
    }
    if matches!(
        name,
        "minecraft:bedrock"
            | "minecraft:glass"
            | "minecraft:barrier"
            | "minecraft:moving_piston"
            | "minecraft:repeater"
            | "minecraft:chorus_flower"
            | "minecraft:scaffolding"
            | "minecraft:tinted_glass"
    ) || name.ends_with("_trapdoor")
        || name.ends_with("_grate")
        || name.ends_with("_stained_glass")
    {
        // These are BlockBehaviour's explicit `never` predicates.
        return false;
    }
    // Blocks.java's `ocelotOrParrot` predicate is used by every leaves
    // property set and by firefly bush. It is entity-specific rather than a
    // general leaves rule.
    if matches!(entity_type, "minecraft:ocelot" | "minecraft:parrot")
        && (state.is_in_tag("minecraft:leaves") || name == "minecraft:firefly_bush")
    {
        return true;
    }
    if name == "minecraft:magma_block" {
        // MagmaBlock's override is `entityType.fireImmune()`; no generated
        // CREATURE entry is fire immune.
        return false;
    }
    state.is_face_sturdy(rivet_registry::core::Direction::Up) && state.light_emission() < 14
}

fn is_valid_empty_spawn_block(state: BlockState, entity_type: &str, pos: &BlockPos) -> bool {
    // `NaturalSpawner.isValidEmptySpawnBlock` only asks whether the empty
    // collision shape is a full block, then applies signal/fluid/tag/danger
    // predicates. Known dynamic shapes use their concrete Paper fallback;
    // unknown dynamic shapes fail closed because their context is unavailable.
    // For an unknown static shape, the generated full-face mask still answers
    // the exact empty-context full-block predicate, so a partial shape must
    // follow Paper rather than being blanket-rejected.
    let is_full = match spawn_collision_shape(state, pos, SpawnCollisionContext::Empty) {
        Some(shape) => shape.is_full(),
        None if !state.has_dynamic_shape() => state.collision_face_mask() == 0x3F,
        None => return false,
    };
    !is_full
        && !is_signal_source(state)
        && state.fluid_empty()
        && !state.is_in_tag("minecraft:prevent_mob_spawning_inside")
        && !is_dangerous_spawn_block(state, entity_type)
}

/// A compact VoxelShape slice for generation-time collision checks. Static
/// state boxes come from the pinned Paper `VoxelShape.toAabbs()` fixture;
/// position/context-sensitive dynamic families remain explicit below. Unknown
/// dynamic shapes stay unavailable for obstruction checks, while
/// `is_valid_empty_spawn_block` separately uses the generated full-face mask
/// for Paper's empty-context full-block predicate.
#[derive(Clone, Copy)]
struct SpawnShapeBox {
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
}

impl SpawnShapeBox {
    const fn new(min_x: f64, min_y: f64, min_z: f64, max_x: f64, max_y: f64, max_z: f64) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    fn translated(self, x: f64, y: f64, z: f64) -> Self {
        Self::new(
            self.min_x + x,
            self.min_y + y,
            self.min_z + z,
            self.max_x + x,
            self.max_y + y,
            self.max_z + z,
        )
    }

    fn intersects(self, block_x: i32, block_y: i32, block_z: i32, entity: &SpawnAabb) -> bool {
        SpawnAabb::new(
            block_x as f64 + self.min_x,
            block_y as f64 + self.min_y,
            block_z as f64 + self.min_z,
            block_x as f64 + self.max_x,
            block_y as f64 + self.max_y,
            block_z as f64 + self.max_z,
        )
        .intersects(entity)
    }

    fn intersects_raw(self, block_x: i32, block_y: i32, block_z: i32, entity: &SpawnAabb) -> bool {
        SpawnAabb::new(
            block_x as f64 + self.min_x,
            block_y as f64 + self.min_y,
            block_z as f64 + self.min_z,
            block_x as f64 + self.max_x,
            block_y as f64 + self.max_y,
            block_z as f64 + self.max_z,
        )
        .intersects_raw(entity)
    }

    fn is_full(self) -> bool {
        self.min_x == 0.0
            && self.min_y == 0.0
            && self.min_z == 0.0
            && self.max_x == 1.0
            && self.max_y == 1.0
            && self.max_z == 1.0
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum SpawnCollisionContext<'a> {
    Empty,
    #[allow(dead_code)]
    Entity {
        entity_type: &'a str,
        entity_min_y: f64,
    },
}

#[derive(Clone, Copy)]
enum SpawnCollisionShape {
    Empty,
    Full,
    Box(SpawnShapeBox),
    /// Exact static Paper collision boxes from the generated StateId table.
    StaticBoxes(&'static [StaticCollisionBox]),
    /// Paper's empty-context scaffolding shape: a full top plate and four
    /// corner posts. The empty collision context reports `isAbove(..., true)`
    /// as true, selecting `SHAPE_STABLE` for every scaffolding state.
    Multi([SpawnShapeBox; 5]),
}

impl SpawnCollisionShape {
    fn is_full(self) -> bool {
        match self {
            Self::Full => true,
            Self::Empty | Self::Multi(_) | Self::StaticBoxes(_) => false,
            Self::Box(shape) => shape.is_full(),
        }
    }

    fn intersects(self, block_x: i32, block_y: i32, block_z: i32, entity: &SpawnAabb) -> bool {
        match self {
            Self::Empty => false,
            // Paper's BlockCollisions takes the optimized Shapes.block() path
            // for an exact full cube, whose AABB test is raw (without the
            // partial-shape CollisionUtil epsilon).
            Self::Full => SpawnShapeBox::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
                .intersects_raw(block_x, block_y, block_z, entity),
            Self::Box(shape) => shape.intersects(block_x, block_y, block_z, entity),
            // Every StaticBoxes value is a partial VoxelShape. Exact full-cube
            // table entries are normalized to Self::Full below, so this path
            // always uses CollisionUtil's 1e-7 overlap margin.
            Self::StaticBoxes(boxes) => boxes
                .iter()
                .copied()
                .map(static_box_to_spawn_shape)
                .any(|shape| shape.intersects(block_x, block_y, block_z, entity)),
            Self::Multi(shapes) => shapes
                .into_iter()
                .any(|shape| shape.intersects(block_x, block_y, block_z, entity)),
        }
    }
}

fn static_box_to_spawn_shape(shape: StaticCollisionBox) -> SpawnShapeBox {
    let unit = |value: i8| f64::from(value) / 32.0;
    SpawnShapeBox::new(
        unit(shape.min_x),
        unit(shape.min_y),
        unit(shape.min_z),
        unit(shape.max_x),
        unit(shape.max_y),
        unit(shape.max_z),
    )
}

fn static_box_is_full(shape: StaticCollisionBox) -> bool {
    shape.min_x == 0
        && shape.min_y == 0
        && shape.min_z == 0
        && shape.max_x == 32
        && shape.max_y == 32
        && shape.max_z == 32
}

/// The collision boxes yielded by `BlockState.getCollisionShape` for the
/// worldgen states that can occur in the generated spawn workspace. Full cubes
/// use Paper's six-face collision sample; leaves/vegetation/fluids are empty;
/// partial snow/cactus shapes retain their VoxelShape bounds.
fn spawn_collision_shape(
    state: BlockState,
    pos: &BlockPos,
    context: SpawnCollisionContext<'_>,
) -> Option<SpawnCollisionShape> {
    let name = state.block().name();

    // Block-entity-backed moving pistons have no entity in this worldgen
    // region, and a missing shulker block entity uses ShulkerBoxBlock's
    // full-cube fallback. Powder snow is empty for CollisionContext.empty(),
    // but a walkable mob standing above it sees the full collision cube in
    // Mob.checkSpawnObstruction's entity context.
    let dynamic_shape = match name {
        "minecraft:powder_snow" => Some(match context {
            SpawnCollisionContext::Entity {
                entity_type,
                entity_min_y,
            } if matches!(entity_type, "minecraft:rabbit" | "minecraft:fox")
                && entity_min_y > pos.get_y() as f64 + 1.0 - 1.0e-5 =>
            {
                SpawnCollisionShape::Full
            }
            _ => SpawnCollisionShape::Empty,
        }),
        "minecraft:moving_piston" => Some(SpawnCollisionShape::Empty),
        name if name.ends_with("shulker_box") => Some(SpawnCollisionShape::Full),
        "minecraft:bamboo" => Some(SpawnCollisionShape::Box(
            SpawnShapeBox::new(6.5 / 16.0, 0.0, 6.5 / 16.0, 9.5 / 16.0, 1.0, 9.5 / 16.0)
                .translated(block_offset(pos).0, 0.0, block_offset(pos).1),
        )),
        "minecraft:scaffolding" => {
            let stable = [
                SpawnShapeBox::new(0.0, 14.0 / 16.0, 0.0, 1.0, 1.0, 1.0),
                SpawnShapeBox::new(0.0, 0.0, 0.0, 2.0 / 16.0, 1.0, 2.0 / 16.0),
                SpawnShapeBox::new(14.0 / 16.0, 0.0, 0.0, 1.0, 1.0, 2.0 / 16.0),
                SpawnShapeBox::new(0.0, 0.0, 14.0 / 16.0, 2.0 / 16.0, 1.0, 1.0),
                SpawnShapeBox::new(14.0 / 16.0, 0.0, 14.0 / 16.0, 1.0, 1.0, 1.0),
            ];
            let entity_shape = match context {
                SpawnCollisionContext::Empty => SpawnCollisionShape::Multi(stable),
                SpawnCollisionContext::Entity { entity_min_y, .. }
                    if entity_min_y > pos.get_y() as f64 + 1.0 - 1.0e-5 =>
                {
                    SpawnCollisionShape::Multi(stable)
                }
                SpawnCollisionContext::Entity { entity_min_y, .. }
                    if state.get_value(BlockStateProperties::STABILITY_DISTANCE).is_some_and(
                        |value| matches!(value, PropertyValue::Int(distance) if distance != 0),
                    ) && state
                        .get_value(BlockStateProperties::BOTTOM)
                        .is_some_and(|value| matches!(value, PropertyValue::Bool(true)))
                        // `SHAPE_BELOW_BLOCK` is two pixels tall; the
                        // entity context must be above that shape, not merely
                        // above the block's bottom face.
                        && entity_min_y > pos.get_y() as f64 + 2.0 / 16.0 - 1.0e-5 =>
                {
                    SpawnCollisionShape::Box(SpawnShapeBox::new(
                        0.0,
                        0.0,
                        0.0,
                        1.0,
                        2.0 / 16.0,
                        1.0,
                    ))
                }
                SpawnCollisionContext::Entity { .. } => SpawnCollisionShape::Empty,
            };
            Some(entity_shape)
        }
        "minecraft:pointed_dripstone" | "minecraft:sulfur_spike" => {
            Some(speleothem_collision_shape(state, pos)?)
        }
        _ if state.has_dynamic_shape() => None,
        _ => None,
    };
    if dynamic_shape.is_some() {
        return dynamic_shape;
    }
    if state.has_dynamic_shape() {
        return None;
    }
    // Every non-dynamic StateId is covered by the Paper-generated static
    // collision table. Unlike behavior words or face masks, this preserves
    // the complete union of boxes for stairs, fences, plates, and all other
    // partial shapes.
    if let Some(boxes) = static_collision_shape_of(state.id()) {
        return Some(if boxes.len() == 1 && static_box_is_full(boxes[0]) {
            SpawnCollisionShape::Full
        } else {
            SpawnCollisionShape::StaticBoxes(boxes)
        });
    }

    if state.is_air()
        || matches!(
            name,
            "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column"
        )
    {
        return Some(SpawnCollisionShape::Empty);
    }
    // Waterlogged solids retain their block collision shape in Paper; only
    // the standalone liquid blocks above are empty. Vegetation and other
    // no-collision blocks are the exact empty-shape cases used by generation.
    if matches!(
        name,
        "minecraft:short_grass"
            | "minecraft:fern"
            | "minecraft:dead_bush"
            | "minecraft:bush"
            | "minecraft:short_dry_grass"
            | "minecraft:tall_dry_grass"
            | "minecraft:seagrass"
            | "minecraft:tall_seagrass"
            | "minecraft:fire"
            | "minecraft:soul_fire"
            | "minecraft:vine"
            | "minecraft:glow_lichen"
            | "minecraft:tall_grass"
            | "minecraft:large_fern"
            | "minecraft:crimson_roots"
            | "minecraft:warped_roots"
            | "minecraft:nether_sprouts"
            | "minecraft:red_mushroom"
            | "minecraft:brown_mushroom"
    ) {
        return Some(SpawnCollisionShape::Empty);
    }
    // Paper's cached collision-shape face query is an exact full-face test.
    // Unlike the heightmap's blocksMotion bit, it correctly classifies leaves,
    // glass, and waterlogged full blocks as full collision cubes.
    if state.collision_face_mask() == 0x3F {
        return Some(SpawnCollisionShape::Full);
    }
    if name == "minecraft:snow" {
        let Some(PropertyValue::Int(layers)) = state.get_value(BlockStateProperties::LAYERS) else {
            return None;
        };
        let height = f64::from(layers.clamp(1, 8)) / 8.0;
        return Some(SpawnCollisionShape::Box(SpawnShapeBox::new(
            0.0, 0.0, 0.0, 1.0, height, 1.0,
        )));
    }
    if name == "minecraft:cactus" {
        return Some(SpawnCollisionShape::Box(SpawnShapeBox::new(
            1.0 / 16.0,
            0.0,
            1.0 / 16.0,
            15.0 / 16.0,
            1.0,
            15.0 / 16.0,
        )));
    }
    None
}

fn block_offset_with_max(pos: &BlockPos, max_horizontal_offset: f64) -> (f64, f64) {
    let seed = rivet_util::mth::get_seed(pos.get_x(), 0, pos.get_z());
    let x = (((seed & 15) as f32 / 15.0_f32) as f64 - 0.5) * 0.5;
    let z = ((((seed >> 8) & 15) as f32 / 15.0_f32) as f64 - 0.5) * 0.5;
    (
        x.clamp(-max_horizontal_offset, max_horizontal_offset),
        z.clamp(-max_horizontal_offset, max_horizontal_offset),
    )
}

fn block_offset(pos: &BlockPos) -> (f64, f64) {
    block_offset_with_max(pos, 0.25)
}

fn speleothem_offset(pos: &BlockPos) -> (f64, f64) {
    // Paper's SpeleothemBlock.MAX_HORIZONTAL_OFFSET is
    // SHAPE_BASE.min(X) = 2/16, not BlockBehaviour's default 1/4.
    block_offset_with_max(pos, 2.0 / 16.0)
}

fn speleothem_collision_shape(state: BlockState, pos: &BlockPos) -> Option<SpawnCollisionShape> {
    // Keep all six axes explicit. `Block.column(sizeXZ, minY, maxY)` uses the
    // same X/Z diameter, but spelling out both horizontal bounds prevents the
    // vertical max from being reused as a Z bound when this table changes.
    let (min_x, min_y, min_z, max_x, max_y, max_z) =
        match state.get_value(BlockStateProperties::SPELEOTHEM_THICKNESS) {
            Some(PropertyValue::Enum("tip_merge")) => {
                (5.0 / 16.0, 0.0, 5.0 / 16.0, 11.0 / 16.0, 1.0, 11.0 / 16.0)
            }
            Some(PropertyValue::Enum("tip")) => {
                match state.get_value(BlockStateProperties::VERTICAL_DIRECTION) {
                    Some(PropertyValue::Enum("up")) => (
                        5.0 / 16.0,
                        0.0,
                        5.0 / 16.0,
                        11.0 / 16.0,
                        11.0 / 16.0,
                        11.0 / 16.0,
                    ),
                    Some(PropertyValue::Enum("down")) => (
                        5.0 / 16.0,
                        5.0 / 16.0,
                        5.0 / 16.0,
                        11.0 / 16.0,
                        1.0,
                        11.0 / 16.0,
                    ),
                    _ => return None,
                }
            }
            Some(PropertyValue::Enum("frustum")) => {
                (4.0 / 16.0, 0.0, 4.0 / 16.0, 12.0 / 16.0, 1.0, 12.0 / 16.0)
            }
            Some(PropertyValue::Enum("middle")) => {
                (3.0 / 16.0, 0.0, 3.0 / 16.0, 13.0 / 16.0, 1.0, 13.0 / 16.0)
            }
            Some(PropertyValue::Enum("base")) => {
                (2.0 / 16.0, 0.0, 2.0 / 16.0, 14.0 / 16.0, 1.0, 14.0 / 16.0)
            }
            _ => return None,
        };
    let (offset_x, offset_z) = speleothem_offset(pos);
    Some(SpawnCollisionShape::Box(SpawnShapeBox::new(
        min_x + offset_x,
        min_y,
        min_z + offset_z,
        max_x + offset_x,
        max_y,
        max_z + offset_z,
    )))
}

#[derive(Clone, Copy)]
struct SpawnAabb {
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
}

impl SpawnAabb {
    fn new(min_x: f64, min_y: f64, min_z: f64, max_x: f64, max_y: f64, max_z: f64) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    fn intersects(self, other: &Self) -> bool {
        self.intersects_with_epsilon(other)
    }

    fn intersects_raw(self, other: &Self) -> bool {
        self.min_x < other.max_x
            && self.max_x > other.min_x
            && self.min_y < other.max_y
            && self.max_y > other.min_y
            && self.min_z < other.max_z
            && self.max_z > other.min_z
    }

    /// Paper's `CollisionUtil.voxelShapeIntersect`: touching or overlapping by
    /// at most `COLLISION_EPSILON` is not a collision for partial VoxelShapes.
    fn intersects_with_epsilon(self, other: &Self) -> bool {
        const COLLISION_EPSILON: f64 = 1.0e-7;
        self.min_x - other.max_x < -COLLISION_EPSILON
            && self.max_x - other.min_x > COLLISION_EPSILON
            && self.min_y - other.max_y < -COLLISION_EPSILON
            && self.max_y - other.min_y > COLLISION_EPSILON
            && self.min_z - other.max_z < -COLLISION_EPSILON
            && self.max_z - other.min_z > COLLISION_EPSILON
    }
}

fn spawn_aabb(x: f64, y: i32, z: f64, width: f64, height: f64) -> SpawnAabb {
    let half = width / 2.0;
    SpawnAabb::new(
        x - half,
        y as f64,
        z - half,
        x + half,
        y as f64 + height,
        z + half,
    )
}

fn collision_free(
    region: &WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    entity: SpawnAabb,
    context: SpawnCollisionContext<'_>,
) -> Option<bool> {
    // BlockCollisions scans one block beyond each AABB face. That extra ring is
    // observable for offset shapes such as bamboo and pointed dripstone, whose
    // VoxelShape may protrude into a neighbouring block.
    let min_x = rivet_util::mth::floor_d(entity.min_x - 1.0e-7).wrapping_sub(1);
    let max_x = rivet_util::mth::floor_d(entity.max_x + 1.0e-7).wrapping_add(1);
    let min_y = rivet_util::mth::floor_d(entity.min_y - 1.0e-7).wrapping_sub(1);
    let max_y = rivet_util::mth::floor_d(entity.max_y + 1.0e-7).wrapping_add(1);
    let min_z = rivet_util::mth::floor_d(entity.min_z - 1.0e-7).wrapping_sub(1);
    let max_z = rivet_util::mth::floor_d(entity.max_z + 1.0e-7).wrapping_add(1);
    for block_x in min_x..=max_x {
        for block_z in min_z..=max_z {
            for block_y in min_y..=max_y {
                let block_pos = BlockPos::new(block_x, block_y, block_z);
                let state = region.get_block_state(&block_pos);
                let shape = spawn_collision_shape(state, &block_pos, context)?;
                if shape.intersects(block_x, block_y, block_z, &entity) {
                    return Some(false);
                }
            }
        }
    }
    Some(true)
}

fn is_signal_source(state: BlockState) -> bool {
    // `BlockState.isSignalSource()` is a block behavior, not part of the
    // heightmap word. Keep the generated block-name identity list explicit so
    // rails/redstone controls are rejected before entity-layer construction.
    matches!(
        state.block().name(),
        "minecraft:acacia_button"
            | "minecraft:bamboo_button"
            | "minecraft:birch_button"
            | "minecraft:cherry_button"
            | "minecraft:crimson_button"
            | "minecraft:dark_oak_button"
            | "minecraft:jungle_button"
            | "minecraft:mangrove_button"
            | "minecraft:oak_button"
            | "minecraft:pale_oak_button"
            | "minecraft:polished_blackstone_button"
            | "minecraft:spruce_button"
            | "minecraft:stone_button"
            | "minecraft:warped_button"
            | "minecraft:bamboo_pressure_plate"
            | "minecraft:crimson_pressure_plate"
            | "minecraft:dark_oak_pressure_plate"
            | "minecraft:heavy_weighted_pressure_plate"
            | "minecraft:light_weighted_pressure_plate"
            | "minecraft:oak_pressure_plate"
            | "minecraft:polished_blackstone_pressure_plate"
            | "minecraft:spruce_pressure_plate"
            | "minecraft:stone_pressure_plate"
            | "minecraft:warped_pressure_plate"
            | "minecraft:acacia_pressure_plate"
            | "minecraft:birch_pressure_plate"
            | "minecraft:cherry_pressure_plate"
            | "minecraft:jungle_pressure_plate"
            | "minecraft:mangrove_pressure_plate"
            | "minecraft:pale_oak_pressure_plate"
            | "minecraft:daylight_detector"
            | "minecraft:detector_rail"
            | "minecraft:redstone_block"
            | "minecraft:jukebox"
            | "minecraft:lectern"
            | "minecraft:lever"
            | "minecraft:lightning_rod"
            | "minecraft:observer"
            | "minecraft:comparator"
            | "minecraft:repeater"
            | "minecraft:redstone_wire"
            | "minecraft:redstone_torch"
            | "minecraft:redstone_wall_torch"
            | "minecraft:sculk_sensor"
            | "minecraft:calibrated_sculk_sensor"
            | "minecraft:target"
            | "minecraft:trapped_chest"
            | "minecraft:tripwire_hook"
    )
}

fn is_dangerous_spawn_block(state: BlockState, entity_type: &str) -> bool {
    let name = state.block().name();
    // `EntityType.isBlockDangerous` first applies the type's immune-to tag.
    // The only generated CREATURE entries with a non-empty immunity tag are
    // foxes (sweet berry bushes) and polar bears (powder snow).
    let immune = match entity_type {
        "minecraft:fox" => state.is_in_tag("minecraft:fox_immune_to"),
        "minecraft:polar_bear" => state.is_in_tag("minecraft:polar_bear_immune_to"),
        _ => false,
    };
    !immune
        && (state.is_in_tag("minecraft:fire")
            || matches!(
                name,
                "minecraft:lava"
                    | "minecraft:magma_block"
                    | "minecraft:lava_cauldron"
                    | "minecraft:wither_rose"
                    | "minecraft:sweet_berry_bush"
                    | "minecraft:cactus"
                    | "minecraft:powder_snow"
            )
            || is_lit_campfire(state))
}

fn is_lit_campfire(state: BlockState) -> bool {
    matches!(
        state.block().name(),
        "minecraft:campfire" | "minecraft:soul_campfire"
    ) && matches!(
        state.get_value(BlockStateProperties::LIT),
        Some(PropertyValue::Bool(true))
    )
}

fn entity_spawn_floor(state: BlockState, entity_type: &str, biome_name: &str) -> bool {
    if entity_type == "minecraft:turtle" {
        return state.is_in_tag("minecraft:sand");
    }
    if entity_type == "minecraft:polar_bear"
        && matches!(
            biome_name,
            "minecraft:frozen_ocean" | "minecraft:deep_frozen_ocean"
        )
    {
        return state.is_in_tag("minecraft:polar_bears_spawnable_on_alternate");
    }
    let tag = match entity_type {
        "minecraft:armadillo" => "minecraft:armadillo_spawnable_on",
        "minecraft:camel" => "minecraft:camels_spawnable_on",
        "minecraft:fox" => "minecraft:foxes_spawnable_on",
        "minecraft:frog" => "minecraft:frogs_spawnable_on",
        "minecraft:goat" => "minecraft:goats_spawnable_on",
        "minecraft:mooshroom" => "minecraft:mooshrooms_spawnable_on",
        "minecraft:parrot" => "minecraft:parrots_spawnable_on",
        "minecraft:rabbit" => "minecraft:rabbits_spawnable_on",
        "minecraft:wolf" => "minecraft:wolves_spawnable_on",
        _ => "minecraft:animals_spawnable_on",
    };
    state.is_in_tag(tag)
}

/// `EntityType.getSpawnAABB` dimensions for every current CREATURE entry.
/// The generated entity registry is not landed yet, but these are the exact
/// Paper 26.2 base dimensions used before construction (scale defaults to 1).
/// They remain `f32` because Paper stores dimensions as Java floats and only
/// promotes them when constructing the double-precision AABB.
fn spawn_dimensions(entity_type: &str) -> (f32, f32) {
    match entity_type {
        "minecraft:armadillo" => (0.7, 0.65),
        "minecraft:camel" => (1.7, 2.375),
        "minecraft:chicken" => (0.4, 0.7),
        "minecraft:cow" | "minecraft:mooshroom" => (0.9, 1.4),
        "minecraft:ocelot" => (0.6, 0.7),
        "minecraft:sheep" => (0.9, 1.3),
        "minecraft:donkey" => (1.3964844, 1.5),
        "minecraft:fox" => (0.6, 0.7),
        "minecraft:frog" => (0.5, 0.5),
        "minecraft:goat" => (0.9, 1.3),
        "minecraft:horse" => (1.3964844, 1.6),
        "minecraft:llama" => (0.9, 1.87),
        "minecraft:panda" => (1.3, 1.25),
        "minecraft:parrot" => (0.5, 0.9),
        "minecraft:pig" => (0.9, 0.9),
        "minecraft:polar_bear" => (1.4, 1.4),
        "minecraft:rabbit" => (0.49, 0.6),
        "minecraft:turtle" => (1.2, 0.4),
        "minecraft:wolf" => (0.6, 0.85),
        // Generated data is closed over the entries above. Keep an explicit
        // vanilla fallback for a future table addition; it remains a normal
        // collision gate and cannot turn an ordinary rejection into an entity
        // capability error.
        _ => (0.9, 0.9),
    }
}

fn no_collision(
    region: &WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    x: f64,
    y: i32,
    z: f64,
    width: f64,
    height: f64,
) -> bool {
    // NaturalSpawner calls the AABB-only overload, which uses
    // CollisionContext.empty(), not the candidate entity's context.
    collision_free(
        region,
        spawn_aabb(x, y, z, width, height),
        SpawnCollisionContext::Empty,
    )
    .unwrap_or(false)
}

/// The post-construction `Mob.checkSpawnObstruction` gate available without an
/// entity instance. Paper's `isUnobstructed(entity)` path in `WorldGenRegion`
/// checks hard-colliding entities, not block VoxelShapes; the initial
/// NaturalSpawner `noCollision(AABB)` call is the sole block-shape query.
fn spawn_obstruction_ok(
    region: &WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    entity_type: &str,
    x: f64,
    y: i32,
    z: f64,
    width: f64,
    height: f64,
) -> bool {
    let entity = spawn_aabb(x, y, z, width, height);
    let min_x = rivet_util::mth::floor_d(entity.min_x);
    let max_x = rivet_util::mth::floor_d(entity.max_x - f64::EPSILON);
    let min_y = rivet_util::mth::floor_d(entity.min_y);
    let max_y = rivet_util::mth::floor_d(entity.max_y - f64::EPSILON);
    let min_z = rivet_util::mth::floor_d(entity.min_z);
    let max_z = rivet_util::mth::floor_d(entity.max_z - f64::EPSILON);
    for block_x in min_x..=max_x {
        for block_z in min_z..=max_z {
            for block_y in min_y..=max_y {
                if !region
                    .get_block_state(&BlockPos::new(block_x, block_y, block_z))
                    .fluid_empty()
                {
                    return false;
                }
            }
        }
    }
    if entity_type == "minecraft:ocelot" {
        // Ocelot overrides Mob.checkSpawnObstruction: after the generic
        // no-liquid/unobstructed test it requires sea-level height and a grass
        // or leaves support block. This is deliberately post-construction,
        // after its placement predicate's 2/3 random roll.
        if y < region.get_sea_level() {
            return false;
        }
        let below = region.get_block_state(&BlockPos::containing(x, y as f64, z).below());
        if below.block().name() != "minecraft:grass_block" && !below.is_in_tag("minecraft:leaves") {
            return false;
        }
    }
    true
}

/// The generation-time brightness predicate used by the animal spawn-rule
/// methods. `WorldGenRegion` reads the visible block/sky nibbles published by
/// the completed LIGHT step. Missing light correctness, sky emptiness data, or
/// malformed nibble storage fails closed rather than deriving a substitute from
/// static block opacity or the candidate block's own emission.
fn is_bright_enough_to_spawn(
    region: &WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    pos: &BlockPos,
) -> bool {
    region
        .get_raw_brightness(pos)
        .is_some_and(|brightness| brightness > 8)
}

fn check_spawn_rules(
    region: &WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    entity_type: &str,
    biome_name: &str,
    pos: &BlockPos,
    random: &mut impl RandomSource,
) -> bool {
    // `SpawnPlacements.checkSpawnRules` delegates to each registered static
    // predicate. Ocelot's predicate is intentionally only its 2/3 random roll;
    // it does not inherit Animal's brightness or floor test.
    if entity_type == "minecraft:ocelot" {
        return random.next_int_bound(3) != 0;
    }
    // All remaining generated CREATURE entries use an animal-family rule for
    // CHUNK_GENERATION, so the completed-light brightness requirement applies.
    if !is_bright_enough_to_spawn(region, pos) {
        return false;
    }
    if entity_type == "minecraft:turtle" && pos.get_y() >= region.get_sea_level().wrapping_add(4) {
        return false;
    }
    // PolarBear::checkPolarBearSpawnRules reads the biome at the candidate,
    // not the max-height biome used to select the chunk's CREATURE table.
    // Resolve that live cached biome for the special alternate floor rule;
    // other creature predicates are independent of biome here.
    let candidate_biome_name = if entity_type == "minecraft:polar_bear" {
        let candidate_biome = WorldGenLevel::get_biome(region, pos);
        BIOME_BY_ID
            .get(dense_biome_id(&candidate_biome) as usize)
            .copied()
            .unwrap_or(biome_name)
    } else {
        biome_name
    };
    entity_spawn_floor(
        region.get_block_state(&pos.below()),
        entity_type,
        candidate_biome_name,
    )
}

impl fmt::Debug for GenerationChunkHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenerationChunkHolder")
            .field("pos", &self.chunk.get_pos())
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::level_chunk::{
        StateId as ServerStateId, container_factory, state_flags,
    };
    use crate::server::level::server_level::{ServerLevel, ServerLevelConfig};
    use crate::server::lighting::LightChunk;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::list_tag::ListTag;
    use rivet_nbt::tag::Tag;
    use rivet_registry::fluid_id::FluidId;
    use rivet_registry::generated::block_states::StateId;
    use rivet_util::RandomSource;
    use rivet_world::level::WorldGenLevel;
    use rivet_world::levelgen::feature::FeatureBehavior;
    use rivet_world::levelgen::feature::configurations::HugeMushroomFeatureConfiguration;
    use rivet_world::levelgen::feature::configurations::MultifaceGrowthConfiguration;
    use rivet_world::levelgen::feature::configurations::ProbabilityFeatureConfiguration;
    use rivet_world::levelgen::feature::configurations::UnderwaterMagmaConfiguration;
    use rivet_world::levelgen::feature::configurations::disk_configuration::DiskConfiguration;
    use rivet_world::levelgen::feature::configurations::geode_configuration::GeodeConfiguration;
    use rivet_world::levelgen::feature::configurations::ore_configuration::OreConfiguration;
    use rivet_world::levelgen::feature::configurations::spring_configuration::SpringConfiguration;
    use rivet_world::levelgen::feature::monster_room_feature::MONSTER_ROOM;
    use rivet_world::levelgen::heightmap::Types;
    use rivet_world::lighting::swmr_nibble_array::{ARRAY_SIZE, InitState, SwmrNibbleArray};
    use rivet_world::ticks::ScheduledTick;

    /// The shared test realization (built once — the worldgen registry
    /// bootstrap is not free). The seed mirrors the pinned loaded-world corpus.
    fn test_generator() -> Arc<OverworldGenerator> {
        static GENERATOR: std::sync::LazyLock<Arc<OverworldGenerator>> =
            std::sync::LazyLock::new(|| Arc::new(OverworldGenerator::new(42)));
        GENERATOR.clone()
    }

    #[test]
    fn vanilla_structure_feature_index_matches_paper_registry_steps() {
        let index = StructureFeatureIndex::vanilla_registry();
        assert_eq!(index.total(), 34);
        assert_eq!(index.count_for_step(Decoration::UndergroundStructures), 5);
        assert_eq!(index.count_for_step(Decoration::SurfaceStructures), 26);
        assert_eq!(index.count_for_step(Decoration::UndergroundDecoration), 3);
        assert_eq!(index.count_for_step_index(11), 0);
    }

    fn feature_holder(generator: &Arc<OverworldGenerator>, pos: ChunkPos) -> GenerationChunkHolder {
        generator.create_holder_with_workspace_and_structure_feature_index(
            pos,
            FeatureWorkspace::new(),
            Some(StructureFeatureIndex::explicit_count(0)),
        )
    }

    fn transaction_probe_holder(
        generator: &Arc<OverworldGenerator>,
        starting_status: ChunkStatus,
    ) -> GenerationChunkHolder {
        let mut chunk = fresh_worldgen_chunk(ChunkPos::ZERO, generator);
        chunk.set_persisted_status(starting_status);
        let context = WorldGenContext::new(
            |_| {},
            |_| {},
            |_| {},
            |_| {},
            |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
                Err(GenError::FeaturePlacementDecode {
                    chunk_pos: ChunkPos::ZERO,
                    step_index: 9,
                    global_feature_index: 17,
                    feature_key: "minecraft:dark_forest_vegetation",
                })
            },
        );
        GenerationChunkHolder {
            chunk,
            context,
            features_failure: None,
            generator: Arc::clone(generator),
            feature_writebacks: Vec::new(),
            pending_feature_writebacks: Rc::new(RefCell::new(None)),
            feature_workspace: FeatureWorkspace::new(),
        }
    }

    /// A direct FEATURES seam probe that writes the east distance-1 dependency
    /// chunk, then either succeeds (publishing the writeback) or fails after
    /// priming the center and staging the writeback. This keeps ownership and
    /// rollback coverage independent of the seed-42 feature boundary.
    fn writeback_probe_holder(
        generator: &Arc<OverworldGenerator>,
        fail: bool,
    ) -> GenerationChunkHolder {
        let mut chunk = fresh_worldgen_chunk(ChunkPos::ZERO, generator);
        chunk.set_persisted_status(ChunkStatus::Carvers);
        let pending_feature_writebacks = Rc::new(RefCell::new(None));
        let writeback_sink = Rc::clone(&pending_feature_writebacks);
        let generator = Arc::clone(generator);
        let context_generator = Arc::clone(&generator);
        let context = WorldGenContext::new(
            |_| {},
            |_| {},
            |_| {},
            |_| {},
            move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                if fail {
                    chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
                }
                let mut region = compose_feature_region(chunk, &context_generator);
                assert!(region.set_block(
                    &BlockPos::new(16, 64, 0),
                    Blocks::STONE.default_block_state(),
                    2,
                    512,
                ));
                *writeback_sink.borrow_mut() = Some(region.into_distance_one_proto_writebacks());
                if fail {
                    Err(GenError::FeaturePlacementDecode {
                        chunk_pos: ChunkPos::ZERO,
                        step_index: 9,
                        global_feature_index: 17,
                        feature_key: "minecraft:dark_forest_vegetation",
                    })
                } else {
                    Ok(())
                }
            },
        );
        GenerationChunkHolder {
            chunk,
            context,
            features_failure: None,
            generator,
            feature_writebacks: Vec::new(),
            pending_feature_writebacks,
            feature_workspace: FeatureWorkspace::new(),
        }
    }

    /// Build a successful FEATURES seam that mutates both the distance-one
    /// write zone and a farther cached dependency. The latter covers the
    /// owner-directed heightmap/post-processing/tick state that a temporary
    /// region must return to its workspace even though `setBlock` itself is
    /// gated to distance one.
    fn workspace_writeback_probe_holder(
        generator: &Arc<OverworldGenerator>,
        workspace: &FeatureWorkspace,
    ) -> GenerationChunkHolder {
        workspace_writeback_probe_holder_at(generator, workspace, ChunkPos::ZERO)
    }

    fn workspace_writeback_probe_holder_at(
        generator: &Arc<OverworldGenerator>,
        workspace: &FeatureWorkspace,
        center_pos: ChunkPos,
    ) -> GenerationChunkHolder {
        let mut chunk = fresh_worldgen_chunk(center_pos, generator);
        chunk.set_persisted_status(ChunkStatus::Carvers);
        let pending_feature_writebacks = Rc::new(RefCell::new(None));
        let generator_for_context = Arc::clone(generator);
        let workspace_for_context = workspace.clone();
        let context = WorldGenContext::new(
            |_| {},
            |_| {},
            |_| {},
            |_| {},
            move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                let center_pos = chunk.get_pos();
                let mut region = compose_feature_region_with_workspace(
                    chunk,
                    &generator_for_context,
                    Some(&workspace_for_context),
                );
                let east_block =
                    BlockPos::new(center_pos.x().wrapping_add(1) * 16, 64, center_pos.z() * 16);
                assert!(
                    region.set_block(&east_block, Blocks::STONE.default_block_state(), 2, 512,)
                );
                let far_block =
                    BlockPos::new(center_pos.x().wrapping_add(8) * 16, 64, center_pos.z() * 16);
                <WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> as WorldGenLevel>::schedule_tick(
                    &mut region,
                    &far_block,
                    FluidId(4),
                    0,
                );
                region.mark_pos_for_post_processing(&far_block);
                for (pos, dependency) in region.into_owned_proto_entries() {
                    workspace_for_context.insert(dependency);
                    assert!(
                        pos != center_pos,
                        "the center must remain owned by the generation holder"
                    );
                }
                Ok(())
            },
        );
        GenerationChunkHolder {
            chunk,
            context,
            features_failure: None,
            generator: Arc::clone(generator),
            feature_writebacks: Vec::new(),
            pending_feature_writebacks,
            feature_workspace: workspace.clone(),
        }
    }

    fn assert_generated_proto_equal(
        actual: &ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
        expected: &ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    ) {
        assert_eq!(actual.get_pos(), expected.get_pos());
        assert_eq!(
            actual.get_persisted_status(),
            expected.get_persisted_status()
        );
        let actual_heightmaps: Vec<Option<Vec<i64>>> = actual
            .heightmaps()
            .iter()
            .map(|heightmap| heightmap.as_ref().map(|map| map.get_raw_data().to_vec()))
            .collect();
        let expected_heightmaps: Vec<Option<Vec<i64>>> = expected
            .heightmaps()
            .iter()
            .map(|heightmap| heightmap.as_ref().map(|map| map.get_raw_data().to_vec()))
            .collect();
        assert_eq!(actual_heightmaps, expected_heightmaps);
        for y in actual.get_min_y()..actual.get_min_y() + actual.get_height() {
            for z in 0..16 {
                for x in 0..16 {
                    assert_eq!(
                        actual.get_block_state(x, y, z),
                        expected.get_block_state(x, y, z),
                        "block mismatch at ({x}, {y}, {z})"
                    );
                }
            }
        }
        for (actual_section, expected_section) in
            actual.get_sections().iter().zip(expected.get_sections())
        {
            assert_eq!(
                actual_section.non_empty_block_count(),
                expected_section.non_empty_block_count()
            );
            for y in 0..4 {
                for z in 0..4 {
                    for x in 0..4 {
                        assert_eq!(
                            actual_section.biomes().get(x, y, z),
                            expected_section.biomes().get(x, y, z),
                            "biome mismatch at section cell ({x}, {y}, {z})"
                        );
                    }
                }
            }
        }
        assert_eq!(actual.get_entities(), expected.get_entities());
        assert_eq!(
            actual.get_block_entity_nbts(),
            expected.get_block_entity_nbts()
        );
        assert_eq!(actual.get_post_processing(), expected.get_post_processing());
        assert_eq!(
            actual.get_block_ticks().scheduled_ticks(),
            expected.get_block_ticks().scheduled_ticks()
        );
        assert_eq!(
            actual.get_fluid_ticks().scheduled_ticks(),
            expected.get_fluid_ticks().scheduled_ticks()
        );
        assert_eq!(actual.get_all_starts(), expected.get_all_starts());
        assert_eq!(actual.get_all_references(), expected.get_all_references());
        assert_eq!(actual.base().is_unsaved(), expected.base().is_unsaved());
        assert_eq!(
            actual.get_carving_mask().map(|mask| mask.to_array()),
            expected.get_carving_mask().map(|mask| mask.to_array())
        );
    }

    fn runtime_state_flags_for_test(
        state: &ServerStateId,
    ) -> rivet_world::levelgen::heightmap::StateFlags {
        state_flags(*state)
    }

    fn full_light_workspace_for_test(center: ChunkPos) -> GeneratedLightWorkspace {
        let height_accessor = create_height_accessor(-64, 384);
        let mut chunks: GeneratedLightStorage = HashMap::new();
        for dz in -2i32..=2 {
            for dx in -2i32..=2 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let pos = ChunkPos::new(center.x().wrapping_add(dx), center.z().wrapping_add(dz));
                let mut chunk: LightChunk = rivet_world::chunk::chunk_access::ChunkAccess::new(
                    pos,
                    UpgradeData::empty(height_accessor.get_sections_count() as usize),
                    height_accessor,
                    &container_factory(),
                    0,
                    None,
                    &runtime_state_flags_for_test,
                );
                chunk.set_light_correct(true);
                chunk.set_sky_emptiness_map(Some(vec![true; 24]));
                chunks.insert((pos.x(), pos.z()), chunk);
            }
        }
        GeneratedLightWorkspace::new(height_accessor, true, false, center, &mut chunks)
            .expect("test FULL workspace has all radius-two neighbours")
    }

    /// The `ChunkGenerator` realization delegates to the noisegen shell's real
    /// bodies — the `.chunk.generator` reconciliation note's single source of
    /// truth, grounded in the overworld preset's geometry (min_y -64, height
    /// 384, sea level 63, per Paper's `OverworldOptions`).
    #[test]
    fn generator_delegates_to_the_shell_real_bodies() {
        let generator = test_generator();
        let shell = generator.generator();
        // The realization answers the abstract contract from the settings.
        assert_eq!(generator.get_min_y(), shell.get_min_y());
        assert_eq!(generator.get_gen_depth(), shell.get_gen_depth());
        assert_eq!(generator.get_sea_level(), shell.get_sea_level());
        // Paper overworld geometry.
        assert_eq!(generator.get_min_y(), -64);
        assert_eq!(generator.get_gen_depth(), 384);
        assert_eq!(generator.get_sea_level(), 63);
        // The seed the generator was realized for is carried.
        assert_eq!(generator.seed(), 42);
        // `get_first_free_height` delegates to the real `get_base_height`: a
        // real surface height (above the void) rather than a panic seam.
        let height_accessor =
            create_height_accessor(generator.get_min_y(), generator.get_gen_depth());
        let height = generator.get_first_free_height(
            0,
            0,
            Types::WorldSurfaceWg,
            &height_accessor,
            generator.random_state(),
        );
        assert!(
            height > generator.get_min_y(),
            "base height at (0,0) should be above the void, got {height}"
        );
    }

    /// The biome source resolves the overworld table over the realized climate
    /// sampler: deterministic, consistent between the two trait surfaces, and
    /// non-trivial (more than one biome over the broad sample grid).
    #[test]
    fn biome_source_resolves_over_the_realized_sampler() {
        let generator = test_generator();
        let source = generator.biome_source();
        // Deterministic for a fixed quart position (the `NoiseBiomeSource`
        // path — the trait's own sampler).
        assert_eq!(
            NoiseBiomeSource::get_noise_biome(source, 0, 0, 0),
            NoiseBiomeSource::get_noise_biome(source, 0, 0, 0)
        );
        // The `NoiseBiomeSource` path (internal sampler) agrees with the
        // explicit `BiomeResolver` path over the same sampler.
        let via_noise_source = NoiseBiomeSource::get_noise_biome(source, 0, 0, 0);
        let via_resolver = BiomeResolver::get_noise_biome(source, 0, 0, 0, source.sampler());
        assert_eq!(via_noise_source, via_resolver);
        // The overworld table is non-trivial over a broad grid.
        let mut seen = std::collections::HashSet::new();
        for qx in (0..128).step_by(8) {
            for qz in (0..128).step_by(8) {
                seen.insert(dense_biome_id(&NoiseBiomeSource::get_noise_biome(
                    source, qx, 0, qz,
                )));
            }
        }
        assert!(
            seen.len() >= 2,
            "overworld biome source should vary; got {seen:?}"
        );
    }

    /// The executor drives a fresh chunk EMPTY→BIOMES→NOISE and each body
    /// produces real data — the BIOMES body fills the biome container, the
    /// NOISE body writes terrain blocks and the WORLDGEN heightmaps. A second
    /// run to the same status is an idempotent no-op.
    #[test]
    fn generate_through_biomes_then_noise() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(1, -2));
        assert_eq!(holder.status(), ChunkStatus::Empty);

        // The BIOMES body ran: it must change the biome container away from
        // the empty default (a real overworld chunk can legitimately be a
        // single biome, so the honest check is that generation filled it).
        let mut before = std::collections::HashSet::new();
        let mut after = std::collections::HashSet::new();
        for qx in 0..4 {
            for qz in 0..4 {
                before.insert(holder.chunk.get_noise_biome(qx, 0, qz));
            }
        }
        holder
            .generate_through(ChunkStatus::Biomes)
            .expect("BIOMES");
        assert_eq!(holder.status(), ChunkStatus::Biomes);
        for qx in 0..4 {
            for qz in 0..4 {
                after.insert(holder.chunk.get_noise_biome(qx, 0, qz));
            }
        }
        assert!(
            before != after,
            "BIOMES must replace the empty biome container with resolved biomes; before={before:?} after={after:?}"
        );

        holder.generate_through(ChunkStatus::Noise).expect("NOISE");
        assert_eq!(holder.status(), ChunkStatus::Noise);
        // The NOISE body ran: a surface block was written above the void at a
        // surface height, and the WORLDGEN surface heightmap was primed (an
        // unprimed heightmap reads as `min_y - 1`).
        let min_y = holder.chunk.get_min_y();
        let world_surface = holder.chunk.heightmaps()[Types::WorldSurfaceWg as usize]
            .as_ref()
            .expect("fill_from_noise primes the WORLD_SURFACE_WG heightmap");
        let height = world_surface.get_height_at(0, 0, min_y);
        assert!(
            height > min_y,
            "NOISE should write terrain; world surface height at (0,0) = {height}"
        );
        let block = holder.chunk.get_block_state(0, height, 0);
        assert_ne!(
            block,
            Blocks::AIR.default_block_state(),
            "a surface block (not AIR) should sit at the surface height"
        );

        // Re-running to the same status is an idempotent no-op (the chunk is
        // already at NOISE).
        holder
            .generate_through(ChunkStatus::Noise)
            .expect("idempotent");
        assert_eq!(holder.status(), ChunkStatus::Noise);
    }

    /// The SURFACE rung runs the real `NoiseBasedChunkGenerator.buildSurface`
    /// over the NOISE output: the executor drives EMPTY→BIOMES→NOISE→SURFACE,
    /// stamps the chunk SURFACE, and the surface body replaced at least one
    /// NOISE-default cell with a biome surface material — the overworld surface
    /// rule's top band defaults to `grass_or_dirt_if_underwater` (never the
    /// stone `default_block`), so a land column's top cell must change. The
    /// worldgen surface heights are preserved: the surface write replaces
    /// non-air with non-air, so `WORLD_SURFACE_WG` never moves (Paper's
    /// `buildSurface` writes through `ChunkAccess::setBlockState`, which keeps
    /// the heightmap). Re-running to SURFACE is an idempotent no-op.
    #[test]
    fn generate_through_biomes_then_noise_then_surface() {
        fn surface_height(chunk: &GenerationChunkHolder, x: i32, z: i32, min_y: i32) -> i32 {
            chunk.chunk.heightmaps()[Types::WorldSurfaceWg as usize]
                .as_ref()
                .expect("WORLD_SURFACE_WG primed")
                .get_height_at(x, z, min_y)
        }

        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(1, -2));
        assert_eq!(holder.status(), ChunkStatus::Empty);

        holder.generate_through(ChunkStatus::Noise).expect("NOISE");
        assert_eq!(holder.status(), ChunkStatus::Noise);
        let min_y = holder.chunk.get_min_y();

        // Snapshot, per column, the worldgen surface height and the 16 cells
        // below it — deep enough to hold the overworld top material plus the
        // band depth.
        let mut before_heights = Vec::with_capacity(256);
        let mut before_band: Vec<Vec<BlockState>> = Vec::with_capacity(256);
        for x in 0..16i32 {
            for z in 0..16i32 {
                let h = surface_height(&holder, x, z, min_y);
                before_heights.push(h);
                before_band.push(
                    (h - 16..=h)
                        .map(|y| holder.chunk.get_block_state(x, y, z))
                        .collect(),
                );
            }
        }

        holder
            .generate_through(ChunkStatus::Surface)
            .expect("SURFACE");
        assert_eq!(holder.status(), ChunkStatus::Surface);

        // The surface body ran: at least one column's surface band changed away
        // from the NOISE stone default (the overworld top band defaults to
        // grass/dirt, never stone). A cell counts only if the surface height is
        // stable, so a height-only change cannot satisfy this.
        let mut any_changed = false;
        let mut after_heights = Vec::with_capacity(256);
        for x in 0..16i32 {
            for z in 0..16i32 {
                let h = surface_height(&holder, x, z, min_y);
                after_heights.push(h);
                let index = x as usize * 16 + z as usize;
                if h == before_heights[index] {
                    let band = &before_band[index];
                    if band.iter().enumerate().any(|(i, before)| {
                        let y = h - 16 + i as i32;
                        *before != holder.chunk.get_block_state(x, y, z)
                    }) {
                        any_changed = true;
                    }
                }
            }
        }
        assert!(
            any_changed,
            "SURFACE must replace at least one NOISE-default cell with a surface material"
        );
        // The worldgen surface heights are stable: the surface write replaced
        // non-air with non-air, so WORLD_SURFACE_WG never moves.
        assert_eq!(after_heights, before_heights);

        // Re-running to the same status is an idempotent no-op.
        holder
            .generate_through(ChunkStatus::Surface)
            .expect("idempotent");
        assert_eq!(holder.status(), ChunkStatus::Surface);
    }

    /// The CARVERS rung runs the real `NoiseBasedChunkGenerator.applyCarvers`
    /// over the SURFACE output: the executor drives EMPTY→BIOMES→NOISE→SURFACE→
    /// CARVERS, stamps the chunk CARVERS, and the carvers body actually carved —
    /// the carving mask is written back (only `applyCarvers` writes it) and is
    /// non-empty, and at least one carved cell differs from the SURFACE snapshot
    /// the carvers consumed (air/water carved through the SURFACE output, which
    /// is what the top-material binder feeds them).
    #[test]
    fn generate_through_carvers_runs_the_real_apply_carvers() {
        let generator = test_generator();
        let pos = ChunkPos::new(2, 3);
        let mut holder = generator.create_holder(pos);
        assert_eq!(holder.status(), ChunkStatus::Empty);

        holder
            .generate_through(ChunkStatus::Surface)
            .expect("SURFACE");
        assert_eq!(holder.status(), ChunkStatus::Surface);
        let min_y = holder.chunk.get_min_y();
        let height = holder.chunk.get_height();

        // Snapshot the full SURFACE output — the carvers consume this (the
        // top-material binder) and carve through it.
        let mut before = Vec::with_capacity((16 * 16 * height) as usize);
        for y in min_y..min_y + height {
            for z in 0..16i32 {
                for x in 0..16i32 {
                    before.push(holder.chunk.get_block_state(x, y, z));
                }
            }
        }
        let index =
            |x: i32, y: i32, z: i32| -> usize { ((y - min_y) * 16 * 16 + z * 16 + x) as usize };

        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        assert_eq!(holder.status(), ChunkStatus::Carvers);

        // The real driver ran: only `applyCarvers` writes the carving mask, and
        // it came back present with carved cells — a fresh EMPTY chunk has no
        // mask, and no other step touches it.
        let mask = holder
            .chunk
            .get_carving_mask()
            .expect("applyCarvers must write the carving mask");
        let carved: Vec<BlockPos> = mask.stream(&pos).collect();
        assert!(
            !carved.is_empty(),
            "the overworld carvers must carve at least one cell at {pos:?}"
        );

        // And the carve wrote blocks: each mask bit is a cell the driver
        // carved, so at least one such cell must differ from the SURFACE output
        // it consumed (air/water through the surface material).
        let mut any_carved_cell_changed = false;
        for block in &carved {
            let x = block.get_x();
            let z = block.get_z();
            let y = block.get_y();
            if y >= min_y
                && y < min_y + height
                && before[index(x, y, z)] != holder.chunk.get_block_state(x, y, z)
            {
                any_carved_cell_changed = true;
            }
        }
        assert!(
            any_carved_cell_changed,
            "carving must write air/water through the SURFACE output at a carved cell"
        );

        // Re-running to the same status is an idempotent no-op — the already
        // stamped chunk is not carved twice.
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("idempotent");
        assert_eq!(holder.status(), ChunkStatus::Carvers);
    }

    /// The CARVERS step is deterministic: two independent chunks at the same
    /// position, driven through the same seed-42 generator, carve identically.
    /// This pins the RNG draw order — the shared `WorldgenRandom` is re-seeded
    /// per source chunk/carver with `setLargeFeatureSeed(seed + index, x, z)`
    /// (wrapping long arithmetic), so the same seed and position must reproduce
    /// the exact same carving mask and the exact same carved blocks.
    #[test]
    fn generate_through_carvers_is_deterministic() {
        let generator = test_generator();
        let pos = ChunkPos::new(-3, 5);
        let mut a = generator.create_holder(pos);
        let mut b = generator.create_holder(pos);

        a.generate_through(ChunkStatus::Carvers).expect("a CARVERS");
        b.generate_through(ChunkStatus::Carvers).expect("b CARVERS");

        // Same seed + position → identical carving mask (the bit pattern, not
        // just the stream). A fresh EMPTY chunk starts mask-less, so the mask
        // presence is part of the determinism contract.
        let mask_a = a
            .chunk
            .get_carving_mask()
            .expect("applyCarvers must write the carving mask");
        let mask_b = b
            .chunk
            .get_carving_mask()
            .expect("applyCarvers must write the carving mask");
        assert_eq!(
            mask_a.to_array(),
            mask_b.to_array(),
            "same seed + position must reproduce the identical carving mask"
        );
        assert!(
            !mask_a.to_array().is_empty(),
            "determinism test must be non-vacuous: the carvers carved at {pos:?}"
        );

        // And the carve wrote identical blocks: every carved cell's state
        // matches between the two runs.
        let min_y = a.chunk.get_min_y();
        let height = a.chunk.get_height();
        for block in mask_a.stream(&pos) {
            let x = block.get_x();
            let z = block.get_z();
            let y = block.get_y();
            if y >= min_y && y < min_y + height {
                assert_eq!(
                    a.chunk.get_block_state(x, y, z),
                    b.chunk.get_block_state(x, y, z),
                    "carved cell ({x}, {y}, {z}) must be deterministic"
                );
            }
        }
    }

    /// Hostile: the out-of-build-height read default is real `void_air` — raw
    /// id 794, default state 15292 — not AIR and not another block's default.
    /// The NOISE test reads at the surface height (inside build height), so it
    /// can never observe this default; this test pins the state-id contract
    /// directly, catching a wrong raw id (830 resolves to
    /// `minecraft:mud_brick_wall`'s default 18441) that the heightmap/terrain
    /// walks in `fill_from_noise` would silently feed back.
    #[test]
    fn get_block_state_outside_build_height_returns_void_air() {
        let generator = test_generator();
        let holder = generator.create_holder(ChunkPos::new(3, -1));
        // Build height is [min_y, min_y + height - 1]; one below and one above
        // are both outside it.
        let max_y = holder.chunk.get_min_y() + holder.chunk.get_height() - 1;
        for y in [holder.chunk.get_min_y() - 1, max_y + 1] {
            let state = holder.chunk.get_block_state(0, y, 0);
            assert_eq!(
                state,
                BlockState::of(BlockId(794)),
                "out-of-build-height read at y={y} must be the void_air default state"
            );
            assert_eq!(state.id(), StateId(15292));
            assert_ne!(state, Blocks::AIR.default_block_state());
            assert_ne!(
                state.id(),
                StateId(18441),
                "must not resolve to minecraft:mud_brick_wall's default"
            );
        }
    }

    #[test]
    fn feature_region_outside_build_height_reads_void_air() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let min_y = holder.chunk.get_min_y();
        let region = compose_feature_region(&mut holder.chunk, &generator);
        assert_eq!(
            region.get_block_state(&BlockPos::new(0, min_y - 1, 0)),
            BlockState::of(BlockId(794)),
        );
    }

    /// Hostile: stages the holder cannot complete are refused before any work
    /// runs, with a typed error, and the chunk is never stamped past the
    /// supported rung — fresh, and again after a successful NOISE.
    ///
    /// The value-layer boundary is `LIGHT`: the INITIALIZE_LIGHT/LIGHT steps are
    /// wired (`WorldGenContext::generate_through`, engine-gated) but the holder
    /// wires no light engine, so a fresh EMPTY chunk targeting either is
    /// refused as `GenError::LightEngineMissing` before any work runs (the
    /// chunk stays EMPTY). A target at FULL stops at the consuming promotion
    /// boundary (`UnsupportedStatus`). CARVERS itself is wired (the real
    /// `NoiseBasedChunkGenerator.applyCarvers`, see
    /// `generate_through_carvers_runs_the_real_apply_carvers`), so a fresh
    /// EMPTY chunk targeting it runs BIOMES→NOISE→SURFACE→CARVERS and is
    /// stamped CARVERS. FEATURES is wired-but-blocked (see
    /// `generate_through_features_runs_prologue_then_fails_typed`): the
    /// features body primes the final heightmaps, runs the full 17x17
    /// dependency window and 3x3 biome union, decodes and runs the lake, geode,
    /// and monster-room paths, and fails typed at the first selected unsupported
    /// path, so the chunk is never stamped FEATURES.
    #[test]
    fn downstream_stages_fail_loudly_and_never_stamp() {
        let generator = test_generator();
        let mut fresh = generator.create_holder(ChunkPos::ZERO);
        // INITIALIZE_LIGHT..LIGHT: the path (through the wired FEATURES step)
        // needs a light engine, and the holder wires none, so the whole path is
        // refused before any work runs. The chunk is untouched.
        for status in [ChunkStatus::InitializeLight, ChunkStatus::Light] {
            assert!(
                matches!(
                    fresh.generate_through(status),
                    Err(GeneratedChunkError::Generation(
                        GenError::LightEngineMissing { .. }
                    ))
                ),
                "target {status:?} must be rejected as LightEngineMissing (no light engine)"
            );
            assert_eq!(fresh.status(), ChunkStatus::Empty);
        }
        // The center-only holder intentionally has no SPAWN seam: the later
        // scheduler/G4 owner must supply the radius-one workspace. A fresh
        // request reaches the earlier light prerequisite first, so it is
        // rejected as LightEngineMissing before the missing seam is observed.
        // FULL is a separate consuming promotion boundary, so it is rejected as
        // UnsupportedStatus.
        for status in [ChunkStatus::Spawn, ChunkStatus::Full] {
            let result = fresh.generate_through(status);
            assert!(
                matches!(
                    &result,
                    Err(GeneratedChunkError::Generation(
                        GenError::LightEngineMissing { .. }
                    ))
                ) || matches!(
                    &result,
                    Err(GeneratedChunkError::UnsupportedStatus(s)) if *s == status
                ),
                "target {status:?} must be rejected by the path's unlit/unsupported boundary"
            );
            assert_eq!(fresh.status(), ChunkStatus::Empty);
        }

        // CARVERS is wired: a fresh EMPTY chunk targeting it runs the real
        // carvers body and is stamped CARVERS (see
        // `generate_through_carvers_runs_the_real_apply_carvers`).
        let mut carvers = generator.create_holder(ChunkPos::new(0, 1));
        carvers
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        assert_eq!(carvers.status(), ChunkStatus::Carvers);
        // From CARVERS, INITIALIZE_LIGHT is refused as LightEngineMissing
        // before any work, and the persisted status stays CARVERS — never a
        // silent stamp past it.
        let err = carvers
            .generate_through(ChunkStatus::InitializeLight)
            .unwrap_err();
        assert!(matches!(
            err,
            GeneratedChunkError::Generation(GenError::LightEngineMissing { .. })
        ));
        assert_eq!(carvers.status(), ChunkStatus::Carvers);

        // After a real NOISE, requesting a downstream stage still fails loudly
        // and the persisted status stays NOISE — never a silent stamp to FULL.
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder.generate_through(ChunkStatus::Noise).expect("NOISE");
        let err = holder.generate_through(ChunkStatus::Full).unwrap_err();
        assert!(matches!(
            err,
            GeneratedChunkError::UnsupportedStatus(ChunkStatus::Full)
        ));
        assert_eq!(holder.status(), ChunkStatus::Noise);
    }

    /// The center-only holder has no fake SPAWN seam: ordinary status-driven
    /// promotion stops at the exact workspace/scheduler attachment boundary.
    /// The public `generate_spawn_with_region` path below is the usable SPAWN
    /// foundation; the later scheduler/G4 owner must supply its radius-one
    /// `SpawnRegionProtos` workspace before invoking it.
    #[test]
    fn center_only_holder_spawn_refuses_without_workspace_scheduler() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder.chunk.set_persisted_status(ChunkStatus::Light);
        let err = holder
            .generate_through(ChunkStatus::Spawn)
            .expect_err("center-only SPAWN cannot fabricate a radius-one region");
        assert!(matches!(
            err,
            GeneratedChunkError::UnsupportedStatus(ChunkStatus::Spawn)
        ));
        assert_eq!(holder.status(), ChunkStatus::Light);
    }

    /// A whole-path LIGHT refusal happens during prevalidation, before the
    /// FEATURES task is entered. It must not poison the holder's terminal
    /// FEATURES-failure cache: a later direct FEATURES request still runs the
    /// decoration body and reports its own typed boundary.
    #[test]
    fn light_refusal_does_not_cache_features_failure() {
        let generator = test_generator();
        let mut holder = feature_holder(&generator, ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");

        let light_error = holder
            .generate_through(ChunkStatus::Light)
            .expect_err("the holder has no usable light engine");
        assert!(matches!(
            light_error,
            GeneratedChunkError::Generation(GenError::LightEngineMissing { .. })
        ));
        assert_eq!(holder.status(), ChunkStatus::Carvers);

        let features_error = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("FEATURES must run after the earlier LIGHT refusal");
        assert!(matches!(
            features_error,
            GeneratedChunkError::Generation(
                GenError::FeaturePlacementDecode { .. }
                    | GenError::SettingsNotGenerated { .. }
                    | GenError::StructureDecorationIndexUnavailable { .. }
            )
        ));
        assert_eq!(holder.status(), ChunkStatus::Carvers);
    }

    /// Without a structure-decoration authority, FEATURES refuses before
    /// heightmap priming, feature RNG, placement, or dependency writeback. A
    /// caller that has proved the capability must opt into the separate holder
    /// constructor used by the seed-42 execution fixtures.
    #[test]
    fn unavailable_structure_decoration_refuses_without_mutation() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        let expected = generator.create_holder(ChunkPos::ZERO);

        let error = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("missing structure decoration authority must refuse");
        assert!(matches!(
            error,
            GeneratedChunkError::Generation(
                GenError::StructureDecorationIndexUnavailable { chunk_pos }
            ) if chunk_pos == ChunkPos::ZERO
        ));
        assert_generated_proto_equal(&holder.chunk, &expected.chunk);
        assert_eq!(holder.status(), ChunkStatus::Empty);
        assert!(holder.take_feature_writebacks().is_empty());

        let retry = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("the unavailable structure boundary must cache");
        assert!(matches!(
            retry,
            GeneratedChunkError::Generation(GenError::StructureDecorationIndexUnavailable { .. })
        ));
        assert_generated_proto_equal(&holder.chunk, &expected.chunk);
    }

    /// A typed FEATURES failure reached through a LIGHT target must restore
    /// every pre-FEATURES field, not merely the persisted status. This is the
    /// counterfactual that used to bypass the direct-FEATURES snapshot.
    #[test]
    fn light_target_features_failure_rolls_back_the_complete_proto() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder.attach_generated_light_workspace(full_light_workspace_for_test(ChunkPos::ZERO));

        let err = holder
            .generate_through(ChunkStatus::Light)
            .expect_err("the FEATURES boundary must be reached before LIGHT");
        assert!(matches!(
            err,
            GeneratedChunkError::Generation(GenError::FeaturePlacementDecode { .. })
                | GeneratedChunkError::Generation(GenError::SettingsNotGenerated { .. })
                | GeneratedChunkError::Generation(
                    GenError::StructureDecorationIndexUnavailable { .. }
                )
        ));
        assert_eq!(holder.status(), ChunkStatus::Empty);
        assert!(
            holder
                .chunk
                .get_sections()
                .iter()
                .all(|section| section.has_only_air())
        );
        assert!(holder.chunk.heightmaps().iter().all(Option::is_none));
        assert!(holder.chunk.get_post_processing().iter().all(Vec::is_empty));
        assert!(holder.chunk.get_entities().is_empty());
        assert!(holder.chunk.get_block_entity_nbts().is_empty());
        assert!(holder.chunk.get_block_ticks().scheduled_ticks().is_empty());
        assert!(holder.chunk.get_fluid_ticks().scheduled_ticks().is_empty());
        assert!(holder.chunk.get_all_starts().is_empty());
        assert!(holder.chunk.get_all_references().is_empty());
        assert!(!holder.chunk.base().is_unsaved());
        assert!(holder.features_failure.is_some());
    }

    /// SPAWN has the same FEATURES prefix as LIGHT, but the shared-SPAWN
    /// semantics on this executor keep a center-only `generate_through(SPAWN)`
    /// at its typed seam refusal: Paper's `generateSpawn` needs the
    /// scheduler-owned radius-one cache, so the region API
    /// ([`GenerationChunkHolder::generate_spawn_with_region`]) is the only SPAWN
    /// entry and no borrowed run reaches the decoration boundary here. The
    /// refusal is decided in preflight — nothing is mutated and the terminal
    /// FEATURES-failure cache stays untouched.
    #[test]
    fn spawn_target_preflight_refusal_preserves_the_complete_proto() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 1));
        holder.attach_generated_light_workspace(full_light_workspace_for_test(ChunkPos::new(0, 1)));

        let err = holder
            .generate_through(ChunkStatus::Spawn)
            .expect_err("the FEATURES boundary must be reached before SPAWN");
        assert!(
            matches!(
                err,
                GeneratedChunkError::UnsupportedStatus(ChunkStatus::Spawn)
            ),
            "center-only SPAWN must refuse at the region-seam boundary: {err:?}"
        );
        assert_eq!(holder.status(), ChunkStatus::Empty);
        assert!(
            holder
                .chunk
                .get_sections()
                .iter()
                .all(|section| section.has_only_air())
        );
        assert!(holder.chunk.heightmaps().iter().all(Option::is_none));
        assert!(holder.chunk.get_post_processing().iter().all(Vec::is_empty));
        assert!(holder.chunk.get_entities().is_empty());
        assert!(holder.chunk.get_block_entity_nbts().is_empty());
        assert!(holder.chunk.get_block_ticks().scheduled_ticks().is_empty());
        assert!(holder.chunk.get_fluid_ticks().scheduled_ticks().is_empty());
        assert!(holder.chunk.get_all_starts().is_empty());
        assert!(holder.chunk.get_all_references().is_empty());
        assert!(!holder.chunk.base().is_unsaved());
        // The seam refusal precedes every FEATURES entry, so no terminal
        // boundary was cached.
        assert!(holder.features_failure.is_none());
    }

    /// A panic from the caller-owned FEATURES seam must use the same rollback
    /// boundary as a typed feature error, then resume the original payload.
    #[test]
    fn features_panic_rolls_back_the_complete_proto_before_resuming() {
        let generator = test_generator();
        let context = WorldGenContext::new(
            |_chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {},
            |_chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {},
            |_chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {},
            |_chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {},
            |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                chunk.set_block_state(0, chunk.get_min_y(), 0, BlockState::of(BlockId(1)));
                panic!("test FEATURES panic");
            },
        );
        let mut holder = GenerationChunkHolder {
            chunk: fresh_worldgen_chunk(ChunkPos::ZERO, &generator),
            context,
            features_failure: None,
            generator,
            feature_writebacks: Vec::new(),
            pending_feature_writebacks: Rc::new(RefCell::new(None)),
            feature_workspace: FeatureWorkspace::new(),
        };

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            holder
                .generate_through(ChunkStatus::Features)
                .expect("the FEATURES panic must resume");
        }));
        assert!(panic.is_err());
        assert_eq!(holder.status(), ChunkStatus::Empty);
        assert_eq!(
            holder.chunk.get_block_state(0, holder.chunk.get_min_y(), 0),
            Blocks::AIR.default_block_state()
        );
        assert!(holder.chunk.heightmaps().iter().all(Option::is_none));
        assert!(holder.chunk.get_post_processing().iter().all(Vec::is_empty));
        assert!(holder.features_failure.is_none());
    }

    /// The FEATURES rung runs `addVanillaDecorations`'s full prologue and
    /// dependency-window cache — `Heightmap.primeHeightmaps(chunk,
    /// FINAL_HEIGHTMAPS)` (the `ChunkStatusTasks.generateFeatures` priming),
    /// the decoration-seed derivation (`SectionPos.of(centerPos,
    /// level.getMinSectionY()).origin()` fed to `setDecorationSeed(seed,
    /// originX, originZ)`), the 17x17 `WorldGenRegion` cache (borrowed center,
    /// CARVERS at distances 0/1, and STRUCTURE_STARTS through distance 8), the
    /// Paper-order 3x3 biome-union gather + `retainAll`, and then resolves
    /// generation settings for the FULL `biomeSource.possibleBiomes()` list in
    /// source order (the exact argument Paper's `ChunkGenerator.featuresPerStep`
    /// memoizes, `ChunkGenerator.java` 97-100). Every possible biome (55) now
    /// resolves, so the full-list `FeatureSorter` is built, the per-step loop
    /// maps the 3x3 union through it, and runs the registry-backed lake, geode,
    /// and monster-room paths at their exact feature seeds before the first
    /// unsupported selected path stops the slice.
    /// For seed 42 chunk (0,0), the lakes and amethyst rarity filters drop;
    /// `minecraft:amethyst_geode` and `minecraft:monster_room` execute through
    /// their registry-backed leaves, and the Batch 2/3 ore/disk/spring/block
    /// and underwater_magma decode arms advance the run through the full
    /// UNDERGROUND_ORES step — underwater_magma (global 26) now executes but
    /// places no magma in this dry origin union, so it consumes no placement
    /// RNG past its scan. Batch 4 then decodes and executes `glow_lichen` through
    /// `minecraft:multiface_growth`. The random-selector branch at global 17
    /// selects `minecraft:dark_oak_leaf_litter`; the next typed boundary is
    /// `minecraft:freeze_top_layer` at step 10/global index 0 when its
    /// biome freeze query reaches the unimplemented `shouldFreeze` seam. The chunk
    /// is never stamped FEATURES (it stays CARVERS).
    #[test]
    fn generate_through_features_rolls_back_fresh_holder_and_caches_failure() {
        let generator = test_generator();
        let mut holder = feature_holder(&generator, ChunkPos::new(0, 0));
        let fresh = generator.create_holder(ChunkPos::new(0, 0));
        assert_eq!(holder.status(), ChunkStatus::Empty);

        let err = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("FEATURES must stop at the first selected mismatch");
        match err {
            GeneratedChunkError::Generation(GenError::FeaturePlacementDecode {
                chunk_pos,
                step_index,
                global_feature_index,
                feature_key,
            }) => {
                assert_eq!(chunk_pos, ChunkPos::new(0, 0));
                assert_eq!(step_index, 10);
                assert_eq!(global_feature_index, 0);
                assert_eq!(feature_key, "minecraft:freeze_top_layer");
            }
            other => {
                panic!(
                    "FEATURES must stop at the selected freeze_top_layer boundary; got {other:?}"
                )
            }
        }

        // The FEATURES prologue and earlier generation rungs may mutate every
        // part of the center proto before the typed placement boundary. A
        // failed transaction must restore the fresh EMPTY representation, not
        // merely leave the status below FEATURES.
        assert_eq!(holder.status(), fresh.status());
        assert_eq!(holder.status(), ChunkStatus::Empty);
        assert!(
            holder.take_feature_writebacks().is_empty(),
            "a failed FEATURES transaction must discard dependency writebacks"
        );
        let holder_heightmaps: Vec<Option<Vec<i64>>> = holder
            .chunk
            .heightmaps()
            .iter()
            .map(|heightmap| heightmap.as_ref().map(|map| map.get_raw_data().to_vec()))
            .collect();
        let fresh_heightmaps: Vec<Option<Vec<i64>>> = fresh
            .chunk
            .heightmaps()
            .iter()
            .map(|heightmap| heightmap.as_ref().map(|map| map.get_raw_data().to_vec()))
            .collect();
        assert_eq!(holder_heightmaps, fresh_heightmaps);
        for ty in FINAL_HEIGHTMAPS {
            assert!(
                holder.chunk.heightmaps()[ty as usize].is_none(),
                "a failed FEATURES transaction must roll back {ty:?}"
            );
        }

        let min_y = holder.chunk.get_min_y();
        let max_y = min_y + holder.chunk.get_height();
        for y in min_y..max_y {
            for z in 0..16 {
                for x in 0..16 {
                    assert_eq!(
                        holder.chunk.get_block_state(x, y, z),
                        fresh.chunk.get_block_state(x, y, z),
                        "a failed FEATURES transaction must roll back block ({x}, {y}, {z})"
                    );
                }
            }
        }
        for (holder_section, fresh_section) in holder
            .chunk
            .get_sections()
            .iter()
            .zip(fresh.chunk.get_sections())
        {
            assert_eq!(
                holder_section.non_empty_block_count(),
                fresh_section.non_empty_block_count(),
                "a failed FEATURES transaction must roll back section block counts"
            );
            for y in 0..4 {
                for z in 0..4 {
                    for x in 0..4 {
                        assert_eq!(
                            holder_section.biomes().get(x, y, z),
                            fresh_section.biomes().get(x, y, z),
                            "a failed FEATURES transaction must roll back biome writes"
                        );
                    }
                }
            }
        }
        assert_eq!(holder.chunk.get_entities(), fresh.chunk.get_entities());
        assert_eq!(
            holder.chunk.get_block_entity_nbts(),
            fresh.chunk.get_block_entity_nbts()
        );
        assert_eq!(
            holder.chunk.get_post_processing(),
            fresh.chunk.get_post_processing()
        );
        assert_eq!(
            holder.chunk.get_block_ticks().scheduled_ticks(),
            fresh.chunk.get_block_ticks().scheduled_ticks()
        );
        assert_eq!(
            holder.chunk.get_fluid_ticks().scheduled_ticks(),
            fresh.chunk.get_fluid_ticks().scheduled_ticks()
        );
        assert_eq!(holder.chunk.get_all_starts(), fresh.chunk.get_all_starts());
        assert_eq!(
            holder.chunk.get_all_references(),
            fresh.chunk.get_all_references()
        );
        assert_eq!(
            holder.chunk.base().is_unsaved(),
            fresh.chunk.base().is_unsaved()
        );
        assert_eq!(
            holder.chunk.get_carving_mask().map(|mask| mask.to_array()),
            fresh.chunk.get_carving_mask().map(|mask| mask.to_array())
        );

        // A retry is an idempotent terminal boundary: the holder returns the
        // cached typed failure instead of replaying partial feature placement.
        let retry = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("the cached FEATURES boundary must remain refused");
        assert!(matches!(
            retry,
            GeneratedChunkError::Generation(GenError::FeaturePlacementDecode {
                chunk_pos,
                step_index: 10,
                global_feature_index: 0,
                feature_key: "minecraft:freeze_top_layer",
            }) if chunk_pos == ChunkPos::ZERO
        ));
        assert_eq!(holder.status(), ChunkStatus::Empty);
        assert_eq!(holder.chunk.get_entities(), fresh.chunk.get_entities());
        assert_eq!(
            holder.chunk.get_block_entity_nbts(),
            fresh.chunk.get_block_entity_nbts()
        );
        assert!(
            holder
                .chunk
                .get_sections()
                .iter()
                .all(|section| section.has_only_air()),
            "cached retry must not replay partial feature block writes"
        );
    }

    #[test]
    fn features_transaction_restores_every_lower_start_status_and_retry() {
        let generator = test_generator();
        for starting_status in [
            ChunkStatus::Empty,
            ChunkStatus::Biomes,
            ChunkStatus::Noise,
            ChunkStatus::Surface,
            ChunkStatus::Carvers,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureReferences,
        ] {
            let expected = transaction_probe_holder(&generator, starting_status);
            let mut holder = transaction_probe_holder(&generator, starting_status);

            let error = holder
                .generate_through(ChunkStatus::Features)
                .expect_err("FEATURES must hit the pinned typed boundary");
            assert!(matches!(
                error,
                GeneratedChunkError::Generation(GenError::FeaturePlacementDecode { .. })
                    | GeneratedChunkError::Generation(GenError::SettingsNotGenerated { .. })
                    | GeneratedChunkError::Generation(
                        GenError::StructureDecorationIndexUnavailable { .. }
                    )
            ));
            assert_eq!(holder.status(), starting_status);
            assert_generated_proto_equal(&holder.chunk, &expected.chunk);
            assert!(
                holder.take_feature_writebacks().is_empty(),
                "failed FEATURES from {starting_status:?} must not publish writebacks"
            );

            let retry = holder
                .generate_through(ChunkStatus::Features)
                .expect_err("the failure must be cached for retry");
            assert!(matches!(
                retry,
                GeneratedChunkError::Generation(GenError::FeaturePlacementDecode { .. })
                    | GeneratedChunkError::Generation(GenError::SettingsNotGenerated { .. })
                    | GeneratedChunkError::Generation(
                        GenError::StructureDecorationIndexUnavailable { .. }
                    )
            ));
            assert_eq!(holder.status(), starting_status);
            assert_generated_proto_equal(&holder.chunk, &expected.chunk);
        }
    }

    #[test]
    fn successful_features_publish_distance_one_writebacks() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let mut holder = writeback_probe_holder(&generator, false);

        holder
            .generate_through(ChunkStatus::Features)
            .expect("the successful FEATURES seam must publish its writeback");
        assert_eq!(holder.status(), ChunkStatus::Features);
        let writebacks = holder.take_feature_writebacks();
        let positions: Vec<_> = writebacks.iter().map(|(pos, _)| *pos).collect();
        assert_eq!(
            positions,
            vec![
                ChunkPos::new(-1, -1),
                ChunkPos::new(-1, 0),
                ChunkPos::new(-1, 1),
                ChunkPos::new(0, -1),
                ChunkPos::new(0, 1),
                ChunkPos::new(1, -1),
                ChunkPos::new(1, 0),
                ChunkPos::new(1, 1),
            ],
            "successful FEATURES writebacks must preserve cache order"
        );
        let east = writebacks
            .into_iter()
            .find(|(pos, _)| *pos == ChunkPos::new(1, 0))
            .map(|(_, chunk)| chunk)
            .expect("east distance-1 writeback");
        assert_eq!(
            east.get_block_state(0, 64, 0),
            Blocks::STONE.default_block_state(),
            "the owner must receive the cross-chunk feature write"
        );
        assert!(
            holder.take_feature_writebacks().is_empty(),
            "writebacks are consumed exactly once"
        );
    }

    #[test]
    fn successful_features_persist_all_owned_dependency_mutations() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let workspace = FeatureWorkspace::new();
        let mut holder = workspace_writeback_probe_holder(&generator, &workspace);

        holder
            .generate_through(ChunkStatus::Features)
            .expect("the successful FEATURES seam must publish all owned dependencies");
        assert_eq!(holder.status(), ChunkStatus::Features);
        assert_eq!(workspace.len(), 17 * 17 - 1);

        workspace.with_chunk(ChunkPos::new(1, 0), |chunk| {
            let chunk = chunk.expect("the distance-one owner must be retained");
            assert_eq!(
                chunk.get_block_state(0, 64, 0),
                Blocks::STONE.default_block_state(),
                "distance-one block writes must reach the authoritative workspace"
            );
        });
        workspace.with_chunk(ChunkPos::new(8, 0), |chunk| {
            let chunk = chunk.expect("the far dependency owner must be retained");
            let section_index = chunk.get_section_index(64) as usize;
            assert_eq!(
                chunk.get_post_processing()[section_index],
                vec![0],
                "far dependency post-processing must survive region teardown"
            );
            let ticks = chunk.get_fluid_ticks().scheduled_ticks();
            assert_eq!(ticks.len(), 1);
            assert_eq!(ticks[0].r#type, FluidId(4));
            assert_eq!(ticks[0].pos, BlockPos::new(128, 64, 0));
        });
    }

    /// A workspace entry retained by a neighboring center can sit below the
    /// slot status this center's FEATURES cache requires: the StructureStarts
    /// placeholder at distance two from C1 is a Carvers dependency of adjacent
    /// C2. Paper's scheduler never hands a decoration pass an
    /// under-generated dependency, so composition must regenerate stale
    /// lower-status entries rather than silently reusing void terrain.
    #[test]
    fn feature_region_regenerates_workspace_entries_below_required_status() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let workspace = FeatureWorkspace::new();
        // Simulate C1's retained distance-two StructureStarts placeholder at
        // (2, 0): primed heightmaps only, never advanced through CARVERS.
        let mut stale = fresh_worldgen_chunk(ChunkPos::new(2, 0), &generator);
        stale.prime_heightmaps(&FINAL_HEIGHTMAPS);
        stale.set_persisted_status(ChunkStatus::StructureStarts);
        workspace.insert(stale);
        // An up-to-date entry at the same position must still be reused.
        let fresh_entry = {
            let mut chunk = generate_ring_chunk(ChunkPos::new(0, 1), &generator);
            chunk.set_persisted_status(ChunkStatus::Carvers);
            chunk
        };
        let fresh_marker = fresh_entry.get_block_state(0, 64, 0);
        workspace.insert(fresh_entry);

        // Drive a successful composition-only FEATURES pass from (1, 0) (the
        // probe body composes the region and republishes every owned
        // dependency, then returns Ok) so the stale (2, 0) entry is a
        // distance-one Carvers dependency and the workspace transaction
        // commits.
        let mut holder =
            workspace_writeback_probe_holder_at(&generator, &workspace, ChunkPos::new(1, 0));
        holder
            .generate_through(ChunkStatus::Features)
            .expect("the probe FEATURES seam must compose and commit");

        // The stale StructureStarts entry was replaced by a CARVERS-status
        // ring chunk; the adequate Carvers entry survives untouched.
        workspace.with_chunk(ChunkPos::new(2, 0), |chunk| {
            let chunk = chunk.expect("regenerated entry must be retained");
            assert!(
                chunk
                    .get_persisted_status()
                    .is_or_after(ChunkStatus::Carvers)
            );
        });
        workspace.with_chunk(ChunkPos::new(0, 1), |chunk| {
            let chunk = chunk.expect("adequate entry must be retained");
            assert_eq!(
                chunk.get_block_state(0, 64, 0),
                fresh_marker,
                "an entry meeting its slot status must not be regenerated"
            );
        });
    }

    #[test]
    fn failed_features_drop_staged_writebacks_and_restore_before_retry() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let expected = {
            let mut expected = fresh_worldgen_chunk(ChunkPos::ZERO, &generator);
            expected.set_persisted_status(ChunkStatus::Carvers);
            expected
        };
        let mut holder = writeback_probe_holder(&generator, true);

        let error = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("the failing FEATURES seam must roll back");
        assert!(matches!(
            error,
            GeneratedChunkError::Generation(GenError::FeaturePlacementDecode {
                step_index: 9,
                global_feature_index: 17,
                feature_key: "minecraft:dark_forest_vegetation",
                ..
            })
        ));
        assert_eq!(holder.status(), ChunkStatus::Carvers);
        assert_generated_proto_equal(&holder.chunk, &expected);
        assert!(
            holder.take_feature_writebacks().is_empty(),
            "failed FEATURES must never publish staged dependency mutations"
        );

        let retry = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("the failed FEATURES attempt must be cached");
        assert!(matches!(
            retry,
            GeneratedChunkError::Generation(GenError::FeaturePlacementDecode {
                step_index: 9,
                global_feature_index: 17,
                feature_key: "minecraft:dark_forest_vegetation",
                ..
            })
        ));
        assert_eq!(holder.status(), ChunkStatus::Carvers);
        assert_generated_proto_equal(&holder.chunk, &expected);
        assert!(holder.take_feature_writebacks().is_empty());
    }

    /// FULL remains the consuming ProtoChunk → LevelChunk boundary even after
    /// FEATURES has reached its cached typed failure. The historical FEATURES
    /// error must never shadow the stable FULL UnsupportedStatus response.
    #[test]
    fn full_rejection_precedes_cached_features_failure() {
        let generator = test_generator();
        let mut holder = feature_holder(&generator, ChunkPos::ZERO);
        let features_error = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("FEATURES must stop at its typed boundary");
        assert!(matches!(
            features_error,
            GeneratedChunkError::Generation(GenError::FeaturePlacementDecode { .. })
                | GeneratedChunkError::Generation(GenError::SettingsNotGenerated { .. })
                | GeneratedChunkError::Generation(
                    GenError::StructureDecorationIndexUnavailable { .. }
                )
        ));
        assert_eq!(holder.status(), ChunkStatus::Empty);

        assert!(matches!(
            holder.generate_through(ChunkStatus::Full),
            Err(GeneratedChunkError::UnsupportedStatus(ChunkStatus::Full))
        ));
        assert_eq!(holder.status(), ChunkStatus::Empty);
    }

    /// The generated placed/configured pair is decoded through registry-backed
    /// holders. The configured feature identity comes from the JSON dispatch
    /// type, and every placement modifier is selected by its own type rather
    /// than by a positional assumption in the generated list.
    #[test]
    fn matching_fluids_modifier_decodes_through_generated_fluid_registry() {
        let generator = test_generator();
        let modifiers =
            decode_placement_modifiers("minecraft:disk_clay", generator.feature_access())
                .expect("matching_fluids must resolve the generated FLUID registry");
        assert_eq!(modifiers.len(), 4);
    }

    #[test]
    fn lake_placed_feature_decodes_through_registry_holders() {
        let generator = test_generator();
        let decoded = decode_placed_feature("minecraft:lake_lava_underground", &generator)
            .expect("the seed-42 lake entry must decode");
        let placed = decoded.placed_holder.value(decoded.placed_registry);
        assert_eq!(placed.placement().len(), 6);
        assert!(matches!(
            placed.feature(),
            Holder::Reference { registry, .. } if *registry == decoded.configured_registry.registry_id()
        ));
        let configured = placed.feature().value(decoded.configured_registry);
        assert_eq!(
            configured.feature,
            feature_id_from_registry_name("minecraft:lake")
                .expect("the lake dispatch type must be registered")
        );
    }

    #[test]
    fn amethyst_geode_placed_feature_decodes_through_registry_holders() {
        let generator = test_generator();
        let decoded = decode_placed_feature("minecraft:amethyst_geode", &generator)
            .expect("the amethyst geode entry must decode");
        let placed = decoded.placed_holder.value(decoded.placed_registry);
        assert!(matches!(
            placed.feature(),
            Holder::Reference { registry, .. } if *registry == decoded.configured_registry.registry_id()
        ));
        let configured = placed.feature().value(decoded.configured_registry);
        assert_eq!(
            configured.feature,
            feature_id_from_registry_name("minecraft:geode")
                .expect("the geode dispatch type must be registered")
        );

        let geode = (configured.config.as_ref() as &dyn std::any::Any)
            .downcast_ref::<GeodeConfiguration>()
            .expect("the geode dispatch must carry GeodeConfiguration");
        for holder in geode
            .geode_block_settings
            .cannot_replace
            .iter()
            .chain(geode.geode_block_settings.invalid_blocks.iter())
        {
            assert!(
                matches!(holder, Holder::Reference { .. }),
                "registry-backed geode holder sets must not contain direct holders"
            );
        }
    }

    #[test]
    fn seed_20044_amethyst_geode_fails_rarity_24_selection() {
        let generator = Arc::new(OverworldGenerator::new(20044));
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let origin = SectionPos::of_chunk_pos(
            &ChunkPos::ZERO,
            holder.chunk.height_accessor().get_min_section_y(),
        )
        .origin();
        let mut region = compose_feature_region(&mut holder.chunk, &generator);

        let mut rarity_probe = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        let decoration_seed = rarity_probe.set_decoration_seed(20044, 0, 0);
        rarity_probe.set_feature_seed(decoration_seed, 2, 2);
        assert!(
            rarity_probe.next_float() >= 1.0 / 24.0,
            "seed 20044 step 2/global 2 must fail minecraft:rarity_filter(24)"
        );

        let mut selection_random = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        let decoration_seed = selection_random.set_decoration_seed(20044, 0, 0);
        selection_random.set_feature_seed(decoration_seed, 2, 2);
        assert!(
            !placement_selects(
                &mut region,
                &generator,
                &mut selection_random,
                &origin,
                "minecraft:amethyst_geode",
            )
            .expect("amethyst geode placement must decode"),
            "the full placed-feature chain must reject the failed rarity filter"
        );
    }

    /// The Batch 2/3 decoder arms decode the generated configured/placed JSON of
    /// each dispatch leaf seated in the seed-42 closure. These focused tests
    /// cover the decoder arms directly — the seed-42 dark-forest parent is
    /// rejected by its surface-water filter before the nested selector executes,
    /// and the runtime then stops at the step-10 `freeze_top_layer` boundary.
    /// The later-step leaves therefore get their own independent decode coverage
    /// here. The simple_block, block_column, and vines arms are not separately
    /// exercised by these tests.
    #[test]
    fn ore_dirt_decodes_through_the_batch2_ore_arm() {
        let generator = test_generator();
        let decoded = decode_placed_feature("minecraft:ore_dirt", &generator)
            .expect("the seed-42 ore_dirt entry must decode");
        let placed = decoded.placed_holder.value(decoded.placed_registry);
        assert_eq!(placed.placement().len(), 4);
        let configured = placed.feature().value(decoded.configured_registry);
        assert_eq!(
            configured.feature,
            feature_id_from_registry_name("minecraft:ore")
                .expect("the ore dispatch type must be registered")
        );
        let ore = (configured.config.as_ref() as &dyn std::any::Any)
            .downcast_ref::<OreConfiguration>()
            .expect("the ore dispatch must carry OreConfiguration");
        assert_eq!(ore.size, 33);
        assert_eq!(ore.target_states.len(), 1);
    }

    #[test]
    fn disk_sand_decodes_through_the_batch2_disk_arm() {
        let generator = test_generator();
        let decoded = decode_placed_feature("minecraft:disk_sand", &generator)
            .expect("the seed-42 disk_sand entry must decode");
        let placed = decoded.placed_holder.value(decoded.placed_registry);
        assert_eq!(placed.placement().len(), 5);
        let configured = placed.feature().value(decoded.configured_registry);
        assert_eq!(
            configured.feature,
            feature_id_from_registry_name("minecraft:disk")
                .expect("the disk dispatch type must be registered")
        );
        let disk = (configured.config.as_ref() as &dyn std::any::Any)
            .downcast_ref::<DiskConfiguration>()
            .expect("the disk dispatch must carry DiskConfiguration");
        assert_eq!(disk.half_height, 2);
    }

    #[test]
    fn spring_water_decodes_through_the_batch2_spring_arm() {
        let generator = test_generator();
        let decoded = decode_placed_feature("minecraft:spring_water", &generator)
            .expect("the seed-42 spring_water entry must decode");
        let configured = decoded
            .placed_holder
            .value(decoded.placed_registry)
            .feature()
            .value(decoded.configured_registry);
        assert_eq!(
            configured.feature,
            feature_id_from_registry_name("minecraft:spring_feature")
                .expect("the spring_feature dispatch type must be registered")
        );
        let spring = (configured.config.as_ref() as &dyn std::any::Any)
            .downcast_ref::<SpringConfiguration>()
            .expect("the spring dispatch must carry SpringConfiguration");
        assert_eq!(spring.valid_blocks.iter().count(), 11);
    }

    #[test]
    fn seagrass_cold_decodes_through_the_batch2_seagrass_arm() {
        let generator = test_generator();
        let decoded = decode_placed_feature("minecraft:seagrass_cold", &generator)
            .expect("the seed-42 seagrass_cold entry must decode");
        let configured = decoded
            .placed_holder
            .value(decoded.placed_registry)
            .feature()
            .value(decoded.configured_registry);
        assert_eq!(
            configured.feature,
            feature_id_from_registry_name("minecraft:seagrass")
                .expect("the seagrass dispatch type must be registered")
        );
        let seagrass = (configured.config.as_ref() as &dyn std::any::Any)
            .downcast_ref::<ProbabilityFeatureConfiguration>()
            .expect("the seagrass dispatch must carry ProbabilityFeatureConfiguration");
        assert_eq!(seagrass.probability, 0.3);
    }

    #[test]
    fn freeze_top_layer_decodes_through_the_batch2_arm() {
        let generator = test_generator();
        let decoded = decode_placed_feature("minecraft:freeze_top_layer", &generator)
            .expect("the freeze_top_layer entry must decode");
        let configured = decoded
            .placed_holder
            .value(decoded.placed_registry)
            .feature()
            .value(decoded.configured_registry);
        assert_eq!(
            configured.feature,
            feature_id_from_registry_name("minecraft:freeze_top_layer")
                .expect("the freeze_top_layer dispatch type must be registered")
        );
        assert!(
            (configured.config.as_ref() as &dyn std::any::Any)
                .downcast_ref::<NoneFeatureConfiguration>()
                .is_some(),
            "freeze_top_layer must carry a NoneFeatureConfiguration"
        );
    }

    /// The Batch 3 `minecraft:underwater_magma` configured entry (the seed-42
    /// global-26 leaf) decodes through the registry-backed arm into a
    /// `FeatureId::new(21)` holder carrying its exact `UnderwaterMagmaConfiguration`
    /// (floor search range 5, probability 0.5, radius 1), and its full
    /// placed-feature chain (count uniform, in_square, height_range,
    /// OCEAN_FLOOR_WG surface_relative_threshold_filter -2, biome) decodes to
    /// five placement modifiers. This proves the configured/placed pair is
    /// decodable and dispatchable — the id-21 concrete feature is now reached
    /// rather than refused.
    #[test]
    fn underwater_magma_decodes_through_the_batch3_arm() {
        let generator = test_generator();
        assert_eq!(
            feature_id_from_registry_name("minecraft:underwater_magma"),
            Some(FeatureId::new(21)),
            "the underwater_magma dispatch type must be registered at id 21"
        );
        let decoded = decode_placed_feature("minecraft:underwater_magma", &generator)
            .expect("the seed-42 underwater_magma entry must decode");
        let placed = decoded.placed_holder.value(decoded.placed_registry);
        assert_eq!(placed.placement().len(), 5);
        let configured = placed.feature().value(decoded.configured_registry);
        assert_eq!(
            configured.feature,
            FeatureId::new(21),
            "the underwater_magma dispatch must resolve to Feature.UNDERWATER_MAGMA"
        );
        let cfg = (configured.config.as_ref() as &dyn std::any::Any)
            .downcast_ref::<UnderwaterMagmaConfiguration>()
            .expect("the underwater_magma dispatch must carry UnderwaterMagmaConfiguration");
        assert_eq!(cfg.floor_search_range, 5);
        assert_eq!(cfg.placement_probability_per_valid_position, 0.5);
        assert_eq!(cfg.placement_radius_around_floor, 1);
    }

    /// The Batch 4 `minecraft:glow_lichen` configured entry decodes through the
    /// registry-backed `minecraft:multiface_growth` arm into FeatureId 20 with
    /// Paper's exact generated configuration and five-modifier placement chain.
    #[test]
    fn glow_lichen_decodes_through_the_batch4_arm() {
        let generator = test_generator();
        assert_eq!(
            feature_id_from_registry_name("minecraft:multiface_growth"),
            Some(FeatureId::new(20)),
            "the multiface_growth dispatch type must be registered at id 20"
        );
        let decoded = decode_placed_feature("minecraft:glow_lichen", &generator)
            .expect("the seed-42 glow_lichen entry must decode");
        let placed = decoded.placed_holder.value(decoded.placed_registry);
        assert_eq!(placed.placement().len(), 5);
        let configured = placed.feature().value(decoded.configured_registry);
        assert_eq!(configured.feature, FeatureId::new(20));
        let cfg = (configured.config.as_ref() as &dyn std::any::Any)
            .downcast_ref::<MultifaceGrowthConfiguration>()
            .expect("glow_lichen must carry MultifaceGrowthConfiguration");
        assert_eq!(cfg.place_block.name(), "minecraft:glow_lichen");
        assert_eq!(cfg.search_range, 20);
        assert!(!cfg.can_place_on_floor);
        assert!(cfg.can_place_on_ceiling);
        assert!(cfg.can_place_on_wall);
        assert_eq!(cfg.chance_of_spreading.to_bits(), 0.5f32.to_bits());
        assert_eq!(cfg.can_be_placed_on.size(), 10);
    }

    /// The seed-42 end-to-end run advances through the entire UNDERGROUND_ORES
    /// step — now including the registry-backed `minecraft:underwater_magma`
    /// leaf at global index 26, which dispatches through the id-21 arm and
    /// executes its column-scanned placement. In this dry origin union
    /// (beach/dark_forest/lush_caves/river) the water-column floor scan fails,
    /// so the feature returns false having consumed no placement-box RNG. The
    /// run then continues into VEGETAL_DECORATION, where Batch 4 decodes and
    /// executes `minecraft:glow_lichen` through `minecraft:multiface_growth`.
    /// It refuses at the next unsupported *selected* path WITHOUT mutating the
    /// RNG past that refusal: the run returns typed immediately at
    /// `minecraft:freeze_top_layer` (step 10/global 0), when its biome freeze
    /// query reaches the unimplemented `shouldFreeze` seam. The chunk stays CARVERS
    /// and FEATURES is never stamped. The typed-unavailable dispatches
    /// for underwater magma (id 21) and multiface growth (id 20) no longer
    /// refuse; both concrete features are reached.
    ///
    /// Seed-1 chunk (0,0) reaches a real `minecraft:spring_water` placement:
    /// the spring leaf previously panicked in the unimplemented
    /// `LevelAccessor.scheduleTick` seam. With the fluid routing implemented,
    /// the pass must retain the scheduled fluid tick in the owning chunk. The
    /// run is bounded at the FLUID_SPRINGS step so the pass ends on the spring
    /// itself rather than the later unsupported seagrass seam.
    #[test]
    fn seed1_spring_water_places_and_schedules_fluid_tick() {
        let generator = Arc::new(OverworldGenerator::new(1));
        let mut holder = feature_holder(&generator, ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let workspace = FeatureWorkspace::new();
        let writebacks = run_biome_decoration_through(
            &mut holder.chunk,
            &generator,
            &workspace,
            StructureFeatureIndex::explicit_count(0),
            Decoration::FluidSprings as usize + 1,
        )
        .expect("the seed-1 FLUID_SPRINGS-bounded pass must not panic");
        assert!(
            !writebacks.is_empty(),
            "a real spring write must reach a distance-one owner or the center"
        );
        // The spring schedules in its owning chunk; collect the retained
        // ticks across the workspace owners and the center.
        let center_ticks = holder.chunk.get_fluid_ticks().scheduled_ticks().to_vec();
        let mut total_ticks = center_ticks.len();
        let mut ticks_seen = center_ticks;
        for (pos, _) in &writebacks {
            workspace.with_chunk(*pos, |chunk| {
                if let Some(chunk) = chunk {
                    let ticks = chunk.get_fluid_ticks().scheduled_ticks();
                    total_ticks += ticks.len();
                    ticks_seen.extend(ticks.iter().cloned());
                }
            });
        }
        assert!(
            total_ticks >= 1,
            "seed-1 spring_water must schedule a fluid tick somewhere"
        );
        assert_eq!(ticks_seen.len(), total_ticks);
        // Zero delay against the region's default game time: every spring's
        // flow is due immediately (`gameTime + 0`).
        for tick in &ticks_seen {
            assert_eq!(tick.delay, 0);
        }
    }

    #[test]
    fn seed42_does_not_mutate_rng_past_the_next_selected_unsupported_leaf() {
        let generator = test_generator();
        let mut holder = feature_holder(&generator, ChunkPos::new(0, 0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let err = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("FEATURES must refuse at the selected mismatch");
        assert!(
            matches!(
                &err,
                GeneratedChunkError::Generation(GenError::FeaturePlacementDecode {
                    step_index: 10,
                    global_feature_index: 0,
                    feature_key: "minecraft:freeze_top_layer",
                    ..
                })
            ),
            "unexpected next FEATURES boundary: {err:?}"
        );
        assert_eq!(holder.status(), ChunkStatus::Carvers);
    }

    /// The three Batch 2 selector leaves are wired in the decoder, and the
    /// generated closure now gives their named and inline placed holders one
    /// shared registry identity. A concrete configuration unit that is not yet
    /// ported remains an explicit deferred value and refuses only if selected;
    /// it is never replaced by a selector-specific branch or a no-op.
    #[test]
    fn selector_dispatch_types_are_registered() {
        for (type_name, id) in [
            ("minecraft:random_selector", 52),
            ("minecraft:simple_random_selector", 54),
            ("minecraft:random_boolean_selector", 55),
        ] {
            assert_eq!(
                feature_id_from_registry_name(type_name),
                Some(FeatureId::new(id)),
                "the {type_name} selector dispatch type must be registered"
            );
        }
    }

    #[test]
    fn generated_closure_follows_selector_and_vegetation_holders() {
        let selector_edges = generated_feature_edges(GeneratedFeatureNode::Configured(
            "minecraft:dark_forest_vegetation",
        ))
        .expect("selector closure edges");
        assert!(selector_edges.contains(&GeneratedFeatureNode::Placed(
            "minecraft:dark_oak_leaf_litter",
        )));
        assert!(
            selector_edges.contains(&GeneratedFeatureNode::Placed("minecraft:oak_leaf_litter",))
        );

        let vegetation_edges =
            generated_feature_edges(GeneratedFeatureNode::Configured("minecraft:moss_patch"))
                .expect("vegetation-patch closure edge");
        assert_eq!(
            vegetation_edges,
            vec![GeneratedFeatureNode::Configured(
                "minecraft:moss_vegetation"
            )]
        );
    }

    #[test]
    fn generated_closure_follows_real_root_system_tree_holder() {
        // The two real root-system corpus entries (ids 167/168) hold an inline
        // placed feature under `config.feature`; the closure must follow it.
        for name in [
            "minecraft:rooted_azalea_tree",
            "minecraft:rooted_sulfur_spring",
        ] {
            let edges = generated_feature_edges(GeneratedFeatureNode::Configured(name))
                .unwrap_or_else(|error| panic!("{name} root-system closure edges: {error}"));
            let expected = if name == "minecraft:rooted_azalea_tree" {
                GeneratedFeatureNode::Configured("minecraft:azalea_tree")
            } else {
                GeneratedFeatureNode::Configured("minecraft:sulfur_spring")
            };
            assert_eq!(edges, vec![expected], "{name} must follow config.feature");
        }

        // Counterfactual: a walker that skips `minecraft:root_system` configs
        // would return no edges here and silently accept a dangling tree holder.
        let entry = CONFIGURED_FEATURE_BY_NAME
            .get("minecraft:rooted_azalea_tree")
            .expect("real root-system fixture");
        let mut json: Value = serde_json::from_str(entry.json).expect("fixture JSON");
        json["config"]["feature"]["feature"] =
            serde_json::json!("minecraft:missing_nested_feature");
        let error = generated_configured_object(
            json.as_object().expect("configured object"),
            &mut Vec::new(),
            "minecraft:rooted_azalea_tree",
        )
        .expect_err("missing root-system tree holder must fail closure validation");
        assert!(error.contains("missing configured feature minecraft:missing_nested_feature"));

        // Non-holder strings in the same config stay opaque metadata.
        let mut metadata_only: Value = serde_json::from_str(entry.json).expect("fixture JSON");
        metadata_only["config"]["root_state_provider"]["type"] =
            serde_json::json!("minecraft:missing_but_not_a_feature_provider");
        generated_configured_object(
            metadata_only.as_object().expect("configured object"),
            &mut Vec::new(),
            "minecraft:rooted_azalea_tree",
        )
        .expect("provider type strings are intentionally opaque");
    }

    #[test]
    fn generated_closure_follows_weighted_data_inside_real_nested_json() {
        let entry = CONFIGURED_FEATURE_BY_NAME
            .get("minecraft:sulfur_spring")
            .expect("weighted generated fixture");
        let mut json: Value = serde_json::from_str(entry.json).expect("fixture JSON");
        // Keep the real weighted-random-selector / data / inline sequence shape,
        // but replace one nested configured holder with a missing named holder.
        json["config"]["features"][0]["data"]["feature"]["config"]["features"][0] = serde_json::json!({
            "feature": "minecraft:missing_nested_feature",
            "placement": [],
        });
        let mut edges = Vec::new();
        let error = generated_configured_object(
            json.as_object().expect("configured object"),
            &mut edges,
            "minecraft:sulfur_spring",
        )
        .expect_err("missing nested weighted data holder must fail");
        assert!(error.contains("missing configured feature minecraft:missing_nested_feature"));
    }

    #[test]
    fn generated_selector_decoders_reject_malformed_real_json_counterfactuals() {
        let generator = test_generator();
        let ops =
            RegistryOps::create_from_access(&JsonOps::INSTANCE, generator.feature_access().clone());

        let random_entry = CONFIGURED_FEATURE_BY_NAME
            .get("minecraft:dark_forest_vegetation")
            .expect("random-selector generated fixture");
        for chance in [-0.1_f64, 1.1] {
            let mut json: Value = serde_json::from_str(random_entry.json).expect("fixture JSON");
            json["config"]["features"][0]["chance"] = serde_json::json!(chance);
            let error = decode_configured_feature_value(
                "minecraft:dark_forest_vegetation",
                &json,
                &ops,
                generator.feature_access(),
            )
            .expect_err("random-selector chance outside [0, 1] must fail");
            assert!(
                error.contains("outside of range"),
                "unexpected chance validation error: {error}"
            );
        }

        let weighted_entry = CONFIGURED_FEATURE_BY_NAME
            .get("minecraft:sulfur_spring")
            .expect("weighted-selector generated fixture");
        for weight in [-1_i64, 0] {
            let mut json: Value = serde_json::from_str(weighted_entry.json).expect("fixture JSON");
            json["config"]["features"][0]["weight"] = serde_json::json!(weight);
            let error = decode_configured_feature_value(
                "minecraft:sulfur_spring",
                &json,
                &ops,
                generator.feature_access(),
            )
            .expect_err("weighted-selector weight below one must fail");
            assert!(
                error.contains("weight must be at least 1"),
                "unexpected weight validation error: {error}"
            );
        }
    }

    #[test]
    fn generated_closure_reads_root_config_for_real_selector_cycles() {
        let entry = CONFIGURED_FEATURE_BY_NAME
            .get("minecraft:dark_forest_vegetation")
            .expect("real selector fixture");
        let mut json: Value = serde_json::from_str(entry.json).expect("fixture JSON");
        // `default` is a PlacedFeature holder. Point it at the real placed entry
        // whose configured holder points back to this configured root. A walker
        // that ignores the root `config` object cannot observe this cycle.
        json["config"]["default"] = serde_json::json!("minecraft:dark_forest_vegetation");
        let error = validate_generated_feature_graph(
            [GeneratedFeatureNode::Configured(
                "minecraft:dark_forest_vegetation",
            )],
            &mut |node| {
                if node == GeneratedFeatureNode::Configured("minecraft:dark_forest_vegetation") {
                    let mut refs = Vec::new();
                    generated_configured_object(
                        json.as_object().expect("configured object"),
                        &mut refs,
                        "minecraft:dark_forest_vegetation",
                    )?;
                    Ok(refs)
                } else {
                    generated_feature_edges(node)
                }
            },
        )
        .expect_err("root configured-feature config must participate in cycle checks");
        assert!(error.contains("generated feature closure cycle"));
    }

    #[test]
    fn generated_closure_reads_real_vegetation_patch_holder_without_scanning_metadata() {
        let entry = CONFIGURED_FEATURE_BY_NAME
            .get("minecraft:moss_patch")
            .expect("real vegetation-patch fixture");
        let mut json: Value = serde_json::from_str(entry.json).expect("fixture JSON");
        // This is a real Holder<PlacedFeature> shape under `vegetation_feature`.
        json["config"]["vegetation_feature"]["feature"] =
            serde_json::json!("minecraft:missing_nested_feature");
        let error = generated_configured_object(
            json.as_object().expect("configured object"),
            &mut Vec::new(),
            "minecraft:moss_patch",
        )
        .expect_err("missing vegetation holder must fail closure validation");
        assert!(error.contains("missing configured feature minecraft:missing_nested_feature"));

        // Other resource strings in the same config are metadata, not holder
        // edges; they must not be looked up as configured or placed features.
        let mut metadata_only: Value = serde_json::from_str(entry.json).expect("fixture JSON");
        metadata_only["config"]["replaceable"] =
            serde_json::json!("#minecraft:missing_but_not_a_feature_holder");
        generated_configured_object(
            metadata_only.as_object().expect("configured object"),
            &mut Vec::new(),
            "minecraft:moss_patch",
        )
        .expect("metadata resource strings are intentionally opaque");
    }

    #[test]
    fn generated_graph_rejects_missing_references_and_cycles() {
        let missing =
            validate_generated_feature_graph([GeneratedFeatureNode::Placed("root")], &mut |node| {
                match node {
                    GeneratedFeatureNode::Placed("root") => {
                        Ok(vec![GeneratedFeatureNode::Configured("missing")])
                    }
                    GeneratedFeatureNode::Configured("missing") => {
                        Err("missing generated configured feature missing".to_string())
                    }
                    other => Err(format!("unexpected node {other:?}")),
                }
            })
            .expect_err("missing graph edge must fail before publication");
        assert!(missing.contains("missing generated configured feature missing"));

        let cycle =
            validate_generated_feature_graph([GeneratedFeatureNode::Placed("root")], &mut |node| {
                match node {
                    GeneratedFeatureNode::Placed("root") => {
                        Ok(vec![GeneratedFeatureNode::Configured("branch")])
                    }
                    GeneratedFeatureNode::Configured("branch") => {
                        Ok(vec![GeneratedFeatureNode::Placed("root")])
                    }
                    other => Err(format!("unexpected node {other:?}")),
                }
            })
            .expect_err("cyclic graph must fail before publication");
        assert!(cycle.contains("generated feature closure cycle"));
    }

    /// The generated closure preserves full generated ids and one registry
    /// identity for every placed/configured entry. Synthetic gap values retain
    /// insertion positions that are absent from the generated name tables. The
    /// dark-forest selector's inline placed
    /// mushroom holders and named branches therefore resolve through the same
    /// `RegistryOps` access as the root feature.
    #[test]
    fn generated_selector_closure_preserves_full_registry_identity() {
        let generator = test_generator();
        let placed = generator
            .feature_access()
            .lookup(&*PLACED_FEATURE)
            .expect("generated placed registry");
        let configured = generator
            .feature_access()
            .lookup(&*CONFIGURED_FEATURE)
            .expect("generated configured registry");
        let placed_entries = sorted_placed_feature_entries().expect("generated placed ids");
        let configured_entries =
            sorted_configured_feature_entries().expect("generated configured ids");
        assert_eq!(
            placed.size(),
            placed_name_slots(&placed_entries).len() as i32
        );
        assert_eq!(
            configured.size(),
            configured_name_slots(&configured_entries).len() as i32
        );
        for (name, entry) in placed_entries {
            let key = ResourceKey::create(&*PLACED_FEATURE, Identifier::parse(name));
            assert!(matches!(
                placed.get(&key),
                Some(Holder::Reference { registry, id })
                    if registry == placed.registry_id() && id == entry.id as u32
            ));
        }
        for (name, entry) in configured_entries {
            let key = ResourceKey::create(&*CONFIGURED_FEATURE, Identifier::parse(name));
            assert!(matches!(
                configured.get(&key),
                Some(Holder::Reference { registry, id })
                    if registry == configured.registry_id() && id == entry.id as u32
            ));
        }
        decode_placed_feature("minecraft:dark_forest_vegetation", &generator)
            .expect("the random-selector root must decode through the generated closure");
    }

    /// Inline selector branches retain their concrete configured holders. The
    /// two mushroom branches are executable registered features (ids 11 and 10),
    /// while the named dark-oak branch remains a registry reference to the
    /// deferred tree leaf; no branch is flattened into a no-op.
    #[test]
    fn generated_selector_keeps_inline_mushroom_branches_typed() {
        let generator = test_generator();
        let configured = generator
            .feature_access()
            .lookup(&*CONFIGURED_FEATURE)
            .expect("generated configured registry");
        let key = ResourceKey::create(
            &*CONFIGURED_FEATURE,
            Identifier::parse("minecraft:dark_forest_vegetation"),
        );
        let root = configured.get(&key).expect("dark forest configured holder");
        let root = root.value(configured);
        let selector = (root.config.as_ref() as &dyn std::any::Any)
            .downcast_ref::<RandomFeatureConfiguration>()
            .expect("dark forest must retain random selector configuration");
        assert_eq!(selector.features.len(), 7);
        for (branch, expected_id) in selector.features[..2].iter().zip([11_u32, 10_u32]) {
            let Holder::Direct(placed) = branch.feature() else {
                panic!("mushroom selector branch must remain inline");
            };
            let configured_branch = placed.feature().value(configured);
            assert_eq!(configured_branch.feature, FeatureId::new(expected_id));
            assert!(
                (configured_branch.config.as_ref() as &dyn std::any::Any)
                    .downcast_ref::<HugeMushroomFeatureConfiguration>()
                    .is_some(),
                "mushroom branch must decode its concrete configuration"
            );
        }
        assert!(matches!(
            selector.features[2].feature(),
            Holder::Reference { registry, id }
                if *registry == generator
                    .feature_access()
                    .lookup(&*PLACED_FEATURE)
                    .expect("generated placed registry")
                    .registry_id()
                    && *id == PLACED_FEATURE_BY_NAME
                        .get("minecraft:dark_oak_leaf_litter")
                        .expect("dark oak placed entry")
                        .id as u32
        ));
    }

    /// Gap slots are real registry positions but fail closed with a sentinel
    /// feature id rather than accidentally dispatching Feature.NO_OP (id 0).
    #[test]
    fn generated_registry_gaps_never_use_no_op_id() {
        let generator = test_generator();
        let configured = generator
            .feature_access()
            .lookup(&*CONFIGURED_FEATURE)
            .expect("generated configured registry");
        let entries = sorted_configured_feature_entries().expect("generated configured ids");
        let gap_id = configured_name_slots(&entries)
            .iter()
            .position(Option::is_none)
            .expect("generated configured gap");
        let gap = configured
            .by_id(gap_id as i32)
            .expect("configured gap value");
        assert_eq!(gap.feature, FeatureId::new(u32::MAX));
        assert!(gap.config.unavailable_feature().is_some());
    }

    /// Every synthetic placed-feature slot points at the configured sentinel,
    /// never at configured id 0. This keeps a missing generated entry from
    /// silently dispatching whatever real feature occupies the first slot.
    #[test]
    fn generated_placed_gaps_point_at_the_fail_closed_sentinel() {
        let generator = test_generator();
        let placed = generator
            .feature_access()
            .lookup(&*PLACED_FEATURE)
            .expect("generated placed registry");
        let configured = generator
            .feature_access()
            .lookup(&*CONFIGURED_FEATURE)
            .expect("generated configured registry");
        let configured_entries =
            sorted_configured_feature_entries().expect("generated configured ids");
        let configured_gap_id = configured_name_slots(&configured_entries)
            .iter()
            .position(Option::is_none)
            .expect("generated configured gap") as u32;
        let placed_entries = sorted_placed_feature_entries().expect("generated placed ids");
        let placed_gap_id = placed_name_slots(&placed_entries)
            .iter()
            .position(Option::is_none)
            .expect("generated placed gap") as i32;
        let gap = placed.by_id(placed_gap_id).expect("placed gap value");
        assert!(matches!(
            gap.feature(),
            Holder::Reference { registry, id }
                if *registry == configured.registry_id() && *id == configured_gap_id
        ));
    }

    /// Paper's `RandomSelectorFeature.place` draws one float per weighted entry
    /// in list order and stops at the first hit. The selector is reached only
    /// after its parent `dark_forest_vegetation` placed feature consumes the two
    /// `InSquarePlacement` `nextInt(16)` draws. At seed 42, step 9/global 17,
    /// those placement draws are followed by two misses and a third draw that
    /// selects `minecraft:dark_oak_leaf_litter`; the next failure is therefore
    /// the selected tree configuration, not the selector or an alternate branch.
    #[test]
    fn seed42_dark_forest_selector_selects_dark_oak_leaf_litter() {
        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        random.set_feature_seed(42, 17, 9);
        assert_eq!(random.next_int_bound(16), 15);
        assert_eq!(random.next_int_bound(16), 7);
        let first = random.next_float();
        let second = random.next_float();
        let third = random.next_float();
        assert!(first >= 0.025);
        assert!(second >= 0.05);
        assert!(third < 0.6666667);
    }

    #[test]
    fn paper_feature_seed_override_reseeds_from_configured_feature_and_preserves_draw_order() {
        let configured_seed = 987_654_321_i64;
        let mut feature_seeds = HashMap::new();
        feature_seeds.insert("minecraft:lake_lava".to_string(), configured_seed);
        let generator = OverworldGenerator::new_with_feature_seeds(42, feature_seeds);
        let origin = BlockPos::new(16, -64, 0);

        let mut actual = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        let decoration_seed = actual.set_decoration_seed(42, origin.get_x(), origin.get_z());
        set_paper_feature_seed(
            &mut actual,
            &generator,
            decoration_seed,
            &origin,
            "minecraft:lake_lava_underground",
            2,
            1,
        );

        let mut expected = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        let expected_decoration_seed =
            expected.set_decoration_seed(42, origin.get_x(), origin.get_z());
        let expected_population_seed =
            expected.set_decoration_seed(configured_seed, origin.get_x(), origin.get_z());
        assert_eq!(expected_decoration_seed, decoration_seed);
        expected.set_feature_seed(expected_population_seed, 2, 1);
        assert_eq!(actual.next_long(), expected.next_long());
        assert_eq!(actual.next_long(), expected.next_long());
    }

    /// Paper's structure loop owns a separate RNG index. Even when a caller
    /// proves that structure decoration is available, placed-feature seeding
    /// remains `globalIndexOfFeature`, not `structure_count + globalIndex`.
    #[test]
    fn paper_feature_seed_does_not_invent_a_structure_offset() {
        let generator = OverworldGenerator::new(42);
        let origin = BlockPos::new(0, -64, 0);
        let mut actual = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        let decoration_seed = actual.set_decoration_seed(42, origin.get_x(), origin.get_z());
        set_paper_feature_seed(
            &mut actual,
            &generator,
            decoration_seed,
            &origin,
            "minecraft:lake_lava_underground",
            17,
            9,
        );

        let mut expected = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        expected.set_decoration_seed(42, origin.get_x(), origin.get_z());
        expected.set_feature_seed(decoration_seed, 17, 9);
        let mut invented_offset = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        invented_offset.set_decoration_seed(42, origin.get_x(), origin.get_z());
        invented_offset.set_feature_seed(decoration_seed, 20, 9);

        assert_eq!(actual.next_long(), expected.next_long());
        assert_ne!(actual.next_long(), invented_offset.next_long());
    }

    #[test]
    fn paper_feature_seed_minus_one_skips_override_draws() {
        let mut feature_seeds = HashMap::new();
        feature_seeds.insert("minecraft:lake_lava".to_string(), -1);
        let generator = OverworldGenerator::new_with_feature_seeds(42, feature_seeds);
        let origin = BlockPos::new(16, -64, 0);

        let mut actual = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        let decoration_seed = actual.set_decoration_seed(42, origin.get_x(), origin.get_z());
        set_paper_feature_seed(
            &mut actual,
            &generator,
            decoration_seed,
            &origin,
            "minecraft:lake_lava_underground",
            2,
            1,
        );

        let mut expected = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        expected.set_decoration_seed(42, origin.get_x(), origin.get_z());
        expected.set_feature_seed(decoration_seed, 2, 1);
        assert_eq!(actual.next_long(), expected.next_long());
        assert_eq!(actual.next_long(), expected.next_long());
    }

    /// The seed-42 dark-forest parent consumes its count and square-placement
    /// draws, but Paper's `SurfaceWaterDepthFilter.forMaxDepth(0)` rejects all
    /// 16 candidates before the heightmap, biome, or nested random-selector
    /// feature runs. Keep this separate from the direct dark-oak survival check:
    /// an empty parent placement is not a dark-oak execution boundary.
    #[test]
    fn seed42_dark_forest_surface_filter_rejects_before_dark_oak_would_survive() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let mut region = compose_feature_region(&mut holder.chunk, &generator);
        let origin = BlockPos::new(0, -64, 0);
        let decoded = decode_placed_feature("minecraft:dark_forest_vegetation", &generator)
            .expect("dark forest placement modifiers");
        let placed = decoded.placed_holder.value(decoded.placed_registry);
        let selection_generator = FeatureSelectionGenerator {
            generator: Arc::clone(&generator),
            feature_key: "minecraft:dark_forest_vegetation",
        };

        // CountPlacement and InSquarePlacement select the exact first candidate
        // Paper uses for step 9/global 17. The next modifier is the water-depth
        // filter, so the candidate's complete parent prefix must be rejected.
        let dummy_feature = ConfiguredFeatureErased {
            feature: FeatureId::new(u32::MAX),
            config: Arc::new(DeferredGeneratedFeatureConfiguration {
                configured_key: "dark forest placement probe".to_string(),
            }),
        };
        let prefix = PlacedFeature::new(
            Holder::Direct(dummy_feature),
            placed.placement()[..2].to_vec(),
        );
        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        random.set_feature_seed(42, 17, 9);
        let candidate = prefix
            .first_placement_position(&mut region, &selection_generator, &mut random, &origin)
            .expect("the first two placement modifiers must select a candidate");
        assert_eq!(candidate, BlockPos::new(15, -64, 7));

        let ocean_floor = region.get_height_at(Types::OceanFloor, 15, 7);
        let world_surface = region.get_height_at(Types::WorldSurface, 15, 7);
        assert!(
            world_surface - ocean_floor > 0,
            "the pinned candidate must be submerged for max-depth-zero filtering"
        );

        let filtered_prefix = PlacedFeature::new(
            Holder::Direct(ConfiguredFeatureErased {
                feature: FeatureId::new(u32::MAX),
                config: Arc::new(DeferredGeneratedFeatureConfiguration {
                    configured_key: "dark forest filter probe".to_string(),
                }),
            }),
            placed.placement()[..3].to_vec(),
        );
        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        random.set_feature_seed(42, 17, 9);
        assert_eq!(
            filtered_prefix.first_placement_position(
                &mut region,
                &selection_generator,
                &mut random,
                &origin,
            ),
            None,
            "all 16 count/in-square candidates must fail the max-depth-zero filter"
        );

        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        random.set_feature_seed(42, 17, 9);
        let parent_selection = placed.first_placement_position(
            &mut region,
            &selection_generator,
            &mut random,
            &origin,
        );
        assert_eq!(parent_selection, None);

        // The same WorldGenRegion implements the Paper VegetationBlock rule for
        // dark-oak saplings: only the block below and the supports_vegetation
        // tag decide survival. This direct counterfactual proves the nested
        // `would_survive` seam independently of the rejected parent path.
        let support_pos = BlockPos::new(0, 0, 0);
        let sapling_pos = support_pos.above();
        let dark_oak_sapling = BlockState::of(
            BlockId::from_name("minecraft:dark_oak_sapling")
                .expect("dark oak sapling block must be generated"),
        );
        assert!(region.set_block(
            &support_pos,
            BlockState::of(BlockId::from_name("minecraft:stone").expect("stone")),
            0,
            512,
        ));
        assert!(
            !region.can_survive(&dark_oak_sapling, &sapling_pos),
            "dark-oak saplings must reject blocks outside supports_vegetation"
        );
        assert!(region.set_block(
            &support_pos,
            BlockState::of(BlockId::from_name("minecraft:dirt").expect("dirt")),
            0,
            512,
        ));
        assert!(
            region.can_survive(&dark_oak_sapling, &sapling_pos),
            "dark-oak saplings must survive on supports_vegetation blocks"
        );

        // Exercise the actual `dark_oak_leaf_litter` `would_survive` placement
        // modifier over the dry counterfactual, not only the direct world-level
        // predicate. This is the exact nested boundary the seed-42 selector
        // would reach on a candidate that survives the parent placement chain.
        let mut survival_random = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        assert!(
            placement_selects(
                &mut region,
                &generator,
                &mut survival_random,
                &sapling_pos,
                "minecraft:dark_oak_leaf_litter",
            )
            .expect("dark-oak would-survive placement must decode")
        );
        assert!(region.set_block(
            &support_pos,
            BlockState::of(BlockId::from_name("minecraft:stone").expect("stone")),
            0,
            512,
        ));
        let mut survival_random = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        assert!(
            !placement_selects(
                &mut region,
                &generator,
                &mut survival_random,
                &sapling_pos,
                "minecraft:dark_oak_leaf_litter",
            )
            .expect("dark-oak would-survive placement must decode")
        );
    }

    /// The tree-bearing seed-42 fixture follows a different path from the
    /// all-water origin: its outer typed boundary is still the parent placed
    /// feature at step 9/global 17, while the nested selector falls through to
    /// `oak_leaf_litter`, whose `would_survive` state is `oak_sapling`.
    #[test]
    fn seed42_chunk44_reports_outer_dark_forest_and_nested_oak_boundary() {
        let generator = test_generator();
        let mut holder = feature_holder(&generator, ChunkPos::new(4, 4));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let error = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("the nested oak survival seam must refuse FEATURES");
        assert!(matches!(
            error,
            GeneratedChunkError::Generation(GenError::FeaturePlacementDecode {
                chunk_pos,
                step_index: 9,
                global_feature_index: 17,
                feature_key: "minecraft:dark_forest_vegetation",
            }) if chunk_pos == ChunkPos::new(4, 4)
        ));

        // The typed error deliberately names the outer decoration dispatch. Pin
        // the nested path separately so a future diagnostic cannot confuse the
        // parent boundary with the configured/placed tree leaf that panicked.
        assert_eq!(
            configured_feature_key_for_placed("minecraft:oak_leaf_litter")
                .expect("oak leaf litter placed feature must resolve"),
            "minecraft:oak_leaf_litter"
        );
        let mut nested_holder = generator.create_holder(ChunkPos::new(4, 4));
        nested_holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let mut region = compose_feature_region(&mut nested_holder.chunk, &generator);
        let decoded = decode_placed_feature("minecraft:oak_leaf_litter", &generator)
            .expect("oak leaf litter placement must decode");
        let nested = decoded.placed_holder.value(decoded.placed_registry);
        let selection_generator = FeatureSelectionGenerator {
            generator: Arc::clone(&generator),
            feature_key: "minecraft:oak_leaf_litter",
        };
        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(0));
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            nested.first_placement_position(
                &mut region,
                &selection_generator,
                &mut random,
                &BlockPos::new(79, 67, 67),
            )
        }))
        .expect_err("oak_leaf_litter must reach its oak_sapling would_survive seam");
        let message = if let Some(message) = panic.downcast_ref::<String>() {
            message.as_str()
        } else if let Some(message) = panic.downcast_ref::<&'static str>() {
            message
        } else {
            "<non-string panic>"
        };
        assert_eq!(
            message,
            "WorldGenRegion.canSurvive is not implemented for minecraft:oak_sapling (RivetTodo #232)"
        );
    }

    /// A valid FEATURES pass may have no selected feature positions. Exercise
    /// the real decoration loop with an empty per-step plan to ensure the pass
    /// returns success instead of inventing a `FeaturePlacementDecode` error.
    #[test]
    fn empty_feature_steps_are_a_valid_decoration_pass() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let placed_registry_id = RegistryBuilder::new(&*PLACED_FEATURE).registry_id();
        let mut placed_by_id = HashMap::new();
        let settings_sources = resolve_feature_settings(
            &generator.biome_source.possible_biomes(),
            placed_registry_id,
            &mut placed_by_id,
        )
        .expect("the generated overworld settings must resolve");
        let mut feature_list = build_features_per_step(
            &settings_sources,
            |(settings, _)| settings.features(),
            false,
        );
        assert!(
            !feature_list.is_empty(),
            "the real overworld plan must contain decoration steps"
        );
        for step in &mut feature_list {
            step.features.clear();
        }
        assert!(
            generator
                .feature_plan
                .set(Ok(FeaturePlan {
                    placed_by_id,
                    settings_sources: Vec::new(),
                    feature_list,
                }))
                .is_ok()
        );

        let mut chunk = fresh_worldgen_chunk(ChunkPos::ZERO, &generator);
        chunk.set_persisted_status(ChunkStatus::Carvers);
        let workspace = FeatureWorkspace::new();
        let writebacks = run_biome_decoration(
            &mut chunk,
            &generator,
            &workspace,
            Some(StructureFeatureIndex::explicit_count(0)),
        )
        .expect("an empty selected-feature pass must succeed");
        assert_eq!(
            writebacks.len(),
            8,
            "the successful pass still returns its eight owned distance-one dependency holders"
        );
    }

    /// The decoration-seed prologue is deterministic and matches the pinned
    /// seed-42 goldens: chunk (0,0) has section origin (0, -64, 0), and
    /// `setDecorationSeed(42, 0, 0)` == 42 == the world seed (both scale terms
    /// vanish), so a seed-42 run at the origin chunk decorates with seed 42.
    /// Chunk (1,0) has origin (16, -64, 0); `setDecorationSeed(42, 16, 0)` is
    /// pinned to the literal golden `-1348197766006825830` (computed against a
    /// live Paper 26.2 load and cross-checked against the crate's
    /// `set_decoration_seed(12345, 3, -7)` golden) — a nonzero-coordinate
    /// literal, not merely "differ from the world seed".
    #[test]
    fn decoration_prologue_matches_pinned_seed42_golden() {
        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        let origin_seed_00 = random.set_decoration_seed(42, 0, 0);
        assert_eq!(origin_seed_00, 42, "chunk (0,0) must decorate with seed 42");

        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        let origin_seed_10 = random.set_decoration_seed(42, 16, 0);
        assert_eq!(
            origin_seed_10, -1348197766006825830,
            "chunk (1,0)'s decoration seed must match the pinned Paper golden"
        );
        // The derived seed is the same regardless of the unique seed base
        // (`set_decoration_seed` resets the source to the world seed first),
        // which is exactly why it is deterministic per chunk.
        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        assert_eq!(
            random.set_decoration_seed(42, 16, 0),
            origin_seed_10,
            "the decoration seed must not depend on the unique-seed base"
        );
    }

    /// The decoration-seed prologue is position- and world-seed-sensitive: a
    /// different chunk section or a different world seed must derive a
    /// different decoration seed (non-vacuous — the prologue actually feeds
    /// position and seed into the RNG).
    #[test]
    fn decoration_prologue_seed_is_position_and_seed_sensitive() {
        let seed = |world_seed: i64, chunk_x: i32, chunk_z: i32| {
            WorldgenRandom::new(XoroshiroRandomSource::new(
                random_support::generate_unique_seed(),
            ))
            .set_decoration_seed(world_seed, chunk_x, chunk_z)
        };
        assert_ne!(seed(42, 0, 0), seed(42, 1, 0), "x position must matter");
        assert_ne!(seed(42, 0, 0), seed(42, 0, 1), "z position must matter");
        assert_ne!(seed(42, 0, 0), seed(43, 0, 0), "world seed must matter");
    }

    /// The consuming FULL seam receives a semantically valid SPAWN parent proto
    /// (`ChunkFullTask.run`'s `new LevelChunk(level, protoChunk, postLoad)`,
    /// Paper `LevelChunk.java` 177): no generated proto is stamped `FULL`.
    /// This unit constructs the parent directly because the holder's normal
    /// LIGHT/SPAWN prerequisites remain a downstream scheduler concern. The
    /// holder moves its SPAWN chunk out by value, and the returned runtime chunk
    /// reports persisted `FULL`; represented entities and block ticks are
    /// retained for their deferred runtime hooks.
    #[test]
    fn full_seam_promotes_semantically_valid_spawn_parent() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(3, -2));
        holder.generate_through(ChunkStatus::Noise).expect("NOISE");
        let mut entity = CompoundTag::new();
        entity.put_string("id", "minecraft:pig");
        holder.chunk.add_entity(entity);
        holder
            .chunk
            .get_block_ticks_mut()
            .schedule(ScheduledTick::probe(Blocks::AIR, BlockPos::new(1, -64, 1)));
        holder
            .chunk
            .get_fluid_ticks_mut()
            .schedule(ScheduledTick::probe(
                FluidId::WATER,
                BlockPos::new(2, -63, 2),
            ));
        let mut block_entity = CompoundTag::new();
        block_entity.put_int("x", 2);
        block_entity.put_int("y", -63);
        block_entity.put_int("z", 3);
        block_entity.put_string("id", "minecraft:chest");
        holder.chunk.base_mut().set_block_entity_nbt(block_entity);
        holder
            .chunk
            .mark_pos_for_post_processing(&BlockPos::new(4, -63, 5));
        holder
            .chunk
            .set_heightmap(Types::WorldSurface, &vec![7; 37]);
        holder.chunk.set_sky_emptiness_map(Some(vec![true; 24]));
        holder.chunk.set_light_correct(true);
        holder.chunk.base_mut().set_inhabited_time(1234);
        let village = Identifier::parse("minecraft:village");
        holder.chunk.set_start_for_structure(village.clone(), 11);
        holder.chunk.add_reference_for_structure(village, 13);
        holder.chunk.set_persisted_status(ChunkStatus::Spawn);

        let (chunk, generated_light_storage) = holder
            .into_level_chunk()
            .expect("the SPAWN-parent generated chunk promotes");
        assert!(generated_light_storage.is_none());
        assert_eq!(chunk.pos(), ChunkPos::new(3, -2));
        assert_eq!(chunk.get_x(), 3);
        assert_eq!(chunk.get_z(), -2);
        assert_eq!(chunk.get_min_y(), -64);
        assert_eq!(chunk.get_height(), 384);
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Full);
        assert_eq!(chunk.post_load_entities().len(), 1);
        assert_eq!(chunk.stored_block_ticks().len(), 1);
        assert_eq!(chunk.stored_fluid_ticks().len(), 1);
        assert_eq!(chunk.pending_block_entities().len(), 1);
        assert!(chunk.is_light_correct());
        assert_eq!(chunk.inhabited_time(), 1234);
        assert!(chunk.is_unsaved());
        assert_eq!(chunk.sky_emptiness_map(), Some(&[true; 24][..]));
        assert_eq!(chunk.post_processing()[0].len(), 1);
        assert_eq!(
            chunk
                .get_all_starts()
                .get(&Identifier::parse("minecraft:village")),
            Some(&11)
        );
        assert_eq!(chunk.structures_references()[0].references, vec![13]);
        let (_, world_surface) = chunk
            .client_heightmaps()
            .into_iter()
            .find(|(ty, _)| {
                *ty == rivet_protocol::protocol::game::heightmap_types::HeightmapType::WorldSurface
            })
            .expect("FULL promotion carries final heightmaps");
        assert_eq!(world_surface, vec![7; 37]);
    }

    #[test]
    fn full_promotion_returns_owned_light_workspace_on_success() {
        let generator = test_generator();
        let center = ChunkPos::ZERO;
        let mut holder = generator.create_holder(center);
        assert!(
            holder
                .attach_generated_light_workspace(full_light_workspace_for_test(center))
                .is_none()
        );
        holder.chunk.set_persisted_status(ChunkStatus::Spawn);

        let (chunk, storage) = holder
            .into_level_chunk()
            .expect("the SPAWN parent promotes with a light workspace");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Full);
        assert_eq!(storage.expect("workspace returned on success").len(), 24);
    }

    #[test]
    fn full_conversion_error_returns_owned_light_workspace() {
        let generator = test_generator();
        let center = ChunkPos::ZERO;
        let mut holder = generator.create_holder(center);
        assert!(
            holder
                .attach_generated_light_workspace(full_light_workspace_for_test(center))
                .is_none()
        );
        holder.chunk.set_persisted_status(ChunkStatus::Noise);

        let error = match holder.into_level_chunk() {
            Ok(_) => panic!("a non-SPAWN parent must refuse FULL conversion"),
            Err(error) => error,
        };
        match error {
            GeneratedChunkError::Convert {
                error: LevelChunkBridgeError::GeneratedStatusNotSpawn(ChunkStatus::Noise),
                generated_light_storage: Some(storage),
            } => assert_eq!(storage.len(), 24),
            other => panic!("unexpected conversion error: {other:?}"),
        }
    }

    /// The ordinary holder cannot claim the SPAWN-parent seam without the
    /// downstream LIGHT/SPAWN prerequisites. A failed end-to-end attempt leaves
    /// the actual status below SPAWN, and the consuming conversion refuses that
    /// status rather than treating it as a ready generated parent.
    #[test]
    fn holder_cannot_claim_spawn_readiness_without_prerequisites() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(-4, 6));
        let generation = holder.generate_through(ChunkStatus::Spawn);
        assert!(
            generation.is_err(),
            "the normal holder lacks LIGHT prerequisites"
        );
        let actual_status = holder.status();
        assert_ne!(actual_status, ChunkStatus::Spawn);
        assert!(matches!(
            holder.into_level_chunk(),
            Err(GeneratedChunkError::Convert {
                error: LevelChunkBridgeError::GeneratedStatusNotSpawn(status),
                generated_light_storage: None,
            }) if status == actual_status
        ));
    }

    /// Every status except the exact SPAWN parent is refused atomically when the
    /// holder is consumed, with a typed `Convert` carrying the bridge's
    /// `GeneratedStatusNotSpawn(actual_status)` — the status gate fires before the proto is
    /// consumed, so there is no clone, no partial promote, and no status
    /// fabrication. A fresh EMPTY holder and a real generated (NOISE) holder
    /// both refuse with their actual status.
    #[test]
    fn every_non_spawn_status_is_refused_atomically_on_consumption() {
        let generator = test_generator();
        for status in ChunkStatus::ALL {
            if status == ChunkStatus::Spawn {
                continue;
            }
            let mut holder = generator.create_holder(ChunkPos::ZERO);
            holder.chunk.set_persisted_status(status);
            let error = holder
                .into_level_chunk()
                .err()
                .expect("a non-SPAWN holder must not promote");
            assert!(
                matches!(
                    &error,
                    GeneratedChunkError::Convert {
                        error: LevelChunkBridgeError::GeneratedStatusNotSpawn(s),
                        ..
                    } if *s == status
                ),
                "expected Convert(GeneratedStatusNotSpawn({status:?})), got {error:?}"
            );
        }
        // A real generated chunk (NOISE, not an arbitrary stamp) refuses with
        // its actual persisted status — the boundary holds on genuine data.
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder.generate_through(ChunkStatus::Noise).expect("NOISE");
        assert_eq!(holder.chunk.get_persisted_status(), ChunkStatus::Noise);
        assert!(matches!(
            holder.into_level_chunk(),
            Err(GeneratedChunkError::Convert {
                error: LevelChunkBridgeError::GeneratedStatusNotSpawn(ChunkStatus::Noise),
                ..
            })
        ));
    }

    /// Consuming the holder is a move, not a clone: `into_level_chunk(self)`
    /// drops the holder (and its five executor closures plus the immutable
    /// generator handle used by the explicit SPAWN-region API) when it
    /// succeeds, so the shared config's strong count returns to its base — the
    /// chunk left the holder by value, never copied. Built on an exclusive
    /// generator so no parallel test interferes with the strong count.
    #[test]
    fn into_level_chunk_moves_the_chunk_out_no_clone() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let base = Arc::strong_count(&generator);
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder.chunk.set_persisted_status(ChunkStatus::Spawn);
        assert_eq!(Arc::strong_count(&generator), base + 6);
        let (_chunk, generated_light_storage) = holder.into_level_chunk().expect("FULL promotes");
        assert!(generated_light_storage.is_none());
        assert_eq!(
            Arc::strong_count(&generator),
            base,
            "the consumed holder must drop its five closure clones and generator handle"
        );
    }

    /// Conversion-error atomicity at the holder boundary: a SPAWN-parent proto
    /// carrying a hostile persisted Starlight state is refused as
    /// `Convert(UnsupportedLightState)` — the same value-layer gate that
    /// `from_generated_spawn_proto` runs before the `map_values` transform — rather
    /// than a fabricated or half-promoted chunk. The promoted position is never
    /// produced, so the caller cannot install it.
    #[test]
    fn conversion_error_is_atomic_for_hostile_spawn_parent() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        assert!(
            holder
                .attach_generated_light_workspace(full_light_workspace_for_test(ChunkPos::ZERO))
                .is_none()
        );
        holder.chunk.set_persisted_status(ChunkStatus::Spawn);
        // `InitState::Other` is a persisted Starlight state the #184 send seam
        // cannot represent — it must surface as Convert(UnsupportedLightState),
        // not a panic or a partial promote. The detached workspace must still
        // carry every owned neighbour on this conversion error.
        let mut nibbles = vec![SwmrNibbleArray::new_with_bytes(vec![0xAB; ARRAY_SIZE]); 26];
        nibbles[3] = SwmrNibbleArray::new_with_state(None, InitState::Other(5));
        holder.chunk.set_block_nibbles(nibbles);
        match holder.into_level_chunk() {
            Err(GeneratedChunkError::Convert {
                error: LevelChunkBridgeError::UnsupportedLightState(_),
                generated_light_storage: Some(storage),
            }) => assert_eq!(storage.len(), 24),
            _ => panic!("unexpected conversion result"),
        }
    }

    /// The install seam composes exactly: a refused conversion never reaches
    /// the map. This drives the real composition — promote, and only on `Ok`
    /// call `chunk_map_mut().install(pos, chunk)` — against one mutable
    /// `ServerLevel`; a NOISE chunk's refusal means the `Ok` arm never runs, so
    /// the position is not served and no pre-existing chunk is replaced. (If a
    /// conversion ever started returning `Ok` for a non-SPAWN proto, the
    /// `panic` fires; if the composition installed on refusal, the assertion
    /// fails.)
    #[test]
    fn no_install_on_non_spawn_conversion_refusal() {
        let generator = test_generator();
        let pos = ChunkPos::new(3, 4);
        let mut holder = generator.create_holder(pos);
        holder.generate_through(ChunkStatus::Noise).expect("NOISE");

        let mut world = ServerLevel::new_region_backed(ServerLevelConfig::default());
        match holder.into_level_chunk() {
            Ok((chunk, _generated_light_storage)) => {
                world.chunk_map_mut().install(pos, chunk);
                panic!("a NOISE chunk must not promote as FULL");
            }
            Err(_) => {
                // The composition boundary: install only on Ok. A refused
                // conversion reaches no install, so the position is unserved
                // and any pre-existing chunk would be untouched.
            }
        }
        assert!(
            world.chunk_map().get_chunk(pos).is_none(),
            "a refused conversion must not reach the install seam"
        );
    }

    /// A promoted chunk installs at exactly its own position through the
    /// tick-thread seam `chunk_map_mut().install(pos, chunk)`: it is served at
    /// that position and nowhere else.
    #[test]
    fn promoted_chunk_installs_at_exact_position() {
        let generator = test_generator();
        let pos = ChunkPos::new(7, -3);
        let chunk = {
            let mut holder = generator.create_holder(pos);
            holder.chunk.set_persisted_status(ChunkStatus::Spawn);
            let (chunk, generated_light_storage) =
                holder.into_level_chunk().expect("FULL promotes");
            assert!(generated_light_storage.is_none());
            chunk
        };

        let mut world = ServerLevel::new_region_backed(ServerLevelConfig::default());
        world.chunk_map_mut().install(pos, chunk);
        assert_eq!(world.chunk_map().get_chunk(pos).unwrap().pos(), pos);
        assert!(
            world.chunk_map().get_chunk(ChunkPos::ZERO).is_none(),
            "installing at {pos:?} must not fabricate the spawn chunk"
        );
    }

    /// Duplicate/replacement semantics of the install seam match the current
    /// `ChunkMap` contract: `install` is `chunks.insert(pos, chunk)`, so a
    /// second install at the same position atomically replaces the first (the
    /// map stays one chunk, serving the *replacement*, never duplicating or
    /// keeping the first). The two promoted chunks are observably
    /// distinguishable by a typed structure start, so the assertion proves the
    /// second is served and the first is gone — not merely that the map has one
    /// entry.
    #[test]
    fn install_replaces_existing_chunk_at_same_position() {
        let generator = test_generator();
        let pos = ChunkPos::new(1, 1);
        let promote = |generator: Arc<OverworldGenerator>, start: i64| {
            let mut holder = generator.create_holder(pos);
            holder
                .chunk
                .set_start_for_structure(Identifier::parse("minecraft:village"), start);
            holder.chunk.set_persisted_status(ChunkStatus::Spawn);
            let (chunk, generated_light_storage) =
                holder.into_level_chunk().expect("FULL promotes");
            assert!(generated_light_storage.is_none());
            chunk
        };

        let mut world = ServerLevel::new_region_backed(ServerLevelConfig::default());
        world
            .chunk_map_mut()
            .install(pos, promote(generator.clone(), 42));
        world.chunk_map_mut().install(pos, promote(generator, 99));

        assert_eq!(
            world.chunk_map().len(),
            1,
            "replacement must not grow the map"
        );
        let served = world.chunk_map().get_chunk(pos).expect("position served");
        // The second install's chunk is what the map serves: start 99, not the
        // replaced first chunk's 42.
        assert_eq!(
            served
                .get_all_starts()
                .get(&Identifier::parse("minecraft:village")),
            Some(&99)
        );
    }

    /// Ownership: the holder owns its ProtoChunk by value (no `Arc<RwLock>`
    /// game state) while the immutable worldgen config is shared across holders
    /// by `Arc` — the five executor closures (BIOMES, NOISE, SURFACE, CARVERS,
    /// FEATURES) plus the holder's explicit SPAWN-region handle each capture a
    /// clone. This test builds its own exclusive
    /// generator (the shared `LazyLock` would be touched by the other parallel
    /// tests, making the strong count global/racy).
    #[test]
    fn holder_owns_chunk_by_value_and_shares_immutable_config() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let base = Arc::strong_count(&generator);
        let holder = generator.create_holder(ChunkPos::new(2, 3));
        // Five executor closures and the holder's SPAWN-region handle each hold
        // a clone of the shared generator.
        assert_eq!(Arc::strong_count(&generator), base + 6);
        drop(holder);
        assert_eq!(Arc::strong_count(&generator), base);
    }

    /// The FEATURES region uses the complete 17x17 dependency window: it
    /// borrows the center CARVERS chunk, owns CARVERS chunks at distances 1,
    /// and owns STRUCTURE_STARTS chunks through distance 8. The nine
    /// `ChunkPos.rangeClosed(center, 1)` reads used by the biome union still
    /// resolve to their own positions, and the outer dependency ring is also
    /// present at its required status.
    #[test]
    fn feature_region_uses_dependency_window_borrowing_center_and_owning_ring() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");

        let region = compose_feature_region(&mut holder.chunk, &generator);
        assert_eq!(region.get_center(), ChunkPos::new(0, 0));
        for pos in ChunkPos::range_closed(&ChunkPos::new(0, 0), 1) {
            let chunk = region.get_chunk(pos.x(), pos.z());
            assert_eq!(
                chunk.get_pos(),
                pos,
                "the dependency window must serve every 3x3 chunk at its own position"
            );
        }
        assert_eq!(
            region
                .try_get_chunk(8, 0, ChunkStatus::StructureStarts, true)
                .expect("the outer FEATURES dependency ring must be present")
                .get_pos(),
            ChunkPos::new(8, 0)
        );
        assert!(
            region
                .try_get_chunk(9, 0, ChunkStatus::Empty, true)
                .is_err(),
            "the FEATURES cache must stop at the direct dependency radius"
        );
    }

    #[test]
    fn features_region_retains_distance_one_proto_writebacks_in_cache_order() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let mut region = compose_feature_region(&mut holder.chunk, &generator);
        let east_pos = BlockPos::new(16, 64, 0);
        assert!(<WorldGenRegion<
            '_,
            BlockState,
            WorldgenBiomeId,
            StructureKey,
        > as WorldGenLevel>::set_block(
            &mut region,
            &east_pos,
            Blocks::STONE.default_block_state(),
            2,
        ));

        let writebacks = region.into_distance_one_proto_writebacks();
        let positions: Vec<_> = writebacks.iter().map(|(pos, _)| *pos).collect();
        assert_eq!(positions.len(), 8, "all eight distance-1 protos stay owned");
        assert_eq!(
            positions,
            vec![
                ChunkPos::new(-1, -1),
                ChunkPos::new(-1, 0),
                ChunkPos::new(-1, 1),
                ChunkPos::new(0, -1),
                ChunkPos::new(0, 1),
                ChunkPos::new(1, -1),
                ChunkPos::new(1, 0),
                ChunkPos::new(1, 1),
            ],
            "writebacks preserve StaticCache2D's x-major/z-inner order"
        );
        let east = writebacks
            .into_iter()
            .find(|(pos, _)| *pos == ChunkPos::new(1, 0))
            .map(|(_, chunk)| chunk)
            .expect("east distance-1 proto writeback");
        assert_eq!(
            east.get_block_state(0, 64, 0),
            Blocks::STONE.default_block_state(),
            "a successful feature-region write must survive region consumption"
        );
    }

    #[test]
    fn features_region_persists_status_and_heightmaps_across_the_full_window() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        holder.chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
        let region = compose_feature_region(&mut holder.chunk, &generator);

        for dx in -8i32..=8 {
            for dz in -8i32..=8 {
                let distance = dx.abs().max(dz.abs());
                let expected_status = if distance <= 1 {
                    ChunkStatus::Carvers
                } else {
                    ChunkStatus::StructureStarts
                };
                let chunk = region
                    .try_get_chunk(dx, dz, expected_status, true)
                    .expect("every FEATURES dependency must be available");
                let diagnostic = match region.try_get_chunk(dx, dz, ChunkStatus::Full, true) {
                    Ok(_) => panic!("FULL must exceed every FEATURES dependency ring"),
                    Err(diagnostic) => diagnostic,
                };
                assert_eq!(diagnostic.actual_status, Some(expected_status));
                assert_eq!(diagnostic.max_allowed_status, Some(expected_status));
                for ty in FINAL_HEIGHTMAPS {
                    assert!(
                        chunk.heightmaps()[ty as usize].is_some(),
                        "dependency ({dx},{dz}) must persist {ty:?}"
                    );
                }
            }
        }
    }

    /// The real FEATURES-pass region must materialize the entities that
    /// `MonsterRoomFeature` queries immediately after chest/spawner writes; the
    /// default `WorldGenLevel` entity seams are panic-only for other worlds.
    #[test]
    fn features_region_materializes_monster_room_entities() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let mut region = compose_feature_region(&mut holder.chunk, &generator);

        let chest_pos = BlockPos::new(0, 0, 0);
        assert!(<WorldGenRegion<
            '_,
            BlockState,
            WorldgenBiomeId,
            StructureKey,
        > as WorldGenLevel>::set_block(
            &mut region,
            &chest_pos,
            Blocks::CHEST.default_block_state(),
            2,
        ));
        assert!(<WorldGenRegion<
            '_,
            BlockState,
            WorldgenBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_randomizable_container(
            &region, &chest_pos
        ));
        <WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> as WorldGenLevel>::set_block_entity_loot_table(
            &mut region,
            &chest_pos,
            42,
            "minecraft:chests/simple_dungeon",
        );

        let spawner_pos = BlockPos::new(1, 0, 0);
        assert!(<WorldGenRegion<
            '_,
            BlockState,
            WorldgenBiomeId,
            StructureKey,
        > as WorldGenLevel>::set_block(
            &mut region,
            &spawner_pos,
            Blocks::SPAWNER.default_block_state(),
            2,
        ));
        assert!(<WorldGenRegion<
            '_,
            BlockState,
            WorldgenBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_spawner_block_entity(
            &region, &spawner_pos
        ));
        assert_eq!(
            <WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region,
                &spawner_pos,
            ),
            None,
            "a fresh DUMMY spawner has no spawn-potential draw"
        );
        <WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &spawner_pos,
            "minecraft:zombie",
            None,
        );
        drop(region);

        let chest_tag = holder
            .chunk
            .get_block_entity_nbts()
            .get(&chest_pos)
            .expect("FEATURES chest NBT must survive region drop");
        assert_eq!(
            chest_tag.get_string("id").map(String::as_str),
            Some("DUMMY")
        );
        assert_eq!(
            chest_tag.get_string("LootTable").map(String::as_str),
            Some("minecraft:chests/simple_dungeon")
        );
        assert_eq!(chest_tag.get_long("LootTableSeed"), Some(42));

        let spawner_tag = holder
            .chunk
            .get_block_entity_nbts()
            .get(&spawner_pos)
            .expect("FEATURES spawner NBT must survive region drop");
        let spawn_data = spawner_tag
            .get_compound("SpawnData")
            .expect("spawner SpawnData must be persisted");
        assert_eq!(
            spawn_data
                .get_compound("entity")
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:zombie")
        );
        assert!(
            spawner_tag
                .get_list("SpawnPotentials")
                .is_some_and(|potentials| potentials.is_empty()),
            "setEntityId must persist explicit empty SpawnPotentials"
        );
    }

    #[test]
    fn features_region_resets_dummy_spawner_payloads_before_materialization() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");

        let preserved_pos = BlockPos::new(0, 0, 0);
        let selected_pos = BlockPos::new(1, 0, 0);
        let malformed_pos = BlockPos::new(2, 0, 0);

        let mut initial_region = compose_feature_region(&mut holder.chunk, &generator);
        for pos in [preserved_pos, selected_pos, malformed_pos] {
            assert!(initial_region.set_block(&pos, Blocks::SPAWNER.default_block_state(), 2, 512));
        }
        drop(initial_region);

        let mut preserved = CompoundTag::new();
        preserved.put_int("x", preserved_pos.get_x());
        preserved.put_int("y", preserved_pos.get_y());
        preserved.put_int("z", preserved_pos.get_z());
        preserved.put_string("id", "DUMMY");
        preserved.put_int("Delay", 17);
        preserved.put_int("MinSpawnDelay", 31);
        preserved.put_int("MaxSpawnDelay", 63);
        preserved.put_int("SpawnCount", 5);
        preserved.put_int("MaxNearbyEntities", 9);
        preserved.put_int("RequiredPlayerRange", 12);
        preserved.put_int("SpawnRange", 6);
        let mut preserved_entity = CompoundTag::new();
        preserved_entity.put_string("id", "minecraft:skeleton");
        preserved_entity.put_int("CustomEntityField", 23);
        let mut preserved_data = CompoundTag::new();
        preserved_data.put("entity".to_string(), Tag::Compound(preserved_entity));
        preserved_data.put_int("CustomSpawnDataField", 29);
        preserved.put("SpawnData".to_string(), Tag::Compound(preserved_data));
        let mut preserved_potential = CompoundTag::new();
        preserved_potential.put_int("weight", 4);
        let mut preserved_potential_data = CompoundTag::new();
        let mut preserved_potential_entity = CompoundTag::new();
        preserved_potential_entity.put_string("id", "minecraft:creeper");
        preserved_potential_data.put(
            "entity".to_string(),
            Tag::Compound(preserved_potential_entity),
        );
        preserved_potential.put("data".to_string(), Tag::Compound(preserved_potential_data));
        let mut preserved_potentials = ListTag::new();
        preserved_potentials
            .list
            .push(Tag::Compound(preserved_potential));
        preserved.put(
            "SpawnPotentials".to_string(),
            Tag::List(preserved_potentials),
        );
        holder.chunk.base_mut().set_block_entity_nbt(preserved);

        let mut selected = CompoundTag::new();
        selected.put_int("x", selected_pos.get_x());
        selected.put_int("y", selected_pos.get_y());
        selected.put_int("z", selected_pos.get_z());
        selected.put_string("id", "DUMMY");
        selected.put_int("Delay", 19);
        let mut selected_entry = CompoundTag::new();
        selected_entry.put_int("weight", 1);
        let mut selected_data = CompoundTag::new();
        let mut selected_entity = CompoundTag::new();
        selected_entity.put_string("id", "minecraft:spider");
        selected_entity.put_int("SelectedEntityField", 37);
        selected_data.put("entity".to_string(), Tag::Compound(selected_entity));
        selected_data.put_int("SelectedSpawnDataField", 41);
        selected_entry.put("data".to_string(), Tag::Compound(selected_data));
        let mut selected_potentials = ListTag::new();
        selected_potentials.list.push(Tag::Compound(selected_entry));
        selected.put(
            "SpawnPotentials".to_string(),
            Tag::List(selected_potentials),
        );
        holder.chunk.base_mut().set_block_entity_nbt(selected);

        let mut malformed = CompoundTag::new();
        malformed.put_int("x", malformed_pos.get_x());
        malformed.put_int("y", malformed_pos.get_y());
        malformed.put_int("z", malformed_pos.get_z());
        malformed.put_string("id", "DUMMY");
        malformed.put_int("Delay", 23);
        malformed.put("SpawnData".to_string(), Tag::List(ListTag::new()));
        holder.chunk.base_mut().set_block_entity_nbt(malformed);

        let mut region = compose_feature_region(&mut holder.chunk, &generator);
        for pos in [preserved_pos, selected_pos, malformed_pos] {
            assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), 2, 512));
            assert_eq!(
                <WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                    &region, &pos,
                ),
                None,
                "DUMMY payload must not create a live spawn-potential draw"
            );
        }

        <WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &preserved_pos,
            "minecraft:zombie",
            Some(0),
        );
        <WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &selected_pos,
            "minecraft:zombie",
            Some(0),
        );
        <WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &malformed_pos,
            "minecraft:zombie",
            Some(0),
        );
        drop(region);

        let persisted = holder
            .chunk
            .get_block_entity_nbts()
            .get(&preserved_pos)
            .expect("preserved spawner payload");
        for key in [
            "Delay",
            "MinSpawnDelay",
            "MaxSpawnDelay",
            "SpawnCount",
            "MaxNearbyEntities",
            "RequiredPlayerRange",
            "SpawnRange",
        ] {
            assert_eq!(persisted.get_int(key), None, "stale field {key}");
        }
        let preserved_data = persisted
            .get_compound("SpawnData")
            .expect("materialized SpawnData");
        assert_eq!(preserved_data.get_int("CustomSpawnDataField"), None);
        let preserved_entity = preserved_data
            .get_compound("entity")
            .expect("materialized entity payload");
        assert_eq!(
            preserved_entity.get_string("id").map(String::as_str),
            Some("minecraft:zombie")
        );
        assert_eq!(preserved_entity.get_int("CustomEntityField"), None);
        assert!(
            persisted
                .get_list("SpawnPotentials")
                .is_some_and(ListTag::is_empty)
        );

        let selected = holder
            .chunk
            .get_block_entity_nbts()
            .get(&selected_pos)
            .expect("selected spawner payload");
        let selected_data = selected
            .get_compound("SpawnData")
            .expect("materialized SpawnData");
        assert_eq!(selected_data.get_int("SelectedSpawnDataField"), None);
        assert_eq!(
            selected_data
                .get_compound("entity")
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:zombie")
        );
        assert_eq!(
            selected_data
                .get_compound("entity")
                .and_then(|entity| entity.get_int("SelectedEntityField")),
            None
        );
        assert!(
            selected
                .get_list("SpawnPotentials")
                .is_some_and(ListTag::is_empty)
        );

        let repaired = holder
            .chunk
            .get_block_entity_nbts()
            .get(&malformed_pos)
            .expect("malformed spawner payload");
        assert_eq!(repaired.get_int("Delay"), None);
        assert_eq!(
            repaired
                .get_compound("SpawnData")
                .and_then(|data| data.get_compound("entity"))
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:zombie")
        );
        assert!(
            repaired
                .get_list("SpawnPotentials")
                .is_some_and(ListTag::is_empty)
        );
    }

    /// A valid room-shaped shell reaches the real leaf's chest/spawner writes
    /// through the FEATURES region, not just through the leaf test double.
    #[test]
    fn monster_room_places_against_the_features_region() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let mut region = compose_feature_region(&mut holder.chunk, &generator);
        let origin = BlockPos::new(8, 64, 8);

        let mut probe = LegacyRandomSource::new(0);
        let xr = probe.next_int_bound(2) + 2;
        let zr = probe.next_int_bound(2) + 2;
        let min_x = -xr - 1;
        let max_x = xr + 1;
        let min_z = -zr - 1;
        let max_z = zr + 1;
        let stone = Blocks::STONE.default_block_state();
        let air = Blocks::AIR.default_block_state();
        for dx in min_x..=max_x {
            for dy in -1..=4 {
                for dz in min_z..=max_z {
                    let boundary = dx == min_x || dx == max_x || dz == min_z || dz == max_z;
                    let opening = dx == min_x && dz == 0 && (dy == 0 || dy == 1);
                    let state = if dy == -1 || dy == 4 || (boundary && !opening && dy == 0) {
                        stone
                    } else {
                        air
                    };
                    let pos = origin.offset(dx, dy, dz);
                    assert!(region.set_block(&pos, state, 2, 512));
                }
            }
        }

        let mut random = LegacyRandomSource::new(0);
        assert!(MONSTER_ROOM.place_with_config(
            &NoneFeatureConfiguration,
            &mut region,
            generator.as_ref(),
            &mut random,
            &origin,
        ));
        assert!(<WorldGenRegion<
            '_,
            BlockState,
            WorldgenBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_spawner_block_entity(
            &region, &origin,
        ));
    }

    #[test]
    fn ring_proto_chunk_preserves_carvers_status_and_final_heightmaps() {
        let generator = test_generator();
        let ring = generate_ring_chunk(ChunkPos::new(1, 0), &generator);
        assert_eq!(ring.get_persisted_status(), ChunkStatus::Carvers);
        for ty in FINAL_HEIGHTMAPS {
            assert!(
                ring.heightmaps()[ty as usize].is_some(),
                "CARVERS ring must retain the primed {ty:?} heightmap"
            );
        }
        assert!(
            ring.heightmaps()[Types::WorldSurfaceWg as usize].is_some(),
            "ring terrain generation must retain WORLD_SURFACE_WG"
        );
    }

    /// The seed-42 origin 3x3 biome union — the exact set the seed-42 (0,0)
    /// chunk decorates with — is `{minecraft:beach, minecraft:dark_forest,
    /// minecraft:lush_caves, minecraft:river}` (the pinned union from the live
    /// Paper load). All four resolve in `BIOME_GENERATION_SETTINGS_BY_NAME`, and
    /// so does every biome in the FULL source list (all 55), so settings
    /// resolution never blocks; the first typed blocker is the first executing
    /// placed feature's value decode, as
    /// `generate_through_features_runs_prologue_then_fails_typed` asserts.
    #[test]
    fn seed42_origin_biome_union_is_the_exact_paper_set() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");

        let region = compose_feature_region(&mut holder.chunk, &generator);
        let possible_biomes = gather_possible_biomes(&region, &generator);
        let mut names: Vec<&str> = possible_biomes.into_iter().collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                "minecraft:beach",
                "minecraft:dark_forest",
                "minecraft:lush_caves",
                "minecraft:river",
            ],
            "the seed-42 origin union must be the pinned Paper set, retainAll-ed against the biome source"
        );
        for name in &names {
            assert!(
                BIOME_GENERATION_SETTINGS_BY_NAME.contains_key(name),
                "every union biome must resolve its generation settings"
            );
        }
    }

    /// The FeatureSorter orders the step-1 features by *global first-appearance
    /// index*, not by registry id: lake_lava_underground (id 80) gets global
    /// index 0 and lake_lava_surface (id 79) index 1, so the sorted
    /// possible-features list places the underground lava lake first. The
    /// decoration then seeds the random with
    /// `setFeatureSeed(decorationSeed, globalIndexOfFeature, stepIndex)`;
    /// pin the exact RNG state that produces.
    ///
    /// This drives the sorter exactly like production (`run_biome_decoration`):
    /// from the FULL `biomeSource.possibleBiomes()` list in source order (all
    /// 55 now resolve their generated settings), not the 3x3 union — the union
    /// only picks which global indices execute. `mushroom_fields` (source index
    /// 0) still carries the two lava lakes at step 1 first, so their global
    /// first-appearance indices are 0/1.
    #[test]
    fn feature_sorter_orders_lava_lakes_by_global_index_and_seeds_them() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");

        let placed_registry_id = RegistryBuilder::new(&*PLACED_FEATURE).registry_id();
        let mut placed_by_id = HashMap::new();
        let mut settings_sources = Vec::new();
        for holder in generator.biome_source().possible_biomes() {
            let dense = dense_biome_id(&holder) as usize;
            let name = *BIOME_BY_ID
                .get(dense)
                .expect("every possible biome has a dense registry id");
            settings_sources.push((
                resolve_biome_settings(name, placed_registry_id, &mut placed_by_id)
                    .expect("every full-list biome resolves its generated settings"),
                name,
            ));
        }
        assert_eq!(settings_sources.len(), 55);
        let feature_list = build_features_per_step(
            &settings_sources,
            |(settings, _)| settings.features(),
            false,
        );

        // Every union biome has step 0 empty; step 1 (LAKES) holds the two
        // lava lakes.
        assert!(
            feature_list[0].features.is_empty(),
            "all four union biomes' step 0 (RAW_GENERATION) is empty"
        );
        let step1 = &feature_list[1];
        let global_of = |id: u32| step1.index_mapping(&Holder::reference(placed_registry_id, id));
        assert_eq!(
            global_of(80),
            Some(0),
            "lake_lava_underground is the step-1 global index 0"
        );
        assert_eq!(
            global_of(79),
            Some(1),
            "lake_lava_surface is the step-1 global index 1"
        );
        assert_eq!(
            placed_by_id.get(&80).copied(),
            Some("minecraft:lake_lava_underground"),
            "the reverse id→key map names the underground lava lake"
        );

        let feature_key_at = |step: usize, index: usize| {
            let Holder::Reference { id, .. } = &feature_list[step].features[index] else {
                panic!("sorted generated feature must be a registry reference")
            };
            placed_by_id
                .get(id)
                .copied()
                .expect("sorted generated feature id must have a reverse name")
        };
        assert_eq!(
            feature_key_at(2, 2),
            "minecraft:amethyst_geode",
            "step 2/global 2 must be amethyst_geode"
        );
        assert_eq!(
            feature_key_at(3, 2),
            "minecraft:monster_room",
            "step 3/global 2 must be monster_room"
        );

        // The exact per-feature seed: `setFeatureSeed(decorationSeed, index,
        // step)` sets `decorationSeed + index + 10000 * step`. For chunk (0,0)
        // the decoration seed is 42, so lake_lava_underground (index 0, step 1)
        // seeds with 10042 and the RNG state matches a fresh source seeded
        // directly with that value.
        let mut reference = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        reference.set_seed(10042);
        let mut decorated = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        let decoration_seed = decorated.set_decoration_seed(42, 0, 0);
        assert_eq!(decoration_seed, 42);
        decorated.set_feature_seed(decoration_seed, 0, 1);
        assert_eq!(
            decorated.next_int(),
            reference.next_int(),
            "setFeatureSeed(42, 0, 1) must seed the exact RNG state placement would consume"
        );
    }

    /// A BIOMES-complete radius-one neighbour proto with a constant biome. The
    /// real executor owns these values on the tick thread; this helper keeps
    /// the test workspace just as explicit without booting LIGHT/FULL/G4.
    fn spawn_region_neighbour(
        generator: &Arc<OverworldGenerator>,
        pos: ChunkPos,
        biome_id: u16,
    ) -> ProtoChunk<BlockState, WorldgenBiomeId, StructureKey> {
        let mut chunk = fresh_worldgen_chunk(pos, generator);
        let source = &generator.biome_source;
        chunk.fill_biomes_from_noise(source, &source.sampler, &|_| WorldgenBiomeId(biome_id));
        chunk.set_persisted_status(ChunkStatus::Biomes);
        chunk
    }

    fn spawn_region_workspace(
        generator: &Arc<OverworldGenerator>,
        center: ChunkPos,
        biome_id: u16,
    ) -> SpawnRegionProtos {
        let neighbours = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dz| (dx, dz)))
            .filter(|(dx, dz)| *dx != 0 || *dz != 0)
            .map(|(dx, dz)| {
                spawn_region_neighbour(
                    generator,
                    ChunkPos::new(center.x().wrapping_add(dx), center.z().wrapping_add(dz)),
                    biome_id,
                )
            });
        SpawnRegionProtos::new(center, neighbours).expect("complete radius-one workspace")
    }

    fn flat_spawn_holder(
        generator: &Arc<OverworldGenerator>,
        center: ChunkPos,
        biome_id: u16,
    ) -> GenerationChunkHolder {
        let mut holder = generator.create_holder(center);
        let source = &generator.biome_source;
        holder
            .chunk
            .fill_biomes_from_noise(source, &source.sampler, &|_| WorldgenBiomeId(biome_id));
        holder.chunk.set_persisted_status(ChunkStatus::Light);
        holder.chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
        for x in 0..16 {
            for z in 0..16 {
                holder.chunk.set_block_state(
                    center.get_min_block_x().wrapping_add(x),
                    0,
                    center.get_min_block_z().wrapping_add(z),
                    Blocks::SAND.default_block_state(),
                );
            }
        }
        // This fixture models a completed open-sky LIGHT result rather than
        // merely toggling `isLightCorrect`; the SPAWN brightness path reads the
        // published nibbles and must fail closed for absent light data.
        let mut sky_nibbles = holder.chunk.sky_nibbles().to_vec();
        for nibble in &mut sky_nibbles {
            nibble.set_full();
            nibble.update_visible();
        }
        holder.chunk.set_sky_nibbles(sky_nibbles);
        holder.chunk.set_light_correct(true);
        holder
    }

    /// Drive the SPAWN seam to the resolve step and force the center proto's
    /// top biome row to `biome`, returning the fresh chunk. The holder is driven
    /// through CARVERS (the real worldgen rungs) before the focused override;
    /// callers compose the production SPAWN-step region explicitly.
    fn spawn_seam_holder_with_top_biome(
        generator: &Arc<OverworldGenerator>,
        pos: ChunkPos,
        biome_id: u16,
    ) -> GenerationChunkHolder {
        let mut holder = generator.create_holder(pos);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        // `BiomeManager.getBiome` can select any of the eight surrounding quart
        // corners. At max build height both y candidates clamp to the top
        // section's final quart row. The center proto wraps x/z quart
        // coordinates exactly as its current lookup does, so filling the whole
        // 4x4 row controls this focused seam without assuming one direct quart.
        let top_y = holder
            .chunk
            .get_min_y()
            .wrapping_add(holder.chunk.get_height())
            .wrapping_sub(1);
        let section_index = holder.chunk.get_section_index(top_y);
        let section = holder.chunk.get_section_mut(section_index as usize);
        for quart_x in 0..4 {
            for quart_z in 0..4 {
                section.set_noise_biome(quart_x, 3, quart_z, WorldgenBiomeId(biome_id));
            }
        }
        holder
    }

    /// The radius-one constructor is strict: a missing neighbour, a duplicate,
    /// or a far-away proto is a typed refusal rather than a cache fallback.
    #[test]
    fn spawn_region_requires_exact_radius_one_neighbours() {
        let generator = test_generator();
        let center = ChunkPos::ZERO;
        let one = spawn_region_neighbour(&generator, ChunkPos::new(-1, -1), 3);
        let result = SpawnRegionProtos::new(center, [one]);
        assert!(matches!(
            result,
            Err(SpawnRegionError::MissingNeighbour { .. })
        ));

        let duplicate = spawn_region_neighbour(&generator, ChunkPos::new(-1, -1), 3);
        let duplicate_again = spawn_region_neighbour(&generator, ChunkPos::new(-1, -1), 3);
        let result = SpawnRegionProtos::new(center, [duplicate, duplicate_again]);
        match result {
            Err(SpawnRegionError::DuplicateChunk { pos }) => {
                assert_eq!(pos, ChunkPos::new(-1, -1));
            }
            _ => panic!("unexpected duplicate result"),
        }
    }

    /// The tick-thread scheduler can recover every owned ring proto after the
    /// bounded region is dropped; no neighbour becomes an inaccessible cache
    /// allocation.
    #[test]
    fn spawn_region_workspace_returns_owned_neighbours() {
        let generator = test_generator();
        let workspace = spawn_region_workspace(&generator, ChunkPos::ZERO, 3);
        let neighbours = workspace.into_neighbours();
        let positions: Vec<_> = neighbours.iter().map(ProtoChunk::get_pos).collect();
        assert_eq!(
            positions,
            vec![
                ChunkPos::new(-1, -1),
                ChunkPos::new(-1, 0),
                ChunkPos::new(-1, 1),
                ChunkPos::new(0, -1),
                ChunkPos::new(0, 1),
                ChunkPos::new(1, -1),
                ChunkPos::new(1, 0),
                ChunkPos::new(1, 1),
            ]
        );
    }

    /// Retrogen skips the generator body exactly as `ChunkStatusTasks` does,
    /// while the shared API still publishes SPAWN and leaves the owned cache
    /// available to the tick-thread caller.
    #[test]
    fn spawn_region_upgrading_skips_population_and_advances() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder.chunk.set_upgrading(ChunkStatus::Spawn);
        holder.chunk.set_persisted_status(ChunkStatus::Light);
        let mut workspace = spawn_region_workspace(&generator, ChunkPos::ZERO, 3);
        holder
            .generate_spawn_with_region(&mut workspace)
            .expect("retrogen SPAWN is skipped");
        assert_eq!(holder.status(), ChunkStatus::Spawn);
        assert!(!holder.chunk.is_upgrading());
        assert!(holder.chunk.get_entities().is_empty());
        assert_eq!(workspace.into_neighbours().len(), 8);
    }

    /// A retrogen marker still requires the center LIGHT prerequisite, even
    /// though the generator body and radius-one workspace are skipped.
    #[test]
    fn spawn_region_upgrading_rejects_empty_center_before_skip() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder.chunk.set_upgrading(ChunkStatus::Spawn);
        let mut workspace = spawn_region_workspace(&generator, ChunkPos::ZERO, 3);
        let err = holder
            .generate_spawn_with_region(&mut workspace)
            .expect_err("EMPTY retrogen input must not stamp SPAWN");
        assert!(matches!(
            err,
            GeneratedChunkError::Generation(GenError::SpawnNotGenerated)
        ));
        assert_eq!(holder.status(), ChunkStatus::Empty);
        assert!(holder.chunk.is_upgrading());
    }

    /// Paper skips the SPAWN generator before constructing its region, so an
    /// upgrading LIGHT-complete center advances even when the owned neighbour
    /// ring is not ready for the normal SPAWN dependency contract.
    #[test]
    fn spawn_region_upgrading_skips_unready_neighbour_before_region() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder.chunk.set_upgrading(ChunkStatus::Spawn);
        holder.chunk.set_persisted_status(ChunkStatus::Light);
        let mut workspace = spawn_region_workspace(&generator, ChunkPos::ZERO, 3);
        workspace.neighbours[0]
            .1
            .set_persisted_status(ChunkStatus::Empty);
        holder
            .generate_spawn_with_region(&mut workspace)
            .expect("Paper retrogen skip does not construct WorldGenRegion");
        assert_eq!(holder.status(), ChunkStatus::Spawn);
        assert!(!holder.chunk.is_upgrading());
    }

    /// Paper's SPAWN region reads the max build-height biome through the
    /// radius-one cache and the placement height through the center heightmap.
    /// Pin a simple 117-target column to prove the region is not using a
    /// detached center-only or a fabricated floor read.
    #[test]
    fn spawn_region_117_target_probe_reads_center_heightmap() {
        let generator = test_generator();
        let center = ChunkPos::ZERO;
        let mut chunk = fresh_worldgen_chunk(center, &generator);
        chunk.set_persisted_status(ChunkStatus::Light);
        chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
        chunk.set_block_state(0, 116, 0, Blocks::STONE.default_block_state());
        let mut workspace = spawn_region_workspace(&generator, center, 3);
        let mut region = compose_spawn_region(&mut chunk, &mut workspace, &generator)
            .expect("radius-one workspace");
        assert_eq!(
            WorldGenLevel::get_height_at(&mut region, Types::MotionBlockingNoLeaves, 0, 0),
            117
        );
    }

    #[test]
    fn spawn_heightmap_registration_includes_leaves_for_parrots() {
        assert_eq!(
            spawn_heightmap_type("minecraft:parrot"),
            Types::MotionBlocking
        );
        assert_eq!(
            spawn_heightmap_type("minecraft:ocelot"),
            Types::MotionBlocking
        );
        assert_eq!(
            spawn_heightmap_type("minecraft:chicken"),
            Types::MotionBlockingNoLeaves
        );
    }

    #[test]
    fn spawn_brightness_reads_completed_light_nibbles_and_drives_rules() {
        let generator = test_generator();
        let pos = ChunkPos::new(-8, -4);
        let mut holder = flat_spawn_holder(&generator, pos, 3);
        let mut workspace = spawn_region_workspace(&generator, pos, 3);
        let region = compose_spawn_region(&mut holder.chunk, &mut workspace, &generator)
            .expect("radius-one workspace");
        let candidate = BlockPos::new(pos.get_min_block_x(), 1, pos.get_min_block_z());
        assert!(is_bright_enough_to_spawn(&region, &candidate));
        let mut random = LegacyRandomSource::new(0x5eed);
        assert!(check_spawn_rules(
            &region,
            "minecraft:turtle",
            "minecraft:beach",
            &candidate,
            &mut random,
        ));
    }

    #[test]
    fn spawn_brightness_reads_biomes_neighbour_with_completed_light() {
        let generator = test_generator();
        let center = ChunkPos::ZERO;
        let mut holder = flat_spawn_holder(&generator, center, 3);
        let mut workspace = spawn_region_workspace(&generator, center, 3);
        let neighbour_pos = ChunkPos::new(1, 0);
        let (_, neighbour) = workspace
            .neighbours
            .iter_mut()
            .find(|(pos, _)| *pos == neighbour_pos)
            .expect("east neighbour");
        let mut sky_nibbles = neighbour.sky_nibbles().to_vec();
        for nibble in &mut sky_nibbles {
            nibble.set_full();
            nibble.update_visible();
        }
        neighbour.set_sky_nibbles(sky_nibbles);
        neighbour.set_light_correct(true);
        assert_eq!(neighbour.get_persisted_status(), ChunkStatus::Biomes);
        let region = compose_spawn_region(&mut holder.chunk, &mut workspace, &generator)
            .expect("radius-one workspace");
        assert_eq!(
            region.get_raw_brightness(&BlockPos::new(16, 1, 0)),
            Some(15)
        );
    }

    #[test]
    fn spawn_brightness_uses_open_sky_fallback_for_null_nibbles() {
        let generator = test_generator();
        let pos = ChunkPos::new(-8, -4);
        let mut holder = flat_spawn_holder(&generator, pos, 3);
        let null_sky = (0..holder.chunk.sky_nibbles().len())
            .map(|_| SwmrNibbleArray::new_with_bytes_and_null(None, true))
            .collect();
        holder.chunk.set_sky_nibbles(null_sky);
        holder.chunk.set_sky_emptiness_map(None);
        let mut workspace = spawn_region_workspace(&generator, pos, 3);
        let region = compose_spawn_region(&mut holder.chunk, &mut workspace, &generator)
            .expect("radius-one workspace");
        let candidate = BlockPos::new(pos.get_min_block_x(), 1, pos.get_min_block_z());
        assert!(is_bright_enough_to_spawn(&region, &candidate));
        let mut random = LegacyRandomSource::new(0x5eed);
        assert!(check_spawn_rules(
            &region,
            "minecraft:turtle",
            "minecraft:beach",
            &candidate,
            &mut random,
        ));
    }

    #[test]
    fn spawn_brightness_fails_closed_without_light_correctness() {
        let generator = test_generator();
        let pos = ChunkPos::new(-8, -4);
        let mut holder = flat_spawn_holder(&generator, pos, 3);
        holder.chunk.set_light_correct(false);
        let mut workspace = spawn_region_workspace(&generator, pos, 3);
        let region = compose_spawn_region(&mut holder.chunk, &mut workspace, &generator)
            .expect("radius-one workspace");
        let candidate = BlockPos::new(pos.get_min_block_x(), 1, pos.get_min_block_z());
        assert!(!is_bright_enough_to_spawn(&region, &candidate));
        let mut random = LegacyRandomSource::new(0x5eed);
        assert!(!check_spawn_rules(
            &region,
            "minecraft:turtle",
            "minecraft:beach",
            &candidate,
            &mut random,
        ));
    }

    #[test]
    fn spawn_candidate_rejection_preserves_paper_offset_draw_order() {
        let mut actual = WorldgenRandom::new(LegacyRandomSource::new(0x5eed));
        let mut expected = WorldgenRandom::new(LegacyRandomSource::new(0x5eed));
        let (mut x, mut z) = (0, 0);
        advance_spawn_candidate(&mut actual, &mut x, &mut z, 0, 0, 0, 0);

        let mut expected_x = expected
            .next_int_bound(5)
            .wrapping_sub(expected.next_int_bound(5));
        let mut expected_z = expected
            .next_int_bound(5)
            .wrapping_sub(expected.next_int_bound(5));
        while !(0..16).contains(&expected_x) || !(0..16).contains(&expected_z) {
            expected_x = expected
                .next_int_bound(5)
                .wrapping_sub(expected.next_int_bound(5));
            expected_z = expected
                .next_int_bound(5)
                .wrapping_sub(expected.next_int_bound(5));
        }
        assert_eq!((x, z), (expected_x, expected_z));
        assert_eq!(
            actual.next_float().to_bits(),
            expected.next_float().to_bits()
        );
    }

    #[test]
    fn spawn_failed_obstruction_still_consumes_paper_snap_yaw_draw() {
        let mut actual = WorldgenRandom::new(LegacyRandomSource::new(0x5eed));
        let mut expected = WorldgenRandom::new(LegacyRandomSource::new(0x5eed));
        let obstruction_ok = false;
        // The candidate passed the pre-construction gates and reached
        // Entity.snapTo. Its post-construction obstruction result is false,
        // but the yaw draw has already happened in Paper's order.
        consume_spawn_snap_yaw(&mut actual);
        assert!(!obstruction_ok);
        let _paper_yaw = expected.next_float();
        assert_eq!(
            actual.next_int_bound(5),
            expected.next_int_bound(5),
            "a failed obstruction check must not restore or skip the snap yaw draw"
        );
    }

    #[test]
    fn spawn_unsupported_boundary_consumes_paper_snap_yaw_draw() {
        let mut actual = WorldgenRandom::new(LegacyRandomSource::new(0x5eed));
        let mut expected = WorldgenRandom::new(LegacyRandomSource::new(0x5eed));
        consume_spawn_snap_yaw(&mut actual);
        let _paper_yaw = expected.next_float();
        assert_eq!(
            actual.next_float().to_bits(),
            expected.next_float().to_bits(),
            "the unsupported boundary must consume Paper's snap yaw"
        );
        assert_eq!(
            actual.next_int_bound(5),
            expected.next_int_bound(5),
            "the next candidate draw must follow Paper's snap yaw"
        );
    }

    /// The default rule is true and a non-empty CREATURE table reaches the
    /// weighted/placement/entity boundary. A turtle's placement gates run
    /// before the current entity layer refuses, without stamping SPAWN or
    /// writing fake entity NBT.
    #[test]
    fn spawn_region_default_true_fails_at_unsupported_entity() {
        let generator = test_generator();
        let pos = ChunkPos::new(-8, -4);
        let mut holder = flat_spawn_holder(&generator, pos, 3);
        let mut workspace = spawn_region_workspace(&generator, pos, 3);
        let err = holder
            .generate_spawn_with_region(&mut workspace)
            .expect_err("entered population must fail at entity boundary");
        assert!(matches!(
            err,
            GeneratedChunkError::SpawnRegion(SpawnRegionError::UnsupportedEntity {
                entity_type: "minecraft:turtle",
                ..
            })
        ));
        assert_eq!(holder.status(), ChunkStatus::Light);
        assert!(holder.chunk.get_entities().is_empty());
    }

    /// The explicit gamerule-off API remains a true no-op after the region and
    /// biome have been established. It must not be used as the default path.
    #[test]
    fn spawn_region_rule_false_bypasses_population_after_cache_reads() {
        let generator = test_generator();
        let pos = ChunkPos::new(-8, -4);
        let mut holder = flat_spawn_holder(&generator, pos, 3);
        let mut workspace = spawn_region_workspace(&generator, pos, 3);
        holder
            .generate_spawn_with_region_rule(&mut workspace, false)
            .expect("SPAWN_MOBS=false is a faithful no-op");
        assert_eq!(holder.status(), ChunkStatus::Spawn);
        assert!(holder.chunk.get_entities().is_empty());
    }

    /// Paper 26.2's pinned seed-42 query for chunk (0,0) is block (0,319,0),
    /// not the geometric center. The production holder reaches SPAWN only
    /// through the shared radius-one region API.
    #[test]
    fn spawn_seam_dark_forest_failed_roll_advances_with_zero_entities() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let dark_forest = BIOME_BY_ID
            .iter()
            .position(|name| *name == "minecraft:dark_forest")
            .expect("dark forest id") as u16;
        let mut workspace = spawn_region_workspace(&generator, ChunkPos::ZERO, dark_forest);

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(0));
        assert_eq!(probe.set_decoration_seed(42, 0, 0), 42);
        assert_eq!(probe.next_float().to_bits(), 0.7275637f32.to_bits());

        holder.chunk.set_persisted_status(ChunkStatus::Light);
        holder.chunk.set_light_correct(true);
        let roster = holder.chunk.get_entities().len();
        holder
            .generate_spawn_with_region(&mut workspace)
            .expect("failed creature-probability roll advances to SPAWN");
        assert_eq!(holder.status(), ChunkStatus::Spawn);
        assert_eq!(holder.chunk.get_entities().len(), roster);
    }

    #[test]
    fn spawn_dimensions_match_paper_entity_type_sizes() {
        assert_eq!(spawn_dimensions("minecraft:cow"), (0.9, 1.4));
        assert_eq!(spawn_dimensions("minecraft:ocelot"), (0.6, 0.7));
        assert_eq!(spawn_dimensions("minecraft:sheep"), (0.9, 1.3));
        assert_eq!(spawn_dimensions("minecraft:turtle"), (1.2, 0.4));
        assert_eq!(spawn_dimensions("minecraft:camel"), (1.7, 2.375));
    }

    #[test]
    fn spawn_dimensions_preserve_java_float_to_aabb_promotion() {
        let (width, height) = spawn_dimensions("minecraft:donkey");
        assert_eq!(width, 1.3964844_f32);
        assert_eq!(height, 1.5_f32);
        assert_eq!(f64::from(width), 1.396484375_f64);
    }

    #[test]
    fn spawn_collision_uses_paper_epsilon_and_speleothem_offset_cap() {
        const EPSILON: f64 = 1.0e-7;
        // The dynamic partial shape has a half-height, so this exercises the
        // Box path rather than accidentally testing a full cube.
        let dynamic_partial = SpawnShapeBox::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0);
        let at_epsilon = SpawnAabb::new(1.0 - EPSILON, 0.25, 0.25, 1.5, 0.4, 0.75);
        let beyond_epsilon = SpawnAabb::new(1.0 - 2.0 * EPSILON, 0.25, 0.25, 1.5, 0.4, 0.75);
        // CollisionUtil.voxelShapeIntersect ignores contact and overlap up to
        // its 1e-7 epsilon for partial VoxelShapes, but accepts a larger overlap.
        assert!(!dynamic_partial.intersects(0, 0, 0, &at_epsilon));
        assert!(dynamic_partial.intersects(0, 0, 0, &beyond_epsilon));

        const STATIC_PARTIAL_BOXES: &[StaticCollisionBox] = &[StaticCollisionBox {
            min_x: 0,
            min_y: 0,
            min_z: 0,
            max_x: 32,
            max_y: 16,
            max_z: 32,
        }];
        let static_partial = SpawnCollisionShape::StaticBoxes(STATIC_PARTIAL_BOXES);
        assert!(!static_partial.intersects(0, 0, 0, &at_epsilon));
        assert!(static_partial.intersects(0, 0, 0, &beyond_epsilon));

        // Paper's optimized Shapes.block() branch uses raw AABB intersection,
        // so the same endpoint remains a collision for an exact full cube.
        assert!(SpawnCollisionShape::Full.intersects(0, 0, 0, &at_epsilon));

        let mut negative_endpoint = None;
        let mut positive_endpoint = None;
        for x in -128..=128 {
            for z in -128..=128 {
                let pos = BlockPos::new(x, 0, z);
                let seed = rivet_util::mth::get_seed(x, 0, z);
                if seed & 15 == 0 {
                    negative_endpoint = Some(pos);
                }
                if seed & 15 == 15 {
                    positive_endpoint = Some(pos);
                }
                if negative_endpoint.is_some() && positive_endpoint.is_some() {
                    break;
                }
            }
            if negative_endpoint.is_some() && positive_endpoint.is_some() {
                break;
            }
        }
        let negative_endpoint = negative_endpoint.expect("seed search must find low nibble");
        let positive_endpoint = positive_endpoint.expect("seed search must find high nibble");
        assert_eq!(speleothem_offset(&negative_endpoint).0, -2.0 / 16.0);
        assert_eq!(speleothem_offset(&positive_endpoint).0, 2.0 / 16.0);
    }

    #[test]
    fn speleothem_collision_bounds_match_paper_columns_on_all_variants() {
        let pos = BlockPos::new(0, 0, 0);
        let pointed = BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap());
        let offset = speleothem_offset(&pos);
        let cases = [
            ("tip_merge", "up", 5.0 / 16.0, 11.0 / 16.0, 0.0, 1.0),
            ("tip", "up", 5.0 / 16.0, 11.0 / 16.0, 0.0, 11.0 / 16.0),
            ("tip", "down", 5.0 / 16.0, 11.0 / 16.0, 5.0 / 16.0, 1.0),
            ("frustum", "up", 4.0 / 16.0, 12.0 / 16.0, 0.0, 1.0),
            ("middle", "up", 3.0 / 16.0, 13.0 / 16.0, 0.0, 1.0),
            ("base", "up", 2.0 / 16.0, 14.0 / 16.0, 0.0, 1.0),
        ];
        for (thickness, direction, min_xz, max_xz, min_y, max_y) in cases {
            let state = pointed
                .set_value(
                    BlockStateProperties::SPELEOTHEM_THICKNESS,
                    PropertyValue::Enum(thickness),
                )
                .unwrap()
                .set_value(
                    BlockStateProperties::VERTICAL_DIRECTION,
                    PropertyValue::Enum(direction),
                )
                .unwrap();
            let Some(SpawnCollisionShape::Box(shape)) = speleothem_collision_shape(state, &pos)
            else {
                panic!("expected a box for pointed dripstone {thickness}/{direction}");
            };
            assert_eq!(shape.min_x, min_xz + offset.0);
            assert_eq!(shape.max_x, max_xz + offset.0);
            assert_eq!(shape.min_z, min_xz + offset.1);
            assert_eq!(shape.max_z, max_xz + offset.1);
            assert_eq!(shape.min_y, min_y);
            assert_eq!(shape.max_y, max_y);

            // A thin AABB immediately inside the true Z max collides, while a
            // same-sized AABB immediately beyond it does not. This guards the
            // horizontal max independently of the vertical max.
            let x0 = shape.min_x + 0.1;
            let x1 = x0 + 0.05;
            let y0 = shape.min_y + (shape.max_y - shape.min_y) / 2.0;
            let y1 = y0 + 0.05;
            let inside_z = shape.max_z - 0.1;
            let inside = SpawnAabb::new(x0, y0, inside_z, x1, y1, shape.max_z - 0.05);
            let outside = SpawnAabb::new(x0, y0, shape.max_z + 2.0e-7, x1, y1, shape.max_z + 0.05);
            assert!(SpawnCollisionShape::Box(shape).intersects(0, 0, 0, &inside));
            assert!(!SpawnCollisionShape::Box(shape).intersects(0, 0, 0, &outside));
        }
    }

    #[test]
    fn spawn_collision_uses_exact_full_face_shape_metadata() {
        let leaves = BlockState::of(BlockId::from_name("minecraft:oak_leaves").unwrap());
        let glass = BlockState::of(BlockId::from_name("minecraft:glass").unwrap());
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let powder_snow = BlockState::of(BlockId::from_name("minecraft:powder_snow").unwrap());
        let frosted_ice = BlockState::of(BlockId::from_name("minecraft:frosted_ice").unwrap());
        let bamboo = BlockState::of(BlockId::from_name("minecraft:bamboo").unwrap());
        let scaffolding = BlockState::of(BlockId::from_name("minecraft:scaffolding").unwrap());
        let pointed_dripstone =
            BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap());
        let sulfur_spike = BlockState::of(BlockId::from_name("minecraft:sulfur_spike").unwrap());
        let moving_piston = BlockState::of(BlockId::from_name("minecraft:moving_piston").unwrap());
        let stairs = BlockState::of(BlockId::from_name("minecraft:oak_stairs").unwrap());
        let stone_pressure_plate =
            BlockState::of(BlockId::from_name("minecraft:stone_pressure_plate").unwrap());
        let firefly_bush = BlockState::of(BlockId::from_name("minecraft:firefly_bush").unwrap());
        let rail = BlockState::of(BlockId::from_name("minecraft:rail").unwrap());
        let powered_rail = BlockState::of(BlockId::from_name("minecraft:powered_rail").unwrap());
        let activator_rail =
            BlockState::of(BlockId::from_name("minecraft:activator_rail").unwrap());
        let pos = BlockPos::new(0, 0, 0);

        assert!(
            spawn_collision_shape(leaves, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| shape.is_full())
        );
        assert!(
            spawn_collision_shape(glass, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| shape.is_full())
        );
        assert!(
            spawn_collision_shape(stone, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| shape.is_full())
        );
        assert!(
            spawn_collision_shape(powder_snow, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| !shape.is_full())
        );
        assert!(
            spawn_collision_shape(
                powder_snow,
                &pos,
                SpawnCollisionContext::Entity {
                    entity_type: "minecraft:rabbit",
                    entity_min_y: 1.0,
                },
            )
            .is_some_and(|shape| shape.is_full())
        );
        assert!(
            spawn_collision_shape(
                powder_snow,
                &pos,
                SpawnCollisionContext::Entity {
                    entity_type: "minecraft:polar_bear",
                    entity_min_y: 1.0,
                },
            )
            .is_some_and(|shape| !shape.is_full())
        );
        let unstable_scaffolding = scaffolding
            .set_value(
                BlockStateProperties::STABILITY_DISTANCE,
                PropertyValue::Int(1),
            )
            .unwrap()
            .set_value(BlockStateProperties::BOTTOM, PropertyValue::Bool(true))
            .unwrap();
        let inside_scaffolding = SpawnAabb::new(0.4, 0.05, 0.4, 0.6, 0.1, 0.6);
        assert!(
            !spawn_collision_shape(unstable_scaffolding, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| shape.intersects(0, 0, 0, &inside_scaffolding))
        );
        assert!(
            !spawn_collision_shape(
                unstable_scaffolding,
                &pos,
                SpawnCollisionContext::Entity {
                    entity_type: "minecraft:cow",
                    entity_min_y: 0.0,
                }
            )
            .is_some_and(|shape| shape.intersects(0, 0, 0, &inside_scaffolding))
        );
        // The context's entity is above SHAPE_BELOW_BLOCK, so the selected
        // shape is the two-pixel lower plate; it is below the entity AABB and
        // therefore does not itself intersect it.
        assert!(matches!(
            spawn_collision_shape(
                unstable_scaffolding,
                &pos,
                SpawnCollisionContext::Entity {
                    entity_type: "minecraft:cow",
                    entity_min_y: 0.2,
                }
            ),
            Some(SpawnCollisionShape::Box(_))
        ));
        let above_unstable_scaffolding = SpawnAabb::new(0.4, 0.2, 0.4, 0.6, 0.3, 0.6);
        assert!(
            !spawn_collision_shape(
                unstable_scaffolding,
                &pos,
                SpawnCollisionContext::Entity {
                    entity_type: "minecraft:cow",
                    entity_min_y: 0.2,
                }
            )
            .is_some_and(|shape| shape.intersects(
                0,
                0,
                0,
                &above_unstable_scaffolding
            ))
        );
        assert!(
            spawn_collision_shape(bamboo, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| !shape.is_full())
        );
        assert!(
            spawn_collision_shape(scaffolding, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| !shape.is_full())
        );
        assert!(
            spawn_collision_shape(pointed_dripstone, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| !shape.is_full())
        );
        assert!(
            spawn_collision_shape(sulfur_spike, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| !shape.is_full())
        );
        assert!(
            spawn_collision_shape(moving_piston, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| !shape.is_full())
        );
        assert!(
            static_collision_shape_of(moving_piston.id()).is_none(),
            "dynamic states must never resolve through the static collision table"
        );
        // Static stairs and fences resolve through the exact generated Paper
        // VoxelShape unions rather than failing closed. Their geometry is
        // partial, so the empty-spawn full-block predicate still accepts the
        // candidate while the initial noCollision query can intersect it.
        assert!(
            spawn_collision_shape(stairs, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| !shape.is_full())
        );
        let fence = BlockState::of(BlockId::from_name("minecraft:oak_fence").unwrap());
        assert!(
            spawn_collision_shape(fence, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| !shape.is_full())
        );
        assert!(is_valid_empty_spawn_block(stairs, "minecraft:cow", &pos));
        assert!(is_valid_empty_spawn_block(fence, "minecraft:cow", &pos));
        assert!(static_collision_shape_of(stairs.id()).is_some_and(|boxes| !boxes.is_empty()));
        assert!(static_collision_shape_of(fence.id()).is_some_and(|boxes| !boxes.is_empty()));
        let low_stair_entity = SpawnAabb::new(0.1, 0.1, 0.1, 0.2, 0.2, 0.2);
        assert!(
            spawn_collision_shape(stairs, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| shape.intersects(0, 0, 0, &low_stair_entity))
        );
        let fence_post_entity = SpawnAabb::new(0.45, 0.1, 0.45, 0.55, 0.2, 0.55);
        assert!(
            spawn_collision_shape(fence, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| shape.intersects(0, 0, 0, &fence_post_entity))
        );
        assert!(
            static_collision_shape_of(stone_pressure_plate.id())
                .is_some_and(|boxes| boxes.is_empty())
        );
        assert!(
            spawn_collision_shape(stone_pressure_plate, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| !shape.is_full())
        );
        assert!(!is_valid_empty_spawn_block(
            stone_pressure_plate,
            "minecraft:cow",
            &pos
        ));
        assert!(!is_valid_empty_spawn_block(
            leaves,
            "minecraft:parrot",
            &pos
        ));
        assert!(!is_valid_empty_spawn_block(glass, "minecraft:cow", &pos));
        assert!(is_valid_empty_spawn_block(bamboo, "minecraft:cow", &pos));
        assert!(is_valid_empty_spawn_block(
            scaffolding,
            "minecraft:cow",
            &pos
        ));
        assert!(is_valid_empty_spawn_block(
            pointed_dripstone,
            "minecraft:cow",
            &pos
        ));
        assert!(is_valid_spawn_floor(firefly_bush, "minecraft:ocelot"));
        assert!(is_valid_spawn_floor(firefly_bush, "minecraft:parrot"));
        assert!(!is_valid_spawn_floor(firefly_bush, "minecraft:cow"));
        assert!(!is_valid_spawn_floor(glass, "minecraft:cow"));
        assert!(!is_valid_spawn_floor(powder_snow, "minecraft:polar_bear"));
        assert!(is_valid_spawn_floor(
            Blocks::ICE.default_block_state(),
            "minecraft:polar_bear"
        ));
        assert!(!is_valid_spawn_floor(
            Blocks::ICE.default_block_state(),
            "minecraft:cow"
        ));
        assert!(is_valid_spawn_floor(frosted_ice, "minecraft:polar_bear"));
        assert!(!is_valid_spawn_floor(frosted_ice, "minecraft:cow"));
        assert!(!is_signal_source(rail));
        assert!(!is_signal_source(powered_rail));
        assert!(!is_signal_source(activator_rail));
    }

    #[test]
    fn static_collision_intersects_exact_boxes_and_leaves_dynamic_states_without_static_geometry() {
        let stairs = BlockState::of(BlockId::from_name("minecraft:oak_stairs").unwrap());
        let pos = BlockPos::new(0, 0, 0);
        let boxes = static_collision_shape_of(stairs.id()).expect("static stair geometry");
        assert!(!boxes.is_empty());
        let shape = spawn_collision_shape(stairs, &pos, SpawnCollisionContext::Empty)
            .expect("generated static stair geometry");

        let mut covered = None;
        let mut gap = None;
        'cells: for x in 0_i32..32 {
            for y in 0_i32..32 {
                for z in 0_i32..32 {
                    let covered_cell = boxes.iter().any(|box_| {
                        i32::from(box_.min_x) <= x
                            && x < i32::from(box_.max_x)
                            && i32::from(box_.min_y) <= y
                            && y < i32::from(box_.max_y)
                            && i32::from(box_.min_z) <= z
                            && z < i32::from(box_.max_z)
                    });
                    if covered_cell && covered.is_none() {
                        covered = Some((x, y, z));
                    } else if !covered_cell && gap.is_none() {
                        gap = Some((x, y, z));
                    }
                    if covered.is_some() && gap.is_some() {
                        break 'cells;
                    }
                }
            }
        }
        let cell_aabb = |(x, y, z): (i32, i32, i32)| {
            SpawnAabb::new(
                x as f64 / 32.0 + 0.01,
                y as f64 / 32.0 + 0.01,
                z as f64 / 32.0 + 0.01,
                (x + 1) as f64 / 32.0 - 0.01,
                (y + 1) as f64 / 32.0 - 0.01,
                (z + 1) as f64 / 32.0 - 0.01,
            )
        };
        assert!(shape.intersects(0, 0, 0, &cell_aabb(covered.expect("covered stair cell"))));
        assert!(!shape.intersects(0, 0, 0, &cell_aabb(gap.expect("stair gap cell"))));

        let moving_piston = BlockState::of(BlockId::from_name("minecraft:moving_piston").unwrap());
        assert!(moving_piston.has_dynamic_shape());
        assert!(static_collision_shape_of(moving_piston.id()).is_none());
    }

    #[test]
    fn spawn_land_pathfindability_matches_block_overrides() {
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let stairs = BlockState::of(BlockId::from_name("minecraft:oak_stairs").unwrap());
        let fence = BlockState::of(BlockId::from_name("minecraft:oak_fence").unwrap());
        let slab = BlockState::of(BlockId::from_name("minecraft:oak_slab").unwrap());
        let ladder = BlockState::of(BlockId::from_name("minecraft:ladder").unwrap());
        let powder_snow = BlockState::of(BlockId::from_name("minecraft:powder_snow").unwrap());
        let cake = BlockState::of(BlockId::from_name("minecraft:cake").unwrap());
        let flower_pot = BlockState::of(BlockId::from_name("minecraft:flower_pot").unwrap());
        let potted_oak_sapling =
            BlockState::of(BlockId::from_name("minecraft:potted_oak_sapling").unwrap());
        let dried_ghast = BlockState::of(BlockId::from_name("minecraft:dried_ghast").unwrap());
        let lantern = BlockState::of(BlockId::from_name("minecraft:lantern").unwrap());
        let scaffolding = BlockState::of(BlockId::from_name("minecraft:scaffolding").unwrap());
        let bamboo = BlockState::of(BlockId::from_name("minecraft:bamboo").unwrap());
        let pointed_dripstone =
            BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap());
        let sulfur_spike = BlockState::of(BlockId::from_name("minecraft:sulfur_spike").unwrap());
        let stone_pressure_plate =
            BlockState::of(BlockId::from_name("minecraft:stone_pressure_plate").unwrap());
        let iron_bars = BlockState::of(BlockId::from_name("minecraft:iron_bars").unwrap());
        let ender_chest = BlockState::of(BlockId::from_name("minecraft:ender_chest").unwrap());
        let copper_chain = BlockState::of(BlockId::from_name("minecraft:copper_chain").unwrap());
        let exposed_copper_statue =
            BlockState::of(BlockId::from_name("minecraft:exposed_copper_golem_statue").unwrap());
        let exposed_copper_lantern =
            BlockState::of(BlockId::from_name("minecraft:exposed_copper_lantern").unwrap());
        let exposed_copper_rod =
            BlockState::of(BlockId::from_name("minecraft:exposed_lightning_rod").unwrap());
        let exposed_copper_chest =
            BlockState::of(BlockId::from_name("minecraft:exposed_copper_chest").unwrap());
        let exposed_copper_block =
            BlockState::of(BlockId::from_name("minecraft:exposed_copper").unwrap());
        let water = BlockState::of(BlockId::from_name("minecraft:water").unwrap());
        let lava = BlockState::of(BlockId::from_name("minecraft:lava").unwrap());
        let closed_door = BlockState::of(BlockId::from_name("minecraft:oak_door").unwrap());
        let open_door = closed_door
            .set_value(BlockStateProperties::OPEN, PropertyValue::Bool(true))
            .unwrap();

        assert!(!is_pathfindable_land(stone));
        assert!(!is_pathfindable_land(stairs));
        assert!(!is_pathfindable_land(fence));
        assert!(!is_pathfindable_land(slab));
        assert!(!is_pathfindable_land(cake));
        assert!(!is_pathfindable_land(flower_pot));
        // Block names are namespaced; the Paper default LAND predicate must
        // recognize the registered `minecraft:potted_*` family as non-empty.
        assert!(!is_pathfindable_land(potted_oak_sapling));
        // DriedGhastBlock overrides LAND pathfinding to false even though its
        // generated shape metadata is not a substitute for the class override.
        assert!(!is_pathfindable_land(dried_ghast));
        assert!(!is_pathfindable_land(lantern));
        assert!(is_pathfindable_land(scaffolding));
        // These partial-collision classes have concrete Paper predicates:
        // BambooStalkBlock and SpeleothemBlock return false, while
        // BasePressurePlateBlock inherits the default non-full answer.
        assert!(!is_pathfindable_land(bamboo));
        assert!(!is_pathfindable_land(pointed_dripstone));
        assert!(!is_pathfindable_land(sulfur_spike));
        assert!(is_pathfindable_land(stone_pressure_plate));
        assert!(!is_pathfindable_land(iron_bars));
        assert!(!is_pathfindable_land(ender_chest));
        assert!(!is_pathfindable_land(copper_chain));
        assert!(!is_pathfindable_land(exposed_copper_statue));
        assert!(!is_pathfindable_land(exposed_copper_lantern));
        assert!(!is_pathfindable_land(exposed_copper_rod));
        assert!(!is_pathfindable_land(exposed_copper_chest));
        assert!(!is_pathfindable_land(exposed_copper_block));
        assert!(!is_pathfindable_land(closed_door));
        assert!(is_pathfindable_land(open_door));
        assert!(is_pathfindable_land(ladder));
        assert!(is_pathfindable_land(powder_snow));
        assert!(is_pathfindable_land(water));
        assert!(!is_pathfindable_land(lava));
    }

    #[test]
    fn spawn_on_ground_respects_region_world_border_edges() {
        let generator = test_generator();
        let center = ChunkPos::ZERO;
        let mut holder = flat_spawn_holder(&generator, center, 3);
        let mut workspace = spawn_region_workspace(&generator, center, 3);
        let mut region = compose_spawn_region(&mut holder.chunk, &mut workspace, &generator)
            .expect("radius-one workspace");
        // Bounds are [0, 2) on both axes, matching WorldBorder's half-open
        // `isWithinBounds` check. All three candidates have a sand floor and
        // two air blocks, so only the border result distinguishes them.
        region.world_border_mut().set_center(1.0, 1.0);
        region.world_border_mut().set_size(2.0);
        assert!(is_spawn_position_ok(
            &region,
            "minecraft:cow",
            &BlockPos::new(0, 1, 0),
        ));
        assert!(is_spawn_position_ok(
            &region,
            "minecraft:cow",
            &BlockPos::new(1, 1, 0),
        ));
        assert!(!is_spawn_position_ok(
            &region,
            "minecraft:cow",
            &BlockPos::new(2, 1, 0),
        ));
        // NO_RESTRICTIONS is a separate Paper placement type and remains
        // border-independent.
        assert!(is_spawn_position_ok(
            &region,
            "minecraft:fox",
            &BlockPos::new(2, 1, 0),
        ));
    }

    /// The public workspace→SPAWN path uses the caller's current border
    /// snapshot, not a default border hidden inside region composition. The
    /// default workspace reaches the typed entity boundary; the explicit
    /// one-block border rejects candidates first and advances cleanly.
    #[test]
    fn configured_border_reaches_spawn_through_public_workspace_api() {
        let generator = test_generator();
        let pos = ChunkPos::new(-8, -4);
        let mut default_holder = flat_spawn_holder(&generator, pos, 3);
        let mut default_workspace = spawn_region_workspace(&generator, pos, 3);
        assert!(matches!(
            default_holder.generate_spawn_with_region(&mut default_workspace),
            Err(GeneratedChunkError::SpawnRegion(
                SpawnRegionError::UnsupportedEntity { .. }
            ))
        ));

        let mut configured_holder = flat_spawn_holder(&generator, pos, 3);
        let neighbours = spawn_region_workspace(&generator, pos, 3).into_neighbours();
        let settings =
            WorldBorderSettings::new(10_000.0, 10_000.0, 0.2, 5.0, 5, 300, 1.0, 200, 99.0);
        let mut configured_workspace =
            SpawnRegionProtos::new_with_world_border_settings(pos, neighbours, settings)
                .expect("complete configured radius-one workspace");
        assert_eq!(configured_workspace.world_border_settings().lerp_time(), 0);
        assert_eq!(configured_workspace.world_border_settings().size(), 1.0);
        configured_holder
            .generate_spawn_with_region(&mut configured_workspace)
            .expect("configured border excludes every candidate before entity construction");
        assert_eq!(configured_holder.status(), ChunkStatus::Spawn);
    }

    #[test]
    fn spawn_region_retry_at_spawn_is_idempotent() {
        let generator = test_generator();
        let pos = ChunkPos::new(-8, -4);
        let mut holder = flat_spawn_holder(&generator, pos, 3);
        holder.chunk.set_persisted_status(ChunkStatus::Spawn);
        let mut workspace = spawn_region_workspace(&generator, pos, 3);
        holder
            .generate_spawn_with_region(&mut workspace)
            .expect("already-complete SPAWN is an idempotent no-op");
        assert_eq!(holder.status(), ChunkStatus::Spawn);
        assert!(holder.chunk.get_entities().is_empty());
    }

    /// A biome with a non-empty CREATURE list, SPAWN_MOBS=false: the shared
    /// region bypasses population faithfully (Ok, zero entities).
    #[test]
    fn spawn_seam_spawn_mobs_false_bypasses_population() {
        let generator = test_generator();
        // beach (id 3) has a non-empty CREATURE list (turtle).
        let mut holder = spawn_seam_holder_with_top_biome(&generator, ChunkPos::new(0, 0), 3);
        holder.chunk.set_persisted_status(ChunkStatus::Light);
        holder.chunk.set_light_correct(true);
        let mut workspace = spawn_region_workspace(&generator, ChunkPos::ZERO, 3);
        holder
            .generate_spawn_with_region_rule(&mut workspace, false)
            .expect("rule off bypasses population");
        assert_eq!(holder.status(), ChunkStatus::Spawn);
        assert!(holder.chunk.get_entities().is_empty());
    }

    /// A non-empty CREATURE list with SPAWN_MOBS=true and an entering first
    /// probability roll runs weighted selection and all ordinary rejection
    /// gates before failing only at unsupported entity construction.
    /// Paper's seed-42 decoration RNG at chunk (-8,-4) rolls 0.090480566 < 0.1.
    #[test]
    fn spawn_seam_entered_population_refuses_at_entity_layer() {
        let generator = test_generator();
        let pos = ChunkPos::new(-8, -4);
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(0));
        probe.set_decoration_seed(
            generator.seed(),
            pos.get_min_block_x(),
            pos.get_min_block_z(),
        );
        assert_eq!(probe.next_float().to_bits(), 0.090480566f32.to_bits());

        // beach (id 3) has a non-empty CREATURE list (turtle) at probability 0.1.
        let mut holder = flat_spawn_holder(&generator, pos, 3);
        let mut workspace = spawn_region_workspace(&generator, pos, 3);
        let err = holder
            .generate_spawn_with_region(&mut workspace)
            .expect_err("candidate reaching construction must refuse typed");
        assert!(
            matches!(
                err,
                GeneratedChunkError::SpawnRegion(SpawnRegionError::UnsupportedEntity {
                    biome: "minecraft:beach",
                    entity_type: "minecraft:turtle",
                    ..
                })
            ),
            "got: {err}"
        );
        assert_eq!(holder.status(), ChunkStatus::Light);
        assert!(holder.chunk.is_light_correct());
        assert!(holder.chunk.get_entities().is_empty());
    }
}
