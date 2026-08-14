//! Port of `net.minecraft.world.level.levelgen.SurfaceRules` (26.2) and
//! `SurfaceSystem` (26.2).
//!
//! The surface-rules tree: the `ConditionSource`/`RuleSource` value codecs,
//! the applied `Condition`/`SurfaceRule` runtime, the `Context` (the
//! per-column lazy caches keyed by `lastUpdateXZ`/`lastUpdateY`), and the
//! `SurfaceSystem` (the per-world noise set + clay-bands).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! levelgen/SurfaceRules.java` (935 lines) and `SurfaceSystem.java` (340
//! lines).
//!
//! ## Codec dispatch
//!
//! `ConditionSource.CODEC` and `RuleSource.CODEC` are `BuiltInRegistries.
//! MATERIAL_CONDITION`/`MATERIAL_RULE` by-name dispatches over the erased
//! value (`MapCodec<? extends ConditionSource>` / `... RuleSource>`). The port
//! erases each concrete source to its `Arc<dyn ConditionSource>` /
//! `Arc<dyn RuleSource>` and dispatches on the `"type"` field exactly like the
//! `density_functions.rs` `DensityFunctionTypeId` pattern. The registry keys
//! are `worldgen/material_condition` (conditions) and
//! `worldgen/material_rule` (rules), registered in the Java `bootstrap`
//! order:
//!
//! - conditions: `biome`, `noise_threshold`, `vertical_gradient`, `y_above`,
//!   `water`, `temperature`, `steep`, `not`, `hole`,
//!   `above_preliminary_surface`, `stone_depth`;
//! - rules: `bandlands`, `block`, `sequence`, `condition`.
//!
//! ## Context ownership model
//!
//! Java's `Context` is a mutable object the applied rules close over
//! (`this.context`), and every applied `Condition`/`SurfaceRule` must be
//! `Send + Sync + 'static` to live in the `NOISE_SETTINGS` registry (the
//! `ArcRuleSource` value the registry stores). A self-referential
//! `&Context`/raw-pointer capture would break that bound, so the port splits
//! Java's single `Context` into two layers:
//!
//! - [`SurfaceContext`] — the `Arc`-shared part every applied rule holds: the
//!   per-column lazy caches (the `lastUpdateXZ`/`lastUpdateY` counters, the
//!   `surfaceDepth`/`surfaceSecondary`/`minSurfaceLevel`/`biome` memo cells)
//!   plus the immutable per-world data (`SurfaceSystem`, the `BiomeManager`
//!   biome getter, the `WorldGenerationContext`). Its fields are `Mutex`-
//!   wrapped (`OWNERSHIP.md`'s sync-tick model — single-threaded, the mutex is
//!   the `&self` interior-mutability seam).
//! - [`Context`] — the `RandomState`-carrying construction-time shell (Java's
//!   `Context.randomState`, needed only at `apply()` time for the noise
//!   samplers / random factories) plus the four shared `Condition` members
//!   (`temperature`/`steep`/`hole`/`abovePreliminarySurface`). The lifetime
//!   parameter is `RandomState`'s borrow of the noise/density-function
//!   registries; applied rules hold only the lifetime-free `SurfaceContext`.
//!
//! ## Context lazy model
//!
//! `Context` owns the two update counters `lastUpdateXZ`/`lastUpdateY`
//! (initialized `Long.MIN_VALUE + 1`), and every cached value re-computes when
//! the counter it keys on changes. `LazyCondition` stores `lastUpdate`
//! (initialized `getContextLastUpdate() - 1`) plus an `Option<bool>` result
//! and throws `IllegalStateException` if a same-counter `test()` finds a null
//! result. `SurfaceSystem`'s own noise samplers / the 2d/3d noise samplers are
//! lazily computed per counter exactly like Java's inner `DoubleSupplier`s.
//!
//! All int/long arithmetic is wrapping (PORTING.md). `getMinSurfaceLevel`'s
//! lerp2 runs in `double` exactly like Java: the `(blockX & 15) / 16.0F` float
//! fractions and the int cache values are widened to the double overload
//! `Mth.lerp2(double, ...)`, then `Mth.floor(double)` floors the result.
//!
//! ## Seams
//!
//! The value codecs, the applied `Condition`/`SurfaceRule` runtime, the
//! `Context` lazy caches, and the `SurfaceSystem` noise set/clay-bands are
//! faithful ports of the Java arithmetic. `buildSurface` is ported as the
//! internal [`SurfaceSystem::build_surface`] driver (crate-visible), driven
//! against the real [`ProtoChunk`](crate::chunk::proto_chunk::ProtoChunk)
//! `ChunkSurface` impl — the production wire from
//! `NoiseBasedChunkGenerator.doFill` defers (RivetTodo #177). Within the
//! driver the column write (`ProtoChunk.setBlockState` +
//! `markPosForPostProcessing`) is the real worldgen write path; the worldgen
//! heightmap reads (`getHeight(WORLD_SURFACE_WG)`) are the `#185` seam — the
//! applied `SteepMaterialCondition` reads a snapshot of the primed heights
//! captured at `build_surface` start (the applied rules are `'static`/`Send +
//! Sync` and cannot carry the `&mut` chunk; the snapshot is bit-identical to
//! Java's live reads for the end-stone and overworld surface writes, with the
//! accepted divergences — an `AIR` write over the topmost `defaultBlock`, and
//! the per-column eroded-badlands raise — documented on `seam_get_height`).
//! The `Biome`-value reads (`coldEnoughToSnow`,
//! `shouldMeltFrozenOceanIcebergSlightly`) are the biome-value seams
//! (RivetTodo #185). `SurfaceRuleData.end` is ported (the end-stone `block`
//! rule), and `SurfaceRuleData.nether`/`overworld`/`overworldLike` are ported
//! faithfully (the `mc.data.worldgen` unit, RivetTodo #179) with the biome
//! `HolderGetter` threaded through the settings bootstrap (the
//! `noise_generator_settings` callers).
//!
//! RivetTodo(#179): the `is_biome` `HolderSet`s the builders resolve are
//! bound to whatever `HolderGetter<BiomeId>` is handed in. In the
//! `worldgen_bootstraps` access the biome registry registers each
//! `SURFACE_RULE_BIOMES` key under its real generated id (`BiomeId::from_name`),
//! so a condition holder resolved through it is a `Reference` whose value (via
//! `Holder::value`) is that real id. But `Registry::get_id` is identity-based
//! and returns the POSITIONAL registration index, so the `Reference`'s `id`
//! field — and `dense_biome_id` on it — is the index into the 33-key subset,
//! while the runtime biome source (`BiomeManager`) yields `Holder::Direct`
//! holders carrying the real ids. `HolderSet::contains` compares holder values,
//! and a `Reference` never equals a `Direct`, so the apply path cannot match
//! today. The encode path requires `Reference` (a `Direct` holder errors in
//! `RegistryFixedCodec::encode`), so the fix is not a Direct conversion: the
//! runtime biome source must be rewired (biome-core) to produce `Reference`s
//! from the same frozen biome registry, and the biome-id read path must resolve
//! References to their value ids. Until then the apply path must not compare
//! holders across forms.
//! The `@Deprecated` single-column [`SurfaceSystem::top_material`] probe (the
//! carver's grass-replacement call) is ported and composed into the carver
//! `CarvingContext` seam through [`bind_carver_top_material`]; the production
//! carver loop that binds it defers (RivetTodo #185).

use crate::biome::BiomeManager;
use crate::biome::biomes;
use crate::biome::dense_biome_id;
use crate::block::blocks::Blocks;
use crate::block::{Block, BlockState};
use crate::chunk::block_column::BlockColumn;
use crate::level::dimension::dimension_type::WAY_BELOW_MIN_Y;
use crate::levelgen::carver::carving_context::CarvingContext;
use crate::levelgen::noise::noises;
use crate::levelgen::noisegen::noise_chunk::NoiseChunk;
use crate::levelgen::noisegen::random_state::RandomState;
use crate::levelgen::placement::{CaveSurface, cave_surface_codec};
use crate::levelgen::random::PositionalRandomFactoryOverloads;
use crate::levelgen::synth::normal_noise::{NoiseParameters, NormalNoise};
use crate::levelgen::vertical_anchor::{VerticalAnchor, vertical_anchor_codec};
use crate::levelgen::world_generation_context::WorldGenerationContext;
use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::core::{BlockPos, ChunkPos, MutableBlockPos};
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFixedCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::mth;
use rivet_util::random::{PositionalRandomFactory, RandomSource};
use rivet_util::worldgen_random::AlgorithmPositionalRandomFactory;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

/// `SurfaceRules.Context.HOW_FAR_BELOW_PRELIMINARY_SURFACE_LEVEL_TO_BUILD_SURFACE`.
const HOW_FAR_BELOW_PRELIMINARY_SURFACE_LEVEL_TO_BUILD_SURFACE: i32 = 8;
/// `SurfaceRules.Context.SURFACE_CELL_BITS`.
const SURFACE_CELL_BITS: i32 = 4;
/// `SurfaceRules.Context.SURFACE_CELL_SIZE`.
const SURFACE_CELL_SIZE: i32 = 16;
/// `SurfaceRules.Context.SURFACE_CELL_MASK`.
const SURFACE_CELL_MASK: i32 = 15;

/// `SurfaceRules.Context.lastUpdateXZ` / `lastUpdateY` initial value —
/// `Long.MIN_VALUE + 1` (`-9223372036854775807L`).
const INITIAL_LAST_UPDATE: i64 = i64::MIN + 1;

/// `Context.noiseSamplers2d`/`noiseSamplers3d` value — the lazy
/// `DoubleSupplier` a noise-threshold condition holds.
pub type NoiseSampler = Arc<dyn Fn() -> f64 + Send + Sync>;

/// `Context.biomeGetter` — `Function<BlockPos, Holder<Biome>>`.
pub type BiomeGetter = Arc<dyn Fn(&BlockPos) -> Holder<BiomeId> + Send + Sync>;

// ---------------------------------------------------------------------------
// Condition / SurfaceRule runtime
// ---------------------------------------------------------------------------

/// `SurfaceRules.Condition` — `boolean test()`.
pub trait Condition: Send + Sync + 'static {
    /// `test()`.
    fn test(&self) -> bool;
}

/// `SurfaceRules.SurfaceRule` — `@Nullable BlockState tryApply(int blockX,
/// int blockY, int blockZ)`.
pub trait SurfaceRule: Send + Sync + 'static {
    /// `tryApply(int, int, int)` — `None` means "no change".
    fn try_apply(&self, block_x: i32, block_y: i32, block_z: i32) -> Option<BlockState>;
}

// ---------------------------------------------------------------------------
// ConditionSource / RuleSource (value dispatch roots)
// ---------------------------------------------------------------------------

/// `SurfaceRules.ConditionSource` — `Function<Context, Condition>`. The
/// dispatch root of the material-condition codec.
pub trait ConditionSource: Any + Debug + Send + Sync + 'static {
    /// `ConditionSource.apply(Context)` — `'a` is the `RandomState` borrow the
    /// `Context` carries; the applied `Condition` never holds the borrow (it
    /// captures the lifetime-free [`SurfaceContext`]).
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition>;

    /// `type_id`-style identity for the erased dispatch.
    fn as_any(&self) -> &dyn Any;
}

/// `SurfaceRules.RuleSource` — `Function<Context, SurfaceRule>`. The dispatch
/// root of the material-rule codec.
pub trait RuleSource: Any + Debug + Send + Sync + 'static {
    /// `RuleSource.apply(Context)`.
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn SurfaceRule>;

    /// `type_id`-style identity for the erased dispatch.
    fn as_any(&self) -> &dyn Any;
}

/// The erased `Arc<dyn RuleSource>` carrier `NoiseGeneratorSettings` stores.
pub type ArcRuleSource = Arc<dyn RuleSource>;

// ---------------------------------------------------------------------------
// ConditionSource type ids (the `worldgen/material_condition` registry keys)
// ---------------------------------------------------------------------------

/// The material-condition dispatch discriminator — an `Identifier` key. The
/// dispatch is the `MATERIAL_CONDITION` by-name registry; this mirrors
/// `DensityFunctionTypeId` (same `Identifier`-typed discriminator field).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConditionSourceTypeId {
    /// The registry name.
    pub location: &'static str,
}

/// `BuiltInRegistries.MATERIAL_CONDITION` bootstrap order.
pub mod material_condition_types {
    use super::ConditionSourceTypeId;

    pub const BIOME: ConditionSourceTypeId = ConditionSourceTypeId { location: "biome" };
    pub const NOISE_THRESHOLD: ConditionSourceTypeId = ConditionSourceTypeId {
        location: "noise_threshold",
    };
    pub const VERTICAL_GRADIENT: ConditionSourceTypeId = ConditionSourceTypeId {
        location: "vertical_gradient",
    };
    pub const Y_ABOVE: ConditionSourceTypeId = ConditionSourceTypeId {
        location: "y_above",
    };
    pub const WATER: ConditionSourceTypeId = ConditionSourceTypeId { location: "water" };
    pub const TEMPERATURE: ConditionSourceTypeId = ConditionSourceTypeId {
        location: "temperature",
    };
    pub const STEEP: ConditionSourceTypeId = ConditionSourceTypeId { location: "steep" };
    pub const NOT: ConditionSourceTypeId = ConditionSourceTypeId { location: "not" };
    pub const HOLE: ConditionSourceTypeId = ConditionSourceTypeId { location: "hole" };
    pub const ABOVE_PRELIMINARY_SURFACE: ConditionSourceTypeId = ConditionSourceTypeId {
        location: "above_preliminary_surface",
    };
    pub const STONE_DEPTH: ConditionSourceTypeId = ConditionSourceTypeId {
        location: "stone_depth",
    };
}

/// `BuiltInRegistries.MATERIAL_RULE` bootstrap order.
pub mod material_rule_types {
    use super::ConditionSourceTypeId;

    pub const BANDLANDS: ConditionSourceTypeId = ConditionSourceTypeId {
        location: "bandlands",
    };
    pub const BLOCK: ConditionSourceTypeId = ConditionSourceTypeId { location: "block" };
    pub const SEQUENCE: ConditionSourceTypeId = ConditionSourceTypeId {
        location: "sequence",
    };
    pub const CONDITION: ConditionSourceTypeId = ConditionSourceTypeId {
        location: "condition",
    };
}

/// `ConditionSource.CODEC` — the `MATERIAL_CONDITION` by-name dispatch over
/// the erased condition source, as the ops-generic factory.
pub fn condition_source_codec<Ops>() -> Arc<dyn Codec<Arc<dyn ConditionSource>, Ops>>
where
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    let dispatch =
        key_dispatch_codec::dispatch_map::<ConditionSourceTypeId, Arc<dyn ConditionSource>, Ops>(
            "type",
            material_condition_type_by_name_codec::<Ops>(),
            Arc::new(|c: &Arc<dyn ConditionSource>| {
                DataResult::success((**c).condition_source_type_id())
            }),
            material_condition_codec_for_type::<Ops>(),
        );
    rivet_serialization::map_codec::codec_of(dispatch)
}

/// Resolve a `ConditionSourceTypeId` to its erased `MapCodec<Arc<dyn
/// ConditionSource>>`.
fn material_condition_codec_for_type<Ops>()
-> key_dispatch_codec::CodecFn<ConditionSourceTypeId, Arc<dyn ConditionSource>, Ops>
where
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    Arc::new(move |k: &ConditionSourceTypeId| {
        let c = material_condition_codec_for_type_inner(k);
        match c {
            Some(mc) => DataResult::success(mc),
            None => DataResult::error(format!(
                "Material condition type '{}' is not ported",
                k.location
            )),
        }
    })
}

fn material_condition_codec_for_type_inner<Ops>(
    k: &ConditionSourceTypeId,
) -> Option<Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>>
where
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    if *k == material_condition_types::BIOME {
        Some(BiomeConditionSource::codec::<Ops>())
    } else if *k == material_condition_types::NOISE_THRESHOLD {
        Some(NoiseThresholdConditionSource::codec::<Ops>())
    } else if *k == material_condition_types::VERTICAL_GRADIENT {
        Some(VerticalGradientConditionSource::codec::<Ops>())
    } else if *k == material_condition_types::Y_ABOVE {
        Some(YConditionSource::codec::<Ops>())
    } else if *k == material_condition_types::WATER {
        Some(WaterConditionSource::codec::<Ops>())
    } else if *k == material_condition_types::TEMPERATURE {
        Some(Temperature::codec::<Ops>())
    } else if *k == material_condition_types::STEEP {
        Some(Steep::codec::<Ops>())
    } else if *k == material_condition_types::NOT {
        Some(NotConditionSource::codec::<Ops>())
    } else if *k == material_condition_types::HOLE {
        Some(Hole::codec::<Ops>())
    } else if *k == material_condition_types::ABOVE_PRELIMINARY_SURFACE {
        Some(AbovePreliminarySurface::codec::<Ops>())
    } else if *k == material_condition_types::STONE_DEPTH {
        Some(StoneDepthCheck::codec::<Ops>())
    } else {
        None
    }
}

/// `BuiltInRegistries.MATERIAL_CONDITION.byNameCodec()` — `Identifier.CODEC
/// .comapFlatMap(name -> this.get(name) ..., id -> id.identifier())`.
fn material_condition_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ConditionSourceTypeId, Ops>> {
    codec::comap_flat_map::<Identifier, ConditionSourceTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(
            |name: &Identifier| match material_condition_type_by_name(name) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/material_condition]: {}",
                    name
                )),
            },
        ),
        Arc::new(|id: &ConditionSourceTypeId| Identifier::parse(id.location)),
    )
}

fn material_condition_type_by_name(name: &Identifier) -> Option<ConditionSourceTypeId> {
    // `Registry.byNameCodec` resolves the full identifier against the registry
    // keys, which are registered in the `minecraft` (default) namespace; a
    // foreign namespace must not match.
    if name.namespace() != rivet_registry::identifier::DEFAULT_NAMESPACE {
        return None;
    }
    match name.path() {
        "biome" => Some(material_condition_types::BIOME),
        "noise_threshold" => Some(material_condition_types::NOISE_THRESHOLD),
        "vertical_gradient" => Some(material_condition_types::VERTICAL_GRADIENT),
        "y_above" => Some(material_condition_types::Y_ABOVE),
        "water" => Some(material_condition_types::WATER),
        "temperature" => Some(material_condition_types::TEMPERATURE),
        "steep" => Some(material_condition_types::STEEP),
        "not" => Some(material_condition_types::NOT),
        "hole" => Some(material_condition_types::HOLE),
        "above_preliminary_surface" => Some(material_condition_types::ABOVE_PRELIMINARY_SURFACE),
        "stone_depth" => Some(material_condition_types::STONE_DEPTH),
        _ => None,
    }
}

/// `RuleSource.CODEC` — the `MATERIAL_RULE` by-name dispatch over the erased
/// rule source, as the ops-generic factory.
pub fn rule_source_codec<Ops>() -> Arc<dyn Codec<ArcRuleSource, Ops>>
where
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    let dispatch = key_dispatch_codec::dispatch_map::<ConditionSourceTypeId, ArcRuleSource, Ops>(
        "type",
        material_rule_type_by_name_codec::<Ops>(),
        Arc::new(|r: &ArcRuleSource| DataResult::success((**r).rule_source_type_id())),
        material_rule_codec_for_type::<Ops>(),
    );
    rivet_serialization::map_codec::codec_of(dispatch)
}

/// Resolve a rule type id to its erased `MapCodec<Arc<dyn RuleSource>>`.
fn material_rule_codec_for_type<Ops>()
-> key_dispatch_codec::CodecFn<ConditionSourceTypeId, ArcRuleSource, Ops>
where
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    Arc::new(move |k: &ConditionSourceTypeId| {
        let c = material_rule_codec_for_type_inner(k);
        match c {
            Some(mc) => DataResult::success(mc),
            None => DataResult::error(format!("Material rule type '{}' is not ported", k.location)),
        }
    })
}

fn material_rule_codec_for_type_inner<Ops>(
    k: &ConditionSourceTypeId,
) -> Option<Arc<dyn MapCodec<ArcRuleSource, Ops>>>
where
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    if *k == material_rule_types::BANDLANDS {
        Some(Bandlands::codec::<Ops>())
    } else if *k == material_rule_types::BLOCK {
        Some(BlockRuleSource::codec::<Ops>())
    } else if *k == material_rule_types::SEQUENCE {
        Some(SequenceRuleSource::codec::<Ops>())
    } else if *k == material_rule_types::CONDITION {
        Some(TestRuleSource::codec::<Ops>())
    } else {
        None
    }
}

/// `BuiltInRegistries.MATERIAL_RULE.byNameCodec()`.
fn material_rule_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ConditionSourceTypeId, Ops>> {
    codec::comap_flat_map::<Identifier, ConditionSourceTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &Identifier| match material_rule_type_by_name(name) {
            Some(id) => DataResult::success(id),
            None => DataResult::error(format!(
                "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/material_rule]: {}",
                name
            )),
        }),
        Arc::new(|id: &ConditionSourceTypeId| Identifier::parse(id.location)),
    )
}

