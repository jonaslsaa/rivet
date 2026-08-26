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
//! so the full list resolves and the run proceeds to the per-step loop. The
//! lake, amethyst-geode, monster-room, and the Batch 2/3/4 dispatch leaves (ore,
//! disk, spring, simple_block, block_column, vines, seagrass, freeze_top_layer,
//! underwater_magma, multiface_growth) are decoded from the generated JSON and
//! run with their exact feature seeds; seed-42 `minecraft:glow_lichen` now
//! executes, then the chunk stops at the next selected typed-unavailable path:
//! `minecraft:dark_forest_vegetation` at step 9/global index 17.
//! The chunk stays CARVERS. The INITIALIZE_LIGHT/
//! LIGHT steps are executor-wired but engine-gated (the holder wires no light
//! engine, so it cannot reach LIGHT).
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

use std::collections::{HashMap, HashSet};
use std::fmt;
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
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::generated::feature_data::{
    BIOME_GENERATION_SETTINGS_BY_NAME, CONFIGURED_FEATURE_BY_NAME, MOB_SPAWN_SETTINGS_BY_NAME,
    PLACED_FEATURE_BY_NAME,
};
use rivet_registry::holder::Holder;
use rivet_registry::holder::RegistryId;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::registry_ops::RegistryOps;
use rivet_registry::{Identifier, RegistrationInfo, ResourceKey};
use rivet_serialization::codec::Codec;
use rivet_serialization::json_ops::JsonOps;
use rivet_util::StaticCache2D;
use rivet_util::WorldgenRandom;
use rivet_util::random::LegacyRandomSource;
use rivet_util::random_source::XoroshiroRandomSource;
use rivet_util::random_source::random_support;
use rivet_util::weighted::WeightedRandom;
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
use rivet_world::chunk::status::{ChunkStatus, GENERATION_PYRAMID, GenError, WorldGenContext};
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::chunk::storage::section_reconstruction::{
    BiomeId as WorldgenBiomeId, current_version_container_factory,
};
use rivet_world::chunk::upgrade_data::UpgradeData;
use rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries;
use rivet_world::level::WorldGenLevel;
use rivet_world::level::height_accessor::LevelHeightAccessor;
use rivet_world::level::height_accessor::create as create_height_accessor;
use rivet_world::levelgen::blending::blender::Blender;
use rivet_world::levelgen::feature::configurations::block_column_configuration::block_column_configuration_codec;
use rivet_world::levelgen::feature::configurations::composite_feature_configuration::composite_feature_configuration_codec;
use rivet_world::levelgen::feature::configurations::disk_configuration::disk_configuration_codec;
use rivet_world::levelgen::feature::configurations::geode_configuration::geode_configuration_codec;
use rivet_world::levelgen::feature::configurations::multiface_growth_configuration::multiface_growth_configuration_codec;
use rivet_world::levelgen::feature::configurations::ore_configuration::ore_configuration_codec;
use rivet_world::levelgen::feature::configurations::probability_feature_configuration::probability_feature_configuration_codec;
use rivet_world::levelgen::feature::configurations::random_boolean_feature_configuration::random_boolean_feature_configuration_codec;
use rivet_world::levelgen::feature::configurations::random_feature_configuration::random_feature_configuration_codec;
use rivet_world::levelgen::feature::configurations::simple_block_configuration::simple_block_configuration_codec;
use rivet_world::levelgen::feature::configurations::spring_configuration::spring_configuration_codec;
use rivet_world::levelgen::feature::configurations::underwater_magma_configuration::underwater_magma_configuration_codec;
use rivet_world::levelgen::feature::configurations::{
    FeatureConfiguration, NoneFeatureConfiguration,
};
use rivet_world::levelgen::feature::lake_feature::lake_configuration_codec;
use rivet_world::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
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

/// The overworld generated-chunk error surface — every failure is typed, never
/// a silent fallback.
#[derive(Debug)]
pub enum GeneratedChunkError {
    /// The status executor refused the promotion: a missing data prerequisite
    /// (`GenError::BiomesNotGenerated`/`DataNotCarried`), a demotion, or a
    /// wired-task mismatch. The chunk is left untouched.
    Generation(GenError),
    /// A target past `LIGHT` — the executor rejected it before running any
    /// work. Naming the requested status makes the downstream boundary explicit.
    /// (A target through a light step with no engine is instead refused as
    /// `GenError::LightEngineMissing`, and the wired FEATURES rung stops at
    /// the first selected path outside the decoded lake slice — see
    /// [`GenerationChunkHolder::new`].)
    UnsupportedStatus(ChunkStatus),
    /// The FULL conversion refused: the `LevelChunk` bridge rejected the proto.
    /// [`LevelChunkBridgeError::GeneratedStatusNotSpawn`] fires before the proto is consumed
    /// for any non-SPAWN generated parent status; `UnsupportedLightState` fires
    /// before the value transform, and `PaletteMap` arises from the `map_values`
    /// re-encode itself. A refusal never produces a partial `LevelChunk` or an
    /// install — the holder is consumed on every outcome (it is a self-taking
    /// API), so no half-promoted chunk ever escapes.
    Convert(LevelChunkBridgeError),
    /// The radius-one generated SPAWN workspace or the next entity capability
    /// refused population. The center remains below SPAWN and no entity NBT is
    /// fabricated.
    SpawnRegion(SpawnRegionError),
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
            GeneratedChunkError::Convert(inner) => write!(
                f,
                "a generated chunk could not be promoted to a FULL LevelChunk: {inner}"
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
}

impl SpawnRegionProtos {
    /// Build the exact 3x3 cache around `center` from eight owned neighbours.
    /// Input order is irrelevant; cache iteration is canonicalized by chunk
    /// coordinates when the Paper `WorldGenRegion` is composed.
    pub fn new(
        center: ChunkPos,
        neighbours: impl IntoIterator<Item = ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>>,
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
        Ok(Self { center, neighbours })
    }