fn material_rule_type_by_name(name: &Identifier) -> Option<ConditionSourceTypeId> {
    if name.namespace() != rivet_registry::identifier::DEFAULT_NAMESPACE {
        return None;
    }
    match name.path() {
        "bandlands" => Some(material_rule_types::BANDLANDS),
        "block" => Some(material_rule_types::BLOCK),
        "sequence" => Some(material_rule_types::SEQUENCE),
        "condition" => Some(material_rule_types::CONDITION),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The shared context (per-column lazy caches + immutable world data)
// ---------------------------------------------------------------------------

/// The per-column mutable state `SurfaceRules.Context` owns and every applied
/// rule reads. Java's `Context` fields are plain mutable members; the port
/// wraps each in a `Mutex` (the `&self` interior-mutability seam under
/// `OWNERSHIP.md`'s sync-tick model) and shares the whole set as an
/// `Arc<SurfaceContext>` so applied `Condition`/`SurfaceRule`s can be
/// `Send + Sync + 'static`.
struct SharedCells {
    /// `Context.lastUpdateXZ` — `-9223372036854775807L`.
    last_update_xz: Mutex<i64>,
    /// `Context.lastUpdateY` — `-9223372036854775807L`.
    last_update_y: Mutex<i64>,
    /// `Context.blockX`.
    block_x: Mutex<i32>,
    /// `Context.blockZ`.
    block_z: Mutex<i32>,
    /// `Context.surfaceDepth`.
    surface_depth: Mutex<i32>,
    /// `Context.lastSurfaceDepth2Update` — `this.lastUpdateXZ - 1L`.
    last_surface_depth_2_update: Mutex<i64>,
    /// `Context.surfaceSecondary`.
    surface_secondary: Mutex<f64>,
    /// `Context.lastMinSurfaceLevelUpdate` — `this.lastUpdateXZ - 1L`.
    last_min_surface_level_update: Mutex<i64>,
    /// `Context.minSurfaceLevel`.
    min_surface_level: Mutex<i32>,
    /// `Context.lastPreliminarySurfaceCellOrigin` — `Long.MAX_VALUE`.
    last_preliminary_surface_cell_origin: Mutex<i64>,
    /// `Context.preliminarySurfaceCache` — `new int[4]`.
    preliminary_surface_cache: Mutex<[i32; 4]>,
    /// `Context.pos` — `new BlockPos.MutableBlockPos()`.
    pos: Mutex<MutableBlockPos>,
    /// `Context.biome` — `@Nullable Holder<Biome>`.
    biome: Mutex<Option<Holder<BiomeId>>>,
    /// `Context.blockY`.
    block_y: Mutex<i32>,
    /// `Context.waterHeight`.
    water_height: Mutex<i32>,
    /// `Context.stoneDepthBelow`.
    stone_depth_below: Mutex<i32>,
    /// `Context.stoneDepthAbove`.
    stone_depth_above: Mutex<i32>,
}

impl SharedCells {
    /// The Java constructor defaults — `lastUpdateXZ`/`lastUpdateY`
    /// `Long.MIN_VALUE + 1`, the derived counters `- 1L`, the caches zeroed.
    fn new() -> Self {
        SharedCells {
            last_update_xz: Mutex::new(INITIAL_LAST_UPDATE),
            last_update_y: Mutex::new(INITIAL_LAST_UPDATE),
            block_x: Mutex::new(0),
            block_z: Mutex::new(0),
            surface_depth: Mutex::new(0),
            last_surface_depth_2_update: Mutex::new(INITIAL_LAST_UPDATE.wrapping_sub(1)),
            surface_secondary: Mutex::new(0.0),
            last_min_surface_level_update: Mutex::new(INITIAL_LAST_UPDATE.wrapping_sub(1)),
            min_surface_level: Mutex::new(0),
            last_preliminary_surface_cell_origin: Mutex::new(i64::MAX),
            preliminary_surface_cache: Mutex::new([0; 4]),
            pos: Mutex::new(MutableBlockPos::new(0, 0, 0)),
            biome: Mutex::new(None),
            block_y: Mutex::new(0),
            water_height: Mutex::new(0),
            stone_depth_below: Mutex::new(0),
            stone_depth_above: Mutex::new(0),
        }
    }

    fn last_update_xz(&self) -> i64 {
        *self.last_update_xz.lock().unwrap()
    }

    fn last_update_y(&self) -> i64 {
        *self.last_update_y.lock().unwrap()
    }

    fn block_x(&self) -> i32 {
        *self.block_x.lock().unwrap()
    }

    fn block_z(&self) -> i32 {
        *self.block_z.lock().unwrap()
    }

    fn block_y(&self) -> i32 {
        *self.block_y.lock().unwrap()
    }

    fn surface_depth(&self) -> i32 {
        *self.surface_depth.lock().unwrap()
    }

    fn water_height(&self) -> i32 {
        *self.water_height.lock().unwrap()
    }

    fn stone_depth_below(&self) -> i32 {
        *self.stone_depth_below.lock().unwrap()
    }

    fn stone_depth_above(&self) -> i32 {
        *self.stone_depth_above.lock().unwrap()
    }

    fn min_surface_level(&self) -> i32 {
        *self.min_surface_level.lock().unwrap()
    }

    fn preliminary_surface_cache(&self) -> [i32; 4] {
        *self.preliminary_surface_cache.lock().unwrap()
    }
}

/// The `Arc`-shared half of `SurfaceRules.Context` — Java's `system`, the
/// per-column lazy caches, the biome getter, and the `WorldGenerationContext`
/// (the `Context.context` every `VerticalAnchor.resolveY` reads). Applied
/// `Condition`/`SurfaceRule`s capture an `Arc<SurfaceContext>` (lifetime-free,
/// so the rules stay `'static`); the `RandomState`-carrying shell is
/// [`Context`].
pub(crate) struct SurfaceContext {
    /// `Context.system`.
    system: Arc<SurfaceSystem>,
    /// The per-column mutable caches.
    cells: SharedCells,
    /// `Context.biomeGetter` — `Function<BlockPos, Holder<Biome>>`.
    biome_getter: BiomeGetter,
    /// `Context.context` — the `WorldGenerationContext` (vertical anchors).
    worldgen_context: Arc<WorldGenerationContext>,
    /// `Context.chunk`'s primed `WORLD_SURFACE_WG` heights, captured at
    /// `buildSurface` start — the `#185` heightmap seam. The applied rules are
    /// `'static`/`Send + Sync`, so they cannot carry the `&mut` chunk; the
    /// heights are snapshotted into the shared context instead. `None` for the
    /// single-column [`SurfaceSystem::top_material`] probe, which has no chunk.
    world_surface_heights: Option<Arc<[i32; 256]>>,
}

// RivetTodo(#177): the surface-build runtime is test-exercised through the
// `build_surface` seam driver until `NoiseBasedChunkGenerator` wires it; the
// non-test build sees the reachable-by-tests-only methods as dead.
#[allow(dead_code)]
impl SurfaceContext {
    /// `Context.updateXZ(int blockX, int blockZ)` — increments both counters
    /// and recomputes `surfaceDepth`.
    fn update_xz(&self, block_x: i32, block_z: i32) {
        {
            let mut v = self.cells.last_update_xz.lock().unwrap();
            *v = v.wrapping_add(1);
        }
        {
            let mut v = self.cells.last_update_y.lock().unwrap();
            *v = v.wrapping_add(1);
        }
        *self.cells.block_x.lock().unwrap() = block_x;
        *self.cells.block_z.lock().unwrap() = block_z;
        *self.cells.surface_depth.lock().unwrap() = self.system.get_surface_depth(block_x, block_z);
    }

    /// `Context.updateY(int stoneDepthAbove, int stoneDepthBelow, int
    /// waterHeight, int blockY)`.
    fn update_y(
        &self,
        stone_depth_above: i32,
        stone_depth_below: i32,
        water_height: i32,
        block_y: i32,
    ) {
        {
            let mut v = self.cells.last_update_y.lock().unwrap();
            *v = v.wrapping_add(1);
        }
        *self.cells.biome.lock().unwrap() = None;
        *self.cells.block_y.lock().unwrap() = block_y;
        *self.cells.water_height.lock().unwrap() = water_height;
        *self.cells.stone_depth_below.lock().unwrap() = stone_depth_below;
        *self.cells.stone_depth_above.lock().unwrap() = stone_depth_above;
    }

    /// `Context.getSurfaceSecondary()`.
    fn get_surface_secondary(&self) -> f64 {
        let lxz = self.cells.last_update_xz();
        let mut last = self.cells.last_surface_depth_2_update.lock().unwrap();
        if *last != lxz {
            *last = lxz;
            *self.cells.surface_secondary.lock().unwrap() = self
                .system
                .get_surface_secondary(self.cells.block_x(), self.cells.block_z());
        }
        *self.cells.surface_secondary.lock().unwrap()
    }

    /// `Context.getBiome()`.
    fn get_biome(&self) -> Holder<BiomeId> {
        let mut biome = self.cells.biome.lock().unwrap();
        if biome.is_none() {
            // `this.pos.set(...)` then `this.biomeGetter.apply(this.pos)` — the
            // `MutableBlockPos` carries the same coords as the fresh `BlockPos`
            // the getter reads (Java passes the mutable pos by reference).
            let mut pos = *self.cells.pos.lock().unwrap();
            pos.set(
                self.cells.block_x(),
                self.cells.block_y(),
                self.cells.block_z(),
            );
            *self.cells.pos.lock().unwrap() = pos;
            let resolved =
                (self.biome_getter)(&BlockPos::new(pos.get_x(), pos.get_y(), pos.get_z()));
            *biome = Some(resolved);
        }
        biome.clone().expect("getBiome set the biome")
    }

    /// `Context.getSeaLevel()`.
    fn get_sea_level(&self) -> i32 {
        self.system.get_sea_level()
    }
}

/// `Context.blockCoordToSurfaceCell(int)`.
fn block_coord_to_surface_cell(block_coord: i32) -> i32 {
    block_coord >> SURFACE_CELL_BITS
}

/// `Context.surfaceCellToBlockCoord(int)`.
fn surface_cell_to_block_coord(cell_coord: i32) -> i32 {
    cell_coord << SURFACE_CELL_BITS
}

/// `Context.getMinSurfaceLevel()` — the lazily-computed preliminary-surface
/// corner-cell interpolation. Java widens the `(blockX & 15) / 16.0F` float
/// fractions and the int cache values to the double overload
/// `Mth.lerp2(double, ...)` and floors with `Mth.floor(double)`, so the
/// interpolation stays in `f64`.
fn compute_min_surface_level(cells: &SharedCells, noise_chunk: &NoiseChunk) -> i32 {
    let lxz = cells.last_update_xz();
    let mut last = cells.last_min_surface_level_update.lock().unwrap();
    if *last != lxz {
        *last = lxz;
        let block_x = cells.block_x();
        let block_z = cells.block_z();
        let corner_cell_x = block_coord_to_surface_cell(block_x);
        let corner_cell_z = block_coord_to_surface_cell(block_z);
        let preliminary_surface_cell_origin = ChunkPos::pack_coords(corner_cell_x, corner_cell_z);
        let mut last_origin = cells.last_preliminary_surface_cell_origin.lock().unwrap();
        if *last_origin != preliminary_surface_cell_origin {
            *last_origin = preliminary_surface_cell_origin;
            let mut cache = cells.preliminary_surface_cache.lock().unwrap();
            cache[0] = noise_chunk.preliminary_surface_level(
                surface_cell_to_block_coord(corner_cell_x),
                surface_cell_to_block_coord(corner_cell_z),
            );
            cache[1] = noise_chunk.preliminary_surface_level(
                surface_cell_to_block_coord(corner_cell_x.wrapping_add(1)),
                surface_cell_to_block_coord(corner_cell_z),
            );
            cache[2] = noise_chunk.preliminary_surface_level(
                surface_cell_to_block_coord(corner_cell_x),
                surface_cell_to_block_coord(corner_cell_z.wrapping_add(1)),
            );
            cache[3] = noise_chunk.preliminary_surface_level(
                surface_cell_to_block_coord(corner_cell_x.wrapping_add(1)),
                surface_cell_to_block_coord(corner_cell_z.wrapping_add(1)),
            );
        }
        drop(last_origin);
        let cache = cells.preliminary_surface_cache();
        // Java widens the `(blockX & 15) / 16.0F` float fractions and the int
        // cache values to double: `Mth.lerp2(double, double, double, double,
        // double, double)` and `Mth.floor(double)`.
        let f1 = (block_x & SURFACE_CELL_MASK) as f64 / SURFACE_CELL_SIZE as f64;
        let f2 = (block_z & SURFACE_CELL_MASK) as f64 / SURFACE_CELL_SIZE as f64;
        let preliminary_surface_level = mth::floor_d(mth::lerp2(
            f1,
            f2,
            cache[0] as f64,
            cache[1] as f64,
            cache[2] as f64,
            cache[3] as f64,
        ));
        *cells.min_surface_level.lock().unwrap() = preliminary_surface_level
            .wrapping_add(cells.surface_depth())
            .wrapping_sub(HOW_FAR_BELOW_PRELIMINARY_SURFACE_LEVEL_TO_BUILD_SURFACE);
    }
    cells.min_surface_level()
}

/// The `LazyCondition` cache — `lastUpdate` (initialized
/// `getContextLastUpdate() - 1`) plus `Option<bool>` result. `test` throws the
/// Java `IllegalStateException` (ported as a panic) if a same-counter `test()`
/// finds a null result.
struct LazyCache {
    last_update: Mutex<i64>,
    result: Mutex<Option<bool>>,
}

impl LazyCache {
    /// `LazyCondition(...)` — `this.lastUpdate = this.getContextLastUpdate() - 1L`.
    fn new(context_last_update: i64) -> Self {
        LazyCache {
            last_update: Mutex::new(context_last_update.wrapping_sub(1)),
            result: Mutex::new(None),
        }
    }

    /// `LazyCondition.test()`.
    fn test<F: FnOnce() -> bool>(&self, last_context_update: i64, compute: F) -> bool {
        let mut last_update = self.last_update.lock().unwrap();
        if *last_update == last_context_update {
            if let Some(result) = *self.result.lock().unwrap() {
                return result;
            }
            unreachable!("Update triggered but the result is null");
        }
        *last_update = last_context_update;
        let computed = compute();
        *self.result.lock().unwrap() = Some(computed);
        computed
    }
}

// ---------------------------------------------------------------------------
// Context (the `RandomState`-carrying shell + the shared Condition members)
// ---------------------------------------------------------------------------

/// `SurfaceRules.Context` — the construction-time shell Java's applied rules
/// close over. The mutable per-column state and the `SurfaceSystem`/biome
/// getter live in the `Arc`-shared [`SurfaceContext`]; this type adds the
/// `RandomState` (resolved only at `apply()` time, for the noise samplers /
/// random factories), the `NoiseChunk` (the `getMinSurfaceLevel` reads), and
/// the four shared `Condition` members (`temperature`/`steep`/`hole`/
/// `abovePreliminarySurface`).
pub struct Context<'a> {
    /// The `Arc`-shared context every applied rule captures.
    pub(crate) surface_context: Arc<SurfaceContext>,
    /// `Context.randomState` — borrowed, not consumed: `RandomState` is the
    /// per-world object (owned by the chunk generator), and each chunk's
    /// `Context` reuses it for the noise samplers / random factories resolved
    /// at `apply()` time. Applied rules capture only the resolved values, so
    /// the borrow never leaks into the `'static` rule.
    random_state: &'a RandomState<'a>,
    /// `Context.temperature` — `new TemperatureHelperCondition(this)`.
    temperature: Arc<dyn Condition>,
    /// `Context.steep` — `new SteepMaterialCondition(this)`.
    steep: Arc<dyn Condition>,
    /// `Context.hole` — `new HoleCondition(this)`.
    hole: Arc<dyn Condition>,
    /// `Context.abovePreliminarySurface` — `new AbovePreliminarySurfaceCondition()`.
    above_preliminary_surface: Arc<dyn Condition>,
    /// `Context.noiseChunk` — read only by the `#177` surface-build runtime
    /// (`get_min_surface_level`); see the `build_surface` seam note.
    #[allow(dead_code)]
    noise_chunk: Arc<NoiseChunk>,
    /// `Context.possibleBiomes` — `@Nullable Set<Holder<Biome>>`.
    possible_biomes: Option<Vec<Holder<BiomeId>>>,
    /// `Context.noiseSamplers2d` — the `IdentityHashMap` (keyed by key
    /// equality).
    noise_samplers_2d: Mutex<HashMap<ResourceKey<NoiseParameters>, NoiseSampler>>,
    /// `Context.noiseSamplers3d`.
    noise_samplers_3d: Mutex<HashMap<ResourceKey<NoiseParameters>, NoiseSampler>>,
}

impl<'a> Context<'a> {
    /// `new Context(SurfaceSystem, RandomState, ChunkAccess, NoiseChunk,
    /// Function<BlockPos, Holder<Biome>>, WorldGenerationContext,
    /// @Nullable Set<Holder<Biome>>)` — Java's `ChunkAccess chunk` is the
    /// `#185` heightmap seam: the port snapshots the primed `WORLD_SURFACE_WG`
    /// heights (`world_surface_heights`) so the applied rules read them without
    /// carrying the `&mut` chunk; `None` when there is no chunk (the carver
    /// probe). The `SurfaceSystem` is the `Arc` shared from `RandomState`
    /// (Java's `Context.system` is the same object the `RandomState` holds);
    /// the `RandomState` is borrowed for the context's lifetime.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        system: Arc<SurfaceSystem>,
        random_state: &'a RandomState<'a>,
        noise_chunk: Arc<NoiseChunk>,
        biome_getter: BiomeGetter,
        worldgen_context: Arc<WorldGenerationContext>,
        possible_biomes: Option<Vec<Holder<BiomeId>>>,
        world_surface_heights: Option<Arc<[i32; 256]>>,
    ) -> Self {
        let surface_context = Arc::new(SurfaceContext {
            system,
            cells: SharedCells::new(),
            biome_getter,
            worldgen_context,
            world_surface_heights,
        });
        let temperature = Arc::new(TemperatureHelperCondition {
            cache: LazyCache::new(surface_context.cells.last_update_y()),
            surface_context: surface_context.clone(),
        });
        let steep = Arc::new(SteepMaterialCondition {
            cache: LazyCache::new(surface_context.cells.last_update_xz()),
            surface_context: surface_context.clone(),
        });
        let hole = Arc::new(HoleCondition {
            cache: LazyCache::new(surface_context.cells.last_update_xz()),
            surface_context: surface_context.clone(),
        });
        let above_preliminary_surface = Arc::new(AbovePreliminarySurfaceCondition {
            surface_context: surface_context.clone(),
            noise_chunk: noise_chunk.clone(),
        });
        Context {
            surface_context,
            random_state,
            temperature,
            steep,
            hole,
            above_preliminary_surface,
            noise_chunk,
            possible_biomes,
            noise_samplers_2d: Mutex::new(HashMap::new()),
            noise_samplers_3d: Mutex::new(HashMap::new()),
        }
    }

    /// `Context.updateXZ(int, int)` — the `#177` surface-build runtime.
    #[allow(dead_code)]
    pub(crate) fn update_xz(&self, block_x: i32, block_z: i32) {
        self.surface_context.update_xz(block_x, block_z);
    }

    /// `Context.updateY(int, int, int, int)` — the `#177` surface-build
    /// runtime.
    #[allow(dead_code)]
    pub(crate) fn update_y(
        &self,
        stone_depth_above: i32,
        stone_depth_below: i32,
        water_height: i32,
        block_y: i32,
    ) {
        self.surface_context
            .update_y(stone_depth_above, stone_depth_below, water_height, block_y);
    }

    /// `Context.getSeaLevel()`.
    pub fn get_sea_level(&self) -> i32 {
        self.surface_context.get_sea_level()
    }

    /// `Context.getMinSurfaceLevel()` — the `#177` surface-build runtime.
    #[allow(dead_code)]
    pub(crate) fn get_min_surface_level(&self) -> i32 {
        compute_min_surface_level(&self.surface_context.cells, &self.noise_chunk)
    }

    /// `Context.getNoiseSampler(ResourceKey, boolean is3d)` — the
    /// `computeIfAbsent` over the 2d/3d sampler maps.
    pub(crate) fn get_noise_sampler(
        &self,
        noise_id: &ResourceKey<NoiseParameters>,
        is3d: bool,
    ) -> NoiseSampler {
        let map = if is3d {
            &self.noise_samplers_3d
        } else {
            &self.noise_samplers_2d
        };
        let mut map = map.lock().unwrap();
        map.entry(noise_id.clone())
            .or_insert_with(|| {
                if is3d {
                    self.create_noise_sampler_3d(noise_id)
                } else {
                    self.create_noise_sampler_2d(noise_id)
                }
            })
            .clone()
    }

    /// `Context.createNoiseSampler2d` — the anonymous `DoubleSupplier` whose
    /// `lastUpdateXZ` starts at `Context.this.lastUpdateXZ - 1L`.
    fn create_noise_sampler_2d(&self, noise_id: &ResourceKey<NoiseParameters>) -> NoiseSampler {
        let noise = self.random_state.get_or_create_noise(noise_id);
        let surface_context = self.surface_context.clone();
        let last_update = Mutex::new(surface_context.cells.last_update_xz().wrapping_sub(1));
        let last_noise = Mutex::new(0.0);
        Arc::new(move || {
            let ctx_update = surface_context.cells.last_update_xz();
            let mut last = last_update.lock().unwrap();
            if *last != ctx_update {
                *last_noise.lock().unwrap() = noise.get_value(
                    surface_context.cells.block_x() as f64,
                    0.0,
                    surface_context.cells.block_z() as f64,
                );
                *last = ctx_update;
            }
            *last_noise.lock().unwrap()
        })
    }

    /// `Context.createNoiseSampler3d` — the anonymous `DoubleSupplier` whose
    /// `lastUpdateY` starts at `Context.this.lastUpdateY - 1L`.
    fn create_noise_sampler_3d(&self, noise_id: &ResourceKey<NoiseParameters>) -> NoiseSampler {
        let noise = self.random_state.get_or_create_noise(noise_id);
        let surface_context = self.surface_context.clone();
        let last_update = Mutex::new(surface_context.cells.last_update_y().wrapping_sub(1));
        let last_noise = Mutex::new(0.0);
        Arc::new(move || {
            let ctx_update = surface_context.cells.last_update_y();
            let mut last = last_update.lock().unwrap();
            if *last != ctx_update {
                *last_noise.lock().unwrap() = noise.get_value(
                    surface_context.cells.block_x() as f64,
                    surface_context.cells.block_y() as f64,
                    surface_context.cells.block_z() as f64,
                );
                *last = ctx_update;
            }
            *last_noise.lock().unwrap()
        })
    }
}

// ---------------------------------------------------------------------------
// The shared Context Condition members
// ---------------------------------------------------------------------------

/// `Context.AbovePreliminarySurfaceCondition` — `blockY >= getMinSurfaceLevel()`.
struct AbovePreliminarySurfaceCondition {
    surface_context: Arc<SurfaceContext>,
    noise_chunk: Arc<NoiseChunk>,
}

impl Condition for AbovePreliminarySurfaceCondition {
    fn test(&self) -> bool {
        self.surface_context.cells.block_y()
            >= compute_min_surface_level(&self.surface_context.cells, &self.noise_chunk)
    }
}

/// `Context.HoleCondition` — a `LazyXZCondition` over `surfaceDepth <= 0`.
struct HoleCondition {
    cache: LazyCache,
    surface_context: Arc<SurfaceContext>,
}

impl Condition for HoleCondition {
    fn test(&self) -> bool {
        self.cache
            .test(self.surface_context.cells.last_update_xz(), || {
                self.surface_context.cells.surface_depth() <= 0
            })
    }
}

/// `Context.SteepMaterialCondition` — a `LazyXZCondition` reading the chunk's
/// `WORLD_SURFACE_WG` heights. The chunk reads are the `#185` heightmap seam:
/// the applied condition cannot prime/read the `&mut` heightmap through `&self`,
/// so it reads the snapshot `build_surface` captured at its start (see
/// [`SurfaceContext::world_surface_heights`]); the arithmetic is faithful, and
/// the snapshot divergences (an `AIR`-over-default write, the eroded-badlands
/// pre-extension) are documented on [`seam_get_height`] (RivetTodo #185).
struct SteepMaterialCondition {
    cache: LazyCache,
    surface_context: Arc<SurfaceContext>,
}

/// The four `getHeight(WORLD_SURFACE_WG)` probe coordinates — Java's
/// `Math.max(chunkBlockZ - 1, 0)` / `Math.min(chunkBlockZ + 1, 15)` etc. (the
/// `+/- 1` neighbor offsets, clamped to the chunk edge). Java's int arithmetic
/// wraps, so `wrapping_sub`/`wrapping_add`. Returns `(zNorth, zSouth, xWest,
/// xEast)`.
fn steep_neighbor_probes(chunk_block_x: i32, chunk_block_z: i32) -> (i32, i32, i32, i32) {
    let z_north = chunk_block_z.wrapping_sub(1).max(0);
    let z_south = chunk_block_z.wrapping_add(1).min(15);
    let x_west = chunk_block_x.wrapping_sub(1).max(0);
    let x_east = chunk_block_x.wrapping_add(1).min(15);
    (z_north, z_south, x_west, x_east)
}

impl Condition for SteepMaterialCondition {
    fn test(&self) -> bool {
        self.cache
            .test(self.surface_context.cells.last_update_xz(), || {
                // The `ChunkAccess.getHeight(WORLD_SURFACE_WG, ...)` reads are
                // the `#185` heightmap seam — the applied rule cannot prime/
                // read the `&mut` heightmap through `&self`, so it reads the
                // snapshot `build_surface` captured at its start. The single-
                // column carver probe has no chunk (`None`), so the steep
                // condition cannot fire there — Java's probe does have a
                // chunk, but the seam cannot construct one (see the module
                // doc).
                let Some(heights) = &self.surface_context.world_surface_heights else {
                    return false;
                };
                let chunk_block_x = self.surface_context.cells.block_x() & 15;
                let chunk_block_z = self.surface_context.cells.block_z() & 15;
                let (z_north, z_south, x_west, x_east) =
                    steep_neighbor_probes(chunk_block_x, chunk_block_z);
                let height_north = seam_get_height(heights, chunk_block_x, z_north);
                let height_south = seam_get_height(heights, chunk_block_x, z_south);
                if height_south >= height_north + 4 {
                    return true;
                }
                let height_west = seam_get_height(heights, x_west, chunk_block_z);
                let height_east = seam_get_height(heights, x_east, chunk_block_z);
                height_west >= height_east + 4
            })
    }
}

/// `Context.TemperatureHelperCondition` — a `LazyYCondition` reading
/// `getBiome().value().coldEnoughToSnow(...)`. The `Biome`-value read is the
/// `#185` biome-value seam: `BiomeManager` yields `Holder<BiomeId>` and no
/// runtime `Registry<Biome>` value layer exists (biome-core), so the read
/// resolves through [`seam_cold_enough_to_snow`] (permanently false, never a
/// panic — see the seam).
struct TemperatureHelperCondition {
    cache: LazyCache,
    surface_context: Arc<SurfaceContext>,
}

impl Condition for TemperatureHelperCondition {
    fn test(&self) -> bool {
        self.cache
            .test(self.surface_context.cells.last_update_y(), || {
                // `getBiome().value().coldEnoughToSnow(pos, seaLevel)` — the biome
                // cache is filled (the Java compute reads it), then the value read
                // is deferred through the seam.
                let _biome = self.surface_context.get_biome();
                seam_cold_enough_to_snow()
            })
    }
}

/// The `ChunkAccess.getHeight(WORLD_SURFACE_WG, x, z)` read — `getFirstAvailable
/// (x & 15, z & 15) - 1` over the primed heightmap (issue #185). The read
/// happens against the snapshot `build_surface` captured at its start.
///
/// Java ordering: `buildSurface` walks the columns `x`-outer/`z`-inner; per
/// column it reads `startingHeight`, runs `erodedBadlandsExtension` on an
/// `ERODED_BADLANDS` column (writing `defaultBlock` into `AIR` at/below
/// `startY`, which `setBlockState` folds into the live `WORLD_SURFACE_WG`
/// heightmap), then re-reads `height` and drives the steep probes from it — so
/// Java's `getHeight` sees the extension's raise on the *current* column. The
/// Rust port reads the start-of-`build_surface` snapshot (the applied rules are
/// `'static`/`Send + Sync` and cannot carry the `&mut` chunk), which is
/// bit-identical for every surface write that keeps the topmost non-air block
/// in place — the end-stone rule and the default-block-to-surface-state writes
/// of the overworld rules. The accepted divergences, pinned by the snapshot:
/// (1) a rule writing `AIR` over the topmost `defaultBlock` lowers Java's live
/// height but not the snapshot; (2) an `eroded_badlands` column's extension
/// raises its live height in Java (visible to that column's own west/east
/// probes) but the snapshot predates it. Both are documented residuals of the
/// `#185` heightmap seam (RivetTodo #185).
fn seam_get_height(heights: &[i32; 256], x: i32, z: i32) -> i32 {
    heights[(x & 15) as usize + (z & 15) as usize * 16]
}

/// The `RivetTodo(#185)` biome-value seam —
/// `getBiome().value().coldEnoughToSnow(pos, seaLevel)`.
fn seam_cold_enough_to_snow() -> bool {
    // STUB(mc.world.level.biome.core): `BiomeManager` yields `Holder<BiomeId>`
    // (a positional handle into the worldgen biome registry, not a `Biome`
    // value), and no runtime `Registry<Biome>` value layer exists yet — the
    // `Biome` value codec/serialization (`SYNCHRONIZED_NBT`) is unported
    // (biome-core). The value methods `Biome::cold_enough_to_snow` exist (and
    // are proven in biome.rs), but there is no registry to resolve a `BiomeId`
    // holder through, so this stays permanently false (no snow coverage from
    // the temperature rule). Do NOT panic: the frozen-ocean branch of the
    // overworld rule tree reaches this in production (the `#177` build_surface
    // wire defers, so it is not reachable today). It becomes the real call once
    // the biome value registry lands (RivetTodo(#185)).
    false
}

// ---------------------------------------------------------------------------
// ConditionSource implementations
// ---------------------------------------------------------------------------

/// `SurfaceRules.BiomeConditionSource` — `record(HolderSet<Biome> biomes)`.
#[derive(Debug, Clone)]
pub struct BiomeConditionSource {
    /// `biomes`.
    pub biomes: HolderSet<BiomeId>,
}

impl BiomeConditionSource {
    /// `new BiomeConditionSource(HolderSet<Biome>)`.
    pub fn new(biomes: HolderSet<BiomeId>) -> Self {
        BiomeConditionSource { biomes }
    }

    /// `biomes`.
    pub fn biomes(&self) -> &HolderSet<BiomeId> {
        &self.biomes
    }

    /// `BiomeConditionSource.CODEC` — the required `"biome_is"` field
    /// (`RegistryCodecs.homogeneousList(Registries.BIOME)`).
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        let field = codec::field_of(biomes_field_codec::<Ops>(), "biome_is".to_string());
        erase_condition_map_codec::<BiomeConditionSource, Ops>(
            map_codec::xmap(
                field,
                Arc::new(|b: &HolderSet<BiomeId>| BiomeConditionSource::new(b.clone())),
                Arc::new(|s: &BiomeConditionSource| s.biomes.clone()),
            ),
            "biome".to_string(),
        )
    }
}

impl ConditionSource for BiomeConditionSource {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        // The biome-condition reads `this.biomes.contains(context.getBiome())`
        // and short-circuits on `possibleBiomes`.
        if let Some(possible) = &context.possible_biomes {
            if self.can_never_match(possible) {
                return Arc::new(ConstCondition(false));
            }
            if self.will_always_match(possible) {
                return Arc::new(ConstCondition(true));
            }
        }
        let surface_context = context.surface_context.clone();
        let biomes = self.biomes.clone();
        Arc::new(BiomeCondition {
            cache: LazyCache::new(surface_context.cells.last_update_y()),
            surface_context,
            biomes,
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `LazyYCondition` the biome condition applies — reads
/// `this.biomes.contains(this.context.getBiome())`.
struct BiomeCondition {
    cache: LazyCache,
    surface_context: Arc<SurfaceContext>,
    biomes: HolderSet<BiomeId>,
}

impl Condition for BiomeCondition {
    fn test(&self) -> bool {
        self.cache
            .test(self.surface_context.cells.last_update_y(), || {
                let biome = self.surface_context.get_biome();
                self.biomes.contains(&biome)
            })
    }
}

impl BiomeConditionSource {
    /// `canNeverMatch(Set<Holder<Biome>>)`.
    fn can_never_match(&self, possible_biomes: &[Holder<BiomeId>]) -> bool {
        for biome in self.biomes.iter() {
            if possible_biomes.contains(biome) {
                return false;
            }
        }
        true
    }

    /// `willAlwaysMatch(Set<Holder<Biome>>)`.
    fn will_always_match(&self, possible_biomes: &[Holder<BiomeId>]) -> bool {
        for possible_biome in possible_biomes {
            if !self.biomes.contains(possible_biome) {
                return false;
            }
        }
        true
    }
}

impl std::fmt::Display for BiomeConditionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BiomeConditionSource[biomes={:?}]", self.biomes)
    }
}

/// `RegistryCodecs.homogeneousList(Registries.BIOME)` — the `Codec<HolderSet>`
/// the `"biome_is"` field wraps (the `matching_biomes_predicate.rs` pattern).
/// Java: `RegistryCodecs.homogeneousList(Registries.BIOME).fieldOf("biome_is")`
/// — the list is a plain `Codec`, `.fieldOf` makes the `MapCodec`.
fn biomes_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<HolderSet<BiomeId>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<Holder<BiomeId>, Ops>> = Arc::new(RegistryFixedCodec::create(
        &rivet_registry::registries::BIOME,
    ));
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<BiomeId>, Ops>> = Arc::new(HolderSetCodec::create(
        &rivet_registry::registries::BIOME,
        element,
        false,
    ));
    holder_set
}

/// Lift a concrete condition's `MapCodec<C>` to `MapCodec<Arc<dyn
/// ConditionSource>>` (Java's `MapCodec<? extends ConditionSource>` variance).
fn erase_condition_map_codec<C, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    location: String,
) -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
where
    C: ConditionSource + Clone + 'static,
{
    map_codec::xmap(
        inner,
        Arc::new(move |c: &C| -> Arc<dyn ConditionSource> { Arc::new(c.clone()) }),
        Arc::new(move |f: &Arc<dyn ConditionSource>| -> C {
            f.as_any()
                .downcast_ref::<C>()
                .unwrap_or_else(|| {
                    panic!(
                        "condition codec for '{}' applied to a value of a different type",
                        location
                    )
                })
                .clone()
        }),
    )
}

/// `ConditionSource::type_id` — the material-condition registry key for a
/// concrete source.
impl dyn ConditionSource {
    /// `BuiltInRegistries.MATERIAL_CONDITION` key for the source.
    pub fn condition_source_type_id(&self) -> ConditionSourceTypeId {
        if self.as_any().is::<BiomeConditionSource>() {
            material_condition_types::BIOME
        } else if self.as_any().is::<NoiseThresholdConditionSource>() {
            material_condition_types::NOISE_THRESHOLD
        } else if self.as_any().is::<VerticalGradientConditionSource>() {
            material_condition_types::VERTICAL_GRADIENT
        } else if self.as_any().is::<YConditionSource>() {
            material_condition_types::Y_ABOVE
        } else if self.as_any().is::<WaterConditionSource>() {
            material_condition_types::WATER
        } else if self.as_any().is::<Temperature>() {
            material_condition_types::TEMPERATURE
        } else if self.as_any().is::<Steep>() {
            material_condition_types::STEEP
        } else if self.as_any().is::<NotConditionSource>() {
            material_condition_types::NOT
        } else if self.as_any().is::<Hole>() {
            material_condition_types::HOLE
        } else if self.as_any().is::<AbovePreliminarySurface>() {
            material_condition_types::ABOVE_PRELIMINARY_SURFACE
        } else if self.as_any().is::<StoneDepthCheck>() {
            material_condition_types::STONE_DEPTH
        } else {
            unreachable!("unknown material condition type")
        }
    }
}

/// `RuleSource::type_id` — the material-rule registry key.
impl dyn RuleSource {
    /// `BuiltInRegistries.MATERIAL_RULE` key for the source.
    pub fn rule_source_type_id(&self) -> ConditionSourceTypeId {
        if self.as_any().is::<Bandlands>() {
            material_rule_types::BANDLANDS
        } else if self.as_any().is::<BlockRuleSource>() {
            material_rule_types::BLOCK
        } else if self.as_any().is::<SequenceRuleSource>() {
            material_rule_types::SEQUENCE
        } else if self.as_any().is::<TestRuleSource>() {
            material_rule_types::CONDITION
        } else {
            unreachable!("unknown material rule type")
        }
    }
}

/// A constant `Condition` (`() -> true` / `() -> false`) — the
/// `possibleBiomes` short-circuit.
struct ConstCondition(bool);

impl Condition for ConstCondition {
    fn test(&self) -> bool {
        self.0
    }
}

/// `SurfaceRules.NoiseThresholdConditionSource` — `record(ResourceKey noise,
/// double minThreshold, double maxThreshold, boolean is3d)`.
#[derive(Debug, Clone)]
pub struct NoiseThresholdConditionSource {
    /// `noise`.
    pub noise: ResourceKey<NoiseParameters>,
    /// `minThreshold`.
    pub min_threshold: f64,
    /// `maxThreshold`.
    pub max_threshold: f64,
    /// `is3d`.
    pub is_3d: bool,
}

impl NoiseThresholdConditionSource {
    /// `new NoiseThresholdConditionSource(...)`.
    pub fn new(
        noise: ResourceKey<NoiseParameters>,
        min_threshold: f64,
        max_threshold: f64,
        is_3d: bool,
    ) -> Self {
        NoiseThresholdConditionSource {
            noise,
            min_threshold,
            max_threshold,
            is_3d,
        }
    }

    /// `noise`.
    pub fn noise(&self) -> &ResourceKey<NoiseParameters> {
        &self.noise
    }

    /// `NoiseThresholdConditionSource.CODEC` — the 4-field record.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        let inner = record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|s: &NoiseThresholdConditionSource| s.noise.clone()),
                    codec::field_of(
                        rivet_registry::resource_key::resource_key_codec::<NoiseParameters, Ops>(
                            &crate::levelgen::noise::registry_keys::NOISE,
                        ),
                        "noise".to_string(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &NoiseThresholdConditionSource| s.min_threshold),
                    codec::field_of(codec::double_codec::<Ops>(), "min_threshold".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &NoiseThresholdConditionSource| s.max_threshold),
                    codec::field_of(codec::double_codec::<Ops>(), "max_threshold".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &NoiseThresholdConditionSource| s.is_3d),
                    codec::optional_field_of("is_3d", codec::bool_codec::<Ops>(), false),
                ))
                .apply(
                    instance,
                    Arc::new(|noise, min_threshold, max_threshold, is_3d| {
                        NoiseThresholdConditionSource::new(
                            noise,
                            min_threshold,
                            max_threshold,
                            is_3d,
                        )
                    }),
                )
        });
        erase_condition_map_codec::<NoiseThresholdConditionSource, Ops>(
            inner,
            "noise_threshold".to_string(),
        )
    }
}

impl ConditionSource for NoiseThresholdConditionSource {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        let noise_sampler = context.get_noise_sampler(&self.noise, self.is_3d);
        let min_threshold = self.min_threshold;
        let max_threshold = self.max_threshold;
        Arc::new(NoiseThresholdCondition {
            noise_sampler,
            min_threshold,
            max_threshold,
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `NoiseThresholdCondition` — `value >= min && value <= max`.
struct NoiseThresholdCondition {
    noise_sampler: NoiseSampler,
    min_threshold: f64,
    max_threshold: f64,
}

impl Condition for NoiseThresholdCondition {
    fn test(&self) -> bool {
        let value = (self.noise_sampler)();
        value >= self.min_threshold && value <= self.max_threshold
    }
}

/// `SurfaceRules.VerticalGradientConditionSource` — `record(Identifier
/// randomName, VerticalAnchor trueAtAndBelow, VerticalAnchor falseAtAndAbove)`.
#[derive(Debug, Clone)]
pub struct VerticalGradientConditionSource {
    /// `randomName`.
    pub random_name: Identifier,
    /// `trueAtAndBelow`.
    pub true_at_and_below: VerticalAnchor,
    /// `falseAtAndAbove`.
    pub false_at_and_above: VerticalAnchor,
}

impl VerticalGradientConditionSource {
    /// `new VerticalGradientConditionSource(Identifier, VerticalAnchor,
    /// VerticalAnchor)`.
    pub fn new(
        random_name: Identifier,
        true_at_and_below: VerticalAnchor,
        false_at_and_above: VerticalAnchor,
    ) -> Self {
        VerticalGradientConditionSource {
            random_name,
            true_at_and_below,
            false_at_and_above,
        }
    }

    /// `VerticalGradientConditionSource.CODEC` — the 3-field record.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        let inner = record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|s: &VerticalGradientConditionSource| s.random_name.clone()),
                    codec::field_of(
                        rivet_registry::identifier::identifier_codec::<Ops>(),
                        "random_name".to_string(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &VerticalGradientConditionSource| s.true_at_and_below),
                    codec::field_of(
                        vertical_anchor_codec::<Ops>(),
                        "true_at_and_below".to_string(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &VerticalGradientConditionSource| s.false_at_and_above),
                    codec::field_of(
                        vertical_anchor_codec::<Ops>(),
                        "false_at_and_above".to_string(),
                    ),
                ))
                .apply(
                    instance,
                    Arc::new(|random_name, true_at_and_below, false_at_and_above| {
                        VerticalGradientConditionSource::new(
                            random_name,
                            true_at_and_below,
                            false_at_and_above,
                        )
                    }),
                )
        });
        erase_condition_map_codec::<VerticalGradientConditionSource, Ops>(
            inner,
            "vertical_gradient".to_string(),
        )
    }
}

impl ConditionSource for VerticalGradientConditionSource {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        let true_at_and_below = self
            .true_at_and_below
            .resolve_y(&context.surface_context.worldgen_context);
        let false_at_and_above = self
            .false_at_and_above
            .resolve_y(&context.surface_context.worldgen_context);
        let random_factory = context
            .random_state
            .get_or_create_random_factory(&self.random_name);
        let surface_context = context.surface_context.clone();
        Arc::new(VerticalGradientCondition {
            cache: LazyCache::new(surface_context.cells.last_update_y()),
            surface_context,
            true_at_and_below,
            false_at_and_above,
            random_factory,
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `VerticalGradientCondition` — a `LazyYCondition` over the block-Y
/// gradient with the `at(x, blockY, z)` random draw.
struct VerticalGradientCondition {
    cache: LazyCache,
    surface_context: Arc<SurfaceContext>,
    true_at_and_below: i32,
    false_at_and_above: i32,
    random_factory: AlgorithmPositionalRandomFactory,
}

impl Condition for VerticalGradientCondition {
    fn test(&self) -> bool {
        self.cache
            .test(self.surface_context.cells.last_update_y(), || {
                let block_y = self.surface_context.cells.block_y();
                if block_y <= self.true_at_and_below {
                    true
                } else if block_y >= self.false_at_and_above {
                    false
                } else {
                    let probability = mth::map(
                        block_y as f64,
                        self.true_at_and_below as f64,
                        self.false_at_and_above as f64,
                        1.0,
                        0.0,
                    );
                    let mut random = self.random_factory.at(
                        self.surface_context.cells.block_x(),
                        block_y,
                        self.surface_context.cells.block_z(),
                    );
                    // Java `random.nextFloat() < probability` compares the float
                    // widened to double against the double — the comparison is in
                    // double precision.
                    (random.next_float() as f64) < probability
                }
            })
    }
}

/// `SurfaceRules.YConditionSource` — `record(VerticalAnchor anchor, int
/// surfaceDepthMultiplier, boolean addStoneDepth)`.
#[derive(Debug, Clone)]
pub struct YConditionSource {
    /// `anchor`.
    pub anchor: VerticalAnchor,
    /// `surfaceDepthMultiplier`.
    pub surface_depth_multiplier: i32,
    /// `addStoneDepth`.
    pub add_stone_depth: bool,
}

impl YConditionSource {
    /// `new YConditionSource(VerticalAnchor, int, boolean)`.
    pub fn new(
        anchor: VerticalAnchor,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    ) -> Self {
        YConditionSource {
            anchor,
            surface_depth_multiplier,
            add_stone_depth,
        }
    }

    /// `YConditionSource.CODEC` — the 3-field record (`surface_depth_multiplier`
    /// is `Codec.intRange(-20, 20)`).
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        let inner = record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|s: &YConditionSource| s.anchor),
                    codec::field_of(vertical_anchor_codec::<Ops>(), "anchor".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &YConditionSource| s.surface_depth_multiplier),
                    codec::field_of(
                        codec::int_range(-20, 20),
                        "surface_depth_multiplier".to_string(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &YConditionSource| s.add_stone_depth),
                    codec::field_of(codec::bool_codec::<Ops>(), "add_stone_depth".to_string()),
                ))
                .apply(
                    instance,
                    Arc::new(|anchor, surface_depth_multiplier, add_stone_depth| {
                        YConditionSource::new(anchor, surface_depth_multiplier, add_stone_depth)
                    }),
                )
        });
        erase_condition_map_codec::<YConditionSource, Ops>(inner, "y_above".to_string())
    }
}

impl ConditionSource for YConditionSource {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        let anchor = self.anchor;
        let surface_depth_multiplier = self.surface_depth_multiplier;
        let add_stone_depth = self.add_stone_depth;
        let surface_context = context.surface_context.clone();
        Arc::new(YCondition {
            cache: LazyCache::new(surface_context.cells.last_update_y()),
            surface_context,
            anchor,
            surface_depth_multiplier,
            add_stone_depth,
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `YCondition` — a `LazyYCondition` comparing `blockY (+ stoneDepthAbove)`
/// against `anchor.resolveY(context) + surfaceDepth * multiplier`.
struct YCondition {
    cache: LazyCache,
    surface_context: Arc<SurfaceContext>,
    anchor: VerticalAnchor,
    surface_depth_multiplier: i32,
    add_stone_depth: bool,
}

impl Condition for YCondition {
    fn test(&self) -> bool {
        self.cache
            .test(self.surface_context.cells.last_update_y(), || {
                let block_y = self.surface_context.cells.block_y();
                let stone_depth_above = if self.add_stone_depth {
                    self.surface_context.cells.stone_depth_above()
                } else {
                    0
                };
                let anchor_y = self
                    .anchor
                    .resolve_y(&self.surface_context.worldgen_context);
                let surface_depth = self.surface_context.cells.surface_depth();
                block_y.wrapping_add(stone_depth_above)
                    >= anchor_y
                        .wrapping_add(surface_depth.wrapping_mul(self.surface_depth_multiplier))
            })
    }
}

/// `SurfaceRules.WaterConditionSource` — `record(int offset, int
/// surfaceDepthMultiplier, boolean addStoneDepth)`.
#[derive(Debug, Clone)]
pub struct WaterConditionSource {
    /// `offset`.
    pub offset: i32,
    /// `surfaceDepthMultiplier`.
    pub surface_depth_multiplier: i32,
    /// `addStoneDepth`.
    pub add_stone_depth: bool,
}

impl WaterConditionSource {
    /// `new WaterConditionSource(int, int, boolean)`.
    pub fn new(offset: i32, surface_depth_multiplier: i32, add_stone_depth: bool) -> Self {
        WaterConditionSource {
            offset,
            surface_depth_multiplier,
            add_stone_depth,
        }
    }

    /// `WaterConditionSource.CODEC` — the 3-field record (`offset` plain,
    /// `surface_depth_multiplier` intRange(-20, 20)).
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        let inner = record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|s: &WaterConditionSource| s.offset),
                    codec::field_of(codec::int_codec::<Ops>(), "offset".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &WaterConditionSource| s.surface_depth_multiplier),
                    codec::field_of(
                        codec::int_range(-20, 20),
                        "surface_depth_multiplier".to_string(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &WaterConditionSource| s.add_stone_depth),
                    codec::field_of(codec::bool_codec::<Ops>(), "add_stone_depth".to_string()),
                ))
                .apply(
                    instance,
                    Arc::new(|offset, surface_depth_multiplier, add_stone_depth| {
                        WaterConditionSource::new(offset, surface_depth_multiplier, add_stone_depth)
                    }),
                )
        });
        erase_condition_map_codec::<WaterConditionSource, Ops>(inner, "water".to_string())
    }
}

impl ConditionSource for WaterConditionSource {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        let offset = self.offset;
        let surface_depth_multiplier = self.surface_depth_multiplier;
        let add_stone_depth = self.add_stone_depth;
        let surface_context = context.surface_context.clone();
        Arc::new(WaterCondition {
            cache: LazyCache::new(surface_context.cells.last_update_y()),
            surface_context,
            offset,
            surface_depth_multiplier,
            add_stone_depth,
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `WaterCondition` — a `LazyYCondition` comparing `blockY (+
/// stoneDepthAbove)` against `waterHeight + offset + surfaceDepth *
/// surfaceDepthMultiplier` (or `waterHeight == Integer.MIN_VALUE`).
struct WaterCondition {
    cache: LazyCache,
    surface_context: Arc<SurfaceContext>,
    offset: i32,
    surface_depth_multiplier: i32,
    add_stone_depth: bool,
}

impl Condition for WaterCondition {
    fn test(&self) -> bool {
        self.cache
            .test(self.surface_context.cells.last_update_y(), || {
                let water_height = self.surface_context.cells.water_height();
                let stone_depth_above = if self.add_stone_depth {
                    self.surface_context.cells.stone_depth_above()
                } else {
                    0
                };
                let surface_depth = self.surface_context.cells.surface_depth();
                water_height == i32::MIN
                    || self
                        .surface_context
                        .cells
                        .block_y()
                        .wrapping_add(stone_depth_above)
                        >= water_height
                            .wrapping_add(self.offset)
                            .wrapping_add(surface_depth.wrapping_mul(self.surface_depth_multiplier))
            })
    }
}

/// `SurfaceRules.Temperature` — `enum INSTANCE`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Temperature;

impl Temperature {
    /// `Temperature.CODEC` — `MapCodec.unit(INSTANCE)`.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        erase_condition_map_codec::<Temperature, Ops>(
            map_codec::unit(Temperature),
            "temperature".to_string(),
        )
    }
}

impl ConditionSource for Temperature {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        context.temperature.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `SurfaceRules.Steep` — `enum INSTANCE`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Steep;

impl Steep {
    /// `Steep.CODEC` — `MapCodec.unit(INSTANCE)`.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        erase_condition_map_codec::<Steep, Ops>(map_codec::unit(Steep), "steep".to_string())
    }
}

impl ConditionSource for Steep {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        context.steep.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `SurfaceRules.Hole` — `enum INSTANCE`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Hole;

impl Hole {
    /// `Hole.CODEC` — `MapCodec.unit(INSTANCE)`.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        erase_condition_map_codec::<Hole, Ops>(map_codec::unit(Hole), "hole".to_string())
    }
}

impl ConditionSource for Hole {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        context.hole.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `SurfaceRules.AbovePreliminarySurface` — `enum INSTANCE`.
#[derive(Debug, Clone, Copy, Default)]
pub struct AbovePreliminarySurface;

impl AbovePreliminarySurface {
    /// `AbovePreliminarySurface.CODEC` — `MapCodec.unit(INSTANCE)`.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        erase_condition_map_codec::<AbovePreliminarySurface, Ops>(
            map_codec::unit(AbovePreliminarySurface),
            "above_preliminary_surface".to_string(),
        )
    }
}

impl ConditionSource for AbovePreliminarySurface {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        context.above_preliminary_surface.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `SurfaceRules.NotConditionSource` — `record(ConditionSource target)`.
#[derive(Debug, Clone)]
pub struct NotConditionSource {
    /// `target`.
    pub target: Arc<dyn ConditionSource>,
}

impl NotConditionSource {
    /// `new NotConditionSource(ConditionSource)`.
    pub fn new(target: Arc<dyn ConditionSource>) -> Self {
        NotConditionSource { target }
    }

    /// `NotConditionSource.CODEC` — `ConditionSource.CODEC.xmap(...).fieldOf("invert")`.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        let inner = map_codec::xmap(
            codec::field_of(condition_source_codec::<Ops>(), "invert".to_string()),
            Arc::new(|t: &Arc<dyn ConditionSource>| NotConditionSource::new(t.clone())),
            Arc::new(|s: &NotConditionSource| s.target.clone()),
        );
        erase_condition_map_codec::<NotConditionSource, Ops>(inner, "not".to_string())
    }
}

impl ConditionSource for NotConditionSource {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        Arc::new(NotCondition {
            target: self.target.apply(context),
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `SurfaceRules.NotCondition` — `record(Condition target)`.
struct NotCondition {
    target: Arc<dyn Condition>,
}

impl Condition for NotCondition {
    fn test(&self) -> bool {
        !self.target.test()
    }
}

/// `SurfaceRules.StoneDepthCheck` — `record(int offset, boolean addSurfaceDepth,
/// int secondaryDepthRange, CaveSurface surfaceType)`.
#[derive(Debug, Clone)]
pub struct StoneDepthCheck {
    /// `offset`.
    pub offset: i32,
    /// `addSurfaceDepth`.
    pub add_surface_depth: bool,
    /// `secondaryDepthRange`.
    pub secondary_depth_range: i32,
    /// `surfaceType`.
    pub surface_type: CaveSurface,
}

impl StoneDepthCheck {
    /// `new StoneDepthCheck(int, boolean, int, CaveSurface)`.
    pub fn new(
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        surface_type: CaveSurface,
    ) -> Self {
        StoneDepthCheck {
            offset,
            add_surface_depth,
            secondary_depth_range,
            surface_type,
        }
    }

    /// `StoneDepthCheck.CODEC` — the 4-field record.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn ConditionSource>, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        let inner = record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|s: &StoneDepthCheck| s.offset),
                    codec::field_of(codec::int_codec::<Ops>(), "offset".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &StoneDepthCheck| s.add_surface_depth),
                    codec::field_of(codec::bool_codec::<Ops>(), "add_surface_depth".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &StoneDepthCheck| s.secondary_depth_range),
                    codec::field_of(
                        codec::int_codec::<Ops>(),
                        "secondary_depth_range".to_string(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &StoneDepthCheck| s.surface_type),
                    codec::field_of(
                        Arc::new(cave_surface_codec::<Ops>()),
                        "surface_type".to_string(),
                    ),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |offset, add_surface_depth, secondary_depth_range, surface_type| {
                            StoneDepthCheck::new(
                                offset,
                                add_surface_depth,
                                secondary_depth_range,
                                surface_type,
                            )
                        },
                    ),
                )
        });
        erase_condition_map_codec::<StoneDepthCheck, Ops>(inner, "stone_depth".to_string())
    }
}