    /// The center this workspace surrounds.
    pub fn center(&self) -> ChunkPos {
        self.center
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
    /// The leaked feature `RegistryAccess` — the worldgen access composed with
    /// the frozen placed/configured-feature registries the seed-42 decoder and
    /// the selector/composite features resolve their recursive `Holder`
    /// references through (the `worldgen/placed_feature` /
    /// `worldgen/configured_feature` back-reference the `#181` dispatch and the
    /// Batch 2 selector arms require). See [`build_feature_access`].
    feature_access: &'static RegistryAccess,
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
            feature_access,
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
    /// `'static` closures capture a cheap clone).
    pub fn create_holder(self: &Arc<Self>, pos: ChunkPos) -> GenerationChunkHolder {
        GenerationChunkHolder::new(pos, Arc::clone(self))
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
    context: WorldGenContext<BlockState, WorldgenBiomeId, StructureKey>,
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
    /// runs `addVanillaDecorations` faithfully: the `FINAL_HEIGHTMAPS`
    /// priming, the decoration-seed derivation, a dependency-window composition (`compose_feature_region`: a
    /// `WorldGenRegion` that borrows the center chunk and owns the 17x17
    /// FEATURES cache (288 ring holders, with CARVERS at distances 0/1 and
    /// STRUCTURE_STARTS through distance 8), and the Paper-order biome-union
    /// gather + `retainAll` — and then decodes and runs the registry-backed
    /// lake, amethyst-geode, monster-room, underwater_magma, and glow_lichen
    /// paths at their exact feature seeds before stopping at the first selected
    /// unsupported path (seed-42 chunk (0,0):
    /// `minecraft:dark_forest_vegetation` at step 9/global 17); the chunk stays
    /// CARVERS.
    pub fn new(pos: ChunkPos, generator: Arc<OverworldGenerator>) -> Self {
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
                // (`minecraft:dark_forest_vegetation`, step 9/global 17).
                // It must never be "improved" into a silent skip or a blanket
                // UnsupportedTask.
                // The closure captures one generator clone; the holder keeps
                // one additional handle for the shared SPAWN-region API.
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    run_biome_decoration(chunk, &generator)
                }
            },
        );
        // SPAWN is deliberately not attached to this center-only executor.
        // Paper's `generateSpawn` constructs a radius-one `WorldGenRegion`; the
        // production scheduler must call `generate_spawn_with_region` with its
        // owned eight-neighbour workspace rather than silently reverting to a
        // detached holder execution.
        GenerationChunkHolder {
            chunk,
            context,
            features_failure: None,
            generator,
        }
    }

    /// The chunk's persisted status — `EMPTY` before any step, `CARVERS` after a
    /// successful BIOMES→NOISE→SURFACE→CARVERS run, and never `FULL` (the
    /// executor refuses to stamp it). A FEATURES run primes the final heightmaps,
    /// drives the full 17x17 dependency-window region (the 3x3 window is only
    /// the biome union), resolves the FULL possible-biome settings and builds
    /// the FeatureSorter, decodes and runs the registry-backed lake, geode,
    /// monster-room, underwater_magma, and glow_lichen paths, and then fails
    /// typed at the first selected unsupported path (`FeaturePlacementDecode`,
    /// seed-42: `minecraft:dark_forest_vegetation` at step 9/global 17), so the
    /// chunk is never stamped FEATURES.
    pub fn status(&self) -> ChunkStatus {
        self.chunk.get_persisted_status()
    }