impl ConditionSource for StoneDepthCheck {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn Condition> {
        let ceiling = self.surface_type == CaveSurface::Ceiling;
        let offset = self.offset;
        let add_surface_depth = self.add_surface_depth;
        let secondary_depth_range = self.secondary_depth_range;
        let surface_context = context.surface_context.clone();
        Arc::new(StoneDepthCondition {
            cache: LazyCache::new(surface_context.cells.last_update_y()),
            surface_context,
            ceiling,
            offset,
            add_surface_depth,
            secondary_depth_range,
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `StoneDepthCondition` — a `LazyYCondition` computing the stone-depth
/// comparison.
struct StoneDepthCondition {
    cache: LazyCache,
    surface_context: Arc<SurfaceContext>,
    ceiling: bool,
    offset: i32,
    add_surface_depth: bool,
    secondary_depth_range: i32,
}

impl Condition for StoneDepthCondition {
    fn test(&self) -> bool {
        self.cache
            .test(self.surface_context.cells.last_update_y(), || {
                let stone_depth = if self.ceiling {
                    self.surface_context.cells.stone_depth_below()
                } else {
                    self.surface_context.cells.stone_depth_above()
                };
                let surface_depth = if self.add_surface_depth {
                    self.surface_context.cells.surface_depth()
                } else {
                    0
                };
                let secondary_surface_depth = if self.secondary_depth_range == 0 {
                    0
                } else {
                    // `(int) Mth.map(getSurfaceSecondary(), -1.0, 1.0, 0.0, range)`.
                    mth::map(
                        self.surface_context.get_surface_secondary(),
                        -1.0,
                        1.0,
                        0.0,
                        self.secondary_depth_range as f64,
                    ) as i32
                };
                stone_depth
                    <= 1i32
                        .wrapping_add(self.offset)
                        .wrapping_add(surface_depth)
                        .wrapping_add(secondary_surface_depth)
            })
    }
}

// ---------------------------------------------------------------------------
// RuleSource implementations
// ---------------------------------------------------------------------------

/// `SurfaceRules.Bandlands` — `enum INSTANCE`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Bandlands;

impl Bandlands {
    /// `Bandlands.CODEC` — `MapCodec.unit(INSTANCE)`.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<ArcRuleSource, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        erase_rule_map_codec::<Bandlands, Ops>(map_codec::unit(Bandlands), "bandlands".to_string())
    }
}

impl RuleSource for Bandlands {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn SurfaceRule> {
        Arc::new(BandlandsRule {
            system: context.surface_context.system.clone(),
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `context.system::getBand` — the applied bandlands rule.
struct BandlandsRule {
    system: Arc<SurfaceSystem>,
}

impl SurfaceRule for BandlandsRule {
    fn try_apply(&self, block_x: i32, block_y: i32, block_z: i32) -> Option<BlockState> {
        Some(self.system.get_band(block_x, block_y, block_z))
    }
}

/// `SurfaceRules.BlockRuleSource` — `record(BlockState resultState, StateRule
/// rule)`.
#[derive(Debug, Clone)]
pub struct BlockRuleSource {
    /// `resultState`.
    pub result_state: BlockState,
    /// `rule` — `new StateRule(state)`.
    pub rule: StateRule,
}

impl BlockRuleSource {
    /// `new BlockRuleSource(BlockState)`.
    pub fn new(state: BlockState) -> Self {
        BlockRuleSource {
            result_state: state,
            rule: StateRule::new(state),
        }
    }

    /// `BlockRuleSource.CODEC` — `BlockState.CODEC.xmap(...).fieldOf("result_state")`.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<ArcRuleSource, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        let inner = map_codec::xmap(
            codec::field_of(
                rivet_registry::block_state_codec::block_state_codec::<Ops>(),
                "result_state".to_string(),
            ),
            Arc::new(|s: &BlockState| BlockRuleSource::new(*s)),
            Arc::new(|s: &BlockRuleSource| s.result_state),
        );
        erase_rule_map_codec::<BlockRuleSource, Ops>(inner, "block".to_string())
    }
}

impl RuleSource for BlockRuleSource {
    fn apply<'a>(&self, _context: &Context<'a>) -> Arc<dyn SurfaceRule> {
        Arc::new(self.rule)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `SurfaceRules.StateRule` — `record(BlockState state)`.
#[derive(Debug, Clone, Copy)]
pub struct StateRule {
    /// `state`.
    pub state: BlockState,
}

impl StateRule {
    /// `new StateRule(BlockState)`.
    pub fn new(state: BlockState) -> Self {
        StateRule { state }
    }
}

impl SurfaceRule for StateRule {
    fn try_apply(&self, _block_x: i32, _block_y: i32, _block_z: i32) -> Option<BlockState> {
        Some(self.state)
    }
}

/// `SurfaceRules.SequenceRuleSource` — `record(List<RuleSource> sequence)`.
#[derive(Debug, Clone)]
pub struct SequenceRuleSource {
    /// `sequence`.
    pub sequence: Vec<ArcRuleSource>,
}

impl SequenceRuleSource {
    /// `new SequenceRuleSource(List<RuleSource>)`.
    pub fn new(sequence: Vec<ArcRuleSource>) -> Self {
        SequenceRuleSource { sequence }
    }

    /// `SequenceRuleSource.CODEC` — `RuleSource.CODEC.listOf().xmap(...).fieldOf("sequence")`.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<ArcRuleSource, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        let inner = map_codec::xmap(
            codec::field_of(
                codec::list(rule_source_codec::<Ops>()),
                "sequence".to_string(),
            ),
            Arc::new(|l: &Vec<ArcRuleSource>| SequenceRuleSource::new(l.clone())),
            Arc::new(|s: &SequenceRuleSource| s.sequence.clone()),
        );
        erase_rule_map_codec::<SequenceRuleSource, Ops>(inner, "sequence".to_string())
    }
}

impl RuleSource for SequenceRuleSource {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn SurfaceRule> {
        if self.sequence.len() == 1 {
            return self.sequence[0].apply(context);
        }
        let mut rules = Vec::with_capacity(self.sequence.len());
        for rule in &self.sequence {
            rules.push(rule.apply(context));
        }
        Arc::new(SequenceRule::new(rules))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `SurfaceRules.SequenceRule` — `record(List<SurfaceRule> rules)`.
pub struct SequenceRule {
    /// `rules`.
    pub rules: Vec<Arc<dyn SurfaceRule>>,
}

impl SequenceRule {
    /// `new SequenceRule(List<SurfaceRule>)`.
    pub fn new(rules: Vec<Arc<dyn SurfaceRule>>) -> Self {
        SequenceRule { rules }
    }
}

impl std::fmt::Debug for SequenceRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SequenceRule")
            .field("rules", &self.rules.len())
            .finish_non_exhaustive()
    }
}

impl SurfaceRule for SequenceRule {
    fn try_apply(&self, block_x: i32, block_y: i32, block_z: i32) -> Option<BlockState> {
        for rule in &self.rules {
            if let Some(state) = rule.try_apply(block_x, block_y, block_z) {
                return Some(state);
            }
        }
        None
    }
}

/// `SurfaceRules.TestRuleSource` — `record(ConditionSource ifTrue, RuleSource
/// thenRun)`.
#[derive(Debug, Clone)]
pub struct TestRuleSource {
    /// `ifTrue`.
    pub if_true: Arc<dyn ConditionSource>,
    /// `thenRun`.
    pub then_run: ArcRuleSource,
}

impl TestRuleSource {
    /// `new TestRuleSource(ConditionSource, RuleSource)`.
    pub fn new(if_true: Arc<dyn ConditionSource>, then_run: ArcRuleSource) -> Self {
        TestRuleSource { if_true, then_run }
    }

    /// `TestRuleSource.CODEC` — the `"if_true"`/`"then_run"` record.
    pub fn codec<Ops>() -> Arc<dyn MapCodec<ArcRuleSource, Ops>>
    where
        Ops: DynamicOps + 'static + RegistryOpsLookup,
    {
        let inner = record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|s: &TestRuleSource| s.if_true.clone()),
                    codec::field_of(condition_source_codec::<Ops>(), "if_true".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &TestRuleSource| s.then_run.clone()),
                    codec::field_of(rule_source_codec::<Ops>(), "then_run".to_string()),
                ))
                .apply(instance, Arc::new(TestRuleSource::new))
        });
        erase_rule_map_codec::<TestRuleSource, Ops>(inner, "condition".to_string())
    }
}

impl RuleSource for TestRuleSource {
    fn apply<'a>(&self, context: &Context<'a>) -> Arc<dyn SurfaceRule> {
        Arc::new(TestRule::new(
            self.if_true.apply(context),
            self.then_run.apply(context),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `SurfaceRules.TestRule` — `record(Condition condition, SurfaceRule followup)`.
struct TestRule {
    condition: Arc<dyn Condition>,
    followup: Arc<dyn SurfaceRule>,
}

impl TestRule {
    fn new(condition: Arc<dyn Condition>, followup: Arc<dyn SurfaceRule>) -> Self {
        TestRule {
            condition,
            followup,
        }
    }
}

impl SurfaceRule for TestRule {
    fn try_apply(&self, block_x: i32, block_y: i32, block_z: i32) -> Option<BlockState> {
        if !self.condition.test() {
            return None;
        }
        self.followup.try_apply(block_x, block_y, block_z)
    }
}

/// Lift a concrete rule's `MapCodec<C>` to `MapCodec<Arc<dyn RuleSource>>`.
fn erase_rule_map_codec<C, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    location: String,
) -> Arc<dyn MapCodec<ArcRuleSource, Ops>>
where
    C: RuleSource + Clone + 'static,
{
    map_codec::xmap(
        inner,
        Arc::new(move |c: &C| -> ArcRuleSource { Arc::new(c.clone()) }),
        Arc::new(move |f: &ArcRuleSource| -> C {
            f.as_any()
                .downcast_ref::<C>()
                .unwrap_or_else(|| {
                    panic!(
                        "rule codec for '{}' applied to a value of a different type",
                        location
                    )
                })
                .clone()
        }),
    )
}

// ---------------------------------------------------------------------------
// SurfaceSystem
// ---------------------------------------------------------------------------

/// `SurfaceSystem`'s clay-band block states — the `Blocks` constants (each
/// pinned to its generated registry id by the `Blocks` table test).
fn white_terracotta_state() -> BlockState {
    Blocks::WHITE_TERRACOTTA.default_block_state()
}
fn orange_terracotta_state() -> BlockState {
    Blocks::ORANGE_TERRACOTTA.default_block_state()
}
fn yellow_terracotta_state() -> BlockState {
    Blocks::YELLOW_TERRACOTTA.default_block_state()
}
fn brown_terracotta_state() -> BlockState {
    Blocks::BROWN_TERRACOTTA.default_block_state()
}
fn red_terracotta_state() -> BlockState {
    Blocks::RED_TERRACOTTA.default_block_state()
}
fn light_gray_terracotta_state() -> BlockState {
    Blocks::LIGHT_GRAY_TERRACOTTA.default_block_state()
}
fn terracotta_state() -> BlockState {
    Blocks::TERRACOTTA.default_block_state()
}
/// The `#177` frozen-ocean extension block states.
#[allow(dead_code)]
fn packed_ice_state() -> BlockState {
    Blocks::PACKED_ICE.default_block_state()
}
#[allow(dead_code)]
fn snow_block_state() -> BlockState {
    Blocks::SNOW_BLOCK.default_block_state()
}

/// `Math.round(double)` → the truncating int cast, exactly as Java
/// `(int)Math.round(x)` = `(int)floor(x + 0.5)`.
fn java_math_round(x: f64) -> i32 {
    (x + 0.5).floor() as i32
}

/// `net.minecraft.world.level.levelgen.SurfaceSystem`.
#[derive(Clone)]
/// The six badlands/iceberg extension noises (`badlandsPillarNoise` ...) are
/// read only by the `#177` surface-build extensions.
#[allow(dead_code)]
pub struct SurfaceSystem {
    /// `defaultBlock`.
    default_block: BlockState,
    /// `seaLevel`.
    sea_level: i32,
    /// `clayBands` — the 192-entry band array.
    clay_bands: Vec<BlockState>,
    /// `clayBandsOffsetNoise`.
    clay_bands_offset_noise: NormalNoise,
    /// `badlandsPillarNoise`.
    badlands_pillar_noise: NormalNoise,
    /// `badlandsPillarRoofNoise`.
    badlands_pillar_roof_noise: NormalNoise,
    /// `badlandsSurfaceNoise`.
    badlands_surface_noise: NormalNoise,
    /// `icebergPillarNoise`.
    iceberg_pillar_noise: NormalNoise,
    /// `icebergPillarRoofNoise`.
    iceberg_pillar_roof_noise: NormalNoise,
    /// `icebergSurfaceNoise`.
    iceberg_surface_noise: NormalNoise,
    /// `noiseRandom`.
    noise_random: AlgorithmPositionalRandomFactory,
    /// `surfaceNoise`.
    surface_noise: NormalNoise,
    /// `surfaceSecondaryNoise`.
    surface_secondary_noise: NormalNoise,
}

impl std::fmt::Debug for SurfaceSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceSystem")
            .field("default_block", &self.default_block)
            .field("sea_level", &self.sea_level)
            .field("clay_bands", &self.clay_bands.len())
            .finish_non_exhaustive()
    }
}

impl SurfaceSystem {
    /// `new SurfaceSystem(RandomState, BlockState defaultBlock, int seaLevel,
    /// PositionalRandomFactory noiseRandom)`.
    ///
    /// The `RandomState` parameter is replaced by the noise-getter closure: the
    /// constructor needs `randomState.getOrCreateNoise` nine times, and the
    /// Rust `RandomState` builds this system inside `RandomState::create`
    /// before the struct exists (the `&self` borrow would not be available).
    /// The nine `Noises.*` keys are resolved through `get_or_create_noise` with
    /// the world's noise registry.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        get_or_create_noise: &dyn Fn(&ResourceKey<NoiseParameters>) -> NormalNoise,
        default_block: BlockState,
        sea_level: i32,
        noise_random: AlgorithmPositionalRandomFactory,
    ) -> Self {
        let clay_bands_offset_noise = get_or_create_noise(&noises::CLAY_BANDS_OFFSET);
        let clay_bands = {
            // Java: `noiseRandom.fromHashOf(Identifier.withDefaultNamespace(
            // "clay_bands"))` — the namespaced string hash.
            let mut bands_random = noise_random
                .from_hash_of_identifier(&Identifier::with_default_namespace("clay_bands"));
            Self::generate_bands(&mut bands_random)
        };
        let surface_noise = get_or_create_noise(&noises::SURFACE);
        let surface_secondary_noise = get_or_create_noise(&noises::SURFACE_SECONDARY);
        let badlands_pillar_noise = get_or_create_noise(&noises::BADLANDS_PILLAR);
        let badlands_pillar_roof_noise = get_or_create_noise(&noises::BADLANDS_PILLAR_ROOF);
        let badlands_surface_noise = get_or_create_noise(&noises::BADLANDS_SURFACE);
        let iceberg_pillar_noise = get_or_create_noise(&noises::ICEBERG_PILLAR);
        let iceberg_pillar_roof_noise = get_or_create_noise(&noises::ICEBERG_PILLAR_ROOF);
        let iceberg_surface_noise = get_or_create_noise(&noises::ICEBERG_SURFACE);
        SurfaceSystem {
            default_block,
            sea_level,
            clay_bands,
            clay_bands_offset_noise,
            badlands_pillar_noise,
            badlands_pillar_roof_noise,
            badlands_surface_noise,
            iceberg_pillar_noise,
            iceberg_pillar_roof_noise,
            iceberg_surface_noise,
            noise_random,
            surface_noise,
            surface_secondary_noise,
        }
    }

    /// `buildSurface(RandomState, BiomeManager, boolean useLegacyRandom,
    /// WorldGenerationContext, ChunkAccess, NoiseChunk, RuleSource,
    /// @Nullable Set<Holder<Biome>>)` — **internal seam driver, not the
    /// production wire.** Java calls `SurfaceSystem.buildSurface` from
    /// `NoiseBasedChunkGenerator.doFill`; the port's production call site is
    /// `NoiseBasedChunkGenerator::build_surface_stub` (RivetTodo #177, the
    /// `levelgen.surface` wave). This method drives the column-loop arithmetic
    /// and the `ChunkColumnAdapter` x/z threading against a caller-supplied
    /// [`ChunkSurface`]; the production `ProtoChunk` implements the trait (see
    /// `chunk::proto_chunk`), exercised by the real-chunk integration tests.
    ///
    /// The block-column read is ported faithfully (the `column.getBlock` /
    /// `isStone` / `updateY` / `old == defaultBlock` / `rule.tryApply`
    /// cascade). The `getHeight(WORLD_SURFACE_WG)` reads are the `#185` seam
    /// (snapshotted at [`Self::build_surface`] start into the shared
    /// `SurfaceContext`, then read by `SteepMaterialCondition`); the
    /// `Biome`-value reads (`surfaceBiome.is(Biomes.X)`,
    /// `shouldMeltFrozenOceanIcebergSlightly`) are the biome-value seams
    /// (`dense_biome_id` + `false`); the column write
    /// (`ProtoChunk.setBlockState` + `markPosForPostProcessing`) is the real
    /// worldgen write path (`ChunkColumnAdapter` guards + writes + marks).
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // #177 — seam driver until `NoiseBasedChunkGenerator` wires it.
    pub(crate) fn build_surface<'a>(
        &self,
        random_state: &'a RandomState<'a>,
        biome_manager: Arc<BiomeManager>,
        use_legacy_random: bool,
        generation_context: Arc<WorldGenerationContext>,
        proto_chunk: &mut dyn ChunkSurface,
        noise_chunk: Arc<NoiseChunk>,
        rule_source: &ArcRuleSource,
        possible_biomes: Option<&[Holder<BiomeId>]>,
    ) {
        let mut column = ChunkColumnAdapter {
            chunk: RefCell::new(proto_chunk),
            x: Cell::new(0),
            z: Cell::new(0),
        };
        let biome_getter = {
            let bm = Arc::clone(&biome_manager);
            Arc::new(move |pos: &BlockPos| bm.get_biome(pos)) as BiomeGetter
        };
        // Capture the primed `WORLD_SURFACE_WG` heights before the context is
        // built (the `#185` heightmap seam): the applied rules read the
        // `SteepMaterialCondition` probes through this snapshot, never the
        // `&mut` chunk. Bit-identical to Java's live reads for the end-stone and
        // overworld surface writes (which keep the topmost non-air block in
        // place); the accepted divergences — an `AIR` write over the topmost
        // `defaultBlock`, and the per-column eroded-badlands raise — are
        // documented on [`seam_get_height`].
        let mut world_surface_heights = [0i32; 256];
        for x in 0..16i32 {
            for z in 0..16i32 {
                world_surface_heights[(x + z * 16) as usize] =
                    column.chunk.borrow().get_height(x, z);
            }
        }
        let context = Context::new(
            random_state.surface_system(),
            random_state,
            noise_chunk,
            biome_getter,
            generation_context,
            possible_biomes.map(|set| set.to_vec()),
            Some(Arc::new(world_surface_heights)),
        );
        let rule = rule_source.apply(&context);
        let min_block_x = column.chunk.borrow().min_block_x();
        let min_block_z = column.chunk.borrow().min_block_z();
        let eroded_badlands = BiomeId::from_name("minecraft:eroded_badlands")
            .unwrap()
            .id();
        let frozen_ocean = BiomeId::from_name("minecraft:frozen_ocean").unwrap().id();
        let deep_frozen_ocean = BiomeId::from_name("minecraft:deep_frozen_ocean")
            .unwrap()
            .id();

        for x in 0..16 {
            for z in 0..16 {
                let block_x = min_block_x.wrapping_add(x);
                let block_z = min_block_z.wrapping_add(z);
                let starting_height = column.chunk.borrow().get_height(x, z).wrapping_add(1);
                column.x.set(block_x);
                column.z.set(block_z);
                let surface_biome = biome_manager.get_biome(&BlockPos::new(
                    block_x,
                    if use_legacy_random {
                        0
                    } else {
                        starting_height
                    },
                    block_z,
                ));
                if dense_biome_id(&surface_biome) == eroded_badlands {
                    let proto_chunk_min_y = column.chunk.borrow().get_min_y();
                    self.eroded_badlands_extension(
                        &mut column,
                        block_x,
                        block_z,
                        starting_height,
                        proto_chunk_min_y,
                    );
                }

                let height = column.chunk.borrow().get_height(x, z).wrapping_add(1);
                context.update_xz(block_x, block_z);
                let mut stone_above_depth = 0;
                let mut water_height = i32::MIN;
                let mut next_ceiling_stone_y = i32::MAX;
                let end_y = column.chunk.borrow().get_min_y();

                let mut y = height;
                while y >= end_y {
                    let old = column.get_block(y);
                    if old.is_air() {
                        stone_above_depth = 0;
                        water_height = i32::MIN;
                    } else if !old.fluid_empty() {
                        if water_height == i32::MIN {
                            water_height = y.wrapping_add(1);
                        }
                    } else {
                        if next_ceiling_stone_y >= y {
                            next_ceiling_stone_y = WAY_BELOW_MIN_Y;
                            let mut lookahead_y = y.wrapping_sub(1);
                            while lookahead_y >= end_y.wrapping_sub(1) {
                                let next_state = column.get_block(lookahead_y);
                                if !self.is_stone(next_state) {
                                    next_ceiling_stone_y = lookahead_y.wrapping_add(1);
                                    break;
                                }
                                lookahead_y -= 1;
                            }
                        }

                        stone_above_depth += 1;
                        let stone_below_depth =
                            y.wrapping_sub(next_ceiling_stone_y).wrapping_add(1);
                        context.update_y(stone_above_depth, stone_below_depth, water_height, y);
                        if old == self.default_block
                            && let Some(state) = rule.try_apply(block_x, y, block_z)
                        {
                            column.set_block(y, state);
                        }
                    }
                    y -= 1;
                }

                let biome = dense_biome_id(&surface_biome);
                if biome == frozen_ocean || biome == deep_frozen_ocean {
                    // `surfaceBiome.value().shouldMeltFrozenOceanIcebergSlightly(
                    // blockPos.set(blockX, seaLevel, blockZ), seaLevel)` — the
                    // `Biome`-value read is the biome-value seam (false).
                    self.frozen_ocean_extension(
                        context.get_min_surface_level(),
                        false,
                        &mut column,
                        block_x,
                        block_z,
                        starting_height,
                    );
                }
            }
        }
    }

    /// `getSurfaceDepth(int, int)`.
    pub fn get_surface_depth(&self, block_x: i32, block_z: i32) -> i32 {
        let noise_value = self
            .surface_noise
            .get_value(block_x as f64, 0.0, block_z as f64);
        let mut random = self.noise_random.at(block_x, 0, block_z);
        (noise_value * 2.75 + 3.0 + random.next_double() * 0.25) as i32
    }

    /// `getSurfaceSecondary(int, int)`.
    pub fn get_surface_secondary(&self, block_x: i32, block_z: i32) -> f64 {
        self.surface_secondary_noise
            .get_value(block_x as f64, 0.0, block_z as f64)
    }

    /// `isStone(BlockState)` — the `#177` surface-build runtime.
    #[allow(dead_code)]
    fn is_stone(&self, state: BlockState) -> bool {
        !state.is_air() && state.fluid_empty()
    }

    /// `getSeaLevel()`.
    pub fn get_sea_level(&self) -> i32 {
        self.sea_level
    }

    /// `topMaterial(RuleSource, CarvingContext, Function<BlockPos,
    /// Holder<Biome>>, ChunkAccess, NoiseChunk, BlockPos, boolean underFluid)`
    /// — the `@Deprecated` single-column probe.
    ///
    /// Java's `CarvingContext` param is decomposed into its two consumable
    /// parts — `carvingContext.randomState()` and the embedded
    /// `WorldGenerationContext` base — so a bound carver seam closure can
    /// capture them (the closure lives inside the `CarvingContext` and cannot
    /// also borrow it). Java's `ChunkAccess chunk` is the `#185` heightmap
    /// seam (the `Context` reads heights through the seam, so it does not
    /// carry the chunk). The receiver is the shared `Arc` — the same object
    /// `RandomState` holds — so the probe's fresh `Context` shares the system
    /// without a deep clone.
    #[allow(dead_code)] // #185 — the carver probe, exercised through the bound seam tests.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn top_material<'a>(
        self: &Arc<SurfaceSystem>,
        rule_source: &ArcRuleSource,
        random_state: &'a RandomState<'a>,
        worldgen_context: Arc<WorldGenerationContext>,
        noise_chunk: Arc<NoiseChunk>,
        biome_getter: BiomeGetter,
        pos: &BlockPos,
        under_fluid: bool,
    ) -> Option<BlockState> {
        let context = Context::new(
            self.clone(),
            random_state,
            noise_chunk,
            biome_getter,
            worldgen_context,
            None,
            // No chunk: the single-column probe has no heightmap to snapshot,
            // so `SteepMaterialCondition` cannot fire here (RivetTodo #185).
            None,
        );
        let rule = rule_source.apply(&context);
        let block_x = pos.get_x();
        let block_y = pos.get_y();
        let block_z = pos.get_z();
        context.update_xz(block_x, block_z);
        context.update_y(
            1,
            1,
            if under_fluid {
                block_y.wrapping_add(1)
            } else {
                i32::MIN
            },
            block_y,
        );
        rule.try_apply(block_x, block_y, block_z)
    }

    /// `erodedBadlandsExtension(BlockColumn, int, int, int, LevelHeightAccessor)`
    /// — the `#177` surface-build runtime.
    #[allow(dead_code)]
    fn eroded_badlands_extension(
        &self,
        column: &mut dyn BlockColumn<BlockState>,
        block_x: i32,
        block_z: i32,
        height: i32,
        proto_chunk_min_y: i32,
    ) {
        let pillar_noise_scale = 0.2;
        // Java `Math.min(Math.abs(...), ...)` — `min_f64` propagates NaN from
        // either operand like Java's `Math.min` (Rust `f64::min` would drop it).
        let pillar_buffer = mth::min_f64(
            (self
                .badlands_surface_noise
                .get_value(block_x as f64, 0.0, block_z as f64)
                * 8.25)
                .abs(),
            self.badlands_pillar_noise.get_value(
                block_x as f64 * pillar_noise_scale,
                0.0,
                block_z as f64 * pillar_noise_scale,
            ) * 15.0,
        );
        if pillar_buffer <= 0.0 {
            return;
        }
        let floor_noise_sample_resolution = 0.75;
        let floor_amplitude = 1.5;
        let pillar_floor = (self.badlands_pillar_roof_noise.get_value(
            block_x as f64 * floor_noise_sample_resolution,
            0.0,
            block_z as f64 * floor_noise_sample_resolution,
        ) * floor_amplitude)
            .abs();
        let extension_top = 64.0
            + mth::min_f64(
                pillar_buffer * pillar_buffer * 2.5,
                (pillar_floor * 50.0).ceil() + 24.0,
            );
        let start_y = mth::floor_d(extension_top);
        if height > start_y {
            return;
        }
        // `for (y = startY; y >= getMinY(); y--)`: break on `oldState.is(
        // defaultBlock.getBlock())`, return on `oldState.is(Blocks.WATER)`.
        let mut y = start_y;
        while y >= proto_chunk_min_y {
            let old_state = column.get_block(y);
            if old_state.block() == self.default_block.block() {
                break;
            }
            if old_state.block() == Blocks::WATER.id() {
                return;
            }
            y -= 1;
        }
        // `for (y = startY; y >= getMinY() && getBlock(y).isAir(); y--)`.
        let mut y = start_y;
        while y >= proto_chunk_min_y && column.get_block(y).is_air() {
            column.set_block(y, self.default_block);
            y -= 1;
        }
    }

    /// `frozenOceanExtension(int minSurfaceLevel, Biome surfaceBiome,
    /// BlockColumn, MutableBlockPos, int, int, int height)` — the `#177`
    /// surface-build runtime. The `Biome` melt read is the biome-value seam
    /// (`_surface_biome_melt`).
    #[allow(dead_code)]
    fn frozen_ocean_extension(
        &self,
        min_surface_level: i32,
        _surface_biome_melt: bool,
        column: &mut dyn BlockColumn<BlockState>,
        block_x: i32,
        block_z: i32,
        height: i32,
    ) {
        let pillar_scale = 1.28;
        // Java `Math.min(Math.abs(...), ...)` — NaN-propagating `min_f64` (see
        // `eroded_badlands_extension`).
        let iceberg = mth::min_f64(
            (self
                .iceberg_surface_noise
                .get_value(block_x as f64, 0.0, block_z as f64)
                * 8.25)
                .abs(),
            self.iceberg_pillar_noise.get_value(
                block_x as f64 * pillar_scale,
                0.0,
                block_z as f64 * pillar_scale,
            ) * 15.0,
        );
        if iceberg <= 1.8 {
            return;
        }
        let roof_scale = 1.17;
        let roof_amplitude = 1.5;
        let iceberg_roof = (self.iceberg_pillar_roof_noise.get_value(
            block_x as f64 * roof_scale,
            0.0,
            block_z as f64 * roof_scale,
        ) * roof_amplitude)
            .abs();
        let mut top = mth::min_f64(iceberg * iceberg * 1.2, (iceberg_roof * 40.0).ceil() + 14.0);
        if _surface_biome_melt {
            top -= 2.0;
        }
        let extension_bottom;
        if top > 2.0 {
            extension_bottom = self.sea_level as f64 - top - 7.0;
            top += self.sea_level as f64;
        } else {
            top = 0.0;
            extension_bottom = 0.0;
        }
        let extension_top = top;
        let mut random = self.noise_random.at(block_x, 0, block_z);
        let max_snow_depth = 2 + random.next_int_bound(4);
        let min_snow_height = self.sea_level + 18 + random.next_int_bound(10);
        let mut snow_depth = 0;
        let mut y = height.max(extension_top as i32 + 1);
        while y >= min_surface_level {
            let block = column.get_block(y);
            // Java's `||` short-circuits — the water-case `nextDouble` only
            // draws when the air case is false, so the single expression
            // (not two pre-computed `let`s) preserves the RNG order.
            if (block.is_air() && y < extension_top as i32 && random.next_double() > 0.01)
                || (block.block() == Blocks::WATER.id()
                    && y > extension_bottom as i32
                    && y < self.sea_level
                    && extension_bottom != 0.0
                    && random.next_double() > 0.15)
            {
                if snow_depth <= max_snow_depth && y > min_snow_height {
                    column.set_block(y, snow_block_state());
                    snow_depth += 1;
                } else {
                    column.set_block(y, packed_ice_state());
                }
            }
            y -= 1;
        }
    }

    /// `generateBands(RandomSource)` — the 192-entry clay band array. Exact
    /// RNG order (see the Java).
    fn generate_bands(random: &mut impl RandomSource) -> Vec<BlockState> {
        let mut clay_bands = vec![terracotta_state(); 192];
        let mut i = 0usize;
        while i < clay_bands.len() {
            i += random.next_int_bound(5) as usize + 1;
            if i < clay_bands.len() {
                clay_bands[i] = orange_terracotta_state();
            }
            // Java's `for (int i = 0; i < clayBands.length; i++)` — the loop
            // update `i++` runs after every body iteration, so each step
            // advances by `nextInt(5) + 2`. Omitting it shifts the orange band
            // positions and the RNG draw count consumed before `makeBands`.
            i += 1;
        }
        Self::make_bands(random, &mut clay_bands, 1, yellow_terracotta_state());
        Self::make_bands(random, &mut clay_bands, 2, brown_terracotta_state());
        Self::make_bands(random, &mut clay_bands, 1, red_terracotta_state());
        let white_band_count = random.next_int_between_inclusive(9, 15);
        let mut i = 0;
        let mut start = 0usize;
        while i < white_band_count && start < clay_bands.len() {
            clay_bands[start] = white_terracotta_state();
            // Java `start - 1 > 0` — the `start >= 2` form (a `usize` index
            // cannot subtract below 0).
            if start >= 2 && random.next_boolean() {
                clay_bands[start - 1] = light_gray_terracotta_state();
            }
            if start + 1 < clay_bands.len() && random.next_boolean() {
                clay_bands[start + 1] = light_gray_terracotta_state();
            }
            i += 1;
            start += random.next_int_bound(16) as usize + 4;
        }
        clay_bands
    }

    /// `makeBands(RandomSource, BlockState[], int baseWidth, BlockState)`.
    fn make_bands(
        random: &mut impl RandomSource,
        clay_bands: &mut [BlockState],
        base_width: i32,
        state: BlockState,
    ) {
        let band_count = random.next_int_between_inclusive(6, 15);
        for _ in 0..band_count {
            let width = base_width + random.next_int_bound(3);
            let start = random.next_int_bound(clay_bands.len() as i32);
            let mut p = 0;
            while start + p < clay_bands.len() as i32 && p < width {
                clay_bands[(start + p) as usize] = state;
                p += 1;
            }
        }
    }

    /// `getBand(int worldX, int y, int worldZ)` — the wrapping clay band index.
    pub fn get_band(&self, world_x: i32, y: i32, world_z: i32) -> BlockState {
        let offset = java_math_round(
            self.clay_bands_offset_noise
                .get_value(world_x as f64, 0.0, world_z as f64)
                * 4.0,
        );
        let len = self.clay_bands.len() as i32;
        let index = y.wrapping_add(offset).wrapping_add(len).wrapping_rem(len);
        self.clay_bands[index as usize]
    }
}

/// Bind a [`CarvingContext`]'s `topMaterial` seam to this surface system —
/// the `@Deprecated` grass-replacement composition the carver's
/// `WorldCarver.carveBlock` consumes. The closure captures the rule source,
/// noise chunk, biome getter, the shared `WorldGenerationContext`, and the
/// borrowed `RandomState` (the `TopMaterialFn<'a>` lifetime); the shared
/// `SurfaceSystem` Arc comes from the `RandomState`.
///
/// This is the surface-unit half of the carver composition: the production
/// carver loop (`NoiseBasedChunkGenerator::apply_carvers_stub`, RivetTodo
/// #185) is not wired, so the binding is exercised by the tests and is ready
/// for that loop to call once the `#399`/`#185` seams land.
#[allow(dead_code)] // #185 — the carver-loop binding, exercised by the seam tests.
pub(crate) fn bind_carver_top_material<'a>(
    carving_context: &mut CarvingContext<'a>,
    rule_source: &ArcRuleSource,
    noise_chunk: Arc<NoiseChunk>,
    biome_getter: BiomeGetter,
) {
    let random_state = carving_context.random_state();
    let system = random_state.surface_system();
    let worldgen_context = Arc::new(*carving_context.world_context());
    let rule_source = rule_source.clone();
    carving_context.set_top_material(Arc::new(move |pos: &BlockPos, under_fluid: bool| {
        system.top_material(
            &rule_source,
            random_state,
            worldgen_context.clone(),
            noise_chunk.clone(),
            biome_getter.clone(),
            pos,
            under_fluid,
        )
    }));
}

/// The `buildSurface` chunk seam — the `BlockColumn` over the chunk + the
/// worldgen heightmap reads (`#216`/`#185`). The production `ProtoChunk`
/// implements this (issue #179); a test double can too.
pub trait ChunkSurface {
    /// `ProtoChunk.getHeight(WORLD_SURFACE_WG, x, z)` — the `#185` seam.
    fn get_height(&self, x: i32, z: i32) -> i32;
    /// `ProtoChunk.getMinY()`.
    fn get_min_y(&self) -> i32;
    /// `ChunkPos.getMinBlockX()` — the chunk's west edge in block coords.
    fn min_block_x(&self) -> i32;
    /// `ChunkPos.getMinBlockZ()` — the chunk's north edge in block coords.
    fn min_block_z(&self) -> i32;
    /// `LevelHeightAccessor.isInsideBuildHeight(int)` — the build-height guard
    /// the write path applies before `setBlockState`.
    fn is_inside_build_height(&self, y: i32) -> bool;
    /// `ProtoChunk.getBlockState(x, y, z)`.
    fn get_block_state(&self, x: i32, y: i32, z: i32) -> BlockState;
    /// `ProtoChunk.setBlockState(x, y, z, state)` — the worldgen write
    /// (section write + heightmap updates + post-processing mark).
    fn set_block_state(&mut self, x: i32, y: i32, z: i32, state: BlockState);
    /// `ProtoChunk.markPosForPostProcessing(BlockPos)` — the surface
    /// `BlockColumn.setBlock` calls this for every non-empty fluid write
    /// (Java, after `protoChunk.setBlockState`).
    fn mark_pos_for_post_processing(&mut self, x: i32, y: i32, z: i32);
}

/// The `buildSurface` `BlockColumn` over the chunk seam — Java's anonymous
/// `BlockColumn` that closes over `columnPos` (a single mutable
/// `MutableBlockPos` reused per column) and the `protoChunk`. `x`/`z` are the
/// `Cell` stand-ins for the mutated `columnPos` coordinates; the `RefCell`
/// wraps the `&mut dyn ChunkSurface` so the column can `get`/`set` through the
/// same `&mut` seam Java mutates (the `#216` block-write is a `&mut` seam
/// under a shared adapter). The `#177` surface-build column adapter.
#[allow(dead_code)]
struct ChunkColumnAdapter<'a> {
    chunk: RefCell<&'a mut dyn ChunkSurface>,
    x: Cell<i32>,
    z: Cell<i32>,
}

impl BlockColumn<BlockState> for ChunkColumnAdapter<'_> {
    fn get_block(&self, block_y: i32) -> BlockState {
        self.chunk
            .borrow()
            .get_block_state(self.x.get(), block_y, self.z.get())
    }

    fn set_block(&mut self, block_y: i32, state: BlockState) {
        // Java: `heightAccessor.isInsideBuildHeight(blockY)` guards the write,
        // and a non-empty fluid state triggers `markPosForPostProcessing` —
        // both on the `ChunkSurface` seam (issue #179).
        let mut chunk = self.chunk.borrow_mut();
        if chunk.is_inside_build_height(block_y) {
            chunk.set_block_state(self.x.get(), block_y, self.z.get(), state);
            if !state.fluid_empty() {
                chunk.mark_pos_for_post_processing(self.x.get(), block_y, self.z.get());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SurfaceRuleData builders (the `mc.data.worldgen` unit's static surface, kept
// here so `noise_generator_settings.rs` can compose the real preset trees)
// ---------------------------------------------------------------------------

/// `SurfaceRuleData.makeStateRule(Block)` — `SurfaceRules.state(block.defaultBlockState())`.
fn make_state_rule(block: Block) -> ArcRuleSource {
    state(block.default_block_state())
}

/// `SurfaceRuleData.air()` — Java's `makeStateRule(Blocks.AIR)` = a `block`
/// rule carrying the air default state (`SurfaceRules.state`), so it encodes
/// through the `block` `MATERIAL_RULE` arm exactly like Java.
pub fn surface_rule_air() -> ArcRuleSource {
    make_state_rule(Blocks::AIR)
}

/// `SurfaceRuleData.end()` — Java's `return ENDSTONE` =
/// `makeStateRule(Blocks.END_STONE)` (a `block` rule carrying the end-stone
/// default state). The end preset's real surface rule.
pub fn surface_rule_end() -> ArcRuleSource {
    make_state_rule(Blocks::END_STONE)
}

/// `SurfaceRuleData.surfaceNoiseAbove(double)` — the private
/// `noiseCondition2d(Noises.SURFACE, threshold / 8.25, Double.MAX_VALUE)`
/// helper.
fn surface_noise_above(threshold: f64) -> Arc<dyn ConditionSource> {
    noise_condition_2d_range((*noises::SURFACE).clone(), threshold / 8.25, f64::MAX)
}

/// `SurfaceRuleData.nether(HolderGetter<Biome>)` — the lava/nylium/soul-sand/
/// gravel nether surface tree, ported faithfully from `SurfaceRuleData.java`.
///
/// The `HolderGetter<Biome>` resolves each `Biomes.*` key through
/// `HolderSet.direct(getOrThrow)` exactly like Java's `isBiome`. A missing
/// biome key panics with Java's `Missing element <key>` message (`getOrThrow`).
pub(crate) fn surface_rule_nether(biomes_getter: &dyn HolderGetter<BiomeId>) -> ArcRuleSource {
    let above_nether_lava_level = y_block_check(VerticalAnchor::absolute(31), 0);
    let above_nether_lava_surface = y_block_check(VerticalAnchor::absolute(32), 0);
    let nether_band_around_lava_level_bottom = y_start_check(VerticalAnchor::absolute(30), 0);
    let nether_band_around_lava_level_top =
        not_condition(y_start_check(VerticalAnchor::absolute(35), 0));
    let close_to_ceiling = y_block_check(VerticalAnchor::below_top(5), 0);
    let hole = hole_condition();
    let soul_sand_layer = noise_condition_2d((*noises::SOUL_SAND_LAYER).clone(), -0.012);
    let gravel_layer = noise_condition_2d((*noises::GRAVEL_LAYER).clone(), -0.012);
    let patch = noise_condition_2d((*noises::PATCH).clone(), -0.012);
    let netherrack = noise_condition_2d((*noises::NETHERRACK).clone(), 0.54);
    let nether_wart = noise_condition_2d((*noises::NETHER_WART).clone(), 1.17);
    let nether_state_selector = noise_condition_2d((*noises::NETHER_STATE_SELECTOR).clone(), 0.0);
    let gravel_patch = if_true(
        patch.clone(),
        if_true(
            nether_band_around_lava_level_bottom.clone(),
            if_true(
                nether_band_around_lava_level_top.clone(),
                make_state_rule(Blocks::GRAVEL),
            ),
        ),
    );
    sequence(&[
        if_true(
            vertical_gradient(
                "bedrock_floor",
                VerticalAnchor::bottom(),
                VerticalAnchor::above_bottom(5),
            ),
            make_state_rule(Blocks::BEDROCK),
        ),
        if_true(
            not_condition(vertical_gradient(
                "bedrock_roof",
                VerticalAnchor::below_top(5),
                VerticalAnchor::top(),
            )),
            make_state_rule(Blocks::BEDROCK),
        ),
        if_true(close_to_ceiling, make_state_rule(Blocks::NETHERRACK)),
        if_true(
            is_biome(biomes_getter, &[&*biomes::BASALT_DELTAS]),
            sequence(&[
                if_true(under_ceiling(), make_state_rule(Blocks::BASALT)),
                if_true(
                    under_floor(),
                    sequence(&[
                        gravel_patch.clone(),
                        if_true(
                            nether_state_selector.clone(),
                            make_state_rule(Blocks::BASALT),
                        ),
                        make_state_rule(Blocks::BLACKSTONE),
                    ]),
                ),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::SOUL_SAND_VALLEY]),
            sequence(&[
                if_true(
                    under_ceiling(),
                    sequence(&[
                        if_true(
                            nether_state_selector.clone(),
                            make_state_rule(Blocks::SOUL_SAND),
                        ),
                        make_state_rule(Blocks::SOUL_SOIL),
                    ]),
                ),
                if_true(
                    under_floor(),
                    sequence(&[
                        gravel_patch,
                        if_true(
                            nether_state_selector.clone(),
                            make_state_rule(Blocks::SOUL_SAND),
                        ),
                        make_state_rule(Blocks::SOUL_SOIL),
                    ]),
                ),
            ]),
        ),
        if_true(
            on_floor(),
            sequence(&[
                if_true(
                    not_condition(above_nether_lava_surface.clone()),
                    if_true(hole.clone(), make_state_rule(Blocks::LAVA)),
                ),
                if_true(
                    is_biome(biomes_getter, &[&*biomes::WARPED_FOREST]),
                    if_true(
                        not_condition(netherrack.clone()),
                        if_true(
                            above_nether_lava_level.clone(),
                            sequence(&[
                                if_true(
                                    nether_wart.clone(),
                                    make_state_rule(Blocks::WARPED_WART_BLOCK),
                                ),
                                make_state_rule(Blocks::WARPED_NYLIUM),
                            ]),
                        ),
                    ),
                ),
                if_true(
                    is_biome(biomes_getter, &[&*biomes::CRIMSON_FOREST]),
                    if_true(
                        not_condition(netherrack),
                        if_true(
                            above_nether_lava_level.clone(),
                            sequence(&[
                                if_true(nether_wart, make_state_rule(Blocks::NETHER_WART_BLOCK)),
                                make_state_rule(Blocks::CRIMSON_NYLIUM),
                            ]),
                        ),
                    ),
                ),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::NETHER_WASTES]),
            sequence(&[
                if_true(
                    under_floor(),
                    if_true(
                        soul_sand_layer,
                        sequence(&[
                            if_true(
                                not_condition(hole.clone()),
                                if_true(
                                    nether_band_around_lava_level_bottom,
                                    if_true(
                                        nether_band_around_lava_level_top.clone(),
                                        make_state_rule(Blocks::SOUL_SAND),
                                    ),
                                ),
                            ),
                            make_state_rule(Blocks::NETHERRACK),
                        ]),
                    ),
                ),
                if_true(
                    on_floor(),
                    if_true(
                        above_nether_lava_level,
                        if_true(
                            nether_band_around_lava_level_top,
                            if_true(
                                gravel_layer,
                                sequence(&[
                                    if_true(
                                        above_nether_lava_surface,
                                        make_state_rule(Blocks::GRAVEL),
                                    ),
                                    if_true(not_condition(hole), make_state_rule(Blocks::GRAVEL)),
                                ]),
                            ),
                        ),
                    ),
                ),
            ]),
        ),
        make_state_rule(Blocks::NETHERRACK),
    ])
}

/// `SurfaceRuleData.overworld(HolderGetter<Biome>)` — Java's
/// `return overworldLike(biomes, true, false, true)`.
pub(crate) fn surface_rule_overworld(biomes_getter: &dyn HolderGetter<BiomeId>) -> ArcRuleSource {
    surface_rule_overworld_like(biomes_getter, true, false, true)
}

/// `SurfaceRuleData.overworldLike(HolderGetter<Biome>, boolean
/// doPreliminarySurfaceCheck, boolean bedrockRoof, boolean bedrockFloor)` — the
/// overworld tree parametrized for the `caves`/`floating_islands` presets,
/// ported faithfully from `SurfaceRuleData.java` (the local ordering and the
/// final `ImmutableList` builder order preserved exactly).
pub(crate) fn surface_rule_overworld_like(
    biomes_getter: &dyn HolderGetter<BiomeId>,
    do_preliminary_surface_check: bool,
    bedrock_roof: bool,
    bedrock_floor: bool,
) -> ArcRuleSource {
    let wooded_badlands_top = y_block_check(VerticalAnchor::absolute(97), 2);
    let badlands_top = y_block_check(VerticalAnchor::absolute(256), 0);
    let badlands_height_condition = y_start_check(VerticalAnchor::absolute(63), -1);
    let badlands_mid = y_start_check(VerticalAnchor::absolute(74), 1);
    let mangrove_swamp_puddle_level = y_block_check(VerticalAnchor::absolute(60), 0);
    let swamp_puddle_level = y_block_check(VerticalAnchor::absolute(62), 0);
    let above_overworld_sea_level = y_block_check(VerticalAnchor::absolute(63), 0);
    let not_underwater = water_block_check(-1, 0);
    let above_water = water_block_check(0, 0);
    let not_under_deep_water = water_start_check(-6, -1);
    let hole = hole_condition();
    let frozen_ocean = is_biome(
        biomes_getter,
        &[&*biomes::FROZEN_OCEAN, &*biomes::DEEP_FROZEN_OCEAN],
    );
    let steep = steep_condition();
    let grass_or_dirt_if_underwater = sequence(&[
        if_true(above_water.clone(), make_state_rule(Blocks::GRASS_BLOCK)),
        make_state_rule(Blocks::DIRT),
    ]);
    let sand_or_sandstone_if_ceiling = sequence(&[
        if_true(on_ceiling(), make_state_rule(Blocks::SANDSTONE)),
        make_state_rule(Blocks::SAND),
    ]);
    let gravel_or_stone_if_ceiling = sequence(&[
        if_true(on_ceiling(), make_state_rule(Blocks::STONE)),
        make_state_rule(Blocks::GRAVEL),
    ]);
    let biomes_with_sand_and_sandstone = is_biome(
        biomes_getter,
        &[&*biomes::WARM_OCEAN, &*biomes::BEACH, &*biomes::SNOWY_BEACH],
    );
    let biomes_with_sand_and_very_deep_sandstone = is_biome(biomes_getter, &[&*biomes::DESERT]);
    let sulfur_cave_bands = sequence(&[
        if_true(
            noise_condition_3d_range(
                (*noises::SULFUR_CAVE_GRADIENT).clone(),
                -0.4_f32 as f64,
                -0.1_f32 as f64,
            ),
            make_state_rule(Blocks::CINNABAR),
        ),
        if_true(
            noise_condition_3d_range((*noises::SULFUR_CAVE_GRADIENT).clone(), 0.0, 0.4_f32 as f64),
            make_state_rule(Blocks::SULFUR),
        ),
        if_true(
            noise_condition_3d((*noises::SULFUR_CAVE_GRADIENT).clone(), 0.4_f32 as f64),
            make_state_rule(Blocks::CINNABAR),
        ),
    ]);
    let common_surface_and_under_rules = sequence(&[
        if_true(
            is_biome(biomes_getter, &[&*biomes::STONY_PEAKS]),
            sequence(&[
                if_true(
                    noise_condition_2d_range((*noises::CALCITE).clone(), -0.0125, 0.0125),
                    make_state_rule(Blocks::CALCITE),
                ),
                make_state_rule(Blocks::STONE),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::STONY_SHORE]),
            sequence(&[
                if_true(
                    noise_condition_2d_range((*noises::GRAVEL).clone(), -0.05, 0.05),
                    gravel_or_stone_if_ceiling.clone(),
                ),
                make_state_rule(Blocks::STONE),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::WINDSWEPT_HILLS]),
            if_true(surface_noise_above(1.0), make_state_rule(Blocks::STONE)),
        ),
        if_true(
            biomes_with_sand_and_sandstone.clone(),
            sand_or_sandstone_if_ceiling.clone(),
        ),
        if_true(
            biomes_with_sand_and_very_deep_sandstone.clone(),
            sand_or_sandstone_if_ceiling.clone(),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::DRIPSTONE_CAVES]),
            make_state_rule(Blocks::STONE),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::SULFUR_CAVES]),
            sequence(&[sulfur_cave_bands.clone(), make_state_rule(Blocks::STONE)]),
        ),
    ]);
    let powder_snow_under_rule = if_true(
        noise_condition_2d_range((*noises::POWDER_SNOW).clone(), 0.45, 0.58),
        if_true(above_water.clone(), make_state_rule(Blocks::POWDER_SNOW)),
    );
    let powder_snow_surface_rule = if_true(
        noise_condition_2d_range((*noises::POWDER_SNOW).clone(), 0.35, 0.6),
        if_true(above_water.clone(), make_state_rule(Blocks::POWDER_SNOW)),
    );
    let biome_under_surface_rule = sequence(&[
        if_true(
            is_biome(biomes_getter, &[&*biomes::FROZEN_PEAKS]),
            sequence(&[
                if_true(steep.clone(), make_state_rule(Blocks::PACKED_ICE)),
                if_true(
                    noise_condition_2d_range((*noises::PACKED_ICE).clone(), -0.5, 0.2),
                    make_state_rule(Blocks::PACKED_ICE),
                ),
                if_true(
                    noise_condition_2d_range((*noises::ICE).clone(), -0.0625, 0.025),
                    make_state_rule(Blocks::ICE),
                ),
                if_true(above_water.clone(), make_state_rule(Blocks::SNOW_BLOCK)),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::SNOWY_SLOPES]),
            sequence(&[
                if_true(steep.clone(), make_state_rule(Blocks::STONE)),
                powder_snow_under_rule.clone(),
                if_true(above_water.clone(), make_state_rule(Blocks::SNOW_BLOCK)),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::JAGGED_PEAKS]),
            make_state_rule(Blocks::STONE),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::GROVE]),
            sequence(&[powder_snow_under_rule, make_state_rule(Blocks::DIRT)]),
        ),
        common_surface_and_under_rules.clone(),
        if_true(
            is_biome(biomes_getter, &[&*biomes::WINDSWEPT_SAVANNA]),
            if_true(surface_noise_above(1.75), make_state_rule(Blocks::STONE)),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::WINDSWEPT_GRAVELLY_HILLS]),
            sequence(&[
                if_true(surface_noise_above(2.0), gravel_or_stone_if_ceiling.clone()),
                if_true(surface_noise_above(1.0), make_state_rule(Blocks::STONE)),
                if_true(surface_noise_above(-1.0), make_state_rule(Blocks::DIRT)),
                gravel_or_stone_if_ceiling.clone(),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::MANGROVE_SWAMP]),
            make_state_rule(Blocks::MUD),
        ),
        make_state_rule(Blocks::DIRT),
    ]);
    let biome_surface_rule = sequence(&[
        if_true(
            is_biome(biomes_getter, &[&*biomes::FROZEN_PEAKS]),
            sequence(&[
                if_true(steep.clone(), make_state_rule(Blocks::PACKED_ICE)),
                if_true(
                    noise_condition_2d_range((*noises::PACKED_ICE).clone(), 0.0, 0.2),
                    make_state_rule(Blocks::PACKED_ICE),
                ),
                if_true(
                    noise_condition_2d_range((*noises::ICE).clone(), 0.0, 0.025),
                    make_state_rule(Blocks::ICE),
                ),
                if_true(above_water.clone(), make_state_rule(Blocks::SNOW_BLOCK)),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::SNOWY_SLOPES]),
            sequence(&[
                if_true(steep.clone(), make_state_rule(Blocks::STONE)),
                powder_snow_surface_rule.clone(),
                if_true(above_water.clone(), make_state_rule(Blocks::SNOW_BLOCK)),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::JAGGED_PEAKS]),
            sequence(&[
                if_true(steep.clone(), make_state_rule(Blocks::STONE)),
                if_true(above_water.clone(), make_state_rule(Blocks::SNOW_BLOCK)),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::GROVE]),
            sequence(&[
                powder_snow_surface_rule,
                if_true(above_water.clone(), make_state_rule(Blocks::SNOW_BLOCK)),
            ]),
        ),
        common_surface_and_under_rules.clone(),
        if_true(
            is_biome(biomes_getter, &[&*biomes::WINDSWEPT_SAVANNA]),
            sequence(&[
                if_true(surface_noise_above(1.75), make_state_rule(Blocks::STONE)),
                if_true(
                    surface_noise_above(-0.5),
                    make_state_rule(Blocks::COARSE_DIRT),
                ),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::WINDSWEPT_GRAVELLY_HILLS]),
            sequence(&[
                if_true(surface_noise_above(2.0), gravel_or_stone_if_ceiling.clone()),
                if_true(surface_noise_above(1.0), make_state_rule(Blocks::STONE)),
                if_true(
                    surface_noise_above(-1.0),
                    grass_or_dirt_if_underwater.clone(),
                ),
                gravel_or_stone_if_ceiling.clone(),
            ]),
        ),
        if_true(
            is_biome(
                biomes_getter,
                &[
                    &*biomes::OLD_GROWTH_PINE_TAIGA,
                    &*biomes::OLD_GROWTH_SPRUCE_TAIGA,
                ],
            ),
            sequence(&[
                if_true(
                    surface_noise_above(1.75),
                    make_state_rule(Blocks::COARSE_DIRT),
                ),
                if_true(surface_noise_above(-0.95), make_state_rule(Blocks::PODZOL)),
            ]),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::ICE_SPIKES]),
            if_true(above_water.clone(), make_state_rule(Blocks::SNOW_BLOCK)),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::MANGROVE_SWAMP]),
            make_state_rule(Blocks::MUD),
        ),
        if_true(
            is_biome(biomes_getter, &[&*biomes::MUSHROOM_FIELDS]),
            make_state_rule(Blocks::MYCELIUM),
        ),
        grass_or_dirt_if_underwater.clone(),
    ]);
    let clay_band_1 = noise_condition_2d_range((*noises::SURFACE).clone(), -0.909, -0.5454);
    let clay_band_2 = noise_condition_2d_range((*noises::SURFACE).clone(), -0.1818, 0.1818);
    let clay_band_3 = noise_condition_2d_range((*noises::SURFACE).clone(), 0.5454, 0.909);
    let main_rule_close_to_surface = sequence(&[
        if_true(
            on_floor(),
            sequence(&[
                if_true(
                    is_biome(biomes_getter, &[&*biomes::WOODED_BADLANDS]),
                    if_true(
                        wooded_badlands_top,
                        sequence(&[
                            if_true(clay_band_1.clone(), make_state_rule(Blocks::COARSE_DIRT)),
                            if_true(clay_band_2.clone(), make_state_rule(Blocks::COARSE_DIRT)),
                            if_true(clay_band_3.clone(), make_state_rule(Blocks::COARSE_DIRT)),
                            grass_or_dirt_if_underwater.clone(),
                        ]),
                    ),
                ),
                if_true(
                    is_biome(biomes_getter, &[&*biomes::SWAMP]),
                    if_true(
                        swamp_puddle_level,
                        if_true(
                            not_condition(above_overworld_sea_level.clone()),
                            if_true(
                                noise_condition_2d((*noises::SWAMP).clone(), 0.0),
                                make_state_rule(Blocks::WATER),
                            ),
                        ),
                    ),
                ),
                if_true(
                    is_biome(biomes_getter, &[&*biomes::MANGROVE_SWAMP]),
                    if_true(
                        mangrove_swamp_puddle_level,
                        if_true(
                            not_condition(above_overworld_sea_level.clone()),
                            if_true(
                                noise_condition_2d((*noises::SWAMP).clone(), 0.0),
                                make_state_rule(Blocks::WATER),
                            ),
                        ),
                    ),
                ),
            ]),
        ),
        if_true(
            is_biome(
                biomes_getter,
                &[
                    &*biomes::BADLANDS,
                    &*biomes::ERODED_BADLANDS,
                    &*biomes::WOODED_BADLANDS,
                ],
            ),
            sequence(&[
                if_true(
                    on_floor(),
                    sequence(&[
                        if_true(badlands_top, make_state_rule(Blocks::ORANGE_TERRACOTTA)),
                        if_true(
                            badlands_mid.clone(),
                            sequence(&[
                                if_true(clay_band_1, make_state_rule(Blocks::TERRACOTTA)),
                                if_true(clay_band_2, make_state_rule(Blocks::TERRACOTTA)),
                                if_true(clay_band_3, make_state_rule(Blocks::TERRACOTTA)),
                                bandlands(),
                            ]),
                        ),
                        if_true(
                            not_underwater.clone(),
                            sequence(&[
                                if_true(on_ceiling(), make_state_rule(Blocks::RED_SANDSTONE)),
                                make_state_rule(Blocks::RED_SAND),
                            ]),
                        ),
                        if_true(
                            not_condition(hole.clone()),
                            make_state_rule(Blocks::ORANGE_TERRACOTTA),
                        ),
                        if_true(
                            not_under_deep_water.clone(),
                            make_state_rule(Blocks::WHITE_TERRACOTTA),
                        ),
                        gravel_or_stone_if_ceiling.clone(),
                    ]),
                ),
                if_true(
                    badlands_height_condition,
                    sequence(&[
                        if_true(
                            above_overworld_sea_level.clone(),
                            if_true(
                                not_condition(badlands_mid),
                                make_state_rule(Blocks::ORANGE_TERRACOTTA),
                            ),
                        ),
                        bandlands(),
                    ]),
                ),
                if_true(
                    under_floor(),
                    if_true(
                        not_under_deep_water.clone(),
                        make_state_rule(Blocks::WHITE_TERRACOTTA),
                    ),
                ),
            ]),
        ),
        if_true(
            on_floor(),
            if_true(
                not_underwater.clone(),
                sequence(&[
                    if_true(
                        frozen_ocean.clone(),
                        if_true(
                            hole.clone(),
                            sequence(&[
                                if_true(above_water.clone(), make_state_rule(Blocks::AIR)),
                                if_true(temperature_condition(), make_state_rule(Blocks::ICE)),
                                make_state_rule(Blocks::WATER),
                            ]),
                        ),
                    ),
                    biome_surface_rule,
                ]),
            ),
        ),
        if_true(
            not_under_deep_water.clone(),
            sequence(&[
                if_true(
                    on_floor(),
                    if_true(frozen_ocean, if_true(hole, make_state_rule(Blocks::WATER))),
                ),
                if_true(under_floor(), biome_under_surface_rule),
                if_true(
                    biomes_with_sand_and_sandstone,
                    if_true(deep_under_floor(), make_state_rule(Blocks::SANDSTONE)),
                ),
                if_true(
                    biomes_with_sand_and_very_deep_sandstone,
                    if_true(very_deep_under_floor(), make_state_rule(Blocks::SANDSTONE)),
                ),
            ]),
        ),
        if_true(
            on_floor(),
            sequence(&[
                if_true(
                    is_biome(
                        biomes_getter,
                        &[&*biomes::FROZEN_PEAKS, &*biomes::JAGGED_PEAKS],
                    ),
                    make_state_rule(Blocks::STONE),
                ),
                if_true(
                    is_biome(
                        biomes_getter,
                        &[
                            &*biomes::WARM_OCEAN,
                            &*biomes::LUKEWARM_OCEAN,
                            &*biomes::DEEP_LUKEWARM_OCEAN,
                        ],
                    ),
                    sand_or_sandstone_if_ceiling,
                ),
                gravel_or_stone_if_ceiling,
            ]),
        ),
    ]);
    let rule_above_preliminary_surface = if_true(
        above_preliminary_surface_condition(),
        main_rule_close_to_surface.clone(),
    );
    let mut builder: Vec<ArcRuleSource> = Vec::new();
    if bedrock_roof {
        builder.push(if_true(
            not_condition(vertical_gradient(
                "bedrock_roof",
                VerticalAnchor::below_top(5),
                VerticalAnchor::top(),
            )),
            make_state_rule(Blocks::BEDROCK),
        ));
    }
    if bedrock_floor {
        builder.push(if_true(
            vertical_gradient(
                "bedrock_floor",
                VerticalAnchor::bottom(),
                VerticalAnchor::above_bottom(5),
            ),
            make_state_rule(Blocks::BEDROCK),
        ));
    }
    if do_preliminary_surface_check {
        builder.push(rule_above_preliminary_surface);
    } else {
        builder.push(main_rule_close_to_surface);
    }
    builder.push(if_true(
        is_biome(biomes_getter, &[&*biomes::SULFUR_CAVES]),
        sulfur_cave_bands,
    ));
    builder.push(if_true(
        vertical_gradient(
            "deepslate",
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(8),
        ),
        make_state_rule(Blocks::DEEPSLATE),
    ));
    sequence(&builder)
}