    /// Drive the chunk from its current persisted status through `target`
    /// (inclusive). The BIOMES→NOISE→SURFACE→CARVERS task bodies are wired (an
    /// EMPTY chunk can reach CARVERS); the FEATURES task body is wired (it runs
    /// Java's `ChunkStatusTasks.generateFeatures` + `addVanillaDecorations`'s
    /// full dependency-window composition, decodes and runs the lake, geode,
    /// and monster-room paths, and then fails typed at the first selected
    /// unsupported path — see [`GenerationChunkHolder::new`]). A
    /// target the borrowed executor cannot complete is rejected before any
    /// work with a typed error — a path through a light step with no engine
    /// is refused as `GenError::LightEngineMissing`, and a target at FULL
    /// stops before borrowed execution
    /// ([`GeneratedChunkError::UnsupportedStatus`]). The chunk is left
    /// untouched by every such refusal. (The wired FEATURES rung is the
    /// exception: it runs Java's priming prologue — heightmap priming, the
    /// decoration-seed derivation, the complete 17x17 dependency window, and
    /// the 3x3 biome union read — and then fails typed, so the center proto is
    /// rolled back to the status and data it had before this call; see
    /// [`GenerationChunkHolder::status`].)
    ///
    /// The SPAWN rung is intentionally absent from this center-only executor:
    /// Paper's task requires the scheduler-owned radius-one cache. Call
    /// [`Self::generate_spawn_with_region`] after LIGHT instead; FULL remains
    /// unwired (RivetTodo #185).
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
        // The holder can actually enter FEATURES only for a direct FEATURES
        // request: LIGHT/SPAWN are preflight-invalid without a light engine,
        // while FULL is a separate consuming promotion boundary. Snapshot the
        // whole proto before any call that can enter FEATURES, regardless of
        // whether its current status is EMPTY, an intermediate rung, or
        // CARVERS.
        let features_snapshot = (current.index() < ChunkStatus::Features.index()
            && target == ChunkStatus::Features)
            .then(|| snapshot_generated_chunk(&self.chunk));
        let result = self
            .context
            .generate_through(&GENERATION_PYRAMID, &mut self.chunk, target);
        match result {
            Ok(()) => Ok(()),
            Err(GenError::UnsupportedStatus(status)) => {
                Err(GeneratedChunkError::UnsupportedStatus(status))
            }
            Err(error) => {
                // A FEATURES body can place real data before it reaches its
                // typed boundary. The status deliberately remains CARVERS, so
                // cache that terminal boundary and make later attempts return
                // the same error without repeating placement against the
                // partially realized proto.
                if is_features_failure(error)
                    && self.chunk.get_persisted_status() == ChunkStatus::Carvers
                    && target.index() >= ChunkStatus::Features.index()
                {
                    if let Some(snapshot) = features_snapshot {
                        self.chunk = snapshot;
                    }
                    self.features_failure = Some(error);
                }
                Err(GeneratedChunkError::Generation(error))
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
    /// case, but no `LevelChunk` is ever produced). Because this is a consuming
    /// (`self`) API, on *any* outcome the holder is dropped — the caller never
    /// recovers the original `ProtoChunk`; the guarantee is only that no
    /// half-promoted chunk or install ever escapes.
    pub fn into_level_chunk(self) -> Result<LevelChunk, GeneratedChunkError> {
        LevelChunk::from_generated_spawn_proto(self.chunk).map_err(GeneratedChunkError::Convert)
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
/// glow_lichen leaves with their exact feature seeds. It fails typed
/// (`GenError::FeaturePlacementDecode`) at the first unsupported selected
/// feature — seed-42 chunk (0,0), step 9/global index 17:
/// `minecraft:dark_forest_vegetation`. The generated settings tables are the
/// full 55-biome surface (no `SettingsNotGenerated`), so this boundary is
/// reached deterministically every run. No biome is fabricated or silently
/// skipped.
///
/// Compose the FEATURES `WorldGenRegion` over the complete accumulated
/// dependency window of the FEATURES step. Paper's direct dependencies are
/// `CARVERS` at distances 0 and 1, followed by `STRUCTURE_STARTS` through
/// distance 8, so the cache is 17x17. The decoration biome union reads only
/// the center 3x3, but placement and worldgen reads are bounded by the full
/// status contract and must not be backed by an undersized cache.
fn compose_feature_region<'a>(
    chunk: &'a mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
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
            match status {
                ChunkStatus::Carvers => {
                    holders.push(Box::new(OwnedProtoHolder::new(generate_ring_chunk(
                        pos, generator,
                    ))));
                }
                ChunkStatus::StructureStarts => {
                    let mut structure_chunk = fresh_worldgen_chunk(pos, generator);
                    // `ChunkStatusTasks.generateFeatures` primes the final
                    // maps before decoration, and every dependency chunk must
                    // carry those persisted maps when the region reads it.
                    structure_chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
                    structure_chunk.set_persisted_status(ChunkStatus::StructureStarts);
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
        center_pos.x() - radius,
        center_pos.z() - radius,
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
        generator.registry_access().clone(),
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

/// The shared seed-42 feature `RegistryAccess` — the worldgen access composed
/// with the frozen placed/configured-feature registries the decoder and the
/// selector/composite features resolve their recursive `Holder` references
/// through.
///
/// The two feature registries are frozen up front (empty, present): the
/// `RegistryFileCodec` holder codecs the Batch 2 selectors and the biome
/// generation settings route through require the registry to *exist* in the
/// decode ops to resolve even an inline `Direct` placed/configured holder, and
/// the runtime `place_with_biome_check` path resolves `Holder::Reference` ids
/// against the owning registry. The shared freeze means both the decode ops
/// (`RegistryOps::create_from_access`) and the `WorldGenLevel::registry_access`
/// back-reference observe the same registry ids — the `#181` back-reference
/// rule that keeps a decoded `Reference` resolvable at placement time.
fn build_feature_access(worldgen: &RegistryAccess) -> RegistryAccess {
    let placed = RegistryBuilder::new(&*PLACED_FEATURE).freeze();
    let configured = RegistryBuilder::new(&*CONFIGURED_FEATURE).freeze();
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

fn decode_configured_feature(
    configured_key: &str,
    ops: &FeatureOps,
) -> Result<ConfiguredFeatureErased, String> {
    let entry = CONFIGURED_FEATURE_BY_NAME
        .get(configured_key)
        .ok_or_else(|| format!("missing generated {configured_key} entry"))?;
    let json: Value = serde_json::from_str(entry.json)
        .map_err(|error| format!("decode {configured_key} JSON: {error}"))?;
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
        // Batch 2 dispatch leaves (issue #600 config-decode wave) — each
        // downcast to its own config codec. The config value shapes are the
        // generated `RegistryOps` JSON verbatim, decoded faithfully.
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
        "minecraft:vines" => Arc::new(NoneFeatureConfiguration),
        "minecraft:seagrass" => Arc::new(decode_value(
            probability_feature_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:freeze_top_layer" => Arc::new(NoneFeatureConfiguration),
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
        "minecraft:random_selector" => Arc::new(decode_value(
            random_feature_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:simple_random_selector" => Arc::new(decode_value(
            composite_feature_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:random_boolean_selector" => Arc::new(decode_value(
            random_boolean_feature_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        other => {
            return Err(format!(
                "{configured_key} has unsupported feature type {other}"
            ));
        }
    };
    Ok(ConfiguredFeatureErased { feature, config })
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
    placed_registry: Registry<PlacedFeature>,
    configured_registry: Registry<ConfiguredFeatureErased>,
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
        let placed = self.placed_holder.value(&self.placed_registry);
        placed.place_with_biome_check(&self.configured_registry, level, generator, random, origin);
    }
}

fn decode_placed_feature(
    placed_key: &str,
    generator: &OverworldGenerator,
) -> Result<DecodedPlacedFeature, String> {
    let entry = PLACED_FEATURE_BY_NAME
        .get(placed_key)
        .ok_or_else(|| format!("missing generated {placed_key} entry"))?;
    let json: Value = serde_json::from_str(entry.json)
        .map_err(|error| format!("decode {placed_key} JSON: {error}"))?;
    let configured_key = json
        .get("feature")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{placed_key} JSON has no configured feature"))?;
    let ops =
        RegistryOps::create_from_access(&JsonOps::INSTANCE, generator.feature_access().clone());
    let configured = Arc::new(decode_configured_feature(configured_key, &ops)?);
    let mut configured_builder = RegistryBuilder::new(&*CONFIGURED_FEATURE);
    let configured_registry_id = configured_builder.registry_id();
    let configured_resource_key =
        ResourceKey::create(&*CONFIGURED_FEATURE, Identifier::parse(configured_key));
    let configured_id = configured_builder.register(
        &configured_resource_key,
        configured,
        RegistrationInfo::BUILT_IN,
    );
    let configured_registry = configured_builder.freeze();

    let modifiers = decode_placement_modifiers(placed_key, generator.feature_access())?;
    let placed_value = Arc::new(PlacedFeature::new(
        Holder::reference(configured_registry_id, configured_id.0),
        modifiers,
    ));
    let mut placed_builder = RegistryBuilder::new(&*PLACED_FEATURE);
    let placed_registry_id = placed_builder.registry_id();
    let placed_resource_key = ResourceKey::create(&*PLACED_FEATURE, Identifier::parse(placed_key));
    let placed_id = placed_builder.register(
        &placed_resource_key,
        placed_value,
        RegistrationInfo::BUILT_IN,
    );
    let placed_registry = placed_builder.freeze();
    Ok(DecodedPlacedFeature {
        placed_registry,
        configured_registry,
        placed_holder: Holder::reference(placed_registry_id, placed_id.0),
    })
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
            | Some("minecraft:random_selector")
            | Some("minecraft:simple_random_selector")
            | Some("minecraft:random_boolean_selector")
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
        feature: FeatureId::new(0),
        config: Arc::new(NoneFeatureConfiguration),
    };
    let placed = PlacedFeature::new(Holder::Direct(dummy_feature), modifiers);
    Ok(placed.has_placement_positions(region, &selection_generator, random, origin))
}

fn run_biome_decoration(
    chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
) -> Result<(), GenError> {
    chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
    let center_pos = chunk.get_pos();
    let origin =
        SectionPos::of_chunk_pos(&center_pos, chunk.height_accessor().get_min_section_y()).origin();
    let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(
        random_support::generate_unique_seed(),
    ));
    let decoration_seed =
        random.set_decoration_seed(generator.seed(), origin.get_x(), origin.get_z());

    let mut region = compose_feature_region(chunk, generator);
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

    // The per-step loop — Paper's `addVanillaDecorations`. The structure loop
    // is skipped (the port has no structure manager; Java's
    // `structureManager.shouldGenerateStructures()` gate is a faithful no-op,
    // the #185 structures deferral).
    let generation_steps = Decoration::VALUES.len().max(feature_list.len());
    // Paper walks steps in ascending order and, within a step, the sorted
    // global feature indices of the union biomes mapped through the full-list
    // sorter's `indexMapping`. Registry-backed configured features execute
    // through their exact placed-feature chains; unsupported selected leaves
    // stop the run with a typed boundary.
    let mut saw_feature = false;
    for step_index in 0..generation_steps {
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
            saw_feature = true;
            let feature = &feature_list[step_index].features[global_feature_index];
            let feature_key = match feature {
                Holder::Reference { id, .. } => {
                    placed_by_id.get(id).copied().unwrap_or("minecraft:unknown")
                }
                Holder::Direct(_) => "minecraft:unknown",
            };
            // `setFeatureSeed(decorationSeed, globalIndexOfFeature, stepIndex)`.
            random.set_feature_seed(
                decoration_seed,
                global_feature_index as i32,
                step_index as i32,
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
                placed.place_with_biome_check(
                    &mut region,
                    &dispatch_generator,
                    &mut random,
                    &origin,
                );
                continue;
            }
            let selected =
                placement_selects(&mut region, generator, &mut random, &origin, feature_key)
                    .map_err(|_| GenError::FeaturePlacementDecode {
                        chunk_pos: center_pos,
                        step_index,
                        global_feature_index,
                        feature_key,
                    })?;
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
    if !saw_feature {
        return Err(GenError::FeaturePlacementDecode {
            chunk_pos: center_pos,
            step_index: 0,
            global_feature_index: 0,
            feature_key: "minecraft:unknown",
        });
    }
    Ok(())
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

    Ok(WorldGenRegion::new(
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
                        && spawn_obstruction_ok(region, spawner.ty, fx, y, fz, width, height)
                    {
                        // The current entity registry has no constructor or
                        // insertion surface for these mob types. The candidate
                        // has nevertheless consumed exactly the same placement
                        // gates as Paper; do not classify this as placement
                        // failure and do not fabricate entity NBT.
                        return Err(SpawnRegionError::UnsupportedEntity {
                            chunk_pos: center_pos,
                            biome: biome_name,
                            entity_type: spawner.ty,
                            position: entity_pos,
                        });
                    }
                }

                advance_spawn_candidate(&mut random, &mut x, &mut z, start_x, start_z, xo, zo);
            }
        }
    }
    Ok(())
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
/// collision-face table supplies the static full-shape answer; dynamic or
/// otherwise unavailable shapes fail closed and leave the height candidate
/// unchanged rather than treating a non-pathfindable block as air.
fn is_pathfindable_land(state: BlockState) -> bool {
    let name = state.block().name();
    // PowderSnowBlock explicitly overrides LAND pathfinding to true even
    // though its collision shape is context-sensitive.
    if name == "minecraft:powder_snow" {
        return true;
    }
    // Scaffolding does not override `BlockBehaviour.isPathfindable`: its
    // empty collision context selects the stable, non-full shape, so the
    // inherited LAND predicate is true even though the block is dynamic.
    if name == "minecraft:scaffolding" {
        return true;
    }
    if state.has_dynamic_shape() {
        return false;
    }
    match name {
        // These block classes override the default LAND predicate to false.
        "minecraft:soul_sand"
        | "minecraft:dirt_path"
        | "minecraft:farmland"
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
            || name.starts_with("potted_")
            || name.ends_with("_shelf")
            || name.ends_with("_pane")
            || name.ends_with("_bars")
            || name.ends_with("_chain")
            || name.ends_with("_copper_golem_statue")
            || name.ends_with("_wall_hanging_sign") =>
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
        // The generated collision-face mask is the exact full-face VoxelShape
        // query available in this registry slice, so it answers the default
        // without collapsing a partial shape into blocksMotion.
        _ => state.collision_face_mask() != 0x3F,
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
    // `NaturalSpawner.isValidEmptySpawnBlock` uses the complete collision shape,
    // not the render/occlusion bit. An unavailable shape fails closed so the
    // entity boundary is never reached on an unverified candidate.
    let Some(shape) = spawn_collision_shape(state, pos, SpawnCollisionContext::Empty) else {
        return false;
    };
    !shape.is_full()
        && !is_signal_source(state)
        && state.fluid_empty()
        && !state.is_in_tag("minecraft:prevent_mob_spawning_inside")
        && !is_dangerous_spawn_block(state, entity_type)
}

/// A compact VoxelShape slice for generation-time collision checks. The
/// generated registry does not yet expose arbitrary VoxelShape boxes, so this
/// keeps the exact empty/full/known worldgen shapes that occur in the dynamic
/// block families. Unknown static shapes still fail closed rather than turning
/// an unverified candidate into a spawn.
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

    fn is_full(self) -> bool {
        self.min_x == 0.0
            && self.min_y == 0.0
            && self.min_z == 0.0
            && self.max_x == 1.0
            && self.max_y == 1.0
            && self.max_z == 1.0
    }
}

#[derive(Clone, Copy)]
enum SpawnCollisionContext<'a> {
    Empty,
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
    /// Paper's empty-context scaffolding shape: a full top plate and four
    /// corner posts. The empty collision context reports `isAbove(..., true)`
    /// as true, selecting `SHAPE_STABLE` for every scaffolding state.
    Multi([SpawnShapeBox; 5]),
}

impl SpawnCollisionShape {
    fn is_full(self) -> bool {
        match self {
            Self::Full => true,
            Self::Empty | Self::Multi(_) => false,
            Self::Box(shape) => shape.is_full(),
        }
    }

    fn intersects(self, block_x: i32, block_y: i32, block_z: i32, entity: &SpawnAabb) -> bool {
        match self {
            Self::Empty => false,
            Self::Full => SpawnShapeBox::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
                .intersects(block_x, block_y, block_z, entity),
            Self::Box(shape) => shape.intersects(block_x, block_y, block_z, entity),
            Self::Multi(shapes) => shapes
                .into_iter()
                .any(|shape| shape.intersects(block_x, block_y, block_z, entity)),
        }
    }
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
    // A non-full colliding shape (fence, stair, button, etc.) is not safe to
    // collapse into a cube. Refuse it until the shared VoxelShape registry
    // surface can provide its exact boxes.
    None
}

fn block_offset(pos: &BlockPos) -> (f64, f64) {
    let seed = rivet_util::mth::get_seed(pos.get_x(), 0, pos.get_z());
    let x = (((seed & 15) as f32 / 15.0_f32) as f64 - 0.5) * 0.5;
    let z = ((((seed >> 8) & 15) as f32 / 15.0_f32) as f64 - 0.5) * 0.5;
    (x.clamp(-0.25, 0.25), z.clamp(-0.25, 0.25))
}

fn speleothem_collision_shape(state: BlockState, pos: &BlockPos) -> Option<SpawnCollisionShape> {
    let min_max = match state.get_value(BlockStateProperties::SPELEOTHEM_THICKNESS) {
        Some(PropertyValue::Enum("tip_merge")) => (5.0 / 16.0, 11.0 / 16.0, 0.0, 1.0),
        Some(PropertyValue::Enum("tip")) => {
            match state.get_value(BlockStateProperties::VERTICAL_DIRECTION) {
                Some(PropertyValue::Enum("up")) => (5.0 / 16.0, 11.0 / 16.0, 0.0, 11.0 / 16.0),
                Some(PropertyValue::Enum("down")) => (5.0 / 16.0, 11.0 / 16.0, 5.0 / 16.0, 1.0),
                _ => return None,
            }
        }
        Some(PropertyValue::Enum("frustum")) => (4.0 / 16.0, 12.0 / 16.0, 0.0, 1.0),
        Some(PropertyValue::Enum("middle")) => (3.0 / 16.0, 13.0 / 16.0, 0.0, 1.0),
        Some(PropertyValue::Enum("base")) => (2.0 / 16.0, 14.0 / 16.0, 0.0, 1.0),
        _ => return None,
    };
    let (offset_x, offset_z) = block_offset(pos);
    Some(SpawnCollisionShape::Box(SpawnShapeBox::new(
        min_max.0 + offset_x,
        min_max.2,
        min_max.0 + offset_z,
        min_max.1 + offset_x,
        min_max.3,
        min_max.1 + offset_z,
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
        self.min_x < other.max_x
            && self.max_x > other.min_x
            && self.min_y < other.max_y
            && self.max_y > other.min_y
            && self.min_z < other.max_z
            && self.max_z > other.min_z
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
/// entity instance. Paper re-runs the AABB through `isUnobstructed(entity)`
/// using the entity collision context before scanning liquids; this matters for
/// context-sensitive blocks such as powder snow and scaffolding.
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
    // NaturalSpawner's first collision query uses CollisionContext.empty(),
    // while Mob.checkSpawnObstruction calls isUnobstructed(this) with the
    // candidate entity context. Dynamic blocks such as powder snow and
    // scaffolding can therefore disagree between the two queries.
    if !collision_free(
        region,
        entity,
        SpawnCollisionContext::Entity {
            entity_type,
            entity_min_y: entity.min_y,
        },
    )
    .unwrap_or(false)
    {
        return false;
    }
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
    use crate::server::level::server_level::{ServerLevel, ServerLevelConfig};
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::list_tag::ListTag;
    use rivet_nbt::tag::Tag;
    use rivet_registry::fluid_id::FluidId;
    use rivet_registry::generated::block_states::StateId;
    use rivet_util::RandomSource;
    use rivet_world::level::WorldGenLevel;
    use rivet_world::levelgen::feature::FeatureBehavior;
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
        // SPAWN is wired (the spawn seam is attached) but its path runs through
        // the light steps, which need a light engine the holder does not wire →
        // rejected as LightEngineMissing before any work. FULL is a separate
        // consuming promotion boundary, so it is rejected as UnsupportedStatus.
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

    /// A whole-path LIGHT refusal happens during prevalidation, before the
    /// FEATURES task is entered. It must not poison the holder's terminal
    /// FEATURES-failure cache: a later direct FEATURES request still runs the
    /// decoration body and reports its own typed boundary.
    #[test]
    fn light_refusal_does_not_cache_features_failure() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");

        let light_error = holder
            .generate_through(ChunkStatus::Spawn)
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
    /// `minecraft:multiface_growth`. The next unsupported *selected* path is
    /// `minecraft:dark_forest_vegetation` at step 9/global index 17. The chunk is
    /// never stamped FEATURES; the holder restores its pre-call status and data.
    #[test]
    fn generate_through_features_rolls_back_fresh_holder_and_caches_failure() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
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
                assert_eq!(step_index, 9);
                assert_eq!(global_feature_index, 17);
                assert_eq!(feature_key, "minecraft:dark_forest_vegetation");
            }
            other => {
                panic!(
                    "FEATURES must stop at the selected dark_forest_vegetation mismatch; got {other:?}"
                )
            }
        }

        // The FEATURES prologue and earlier generation rungs may mutate every
        // part of the center proto before the typed placement boundary. A
        // failed transaction must restore the fresh EMPTY representation, not
        // merely leave the status below FEATURES.
        assert_eq!(holder.status(), fresh.status());
        assert_eq!(holder.status(), ChunkStatus::Empty);
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
                step_index: 9,
                global_feature_index: 17,
                feature_key: "minecraft:dark_forest_vegetation",
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

    /// FULL remains the consuming ProtoChunk → LevelChunk boundary even after
    /// FEATURES has reached its cached typed failure. The historical FEATURES
    /// error must never shadow the stable FULL UnsupportedStatus response.
    #[test]
    fn full_rejection_precedes_cached_features_failure() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
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
        let placed = decoded.placed_holder.value(&decoded.placed_registry);
        assert_eq!(placed.placement().len(), 6);
        assert!(matches!(
            placed.feature(),
            Holder::Reference { registry, .. } if *registry == decoded.configured_registry.registry_id()
        ));
        let configured = placed.feature().value(&decoded.configured_registry);
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
        let placed = decoded.placed_holder.value(&decoded.placed_registry);
        assert!(matches!(
            placed.feature(),
            Holder::Reference { registry, .. } if *registry == decoded.configured_registry.registry_id()
        ));
        let configured = placed.feature().value(&decoded.configured_registry);
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
    /// cover the decoder arms directly — the runtime stops at the step-9
    /// dark_forest_vegetation boundary, so the later-step leaves (springs,
    /// seagrass, freeze_top_layer) cannot be reached end-to-end and get their own
    /// independent decode coverage here. The simple_block, block_column, and
    /// vines arms are not separately exercised by these tests.
    #[test]
    fn ore_dirt_decodes_through_the_batch2_ore_arm() {
        let generator = test_generator();
        let decoded = decode_placed_feature("minecraft:ore_dirt", &generator)
            .expect("the seed-42 ore_dirt entry must decode");
        let placed = decoded.placed_holder.value(&decoded.placed_registry);
        assert_eq!(placed.placement().len(), 4);
        let configured = placed.feature().value(&decoded.configured_registry);
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
        let placed = decoded.placed_holder.value(&decoded.placed_registry);
        assert_eq!(placed.placement().len(), 5);
        let configured = placed.feature().value(&decoded.configured_registry);
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
            .value(&decoded.placed_registry)
            .feature()
            .value(&decoded.configured_registry);
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
            .value(&decoded.placed_registry)
            .feature()
            .value(&decoded.configured_registry);
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
            .value(&decoded.placed_registry)
            .feature()
            .value(&decoded.configured_registry);
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
        let placed = decoded.placed_holder.value(&decoded.placed_registry);
        assert_eq!(placed.placement().len(), 5);
        let configured = placed.feature().value(&decoded.configured_registry);
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
        let placed = decoded.placed_holder.value(&decoded.placed_registry);
        assert_eq!(placed.placement().len(), 5);
        let configured = placed.feature().value(&decoded.configured_registry);
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
    /// It refuses at the next unsupported *selected* leaf WITHOUT mutating the
    /// RNG past that refusal: the run returns typed immediately at
    /// `minecraft:dark_forest_vegetation` (step 9/global 17), so the chunk stays
    /// CARVERS and FEATURES is never stamped. The typed-unavailable dispatches
    /// for underwater magma (id 21) and multiface growth (id 20) no longer
    /// refuse; both concrete features are reached.
    #[test]
    fn seed42_does_not_mutate_rng_past_the_next_selected_unsupported_leaf() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
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
                    step_index: 9,
                    global_feature_index: 17,
                    feature_key: "minecraft:dark_forest_vegetation",
                    ..
                })
            ),
            "unexpected next FEATURES boundary: {err:?}"
        );
        assert_eq!(holder.status(), ChunkStatus::Carvers);
    }

    /// The three Batch 2 selector leaves are wired in the decoder, and the
    /// runtime stops at the step-9 dark_forest_vegetation boundary so these
    /// later-step arms are exercised independently here. Full recursive decode
    /// of a selector's inline placed/configured sub-features defers with the
    /// `#126` codec stubs (`configured_feature_direct_codec` and the inline
    /// `placement_modifier_codec` — issue #126, not yet ported), so the wire
    /// surface is pinned: each selector dispatch type routes to its config
    /// codec arm and the recursive sub-feature decode fails typed naming the
    /// `#126` deferral, never fabricating a holder.
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

    /// A selector's recursive inline sub-feature decode fails typed with the
    /// `#126` codec deferral (the inline `ConfiguredFeature`/placement-modifier
    /// codecs are not yet ported), rather than fabricating a placeholder holder
    /// or silently dropping the reference. This is the honest boundary for the
    /// Batch 2 selector arms given the runtime stops at step 6.
    #[test]
    fn selector_recursive_inline_decode_defers_with_126() {
        let generator = test_generator();
        let err = match decode_placed_feature("minecraft:forest_flowers", &generator) {
            Ok(_) => panic!("the inline-sub-feature decode must fail typed"),
            Err(e) => e,
        };
        assert!(
            err.contains("#126") || err.contains("issue #126") || err.contains("STUB"),
            "the selector decode must name the #126 deferral, got: {err}"
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

        let chunk = holder
            .into_level_chunk()
            .expect("the SPAWN-parent generated chunk promotes");
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
            Err(GeneratedChunkError::Convert(
                LevelChunkBridgeError::GeneratedStatusNotSpawn(status)
            )) if status == actual_status
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
                    GeneratedChunkError::Convert(LevelChunkBridgeError::GeneratedStatusNotSpawn(s))
                        if *s == status
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
            Err(GeneratedChunkError::Convert(
                LevelChunkBridgeError::GeneratedStatusNotSpawn(ChunkStatus::Noise)
            ))
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
        let _chunk = holder.into_level_chunk().expect("FULL promotes");
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
        holder.chunk.set_persisted_status(ChunkStatus::Spawn);
        // `InitState::Other` is a persisted Starlight state the #184 send seam
        // cannot represent — it must surface as Convert(UnsupportedLightState),
        // not a panic or a partial promote.
        let mut nibbles = vec![SwmrNibbleArray::new_with_bytes(vec![0xAB; ARRAY_SIZE]); 26];
        nibbles[3] = SwmrNibbleArray::new_with_state(None, InitState::Other(5));
        holder.chunk.set_block_nibbles(nibbles);
        assert!(matches!(
            holder.into_level_chunk(),
            Err(GeneratedChunkError::Convert(
                LevelChunkBridgeError::UnsupportedLightState(_)
            ))
        ));
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
            Ok(chunk) => {
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
            holder.into_level_chunk().expect("FULL promotes")
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
            holder.into_level_chunk().expect("FULL promotes")
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
    fn spawn_brightness_reads_completed_light_nibbles() {
        let generator = test_generator();
        let pos = ChunkPos::new(-8, -4);
        let mut holder = flat_spawn_holder(&generator, pos, 3);
        let mut workspace = spawn_region_workspace(&generator, pos, 3);
        let region = compose_spawn_region(&mut holder.chunk, &mut workspace, &generator)
            .expect("radius-one workspace");
        assert!(is_bright_enough_to_spawn(
            &region,
            &BlockPos::new(pos.get_min_block_x(), 1, pos.get_min_block_z())
        ));
    }

    #[test]
    fn spawn_brightness_fails_closed_without_light_correctness() {
        let generator = test_generator();
        let pos = ChunkPos::new(-8, -4);
        let mut holder = generator.create_holder(pos);
        holder.chunk.set_persisted_status(ChunkStatus::Light);
        let mut workspace = spawn_region_workspace(&generator, pos, 3);
        let region = compose_spawn_region(&mut holder.chunk, &mut workspace, &generator)
            .expect("radius-one workspace");
        assert!(!is_bright_enough_to_spawn(
            &region,
            &BlockPos::new(pos.get_min_block_x(), 1, pos.get_min_block_z())
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
        let moving_piston = BlockState::of(BlockId::from_name("minecraft:moving_piston").unwrap());
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
            .is_some_and(|shape| shape.intersects(0, 0, 0, &above_unstable_scaffolding))
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
            spawn_collision_shape(moving_piston, &pos, SpawnCollisionContext::Empty)
                .is_some_and(|shape| !shape.is_full())
        );
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
    fn spawn_land_pathfindability_matches_block_overrides() {
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let stairs = BlockState::of(BlockId::from_name("minecraft:oak_stairs").unwrap());
        let fence = BlockState::of(BlockId::from_name("minecraft:oak_fence").unwrap());
        let slab = BlockState::of(BlockId::from_name("minecraft:oak_slab").unwrap());
        let ladder = BlockState::of(BlockId::from_name("minecraft:ladder").unwrap());
        let powder_snow = BlockState::of(BlockId::from_name("minecraft:powder_snow").unwrap());
        let cake = BlockState::of(BlockId::from_name("minecraft:cake").unwrap());
        let flower_pot = BlockState::of(BlockId::from_name("minecraft:flower_pot").unwrap());
        let lantern = BlockState::of(BlockId::from_name("minecraft:lantern").unwrap());
        let scaffolding = BlockState::of(BlockId::from_name("minecraft:scaffolding").unwrap());
        let ender_chest = BlockState::of(BlockId::from_name("minecraft:ender_chest").unwrap());
        let copper_chain = BlockState::of(BlockId::from_name("minecraft:copper_chain").unwrap());
        let exposed_copper_statue =
            BlockState::of(BlockId::from_name("minecraft:exposed_copper_golem_statue").unwrap());
        let exposed_copper_lantern =
            BlockState::of(BlockId::from_name("minecraft:exposed_copper_lantern").unwrap());
        let exposed_copper_rod =
            BlockState::of(BlockId::from_name("minecraft:exposed_copper_lightning_rod").unwrap());
        let exposed_copper_chest =
            BlockState::of(BlockId::from_name("minecraft:exposed_copper_chest").unwrap());
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
        assert!(!is_pathfindable_land(lantern));
        assert!(is_pathfindable_land(scaffolding));
        assert!(!is_pathfindable_land(ender_chest));
        assert!(!is_pathfindable_land(copper_chain));
        assert!(!is_pathfindable_land(exposed_copper_statue));
        assert!(!is_pathfindable_land(closed_door));
        assert!(is_pathfindable_land(open_door));
        assert!(is_pathfindable_land(ladder));
        assert!(is_pathfindable_land(powder_snow));
        assert!(is_pathfindable_land(water));
        assert!(!is_pathfindable_land(lava));
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