// ---------------------------------------------------------------------------
// Public builders (the Java static surface-rules helpers)
// ---------------------------------------------------------------------------

/// `SurfaceRules.ON_FLOOR`.
pub fn on_floor() -> Arc<dyn ConditionSource> {
    stone_depth_check(0, false, CaveSurface::Floor)
}

/// `SurfaceRules.UNDER_FLOOR`.
pub fn under_floor() -> Arc<dyn ConditionSource> {
    stone_depth_check(0, true, CaveSurface::Floor)
}

/// `SurfaceRules.DEEP_UNDER_FLOOR`.
pub fn deep_under_floor() -> Arc<dyn ConditionSource> {
    stone_depth_check_with_range(0, true, 6, CaveSurface::Floor)
}

/// `SurfaceRules.VERY_DEEP_UNDER_FLOOR`.
pub fn very_deep_under_floor() -> Arc<dyn ConditionSource> {
    stone_depth_check_with_range(0, true, 30, CaveSurface::Floor)
}

/// `SurfaceRules.ON_CEILING`.
pub fn on_ceiling() -> Arc<dyn ConditionSource> {
    stone_depth_check(0, false, CaveSurface::Ceiling)
}

/// `SurfaceRules.UNDER_CEILING`.
pub fn under_ceiling() -> Arc<dyn ConditionSource> {
    stone_depth_check(0, true, CaveSurface::Ceiling)
}

/// `SurfaceRules.stoneDepthCheck(int, boolean, CaveSurface)`.
pub fn stone_depth_check(
    offset: i32,
    add_surface_depth: bool,
    surface_type: CaveSurface,
) -> Arc<dyn ConditionSource> {
    Arc::new(StoneDepthCheck::new(
        offset,
        add_surface_depth,
        0,
        surface_type,
    ))
}

/// `SurfaceRules.stoneDepthCheck(int, boolean, int, CaveSurface)`.
pub fn stone_depth_check_with_range(
    offset: i32,
    add_surface_depth: bool,
    secondary_depth_range: i32,
    surface_type: CaveSurface,
) -> Arc<dyn ConditionSource> {
    Arc::new(StoneDepthCheck::new(
        offset,
        add_surface_depth,
        secondary_depth_range,
        surface_type,
    ))
}

/// `SurfaceRules.not(ConditionSource)`.
pub fn not_condition(target: Arc<dyn ConditionSource>) -> Arc<dyn ConditionSource> {
    Arc::new(NotConditionSource::new(target))
}

/// `SurfaceRules.yBlockCheck(VerticalAnchor, int)`.
pub fn y_block_check(
    anchor: VerticalAnchor,
    surface_depth_multiplier: i32,
) -> Arc<dyn ConditionSource> {
    Arc::new(YConditionSource::new(
        anchor,
        surface_depth_multiplier,
        false,
    ))
}

/// `SurfaceRules.yStartCheck(VerticalAnchor, int)`.
pub fn y_start_check(
    anchor: VerticalAnchor,
    surface_depth_multiplier: i32,
) -> Arc<dyn ConditionSource> {
    Arc::new(YConditionSource::new(
        anchor,
        surface_depth_multiplier,
        true,
    ))
}

/// `SurfaceRules.waterBlockCheck(int, int)`.
pub fn water_block_check(offset: i32, surface_depth_multiplier: i32) -> Arc<dyn ConditionSource> {
    Arc::new(WaterConditionSource::new(
        offset,
        surface_depth_multiplier,
        false,
    ))
}

/// `SurfaceRules.waterStartCheck(int, int)`.
pub fn water_start_check(offset: i32, surface_depth_multiplier: i32) -> Arc<dyn ConditionSource> {
    Arc::new(WaterConditionSource::new(
        offset,
        surface_depth_multiplier,
        true,
    ))
}

/// `SurfaceRules.isBiome(HolderGetter<Biome>, ResourceKey<Biome>...)` —
/// Java's `@SafeVarargs` varargs is a fixed-size slice here: the port builds
/// `HolderSet.direct(getter.getOrThrow(key))` over the keys in argument order
/// exactly like Java (`HolderSet.direct(biomes::getOrThrow, target)`), so a
/// missing biome key panics with Java's `Missing element <key>` message and a
/// single key encodes as the compact bare-identifier holder set.
pub fn is_biome(
    biomes_getter: &dyn HolderGetter<BiomeId>,
    keys: &[&ResourceKey<BiomeId>],
) -> Arc<dyn ConditionSource> {
    let holders: Vec<Holder<BiomeId>> = keys
        .iter()
        .map(|key| biomes_getter.get_or_throw(key))
        .collect();
    Arc::new(BiomeConditionSource::new(HolderSet::direct(holders)))
}

/// `SurfaceRules.noiseCondition2d(ResourceKey, double)`.
pub fn noise_condition_2d(
    noise: ResourceKey<NoiseParameters>,
    min_range: f64,
) -> Arc<dyn ConditionSource> {
    noise_condition_2d_range(noise, min_range, f64::MAX)
}

/// `SurfaceRules.noiseCondition2d(ResourceKey, double, double)`.
pub fn noise_condition_2d_range(
    noise: ResourceKey<NoiseParameters>,
    min_range: f64,
    max_range: f64,
) -> Arc<dyn ConditionSource> {
    Arc::new(NoiseThresholdConditionSource::new(
        noise, min_range, max_range, false,
    ))
}

/// `SurfaceRules.noiseCondition3d(ResourceKey, double)`.
pub fn noise_condition_3d(
    noise: ResourceKey<NoiseParameters>,
    min_range: f64,
) -> Arc<dyn ConditionSource> {
    noise_condition_3d_range(noise, min_range, f64::MAX)
}

/// `SurfaceRules.noiseCondition3d(ResourceKey, double, double)`.
pub fn noise_condition_3d_range(
    noise: ResourceKey<NoiseParameters>,
    min_range: f64,
    max_range: f64,
) -> Arc<dyn ConditionSource> {
    Arc::new(NoiseThresholdConditionSource::new(
        noise, min_range, max_range, true,
    ))
}

/// `SurfaceRules.verticalGradient(String, VerticalAnchor, VerticalAnchor)`.
pub fn vertical_gradient(
    random_name: &str,
    true_at_and_below: VerticalAnchor,
    false_at_and_above: VerticalAnchor,
) -> Arc<dyn ConditionSource> {
    Arc::new(VerticalGradientConditionSource::new(
        Identifier::parse(random_name),
        true_at_and_below,
        false_at_and_above,
    ))
}

/// `SurfaceRules.steep()`.
pub fn steep_condition() -> Arc<dyn ConditionSource> {
    Arc::new(Steep)
}

/// `SurfaceRules.hole()`.
pub fn hole_condition() -> Arc<dyn ConditionSource> {
    Arc::new(Hole)
}

/// `SurfaceRules.abovePreliminarySurface()`.
pub fn above_preliminary_surface_condition() -> Arc<dyn ConditionSource> {
    Arc::new(AbovePreliminarySurface)
}

/// `SurfaceRules.temperature()`.
pub fn temperature_condition() -> Arc<dyn ConditionSource> {
    Arc::new(Temperature)
}

/// `SurfaceRules.ifTrue(ConditionSource, RuleSource)`.
pub fn if_true(condition: Arc<dyn ConditionSource>, next: ArcRuleSource) -> ArcRuleSource {
    Arc::new(TestRuleSource::new(condition, next))
}

/// `SurfaceRules.sequence(RuleSource...)` — `IllegalArgumentException` for an
/// empty list.
pub fn sequence(rules: &[ArcRuleSource]) -> ArcRuleSource {
    if rules.is_empty() {
        panic!("Need at least 1 rule for a sequence");
    }
    Arc::new(SequenceRuleSource::new(rules.to_vec()))
}

/// `SurfaceRules.state(BlockState)`.
pub fn state(block_state: BlockState) -> ArcRuleSource {
    Arc::new(BlockRuleSource::new(block_state))
}

/// `SurfaceRules.bandlands()`.
pub fn bandlands() -> ArcRuleSource {
    Arc::new(Bandlands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use crate::levelgen::heightmap::Types;
    use crate::levelgen::noisegen::NoiseGeneratorSettings;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A `ChunkGenerator` double exposing a fixed worldgen window.
    struct TestGenerator {
        min_y: i32,
        depth: i32,
    }
    impl ChunkGenerator for TestGenerator {
        fn get_min_y(&self) -> i32 {
            self.min_y
        }
        fn get_gen_depth(&self) -> i32 {
            self.depth
        }
    }

    /// A `WorldGenLevel` double over a fixed window.
    struct TestLevel(SimpleLevelHeightAccessor);
    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.0.get_height()
        }
        fn get_min_y(&self) -> i32 {
            self.0.get_min_y()
        }
    }
    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }
        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    fn worldgen_context(min_y: i32, height: i32, gen_depth: i32) -> WorldGenerationContext {
        let level = TestLevel(create(min_y, height));
        let generator = TestGenerator {
            min_y,
            depth: gen_depth,
        };
        WorldGenerationContext::new(&generator, &level)
    }

    /// A deterministic zero-output `NormalNoise` (`get_value` returns 0.0) —
    /// the surface-system stub noise.
    fn zero_noise() -> NormalNoise {
        NormalNoise::create(
            &mut LegacyRandomSource::new(0),
            NoiseParameters {
                first_octave: 0,
                amplitudes: vec![0.0],
            },
        )
    }

    /// A `SurfaceSystem` wired through `new` with the zero-output stub noise.
    fn stub_surface_system() -> SurfaceSystem {
        let get_noise = |_: &ResourceKey<NoiseParameters>| zero_noise();
        SurfaceSystem::new(
            &get_noise,
            Blocks::STONE.default_block_state(),
            63,
            AlgorithmPositionalRandomFactory::Legacy(
                rivet_util::random::LegacyPositionalRandomFactory::new(0),
            ),
        )
    }

    /// A `SurfaceSystem` literal with a caller-chosen band array (the private
    /// fields are module-visible to the tests).
    fn system_with_bands(clay_bands: Vec<BlockState>) -> SurfaceSystem {
        SurfaceSystem {
            default_block: Blocks::STONE.default_block_state(),
            sea_level: 63,
            clay_bands,
            clay_bands_offset_noise: zero_noise(),
            badlands_pillar_noise: zero_noise(),
            badlands_pillar_roof_noise: zero_noise(),
            badlands_surface_noise: zero_noise(),
            iceberg_pillar_noise: zero_noise(),
            iceberg_pillar_roof_noise: zero_noise(),
            iceberg_surface_noise: zero_noise(),
            noise_random: AlgorithmPositionalRandomFactory::Legacy(
                rivet_util::random::LegacyPositionalRandomFactory::new(0),
            ),
            surface_noise: zero_noise(),
            surface_secondary_noise: zero_noise(),
        }
    }

    /// A `SurfaceContext` stub: a zero-noise system, a plains-biome getter,
    /// and an overworld window.
    fn stub_surface_context() -> Arc<SurfaceContext> {
        Arc::new(SurfaceContext {
            system: Arc::new(stub_surface_system()),
            cells: SharedCells::new(),
            biome_getter: Arc::new(|_: &BlockPos| Holder::direct(BiomeId::from_id(0))),
            worldgen_context: Arc::new(worldgen_context(-64, 384, 384)),
            world_surface_heights: None,
        })
    }

    /// A biome registry with the 33 `SurfaceRuleData`-referenced keys (the
    /// single source of truth in `biome::biomes::SURFACE_RULE_BIOMES`) under
    /// `Registries.BIOME`. The codec tests encode the real trees by KEY, so the
    /// registered `BiomeId` VALUES are irrelevant here (encode never reads
    /// them) and the enumerate-index fill is a codec-test-only convenience —
    /// the production `build_biome_registry` registers the real generated ids
    /// (see `worldgen_bootstraps`).
    fn biome_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*rivet_registry::registries::BIOME);
        for (i, name) in biomes::SURFACE_RULE_BIOMES.iter().enumerate() {
            builder.register(
                &ResourceKey::create(
                    &*rivet_registry::registries::BIOME,
                    Identifier::parse(&format!("minecraft:{name}")),
                ),
                Arc::new(BiomeId::from_id(i as u16)),
                RegistrationInfo::BUILT_IN,
            );
        }
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/biome")),
            Box::new(registry) as rivet_registry::root::AnyBox,
        )])
    }

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, biome_access())
    }

    #[test]
    fn condition_codec_round_trips_stone_depth() {
        let ops = ops();
        let source: Arc<dyn ConditionSource> = stone_depth_check(0, false, CaveSurface::Floor);
        let codec = condition_source_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &source)
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"type": "minecraft:stone_depth", "offset": 0, "add_surface_depth": false, "secondary_depth_range": 0, "surface_type": "floor"})
        );
        let decoded = codec.parse(&ops, &encoded).get_or_throw("decode").clone();
        assert!(decoded.as_any().is::<StoneDepthCheck>());
    }

    #[test]
    fn condition_codec_rejects_unknown_type() {
        let ops = ops();
        let codec = condition_source_codec::<TestOps>();
        let unknown = ops.create_string("minecraft:no_such_condition".to_string());
        let decoded = codec.decode(&ops, &unknown);
        assert!(decoded.result().is_none());
    }

    #[test]
    fn rule_codec_round_trips_block_rule() {
        let ops = ops();
        let source: ArcRuleSource = state(Blocks::AIR.default_block_state());
        let codec = rule_source_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &source)
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"type": "minecraft:block", "result_state": {"Name": "minecraft:air"}})
        );
        let decoded = codec.parse(&ops, &encoded).get_or_throw("decode").clone();
        assert!(decoded.as_any().is::<BlockRuleSource>());
    }

    #[test]
    fn rule_codec_round_trips_sequence_of_conditions() {
        let ops = ops();
        let source: ArcRuleSource = sequence(&[if_true(
            stone_depth_check(0, false, CaveSurface::Floor),
            state(Blocks::STONE.default_block_state()),
        )]);
        let codec = rule_source_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &source)
            .get_or_throw("encode")
            .clone();
        let decoded = codec.parse(&ops, &encoded).get_or_throw("decode").clone();
        assert!(decoded.as_any().is::<SequenceRuleSource>());
    }

    /// Java's `SteepMaterialCondition` probes the +/- 1 neighbors (clamped to
    /// the chunk edge). A corner column reads `(0, 1, 0, 1)`; a mid-cell reads
    /// `(z-1, z+1, x-1, x+1)`. This pins the `wrapping_sub`/`wrapping_add`
    /// arithmetic (which `max`/`min` alone would silently get wrong).
    #[test]
    fn steep_neighbor_probes_clamp_and_offset() {
        // Corner: (0, 0) -> north/south clamp to 0/1, west/east clamp to 0/1.
        assert_eq!(steep_neighbor_probes(0, 0), (0, 1, 0, 1));
        // Corner: (15, 15) -> south clamps to 15, east clamps to 15.
        assert_eq!(steep_neighbor_probes(15, 15), (14, 15, 14, 15));
        // Mid-cell: (7, 3) -> (2, 4, 6, 8).
        assert_eq!(steep_neighbor_probes(7, 3), (2, 4, 6, 8));
    }

    /// The `#185` heightmap seam firing path: `SteepMaterialCondition` reads
    /// the `WORLD_SURFACE_WG` snapshot `build_surface` captured at its start
    /// (never the `&mut` chunk), so a column whose south neighbor is >= 4 blocks
    /// higher must report steep exactly as Java's live `getHeight` reads would.
    #[test]
    fn steep_condition_fires_on_the_primed_heightmap_snapshot() {
        // A helper building a steep condition over a given height snapshot.
        fn condition_for(heights: Option<Arc<[i32; 256]>>) -> SteepMaterialCondition {
            let sc = Arc::new(SurfaceContext {
                system: Arc::new(stub_surface_system()),
                cells: SharedCells::new(),
                biome_getter: Arc::new(|_: &BlockPos| Holder::direct(BiomeId::from_id(0))),
                worldgen_context: Arc::new(worldgen_context(-64, 384, 384)),
                world_surface_heights: heights,
            });
            sc.update_xz(0, 1);
            SteepMaterialCondition {
                cache: LazyCache::new(sc.cells.last_update_xz()),
                surface_context: sc,
            }
        }
        // A snapshot where the chunk-local column (0, 1) probes north height 10
        // (z=0) and south height 14 (z=2): 14 >= 10 + 4 -> steep.
        let mut heights = [0i32; 256];
        heights[0] = 10; // (0, 0) — the north probe at chunk-block z 0.
        heights[32] = 14; // (0, 2) — the south probe at chunk-block z 2.
        assert!(condition_for(Some(Arc::new(heights))).test());
        // Flat snapshot: north == south == 10 -> not steep.
        let flat = [10i32; 256];
        assert!(!condition_for(Some(Arc::new(flat))).test());
        // No snapshot (the single-column carver probe): cannot fire.
        assert!(!condition_for(None).test());
    }

    /// The `#185` eroded-badlands heightmap divergence, pinned honestly: Java's
    /// `buildSurface` walks the columns x-outer/z-inner and runs
    /// `erodedBadlandsExtension` per column (writing `defaultBlock` into `AIR`
    /// at/below `startY`, raising the live `WORLD_SURFACE_WG` height) before
    /// that column's steep probes. The Rust port reads the start-of-
    /// `build_surface` snapshot, which predates every column's extension. On an
    /// `ERODED_BADLANDS` column at `x == 0`, Java's own-column raise becomes
    /// visible to the steep check: the west probe clamps `x - 1` to 0 (Java's
    /// `Math.max(x - 1, 0)`), reading the *current* column's post-extension
    /// height, while the east probe reads the not-yet-processed `x + 1` column.
    /// Java then fires `west >= east + 4`; the snapshot reads the pre-extension
    /// height and does not. This test pins the snapshot-before-extension
    /// ordering through that reachable path, plus the positive control that the
    /// raised column does fire.
    #[test]
    fn steep_snapshot_predates_the_eroded_badlands_extension() {
        // The snapshot captured at `build_surface` start: every column is at
        // its pre-extension height 10 — including the current column (0, 1),
        // an ERODED_BADLANDS column whose extension Java would run just before
        // its probes. At x = 0 the west probe clamps to (0, 1) itself (index
        // 0 + 1*16), the east probe reads (1, 1) (index 1 + 1*16, processed
        // later at x = 1), so 10 >= 10 + 4 is false and steep does not fire.
        let heights = [10i32; 256];
        let sc = Arc::new(SurfaceContext {
            system: Arc::new(stub_surface_system()),
            cells: SharedCells::new(),
            biome_getter: Arc::new(|_: &BlockPos| Holder::direct(BiomeId::from_id(0))),
            worldgen_context: Arc::new(worldgen_context(-64, 384, 384)),
            world_surface_heights: Some(Arc::new(heights)),
        });
        sc.update_xz(0, 1);
        let steep = SteepMaterialCondition {
            cache: LazyCache::new(sc.cells.last_update_xz()),
            surface_context: sc,
        };
        assert!(!steep.test(), "snapshot predates the eroded-badlands raise");

        // Positive control: the SAME column with the extension's raise applied
        // (Java's per-column `erodedBadlandsExtension` lifts (0, 1) to 14; the
        // x = 0 west clamp reads it, the unprocessed east neighbor stays 10,
        // and 14 >= 10 + 4 fires). The snapshot timing is the only thing that
        // suppressed the fire above — if the steep condition ever read a
        // raised column, the first assertion would flip.
        let mut raised = [10i32; 256];
        raised[0 + 1 * 16] = 14;
        let sc = Arc::new(SurfaceContext {
            system: Arc::new(stub_surface_system()),
            cells: SharedCells::new(),
            biome_getter: Arc::new(|_: &BlockPos| Holder::direct(BiomeId::from_id(0))),
            worldgen_context: Arc::new(worldgen_context(-64, 384, 384)),
            world_surface_heights: Some(Arc::new(raised)),
        });
        sc.update_xz(0, 1);
        let steep = SteepMaterialCondition {
            cache: LazyCache::new(sc.cells.last_update_xz()),
            surface_context: sc,
        };
        assert!(steep.test(), "the raised current column fires steep");
    }

    /// The `#185` biome-value seam is fail-loud, never a panic: the
    /// `temperature` condition resolves through `seam_cold_enough_to_snow`
    /// (permanently false — the `Biome` value registry is unported, biome-core),
    /// and the `build_surface` driver must reach it without panicking on the
    /// frozen-ocean paths. This pins the reachable-false contract so a future
    /// "improvement" to panic (or to fabricate a value) fails the test.
    #[test]
    fn temperature_seam_never_panics_and_is_false() {
        // The seam function itself: typed, deterministic, false.
        assert!(!seam_cold_enough_to_snow());
        // A `Context` with a `Temperature` source applies and tests without
        // panicking, reading the biome cache (Java's `getBiome()` is called
        // first — `surface_context.get_biome()` — then the seam).
        let sc = stub_surface_context();
        sc.update_xz(0, 0);
        sc.update_y(1, 1, i32::MIN, 63);
        let temperature = Arc::new(TemperatureHelperCondition {
            cache: LazyCache::new(sc.cells.last_update_y()),
            surface_context: sc,
        });
        assert!(
            !temperature.test(),
            "the temperature seam is permanently false"
        );
    }

    /// `SurfaceRuleData.air()` must round-trip through `rule_source_codec` as a
    /// `block` rule (Java's `makeStateRule(Blocks.AIR)`), never fail with
    /// "Material rule type 'air' is not ported".
    #[test]
    fn surface_rule_air_encodes_as_block_rule() {
        let ops = ops();
        let codec = rule_source_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &surface_rule_air())
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"type": "minecraft:block", "result_state": {"Name": "minecraft:air"}})
        );
        let decoded = codec.parse(&ops, &encoded).get_or_throw("decode").clone();
        assert!(decoded.as_any().is::<BlockRuleSource>());
    }

    /// `SurfaceRuleData.end()` must round-trip through `rule_source_codec` as a
    /// `block` rule carrying end stone (Java's `makeStateRule(Blocks.END_STONE)`),
    /// never the fabricated all-air placeholder.
    #[test]
    fn surface_rule_end_encodes_as_end_stone_block_rule() {
        let ops = ops();
        let codec = rule_source_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &surface_rule_end())
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"type": "minecraft:block", "result_state": {"Name": "minecraft:end_stone"}})
        );
        let decoded = codec.parse(&ops, &encoded).get_or_throw("decode").clone();
        assert!(decoded.as_any().is::<BlockRuleSource>());
    }

    /// `(condition type, result type)` for each top-level rule of an encoded
    /// `sequence` preset — the `SurfaceRuleData.java` declaration-order
    /// skeleton. A `condition` rule reports its `if_true` condition type and the
    /// `then_run` result type; a bare rule (the final `NETHERRACK`/`DEEPSLATE`
    /// fallbacks, or the unwrapped `mainRuleCloseToSurface`) reports an empty
    /// condition and its own rule type.
    fn top_level_condition_then(encoded: &serde_json::Value) -> Vec<(&str, &str)> {
        encoded["sequence"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["if_true"]["type"].as_str().unwrap_or(""),
                    r["then_run"]["type"]
                        .as_str()
                        .or_else(|| r["type"].as_str())
                        .unwrap_or(""),
                )
            })
            .collect()
    }

    /// `SurfaceRuleData.nether` builds the Java top-level 8-rule sequence in
    /// declaration order: bedrock floor, bedrock roof (`not`), the
    /// `closeToCeiling` netherrack cap, the three biome blocks (basalt deltas,
    /// soul sand valley, the `ON_FLOOR` lava/nylium block), nether wastes, and
    /// the final netherrack fallback. A reordered, dropped, or re-typed rule
    /// fails here. (Byte-exact coverage against the committed PR #597 fixture
    /// lives in `builders_encode_byte_exactly_to_the_paper_capture`; this
    /// structural skeleton pins the ordering with a fast lib test.)
    #[test]
    fn nether_tree_matches_java_top_level_ordering() {
        let ops = ops();
        let codec = rule_source_codec::<TestOps>();
        let getter = ops.getter(&*rivet_registry::registries::BIOME).unwrap();
        let encoded = codec
            .encode_start(&ops, &surface_rule_nether(&getter))
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            top_level_condition_then(&encoded),
            vec![
                ("minecraft:vertical_gradient", "minecraft:block"),
                ("minecraft:not", "minecraft:block"),
                ("minecraft:y_above", "minecraft:block"),
                ("minecraft:biome", "minecraft:sequence"),
                ("minecraft:biome", "minecraft:sequence"),
                ("minecraft:stone_depth", "minecraft:sequence"),
                ("minecraft:biome", "minecraft:sequence"),
                ("", "minecraft:block"),
            ]
        );
        // The final bare rule is the netherrack fallback (Java's last
        // `NETHERRACK` element — a plain `block` rule, no `condition` wrapper).
        assert_eq!(
            encoded["sequence"][7]["result_state"]["Name"],
            json!("minecraft:netherrack")
        );
        // The three biome rules resolve to basalt deltas, soul sand valley, and
        // nether wastes in declaration order.
        let biome_is: Vec<&str> = (0..7)
            .filter(|&i| encoded["sequence"][i]["if_true"]["type"] == json!("minecraft:biome"))
            .map(|i| {
                encoded["sequence"][i]["if_true"]["biome_is"]
                    .as_str()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            biome_is,
            vec![
                "minecraft:basalt_deltas",
                "minecraft:soul_sand_valley",
                "minecraft:nether_wastes"
            ]
        );
    }

    /// `SurfaceRuleData.overworld` = `overworldLike(true, false, true)`: the
    /// Java 4-rule top-level sequence (bedrock floor, the
    /// `abovePreliminarySurface`-wrapped main rule, the sulfur-caves bands, the
    /// deepslate gradient).
    #[test]
    fn overworld_tree_matches_java_top_level_ordering() {
        let ops = ops();
        let codec = rule_source_codec::<TestOps>();
        let getter = ops.getter(&*rivet_registry::registries::BIOME).unwrap();
        let encoded = codec
            .encode_start(&ops, &surface_rule_overworld(&getter))
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            top_level_condition_then(&encoded),
            vec![
                ("minecraft:vertical_gradient", "minecraft:block"),
                ("minecraft:above_preliminary_surface", "minecraft:sequence"),
                ("minecraft:biome", "minecraft:sequence"),
                ("minecraft:vertical_gradient", "minecraft:block"),
            ]
        );
        // The deepslate rule is the last element (Java's final `builder.add`).
        assert_eq!(
            encoded["sequence"][3]["if_true"]["random_name"],
            json!("minecraft:deepslate")
        );
    }

    /// The `overworldLike` flags select the Java `ImmutableList` builder
    /// additions: `bedrockRoof`/`bedrockFloor` prepend their `not`-wrapped /
    /// plain vertical gradients, `doPreliminarySurfaceCheck` wraps the main
    /// rule in `abovePreliminarySurface`. The `caves` preset
    /// `overworldLike(false, true, true)` must carry both bedrocks and the bare
    /// main rule; `floating_islands` `overworldLike(false, false, false)` must
    /// carry neither bedrock.
    ///
    /// The committed PR #597 fixture byte-exactly covers `overworld` `(true,
    /// false, true)` and `(false, false, true)` (see
    /// `builders_encode_byte_exactly_to_the_paper_capture`); the `caves`
    /// `(false, true, true)` and `floating_islands` `(false, false, false)`
    /// combos are not in that capture, so the flag selection is pinned here
    /// structurally against the Java-documented builder additions.
    #[test]
    fn overworld_like_flags_control_bedrock_and_preliminary_surface() {
        let ops = ops();
        let codec = rule_source_codec::<TestOps>();
        let getter = ops.getter(&*rivet_registry::registries::BIOME).unwrap();

        let caves = codec
            .encode_start(
                &ops,
                &surface_rule_overworld_like(&getter, false, true, true),
            )
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            top_level_condition_then(&caves),
            vec![
                ("minecraft:not", "minecraft:block"),
                ("minecraft:vertical_gradient", "minecraft:block"),
                ("", "minecraft:sequence"),
                ("minecraft:biome", "minecraft:sequence"),
                ("minecraft:vertical_gradient", "minecraft:block"),
            ]
        );
        // The first `not` is the bedrock roof, the vertical gradient the floor.
        assert_eq!(
            caves["sequence"][0]["if_true"]["invert"]["random_name"],
            json!("minecraft:bedrock_roof")
        );
        assert_eq!(
            caves["sequence"][1]["if_true"]["random_name"],
            json!("minecraft:bedrock_floor")
        );
        // No `abovePreliminarySurface` (doPreliminarySurfaceCheck = false).
        assert!(!caves.to_string().contains("above_preliminary_surface"));

        let floating = codec
            .encode_start(
                &ops,
                &surface_rule_overworld_like(&getter, false, false, false),
            )
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            top_level_condition_then(&floating),
            vec![
                ("", "minecraft:sequence"),
                ("minecraft:biome", "minecraft:sequence"),
                ("minecraft:vertical_gradient", "minecraft:block"),
            ]
        );
        // No bedrock anywhere in the tree.
        assert!(!floating.to_string().contains("minecraft:bedrock"));
    }

    /// `SurfaceRules.isBiome` builds `HolderSet.direct(getOrThrow(key))` in
    /// argument order — a multi-key set encodes as the list in varargs order
    /// (Java's `isBiome(biomes, WARM_OCEAN, BEACH, SNOWY_BEACH)`).
    #[test]
    fn is_biome_preserves_argument_order_in_holder_set() {
        let ops = ops();
        let getter = ops.getter(&*rivet_registry::registries::BIOME).unwrap();
        let cond = is_biome(
            &getter,
            &[&*biomes::WARM_OCEAN, &*biomes::BEACH, &*biomes::SNOWY_BEACH],
        );
        let codec = condition_source_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &cond)
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"type": "minecraft:biome", "biome_is": ["minecraft:warm_ocean", "minecraft:beach", "minecraft:snowy_beach"]})
        );
    }

    /// A single-key `isBiome` encodes as the compact bare identifier (Java's
    /// `HolderSet` list arm degrades a single element), never a one-element
    /// list.
    #[test]
    fn is_biome_single_key_encodes_as_bare_identifier() {
        let ops = ops();
        let getter = ops.getter(&*rivet_registry::registries::BIOME).unwrap();
        let cond = is_biome(&getter, &[&*biomes::BASALT_DELTAS]);
        let codec = condition_source_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &cond)
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"type": "minecraft:biome", "biome_is": "minecraft:basalt_deltas"})
        );
    }

    /// `isBiome` resolves keys through `getOrThrow`: a missing biome key panics
    /// with Java's `Missing element <key>` message (the surface trees would
    /// otherwise silently build a holder set with a hole).
    #[test]
    fn is_biome_missing_key_panics_with_java_message() {
        let ops = ops();
        let getter = ops.getter(&*rivet_registry::registries::BIOME).unwrap();
        let missing = ResourceKey::create(
            &*rivet_registry::registries::BIOME,
            Identifier::parse("minecraft:not_a_real_biome"),
        );
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            is_biome(&getter, &[&missing]);
        }));
        let msg = err
            .unwrap_err()
            .downcast_ref::<String>()
            .cloned()
            .expect("panic message is a String");
        // Java's `ResourceKey.toString()` is `ResourceKey[registry / id]`.
        assert_eq!(
            msg,
            "Missing element ResourceKey[minecraft:worldgen/biome / minecraft:not_a_real_biome]"
        );
    }

    /// The `sulfurCaveBands` float-literal thresholds are widened from float to
    /// double exactly like Java (`-0.4F`/`0.4F` → `-0.4000000059604645`/
    /// `0.4000000059604645`, not `-0.4`/`0.4`). The `surfaceNoiseAbove` helper
    /// thresholds are `threshold / 8.25` in double.
    #[test]
    fn sulfur_cave_bands_encode_widened_float_thresholds() {
        let ops = ops();
        let codec = rule_source_codec::<TestOps>();
        let getter = ops.getter(&*rivet_registry::registries::BIOME).unwrap();
        let encoded = codec
            .encode_start(&ops, &surface_rule_overworld(&getter))
            .get_or_throw("encode")
            .clone();
        let sulfur = &encoded["sequence"][2]["then_run"]["sequence"];
        assert_eq!(
            sulfur[0]["if_true"]["min_threshold"].as_f64(),
            Some(-0.4_f32 as f64)
        );
        assert_eq!(
            sulfur[0]["if_true"]["max_threshold"].as_f64(),
            Some(-0.1_f32 as f64)
        );
        assert_eq!(sulfur[1]["if_true"]["min_threshold"].as_f64(), Some(0.0));
        assert_eq!(
            sulfur[1]["if_true"]["max_threshold"].as_f64(),
            Some(0.4_f32 as f64)
        );
        assert_eq!(
            sulfur[2]["if_true"]["min_threshold"].as_f64(),
            Some(0.4_f32 as f64)
        );
        assert_eq!(
            sulfur[2]["if_true"]["max_threshold"].as_f64(),
            Some(f64::MAX)
        );
        assert_eq!(
            sulfur[0]["if_true"]["noise"],
            json!("minecraft:sulfur_cave_gradient")
        );
        // `surfaceNoiseAbove(threshold)` = `noiseCondition2d(SURFACE,
        // threshold / 8.25, MAX)` — Java's private helper. The overworld tree's
        // `windswept_hills` rule (`surfaceNoiseAbove(1.0)`) must carry the
        // `1.0 / 8.25` double threshold, never `1.0` or a float-widened value.
        let windswept_hills = &encoded["sequence"][1]["then_run"]["sequence"][2]["then_run"]["then_run"]
            ["sequence"][1]["sequence"][4]["sequence"][2]["then_run"];
        assert_eq!(
            windswept_hills["if_true"]["noise"],
            json!("minecraft:surface")
        );
        assert_eq!(
            windswept_hills["if_true"]["min_threshold"].as_f64(),
            Some(1.0 / 8.25)
        );
        assert_eq!(
            windswept_hills["if_true"]["max_threshold"].as_f64(),
            Some(f64::MAX)
        );
    }

    /// The real nether/overworld trees must round-trip through
    /// `rule_source_codec` (the `MATERIAL_RULE` dispatch) — the production
    /// `NoiseGeneratorSettings.surface_rule` field stores the erased
    /// `ArcRuleSource`, so the value codec is the persistence path.
    #[test]
    fn nether_and_overworld_round_trip_through_rule_source_codec() {
        let ops = ops();
        let codec = rule_source_codec::<TestOps>();
        let getter = ops.getter(&*rivet_registry::registries::BIOME).unwrap();
        for rule in [
            surface_rule_nether(&getter),
            surface_rule_overworld(&getter),
            surface_rule_overworld_like(&getter, false, true, true),
        ] {
            let encoded = codec
                .encode_start(&ops, &rule)
                .get_or_throw("encode")
                .clone();
            let decoded = codec.parse(&ops, &encoded).get_or_throw("decode").clone();
            assert!(decoded.as_any().is::<SequenceRuleSource>());
        }
    }

    /// Every builder-constructed tree byte-matches the committed PR #597
    /// fixture (the Paper 26.2 `0a99345` capture of the canonical
    /// `SurfaceRuleData` trees through `RuleSource.CODEC` under `RegistryOps`).
    /// The golden harness round-trips the captured JSON through the codec; this
    /// pins the builder-construction path against the same capture, so a
    /// threshold, ordering, block, or anchor deviation in the ported builders
    /// fails here. The fixture covers `nether`, `overworld`
    /// (`overworldLike(true, false, true)`), `overworld_like_true_false_true`
    /// (identical), `overworld_like_false_false_true`, `end`, and `air`. The
    /// `caves` `(false, true, true)` and `floating_islands` `(false, false,
    /// false)` flag combos are a deliberate, documented deferral: they are not
    /// in the committed capture (the probe ran against `working/Paper`, outside
    /// this worktree) and remain structurally pinned by
    /// `overworld_like_flags_control_bedrock_and_preliminary_surface`. Extending
    /// the fixture with those two trees is a follow-up probe capture, not a
    /// weakening of this test.
    #[test]
    fn builders_encode_byte_exactly_to_the_paper_capture() {
        let ops = ops();
        let codec = rule_source_codec::<TestOps>();
        let getter = ops.getter(&*rivet_registry::registries::BIOME).unwrap();
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tools/rivet-oracle/fixtures/surface-rule-data/surface-rule-data.json"
        ))
        .expect("fixture parses");
        let cases: Vec<(&str, ArcRuleSource)> = vec![
            ("nether", surface_rule_nether(&getter)),
            ("overworld", surface_rule_overworld(&getter)),
            (
                "overworld_like_true_false_true",
                surface_rule_overworld_like(&getter, true, false, true),
            ),
            (
                "overworld_like_false_false_true",
                surface_rule_overworld_like(&getter, false, false, true),
            ),
            ("end", surface_rule_end()),
            ("air", surface_rule_air()),
        ];
        for (name, tree) in cases {
            let canonical = fixture["presets"]
                .as_array()
                .expect("presets array")
                .iter()
                .find(|p| p["name"] == name)
                .expect("preset present")["json"]
                .clone();
            let encoded = codec
                .encode_start(&ops, &tree)
                .get_or_throw("encode")
                .clone();
            assert_eq!(
                serde_json::to_vec(&encoded).expect("encoded JSON serializes"),
                serde_json::to_vec(&canonical).expect("canonical JSON serializes"),
                "{name} builder must encode byte-identically to the Paper capture"
            );
        }
    }

    #[test]
    fn generate_bands_has_192_entries_and_all_band_colors() {
        let mut random = LegacyRandomSource::new(1234);
        let bands = SurfaceSystem::generate_bands(&mut random);
        assert_eq!(bands.len(), 192);
        // Every entry is one of the seven known band ids.
        let known = [484, 485, 488, 496, 498, 492, 554];
        assert!(bands.iter().all(|s| known.contains(&s.block().0)));
        // The default fill is terracotta; each loop adds its color.
        let counts = |id: u16| bands.iter().filter(|s| s.block().0 == id).count();
        assert!(counts(554) > 0, "terracotta base present");
        assert!(counts(485) > 0, "orange bands present");
        assert!(counts(488) > 0, "yellow bands present");
        assert!(counts(496) > 0, "brown bands present");
        assert!(counts(498) > 0, "red bands present");
        assert!(counts(484) > 0, "white bands present");
    }

    #[test]
    fn generate_bands_is_deterministic() {
        let mut a = LegacyRandomSource::new(99);
        let mut b = LegacyRandomSource::new(99);
        let bands_a = SurfaceSystem::generate_bands(&mut a);
        let bands_b = SurfaceSystem::generate_bands(&mut b);
        assert_eq!(bands_a, bands_b);
    }

    /// The exact 192-entry band array `SurfaceSystem.generate_bands` produces
    /// for `LegacyRandomSource::new(1234)` — the ids (terracotta=554,
    /// orange=485, ...). An exact regression derived from Paper's
    /// `generateBands`/`makeBands` semantics (the deterministic
    /// `LegacyRandomSource` draw order — see the port in `generate_bands`/
    /// `make_bands`), not yet mirrored by a `rivet-oracle` slice. Pins the RNG
    /// draw count AND the array, so a change like dropping Java's
    /// `for (i...)` loop `i++` (which shifts both) fails here even though the
    /// structural/determinism tests above still pass.
    #[test]
    fn generate_bands_matches_paper_golden_seed_1234() {
        const GOLDEN: [u16; 192] = [
            484, 492, 554, 554, 496, 496, 554, 554, 554, 485, 554, 554, 554, 554, 484, 498, 498,
            554, 485, 554, 554, 554, 498, 498, 498, 488, 488, 554, 492, 484, 554, 554, 554, 554,
            554, 485, 554, 554, 554, 485, 554, 496, 496, 496, 554, 485, 498, 492, 484, 492, 485,
            498, 498, 498, 496, 488, 488, 488, 496, 496, 496, 496, 554, 485, 554, 554, 492, 484,
            496, 496, 492, 484, 554, 554, 554, 485, 484, 554, 554, 485, 554, 554, 492, 484, 554,
            554, 554, 498, 498, 498, 554, 554, 554, 554, 554, 492, 484, 498, 554, 485, 554, 554,
            488, 498, 498, 554, 484, 492, 496, 496, 496, 496, 496, 554, 554, 554, 496, 496, 554,
            485, 496, 496, 496, 554, 554, 485, 554, 554, 485, 496, 496, 496, 496, 485, 554, 554,
            554, 554, 488, 488, 488, 554, 554, 554, 554, 485, 554, 554, 485, 554, 485, 554, 488,
            488, 488, 488, 498, 498, 496, 496, 496, 496, 554, 554, 488, 488, 488, 554, 554, 554,
            554, 485, 554, 554, 554, 554, 485, 554, 554, 496, 496, 485, 554, 554, 496, 496, 496,
            554, 488, 488, 488, 485,
        ];
        let mut random = LegacyRandomSource::new(1234);
        let bands = SurfaceSystem::generate_bands(&mut random);
        let ids: Vec<u16> = bands.iter().map(|s| s.block().0).collect();
        assert_eq!(ids, GOLDEN);
    }

    #[test]
    fn get_band_wraps_modulo_band_length() {
        // A 192-band array whose block id is `index % 192` so the offset-0
        // (zero-noise) lookup is directly observable.
        let bands: Vec<BlockState> = (0..192)
            .map(|i| BlockState::of(BlockId((i % 192) as u16)))
            .collect();
        let system = system_with_bands(bands);
        assert_eq!(system.get_band(0, 0, 0).block().0, 0);
        assert_eq!(system.get_band(0, 192, 0).block().0, 0);
        assert_eq!(system.get_band(0, 383, 0).block().0, 191);
        // `(y + offset + len) % len` — a negative y wraps to the tail.
        assert_eq!(system.get_band(0, -1, 0).block().0, 191);
    }

    #[test]
    fn hole_condition_lazy_cache_and_surface_depth() {
        let sc = stub_surface_context();
        let hole = HoleCondition {
            cache: LazyCache::new(sc.cells.last_update_xz()),
            surface_context: sc.clone(),
        };
        sc.update_xz(0, 0);
        // surfaceDepth = 3 (the stub's getSurfaceDepth), so `<= 0` is false.
        assert!(!hole.test());
        // Same counter returns the cached result.
        assert!(!hole.test());
        // Mutating the cell without bumping the counter keeps the cache.
        *sc.cells.surface_depth.lock().unwrap() = -1;
        assert!(!hole.test());
        // Bumping the XZ counter recomputes from the cell: `-1 <= 0` is true.
        // (`update_xz` would recompute surface_depth from the system noise, so
        // the counter is bumped directly to observe the lazy-cache path.)
        {
            let mut v = sc.cells.last_update_xz.lock().unwrap();
            *v = v.wrapping_add(1);
        }
        assert!(hole.test());
    }

    #[test]
    fn stone_depth_condition_matches_paper_arithmetic() {
        let sc = stub_surface_context();
        let cond = StoneDepthCondition {
            cache: LazyCache::new(sc.cells.last_update_y()),
            surface_context: sc.clone(),
            ceiling: false,
            offset: 2,
            add_surface_depth: true,
            secondary_depth_range: 6,
        };
        sc.update_xz(0, 0);
        sc.update_y(1, 5, i32::MIN, 40);
        // secondary = (int)map(0.0, -1, 1, 0, 6) = 3; surfaceDepth = 3.
        // stoneDepthAbove(1) <= 1 + 2 + 3 + 3 = 9 -> true.
        assert!(cond.test());
        sc.update_y(10, 5, i32::MIN, 40);
        // 10 <= 9 -> false.
        assert!(!cond.test());
    }

    #[test]
    fn water_condition_matches_paper_arithmetic() {
        let sc = stub_surface_context();
        let cond = WaterCondition {
            cache: LazyCache::new(sc.cells.last_update_y()),
            surface_context: sc.clone(),
            offset: 0,
            surface_depth_multiplier: 2,
            add_stone_depth: false,
        };
        sc.update_xz(0, 0);
        sc.update_y(0, 0, i32::MIN, 40);
        // waterHeight == Integer.MIN_VALUE -> always true.
        assert!(cond.test());
        sc.update_y(0, 0, 40, 46);
        // blockY(46) >= 40 + 0 + surfaceDepth(3)*2 = 46 -> true.
        assert!(cond.test());
        sc.update_y(0, 0, 40, 45);
        // 45 >= 46 -> false.
        assert!(!cond.test());
    }

    #[test]
    fn y_condition_matches_paper_arithmetic() {
        let sc = stub_surface_context();
        let cond = YCondition {
            cache: LazyCache::new(sc.cells.last_update_y()),
            surface_context: sc.clone(),
            anchor: VerticalAnchor::absolute(50),
            surface_depth_multiplier: 3,
            add_stone_depth: true,
        };
        sc.update_xz(0, 0);
        sc.update_y(0, 0, i32::MIN, 60);
        // blockY(60) + stoneDepthAbove(0) >= 50 + 3*3 = 59 -> true.
        assert!(cond.test());
        sc.update_y(0, 0, i32::MIN, 50);
        // 50 >= 59 -> false.
        assert!(!cond.test());
    }

    #[test]
    fn biome_condition_matches_holder_set() {
        let sc = stub_surface_context();
        let biomes = HolderSet::direct(vec![Holder::direct(BiomeId::from_id(0))]);
        let cond = BiomeCondition {
            cache: LazyCache::new(sc.cells.last_update_y()),
            surface_context: sc.clone(),
            biomes,
        };
        sc.update_xz(0, 0);
        sc.update_y(0, 0, i32::MIN, 5);
        assert!(cond.test());
    }

    #[test]
    fn noise_threshold_condition_applies_bounds() {
        // The zero-output noise sampler reads `blockX`/`blockZ` from the cells
        // on the 2d counter; `get_value(0,0,0)` = 0.0.
        let sc = stub_surface_context();
        let sampler = {
            let surface_context = sc.clone();
            let last_update = Mutex::new(surface_context.cells.last_update_xz().wrapping_sub(1));
            let last_noise = Mutex::new(0.0);
            let noise = zero_noise();
            Arc::new(move || {
                let ctx_update = surface_context.cells.last_update_xz();
                let mut last = last_update.lock().unwrap();
                if *last != ctx_update {
                    *last_noise.lock().unwrap() = noise.get_value(
                        surface_context.cells.block_x() as f64,
                        0.0,
                        surface_context.cells.block_z() as f64,
                    );
                    *last = ctx_update;
                }
                *last_noise.lock().unwrap()
            }) as NoiseSampler
        };
        let cond = NoiseThresholdCondition {
            noise_sampler: sampler,
            min_threshold: -1.0,
            max_threshold: 1.0,
        };
        sc.update_xz(0, 0);
        assert!(cond.test()); // 0.0 in [-1, 1]
        let cond_out = NoiseThresholdCondition {
            noise_sampler: cond.noise_sampler.clone(),
            min_threshold: 1.0,
            max_threshold: 2.0,
        };
        assert!(!cond_out.test()); // 0.0 not in [1, 2]
    }

    /// A `BlockColumn` double recording the write requests a surface extension
    /// issues (the extensions read the column through `get_block` and write the
    /// `default_block`/snow/packed-ice through `set_block`).
    struct RecordingColumn {
        writes: Vec<(i32, BlockState)>,
    }

    impl BlockColumn<BlockState> for RecordingColumn {
        fn get_block(&self, _block_y: i32) -> BlockState {
            Blocks::STONE.default_block_state()
        }
        fn set_block(&mut self, block_y: i32, state: BlockState) {
            self.writes.push((block_y, state));
        }
    }

    /// Java's `erodedBadlandsExtension` early-returns on `pillarBuffer <= 0.0`
    /// (the `min(abs(surface*8.25), pillar(x*0.2)*15.0)` guard). The zero-noise
    /// stub gives `pillarBuffer = 0`, so the extension must not touch the
    /// column — a Paper-grounded guard assertion (no fake terrain golden).
    #[test]
    fn eroded_badlands_extension_guards_on_zero_noise() {
        let system = stub_surface_system();
        let mut column = RecordingColumn { writes: Vec::new() };
        system.eroded_badlands_extension(&mut column, 0, 0, 64, -64);
        assert!(column.writes.is_empty());
    }

    /// Java's `frozenOceanExtension` early-returns on `iceberg <= 1.8` (the
    /// `min(abs(surface*8.25), pillar(x*1.28)*15.0)` guard). The zero-noise
    /// stub gives `iceberg = 0`, so no column writes.
    #[test]
    fn frozen_ocean_extension_guards_on_zero_noise() {
        let system = stub_surface_system();
        let mut column = RecordingColumn { writes: Vec::new() };
        system.frozen_ocean_extension(i32::MAX, false, &mut column, 0, 0, 64);
        assert!(column.writes.is_empty());
    }

    /// `Context.updateXZ(int, int)` threads the block coordinates into the
    /// shared cells and bumps both update counters (Java's `updateXZ` sets
    /// `blockX`/`blockZ` and recomputes `surfaceDepth` at the new column).
    /// `surfaceDepth` is x/z-insensitive here because the zero-noise stub
    /// yields `(int)(3.0 + nextDouble*0.25) = 3` for every column — the assert
    /// pins the recompute-on-update behavior, not a terrain value.
    #[test]
    fn update_xz_threads_block_coords_and_bumps_counters() {
        let sc = stub_surface_context();
        let before_xz = sc.cells.last_update_xz();
        let before_y = sc.cells.last_update_y();
        sc.update_xz(5, 9);
        assert_eq!(sc.cells.block_x(), 5);
        assert_eq!(sc.cells.block_z(), 9);
        assert_eq!(sc.cells.last_update_xz(), before_xz.wrapping_add(1));
        assert_eq!(sc.cells.last_update_y(), before_y.wrapping_add(1));
        assert_eq!(sc.cells.surface_depth(), 3);
    }

    /// A `ChunkSurface` double: a fixed-height worldgen window over an
    /// all-stone body, tracking the write requests the surface system issues.
    /// `get_height` returns `surface_height - 1` per column (so
    /// `startingHeight = getHeight + 1 = surface_height`); `is_inside_build_height`
    /// follows the height window (`min_y .. min_y + height`).
    struct MockChunkSurface {
        min_y: i32,
        height: i32,
        surface_height: i32,
        writes: Vec<(i32, i32, i32, BlockState)>,
    }

    impl ChunkSurface for MockChunkSurface {
        fn get_height(&self, _x: i32, _z: i32) -> i32 {
            self.surface_height - 1
        }
        fn get_min_y(&self) -> i32 {
            self.min_y
        }
        fn min_block_x(&self) -> i32 {
            0
        }
        fn min_block_z(&self) -> i32 {
            0
        }
        fn is_inside_build_height(&self, y: i32) -> bool {
            y >= self.min_y && y < self.min_y + self.height
        }
        fn get_block_state(&self, _x: i32, y: i32, _z: i32) -> BlockState {
            if y < self.surface_height {
                Blocks::STONE.default_block_state()
            } else {
                Blocks::AIR.default_block_state()
            }
        }
        fn set_block_state(&mut self, x: i32, y: i32, z: i32, state: BlockState) {
            self.writes.push((x, y, z, state));
        }
        fn mark_pos_for_post_processing(&mut self, _x: i32, _y: i32, _z: i32) {}
    }

    /// Java's anonymous `buildSurface` `BlockColumn` closes over the shared
    /// `columnPos` (set to the column's `blockX`/`blockZ` once per 16x16
    /// iteration); the port's `ChunkColumnAdapter` mirrors that with the `x`/`z`
    /// `Cell`s. Both `get_block` and `set_block` must route through the current
    /// cell values — a Paper-grounded assertion on the column x/z threading.
    #[test]
    fn chunk_column_adapter_threads_xz_to_get_and_set() {
        let mut chunk = MockChunkSurface {
            min_y: -64,
            height: 384,
            surface_height: 64,
            writes: Vec::new(),
        };
        let mut adapter = ChunkColumnAdapter {
            chunk: RefCell::new(&mut chunk),
            x: Cell::new(5),
            z: Cell::new(9),
        };
        // `get_block(y)` reads at (x=5, y=10, z=9): 10 < surface_height -> stone.
        assert_eq!(adapter.get_block(10), Blocks::STONE.default_block_state());
        // `set_block(y, state)` writes at (x=5, y=10, z=9), guarded by the
        // build-height check.
        adapter.set_block(10, Blocks::AIR.default_block_state());
        assert_eq!(
            chunk.writes,
            vec![(5, 10, 9, Blocks::AIR.default_block_state())]
        );
    }

    /// The `RandomState` + `NoiseChunk` needed to drive `build_surface` —
    /// built like the noise_chunk.rs tests (real `RandomState::create` against
    /// a bootstrap noise registry, the zero-output surface-system noises).
    fn build_surface_state() -> (
        NoiseGeneratorSettings,
        RandomState<'static>,
        Arc<NoiseChunk>,
    ) {
        use crate::levelgen::blending::blender::Blender;
        use crate::levelgen::noise::beardifier_marker::BeardifierMarker;
        use crate::levelgen::noise::density_function::DensityFunction;
        use crate::levelgen::noise::noise_settings::OVERWORLD_NOISE_SETTINGS;
        use crate::levelgen::noisegen::noise_based_chunk_generator::create_fluid_picker;
        use crate::levelgen::noisegen::noise_generator_settings::dummy;
        use crate::levelgen::noisegen::noise_router_data::DensityFunctionValue;
        use rivet_registry::registry::Registry;

        let settings = dummy();
        let noise_key = &crate::levelgen::noise::registry_keys::NOISE;
        let mut noise_builder: RegistryBuilder<NoiseParameters> = RegistryBuilder::new(noise_key);
        let mut noise_ctx =
            crate::data::worldgen::bootstrap_context::RecordingContext::<NoiseParameters>::new(
                rivet_registry::holder::RegistryId(0),
                (*noise_key).clone(),
                RegistryAccess::empty(),
            );
        crate::data::worldgen::noise_data::bootstrap(&mut noise_ctx);
        for reg in noise_ctx.registrations() {
            noise_builder.register(
                &reg.key,
                Arc::new(reg.value.clone()),
                RegistrationInfo::BUILT_IN,
            );
        }
        let noise_registry = noise_builder.freeze();
        let df_key = &crate::levelgen::noise::registry_keys::DENSITY_FUNCTION;
        let df_registry: Registry<DensityFunctionValue> = RegistryBuilder::new(df_key).freeze();

        // `RandomState<'a>` borrows the registries; the test keeps them alive
        // by leaking into a 'static box (the borrow checker cannot see the
        // registry outlives the state otherwise).
        let noise_registry: &'static Registry<NoiseParameters> =
            Box::leak(Box::new(noise_registry));
        let df_registry: &'static Registry<DensityFunctionValue> = Box::leak(Box::new(df_registry));
        let random_state = RandomState::create(&settings, noise_registry, df_registry, 1234);

        let chunk = NoiseChunk::new(
            4,
            &random_state,
            0,
            0,
            &OVERWORLD_NOISE_SETTINGS,
            Arc::new(BeardifierMarker::instance()) as Arc<dyn DensityFunction>,
            &settings,
            Box::new(create_fluid_picker(&settings)),
            Blender::empty(),
        );
        (settings, random_state, Arc::new(chunk))
    }

    /// An air rule writes air into every stone surface cell of each column.
    #[test]
    fn build_surface_writes_rule_result_into_stone_columns() {
        let (_settings, random_state, noise_chunk) = build_surface_state();
        let system = random_state.surface_system();
        let mut chunk = MockChunkSurface {
            min_y: -64,
            height: 384,
            surface_height: 64,
            writes: Vec::new(),
        };
        // `Blocks.AIR` is not `default_block` (stone), so the rule only fires
        // on the `old == defaultBlock` stone columns.
        let rule_source: ArcRuleSource = state(Blocks::AIR.default_block_state());
        let biome_manager = Arc::new(BiomeManager::new(
            Arc::new(crate::biome::FixedBiomeSource::new(Holder::direct(
                BiomeId::from_id(0),
            ))),
            0,
        ));
        let gen_context = Arc::new(worldgen_context(-64, 384, 384));

        system.build_surface(
            &random_state,
            biome_manager,
            false,
            gen_context,
            &mut chunk,
            noise_chunk,
            &rule_source,
            None,
        );

        // Every column wrote into each of its 128 stone cells
        // (`[min_y, surface_height) = [-64, 64)`): every one is `default_block`
        // and air is a valid `tryApply` result, so the rule fires all the way
        // down the column (Java's `buildSurface` replaces every
        // `old == defaultBlock` cell with the rule result).
        assert_eq!(chunk.writes.len(), 16 * 16 * 128);
        assert!(chunk.writes.iter().all(|(_, y, _, _)| *y >= -64 && *y < 64));
        assert!(
            chunk
                .writes
                .iter()
                .all(|(_, _, _, s)| { *s == Blocks::AIR.default_block_state() })
        );
        // The air rule applies at `y = 63` (the surface stone) — the first
        // non-air cell the down-column loop reaches.
        assert!(chunk.writes.iter().any(|(_, y, _, _)| *y == 63));
    }

    /// The `old == defaultBlock` guard: a rule whose condition never matches
    /// (`yBlockCheck` against an impossibly high anchor) must not write.
    #[test]
    fn build_surface_skips_writes_when_rule_returns_none() {
        let (_settings, random_state, noise_chunk) = build_surface_state();
        let system = random_state.surface_system();
        let mut chunk = MockChunkSurface {
            min_y: -64,
            height: 384,
            surface_height: 64,
            writes: Vec::new(),
        };
        let rule_source: ArcRuleSource = if_true(
            y_block_check(VerticalAnchor::absolute(1_000_000), 0),
            state(Blocks::AIR.default_block_state()),
        );
        let biome_manager = Arc::new(BiomeManager::new(
            Arc::new(crate::biome::FixedBiomeSource::new(Holder::direct(
                BiomeId::from_id(0),
            ))),
            0,
        ));
        let gen_context = Arc::new(worldgen_context(-64, 384, 384));

        system.build_surface(
            &random_state,
            biome_manager,
            false,
            gen_context,
            &mut chunk,
            noise_chunk,
            &rule_source,
            None,
        );

        assert_eq!(chunk.writes.len(), 0);
    }

    /// The carver composition: `bind_carver_top_material` wires a
    /// `CarvingContext`'s `@Deprecated` `topMaterial` seam to the real
    /// `SurfaceSystem::top_material` probe, and `carving_context.top_material`
    /// returns the rule result exactly like Java's `SurfaceSystem.topMaterial`
    /// single-column probe (`ruleSource.apply(context)` then
    /// `tryApply(x, y, z)` after `updateXZ`/`updateY`).
    ///
    /// The rule is chosen to *depend on the updated context*: `waterBlockCheck`
    /// reads the `waterHeight` the probe threads into `updateY` (Java's
    /// `underFluid ? blockY + 1 : Integer.MIN_VALUE`), so a `false` probe fires
    /// the rule (water height `MIN` matches the `waterHeight == MIN` fast path)
    /// while a `true` probe suppresses it (water height `blockY + 1` falls
    /// through to the `blockY >= waterHeight + offset` comparison, which is
    /// always false). A broken `updateXZ`/`updateY`/`underFluid` plumbing would
    /// fail both assertions.
    #[test]
    fn carver_top_material_seam_drives_the_surface_probe() {
        use crate::levelgen::carver::carving_context::CarvingContext;

        let (_settings, random_state, noise_chunk) = build_surface_state();
        let level = TestLevel(create(-64, 384));
        let generator = TestGenerator {
            min_y: -64,
            depth: 384,
        };
        let mut carving_context = CarvingContext::new(&generator, &level, &random_state);
        let unbound = CarvingContext::new(&generator, &level, &random_state);

        // `waterBlockCheck(0, 0)`: a `true` probe (not under fluid) sets the
        // water height to `MIN`, which the water condition's `== MIN` fast path
        // matches — so the `block` rule fires. A `false` result means "no
        // replacement" (Java `Optional.empty()`).
        let rule_source: ArcRuleSource = if_true(
            water_block_check(0, 0),
            state(Blocks::GRASS_BLOCK.default_block_state()),
        );
        let biome_manager = Arc::new(BiomeManager::new(
            Arc::new(crate::biome::FixedBiomeSource::new(Holder::direct(
                BiomeId::from_id(0),
            ))),
            0,
        ));
        let biome_getter: BiomeGetter = {
            let bm = Arc::clone(&biome_manager);
            Arc::new(move |pos: &BlockPos| bm.get_biome(pos))
        };

        bind_carver_top_material(
            &mut carving_context,
            &rule_source,
            noise_chunk,
            biome_getter,
        );

        let pos = BlockPos::new(3, 5, 7);
        let replacement = carving_context.top_material(&pos, false);
        assert_eq!(replacement, Some(Blocks::GRASS_BLOCK.default_block_state()));

        // `underFluid = true` threads `blockY + 1` as the water height — the
        // `blockY >= waterHeight + offset` comparison is false, so the rule
        // yields no replacement (Java's `Optional.empty()`).
        assert_eq!(carving_context.top_material(&pos, true), None);

        // An unbound seam yields Java's `Optional.empty()` (no replacement).
        assert_eq!(unbound.top_material(&pos, false), None);
    }

    /// A real-chunk integration test for the surface driver (issue #179): a
    /// hand-built 24-section `ProtoChunk<BlockState, BiomeId, &str>` (the real
    /// worldgen chunk shape) filled with stone `default_block` below y=64, with
    /// the worldgen heightmaps primed exactly as `fill_from_noise` leaves them.
    /// `build_surface` with the END rule (end stone — `SurfaceRuleData.end`)
    /// must replace every stone cell in the real chunk's sections and keep the
    /// worldgen heightmaps stable (the surface write never moves the topmost
    /// non-air block).
    #[test]
    fn build_surface_replaces_stone_in_a_real_proto_chunk() {
        use crate::chunk::proto_chunk::ProtoChunk;
        use crate::chunk::storage::chunk_reconstruction::{
            block_state_predicates, resolve_state_flags,
        };
        use crate::chunk::storage::section_reconstruction::current_version_container_factory;
        use crate::chunk::upgrade_data::UpgradeData;

        let (_settings, random_state, noise_chunk) = build_surface_state();
        let system = random_state.surface_system();

        let mut proto: ProtoChunk<
            BlockState,
            crate::chunk::storage::section_reconstruction::BiomeId,
            &'static str,
        > = ProtoChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            create(-64, 384),
            &current_version_container_factory(),
            None,
            Blocks::AIR.default_block_state(),
            Blocks::AIR.default_block_state(),
            &resolve_state_flags,
        );
        // Fill the lower 8 sections (`[-64, 64)`) with stone via the real
        // section write, then prime the worldgen heightmaps column-by-column
        // (topmost stone at y=63 -> `getHeight = 63`, like the doFill output).
        let stone = Blocks::STONE.default_block_state();
        let flags = resolve_state_flags(&stone);
        let predicates = block_state_predicates();
        for y in -64..64 {
            let section_index = proto.get_section_index(y) as usize;
            let y_in_section = y & 15;
            let section = proto.get_section_mut(section_index);
            for x in 0..16 {
                for z in 0..16 {
                    section.set_block_state(
                        x,
                        y_in_section,
                        z,
                        stone,
                        &predicates.is_air,
                        &predicates.is_randomly_ticking,
                        &predicates.fluid_is_empty,
                        &predicates.fluid_is_randomly_ticking,
                        &predicates.is_special_colliding,
                    );
                }
            }
        }
        for x in 0..16 {
            for z in 0..16 {
                proto.update_heightmaps_after(x, 63, z, flags);
            }
        }
        // The primed heightmap reads back the topmost non-air y (63), exactly
        // what Java's `getHeight(WORLD_SURFACE_WG)` returns for this column.
        let height_before = proto
            .get_or_create_heightmap_unprimed(Types::WorldSurfaceWg)
            .get_height_at(0, 0, -64);
        assert_eq!(height_before, 63);

        let biome_manager = Arc::new(BiomeManager::new(
            Arc::new(crate::biome::FixedBiomeSource::new(Holder::direct(
                BiomeId::from_id(0),
            ))),
            0,
        ));
        let gen_context = Arc::new(worldgen_context(-64, 384, 384));
        let rule_source: ArcRuleSource = surface_rule_end();

        system.build_surface(
            &random_state,
            biome_manager,
            false,
            gen_context,
            &mut proto,
            noise_chunk,
            &rule_source,
            None,
        );

        // Every stone `default_block` cell became the END rule result (end
        // stone) — 16x16x128 real section writes through
        // `ProtoChunk::setBlockState` + the heightmap updates.
        assert_eq!(
            proto.get_block_state(0, 63, 0),
            Blocks::END_STONE.default_block_state()
        );
        assert_eq!(
            proto.get_block_state(15, -64, 15),
            Blocks::END_STONE.default_block_state()
        );
        assert_eq!(
            proto.get_block_state(7, 0, 7),
            Blocks::END_STONE.default_block_state()
        );
        // The air above the surface is untouched.
        assert_eq!(
            proto.get_block_state(0, 64, 0),
            Blocks::AIR.default_block_state()
        );
        // The worldgen heightmaps stayed stable: the surface write replaced
        // stone (non-air) with end stone (non-air), so the topmost non-air y
        // never moves and `getHeight` is unchanged.
        assert_eq!(
            proto
                .get_or_create_heightmap_unprimed(Types::WorldSurfaceWg)
                .get_height_at(0, 0, -64),
            63
        );
        assert_eq!(
            proto
                .get_or_create_heightmap_unprimed(Types::OceanFloorWg)
                .get_height_at(15, 15, -64),
            63
        );
        // Negative control for the fluid-marking seam: end stone is a fluid-empty
        // block, so `BlockColumn.setBlock` must NOT have marked any cell for
        // post-processing (Java's `if (!state.getFluidState().isEmpty())`).
        assert_eq!(
            proto
                .get_post_processing()
                .iter()
                .map(Vec::len)
                .sum::<usize>(),
            0
        );
    }

    /// The fluid-marking half of the surface column seam (issue #179): Java's
    /// `BlockColumn.setBlock` marks every non-empty fluid write for
    /// post-processing (`SurfaceSystem.java`). Here a real `ProtoChunk`
    /// (the same hand-built 24-section shape as
    /// `build_surface_replaces_stone_in_a_real_proto_chunk`) is surfaced with a
    /// WATER rule — the default state is a non-empty fluid (fluid id 2) — so
    /// every stone `default_block` cell the rule replaces becomes a water write
    /// that must land its packed offset in the chunk's post-processing list.
    #[test]
    fn build_surface_marks_water_writes_for_post_processing() {
        use crate::chunk::proto_chunk::ProtoChunk;
        use crate::chunk::storage::chunk_reconstruction::{
            block_state_predicates, resolve_state_flags,
        };
        use crate::chunk::storage::section_reconstruction::current_version_container_factory;
        use crate::chunk::upgrade_data::UpgradeData;

        // The real worldgen chunk shape (same as the END_STONE test).
        type SurfaceChunk = ProtoChunk<
            BlockState,
            crate::chunk::storage::section_reconstruction::BiomeId,
            &'static str,
        >;

        let (_settings, random_state, noise_chunk) = build_surface_state();
        let system = random_state.surface_system();

        let mut proto: SurfaceChunk = ProtoChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            create(-64, 384),
            &current_version_container_factory(),
            None,
            Blocks::AIR.default_block_state(),
            Blocks::AIR.default_block_state(),
            &resolve_state_flags,
        );
        let stone = Blocks::STONE.default_block_state();
        let flags = resolve_state_flags(&stone);
        let predicates = block_state_predicates();
        for y in -64..64 {
            let section_index = proto.get_section_index(y) as usize;
            let y_in_section = y & 15;
            let section = proto.get_section_mut(section_index);
            for x in 0..16 {
                for z in 0..16 {
                    section.set_block_state(
                        x,
                        y_in_section,
                        z,
                        stone,
                        &predicates.is_air,
                        &predicates.is_randomly_ticking,
                        &predicates.fluid_is_empty,
                        &predicates.fluid_is_randomly_ticking,
                        &predicates.is_special_colliding,
                    );
                }
            }
        }
        for x in 0..16 {
            for z in 0..16 {
                proto.update_heightmaps_after(x, 63, z, flags);
            }
        }

        let biome_manager = Arc::new(BiomeManager::new(
            Arc::new(crate::biome::FixedBiomeSource::new(Holder::direct(
                BiomeId::from_id(0),
            ))),
            0,
        ));
        let gen_context = Arc::new(worldgen_context(-64, 384, 384));
        // `state(Blocks.WATER)` — a `block` rule returning the water default
        // state (`SurfaceRuleData` builders are `state(...)` shims).
        let rule_source: ArcRuleSource = state(Blocks::WATER.default_block_state());

        // The default water state is a non-empty fluid in the generated
        // behavior table (fluid id 2), so the write path must mark it.
        assert!(!Blocks::WATER.default_block_state().fluid_empty());

        system.build_surface(
            &random_state,
            biome_manager,
            false,
            gen_context,
            &mut proto,
            noise_chunk,
            &rule_source,
            None,
        );

        // The water writes landed through `ProtoChunk::set_block_state`.
        assert_eq!(
            proto.get_block_state(0, 63, 0),
            Blocks::WATER.default_block_state()
        );
        assert_eq!(
            proto.get_block_state(7, 0, 7),
            Blocks::WATER.default_block_state()
        );
        // Every one of the 16x16 columns x 128 stone layers was replaced, and
        // every replacement is a non-empty fluid write — so exactly one
        // post-processing mark per cell, packed into the per-section lists
        // (`getSectionIndex(y)` buckets).
        let marks: usize = proto.get_post_processing().iter().map(Vec::len).sum();
        assert_eq!(marks, 16 * 16 * 128);
        // Spot-check the packed offset for the topmost water cell (x=0, y=63,
        // z=0): section index 7 (y in [48, 64)), `packOffsetCoordinates` =
        // dx | dy<<4 | dz<<8 = 0 | 15<<4 | 0<<8 = 0xF0.
        let section_index = proto.get_section_index(63) as usize;
        assert_eq!(section_index, 7);
        assert!(proto.get_post_processing()[section_index].contains(
            &SurfaceChunk::pack_offset_coordinates(&BlockPos::new(0, 63, 0))
        ));
    }

    #[test]
    fn set_block_state_air_fast_path_is_identity_not_behavioral() {
        // Java's `wasEmpty && state.is(Blocks.AIR)` fast path is a
        // block-IDENTITY check (`getBlock() == Blocks.AIR`), not the
        // behavioral `is_air` predicate. AIR, CAVE_AIR, and VOID_AIR are all
        // `AirBlock` with `.air()` properties (behaviorally air), so only the
        // identity comparison lets exact AIR take the fast path while
        // CAVE_AIR/VOID_AIR fall through to the real section write.
        use crate::chunk::proto_chunk::ProtoChunk;
        use crate::chunk::storage::chunk_reconstruction::resolve_state_flags;
        use crate::chunk::storage::section_reconstruction::current_version_container_factory;
        use crate::chunk::upgrade_data::UpgradeData;

        let mut proto: ProtoChunk<
            BlockState,
            crate::chunk::storage::section_reconstruction::BiomeId,
            &'static str,
        > = ProtoChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            create(-64, 384),
            &current_version_container_factory(),
            None,
            Blocks::AIR.default_block_state(),
            Blocks::AIR.default_block_state(),
            &resolve_state_flags,
        );
        let section_index = proto.get_section_index(0) as usize;

        // Writing exact AIR leaves the section all-air. This is observationally
        // the same whether the fast path returned early or the write stored
        // AIR (both leave every cell AIR and `non_empty_block_count == 0`), so
        // this half alone cannot prove the fast path was taken — it only pins
        // the section's all-air invariant.
        assert!(proto.get_section(section_index).has_only_air());
        proto.set_block_state(0, 0, 0, Blocks::AIR.default_block_state());
        assert!(proto.get_section(section_index).has_only_air());

        // The discriminating half: CAVE_AIR is behaviorally air but its block
        // id (795) differs from AIR's (0), so the identity guard must NOT fire
        // and the write must reach the paletted container. A behavioral fast
        // path would have returned CAVE_AIR without writing, leaving the cell
        // AIR and failing the assertions below.
        let previous = proto.set_block_state(1, 0, 1, Blocks::CAVE_AIR.default_block_state());
        assert_eq!(previous, Blocks::AIR.default_block_state());
        // `LevelChunkSection::get_block_state` reads the paletted container
        // directly (`states.get`, no `has_only_air` mask), so observing CAVE_AIR
        // here is direct proof the cell was written through the real section
        // path — the palette holds the stored cave-air cell.
        assert_eq!(
            proto.get_section(section_index).get_block_state(1, 0, 1),
            Blocks::CAVE_AIR.default_block_state()
        );
        // The write was cell-targeted: an untouched neighbor in the same
        // all-air section is still AIR at the palette level.
        assert_eq!(
            proto.get_section(section_index).get_block_state(2, 0, 2),
            Blocks::AIR.default_block_state()
        );
        // Java-faithful masking: `ProtoChunk.getBlockState` still returns AIR
        // because `hasOnlyAir` is a behavioral count (CAVE_AIR never
        // increments it), exactly like Java's `section.hasOnlyAir() ? AIR :
        // section.getBlockState(...)`.
        assert!(proto.get_section(section_index).has_only_air());
        assert_eq!(
            proto.get_block_state(1, 0, 1),
            Blocks::AIR.default_block_state()
        );
    }
}
