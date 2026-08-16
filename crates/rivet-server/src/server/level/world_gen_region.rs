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
//! minimal `WorldGenLevel` facade. It does NOT own the scheduler, generator
//! realization, or dependency-window composition. The FEATURES caller supplies
//! the complete accumulated window (17x17 for the current step), while this
//! region enforces the step's ring/status contract; those upstream surfaces
//! defer with their owning units (#185).
//!
//! ## The typed seam
//!
//! One upstream type the region consumes is not ported yet, so the region
//! reads it through the smallest typed contract it needs instead of
//! fabricating its internals:
//!
//! - [`GenerationChunkHolderView`] — the `mc.server.level.pipeline.holder`
//!   `GenerationChunkHolder` surface (`getChunkIfPresentUnchecked` /
//!   `getPersistedStatus`, plus the Rust-only mutable half for `setBlock`).
//!   The real holder (futures, scheduling, status ladder) lands with the
//!   holder unit; the region only needs a holder that can hand back a chunk
//!   completed to a given status.
//!
//! The generating step is the real merged `net.minecraft.world.level.chunk.status.ChunkStep`
//! (`rivet_world::chunk::status::ChunkStep`): the region reads
//! `directDependencies()` / `targetStatus()` / `blockStateWriteRadius()` off it.
//!
//! ## The `ServerLevel` seam
//!
//! Java's `WorldGenRegion(ServerLevel, StaticCache2D, ChunkStep, ChunkAccess)`
//! reads `seed`/`levelData`/`random`/`dimensionType`/`minY`/`height`/`seaLevel`
//! and the `getUncachedNoiseBiome`/POI/light/difficulty/border surface off the
//! `ServerLevel`. The M2 STUB seam (MANIFEST) absorbs that residual
//! `ServerLevel` reference as stubs; this value layer decomposes it into the
//! scalar values the region actually reads (`seed`/`min_y`/`height`/`sea_level`)
//! plus the injected [`NoiseBiomeSource`] for `getUncachedNoiseBiome` and the
//! injected [`RegistryAccess`] for `registryAccess()`. The heavy reads (POI
//! update on `setBlock`, persisted block-entity loading, light engine,
//! difficulty, world border, entity/player collections) remain unported and
//! fail or no-op explicitly rather than fabricating access — each with a
//! `RivetTodo` pointing at the owning unit. Feature-local chest/spawner entities
//! created by this region's writes are the narrow exception needed by
//! `MonsterRoomFeature`.
//!
//! ## Biome access
//!
//! Java constructs `biomeManager = new BiomeManager(this, obfuscateSeed(seed))`
//! where `this` is the region as a `NoiseBiomeSource` (the `LevelReader`
//! default `getNoiseBiome` reads a cached chunk, falling back to
//! `getUncachedNoiseBiome`). The port cannot hold `Arc<Self>` (the ownership
//! model forbids a self-referential worldgen view), so `getBiome` uses the
//! `BiomeManager` interpolation with an injected quart lookup that performs
//! the same cached-chunk-first read. `getBiomeManager` still exposes the
//! uncached source-backed manager for callers that need the standalone value.

use std::collections::HashMap;
use std::sync::Arc;

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::float_tag::FloatTag;
use rivet_nbt::int_tag::IntTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_registry::access::RegistryAccess;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, ChunkPos, QuartPos, SectionPos};
use rivet_registry::fluid_id::FluidId;
use rivet_registry::generated::block_behaviors::{
    BEHAVIOR_FLAG_FLUID_EMPTY, BEHAVIOR_FLAG_RANDOM_TICKING, behavior_of,
};
use rivet_registry::generated::block_states::StateId;
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::holder::Holder;
use rivet_serialization::float_format::java_float_equals;
use rivet_util::StaticCache2D;
use rivet_util::mth;
use rivet_util::util::log_and_pause_if_in_ide;
use rivet_world::biome::biome_manager::{BiomeManager, NoiseBiomeSource};
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::proto_chunk::ProtoChunk;
use rivet_world::chunk::status::{ChunkStatus, ChunkStep};
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::chunk::storage::section_reconstruction::BiomeId as WorldgenBiomeId;
use rivet_world::level::WorldGenLevel;
use rivet_world::level::height_accessor::LevelHeightAccessor;
use rivet_world::levelgen::heightmap::Types;

use crate::server::level::level_chunk::{BiomeId as ServerBiomeId, StructureKey, state_flags};

/// `Block.UPDATE_ALL` — `UPDATE_NEIGHBORS | UPDATE_CLIENTS` (1 | 2), the flag
/// `removeBlock`/`destroyBlock` pass to `setBlock`.
const UPDATE_ALL: i32 = 3;

/// `Block.UPDATE_LIMIT` — the update-limit default `LevelWriter`'s 3-arg
/// `setBlock`/`destroyBlock` pass to the 4-arg form. The value-layer
/// `set_block` ignores it (the update machinery defers), so this is a faithful
/// default, not an operative limit.
const UPDATE_LIMIT: i32 = 512;

/// The block-entity state needed by feature placement. This is deliberately a
/// small live state map rather than a block-state shortcut: `setBlock` creates
/// an entity when the written block requires one, and the feature's entity
/// queries only succeed for that live entry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WorldgenBlockEntity {
    Chest {
        loot: Option<(i64, String)>,
    },
    Spawner {
        next_spawn: Option<String>,
        spawn_potentials: Vec<(String, i32)>,
    },
}

fn dummy_block_entity(pos: &BlockPos) -> CompoundTag {
    let mut tag = CompoundTag::new();
    tag.put_int("x", pos.get_x());
    tag.put_int("y", pos.get_y());
    tag.put_int("z", pos.get_z());
    tag.put_string("id", "DUMMY");
    tag
}

/// Decode the stored `SpawnData` compound through the partial shape of
/// `SpawnData.CODEC`. The required `entity` field must be a compound; a
/// malformed optional field is dropped from the retained partial value. The
/// record constructor also canonicalizes a valid id and removes an absent or
/// malformed one.
fn decode_spawn_data(spawn_data: &CompoundTag) -> Option<CompoundTag> {
    if !matches!(spawn_data.get("entity"), Some(Tag::Compound(_))) {
        return None;
    }
    let mut decoded = spawn_data.clone();
    for field in ["custom_spawn_rules", "equipment"] {
        if decoded
            .tags
            .get(field)
            .is_some_and(|value| !matches!(value, Tag::Compound(_)))
        {
            decoded.remove(field);
        }
    }
    if let Some(Tag::Compound(rules)) = decoded.tags.get_mut("custom_spawn_rules") {
        match normalize_custom_spawn_rules(rules) {
            Ok(normalized) => *rules = normalized,
            Err(()) => {
                decoded.remove("custom_spawn_rules");
            }
        }
    }
    if let Some(Tag::Compound(equipment)) = decoded.tags.get_mut("equipment") {
        let normalized = normalize_equipment(equipment);
        if let Some(normalized) = normalized {
            *equipment = normalized;
        } else {
            decoded.remove("equipment");
        }
    }
    let Some(Tag::Compound(entity)) = decoded.tags.get_mut("entity") else {
        unreachable!("entity was checked above")
    };
    match entity.get_string("id") {
        Some(id) => match Identifier::try_parse_result(id) {
            Ok(Some(identifier)) => entity.put_string("id", &identifier.to_string()),
            Ok(None) | Err(_) => {
                entity.remove("id");
            }
        },
        None => {
            entity.remove("id");
        }
    }
    Some(decoded)
}

fn normalize_custom_spawn_rules(rules: &CompoundTag) -> Result<CompoundTag, ()> {
    let mut normalized = CompoundTag::new();
    for field in ["block_light_limit", "sky_light_limit"] {
        if let Some(value) = rules.get(field)
            && let Some(value) = normalize_light_limit(value)?
        {
            normalized.put(field.to_string(), value);
        }
    }
    Ok(normalized)
}

fn normalize_light_limit(value: &Tag) -> Result<Option<Tag>, ()> {
    let bounds = if let Some(value) = value.as_int() {
        Some((value, value))
    } else if let Tag::List(list) = value {
        if list.list.len() == 2 {
            list.list[0].as_int().zip(list.list[1].as_int())
        } else {
            None
        }
    } else if let Tag::Compound(compound) = value {
        compound
            .get("min_inclusive")
            .and_then(Tag::as_int)
            .zip(compound.get("max_inclusive").and_then(Tag::as_int))
    } else {
        None
    };
    let Some((min, max)) = bounds else {
        return Ok(None);
    };
    if !(0..=15).contains(&min) || !(0..=15).contains(&max) || min > max {
        return Err(());
    }
    if min == 0 && max == 15 {
        return Ok(None);
    }
    if min == max {
        Ok(Some(Tag::Int(IntTag::value_of(min))))
    } else {
        let mut range = CompoundTag::new();
        range.put_int("min_inclusive", min);
        range.put_int("max_inclusive", max);
        Ok(Some(Tag::Compound(range)))
    }
}

fn normalize_equipment(equipment: &CompoundTag) -> Option<CompoundTag> {
    let loot_table = equipment.get_string("loot_table")?;
    let identifier = Identifier::try_parse_result(loot_table).ok()??;
    let slot_drop_chances = match equipment.get("slot_drop_chances") {
        Some(value) => normalize_slot_drop_chances(value).ok()?,
        None => None,
    };
    let mut normalized = CompoundTag::new();
    normalized.put_string("loot_table", &identifier.to_string());
    if let Some(slot_drop_chances) = slot_drop_chances {
        normalized.put("slot_drop_chances".to_string(), slot_drop_chances);
    }
    Some(normalized)
}

fn normalize_slot_drop_chances(value: &Tag) -> Result<Option<Tag>, ()> {
    if let Some(chance) = value.as_float() {
        return Ok(Some(Tag::Float(FloatTag::value_of(chance))));
    }
    let Tag::Compound(chances) = value else {
        return Err(());
    };
    const SLOTS: [&str; 8] = [
        "mainhand", "offhand", "feet", "legs", "chest", "head", "body", "saddle",
    ];
    let mut normalized = CompoundTag::new();
    let mut values = Vec::with_capacity(chances.size());
    for (slot, chance) in chances.entry_set() {
        if !SLOTS.contains(&slot.as_str()) {
            return Err(());
        }
        let chance = chance.as_float().ok_or(())?;
        values.push(chance);
        normalized.put(slot.clone(), Tag::Float(FloatTag::value_of(chance)));
    }
    let Some(first) = values.first().copied() else {
        return Ok(None);
    };
    if values.len() == SLOTS.len() && values.iter().all(|value| java_float_equals(*value, first)) {
        Ok(Some(Tag::Float(FloatTag::value_of(first))))
    } else {
        Ok(Some(Tag::Compound(normalized)))
    }
}

fn spawn_data(tag: &CompoundTag) -> Option<CompoundTag> {
    tag.get_compound("SpawnData").and_then(decode_spawn_data)
}

fn spawn_data_id(tag: &CompoundTag) -> Option<String> {
    spawn_data(tag)
        .as_ref()
        .and_then(|data| data.get_compound("entity"))
        .and_then(|entity| entity.get_string("id"))
        .cloned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SpawnPotentialsDecode {
    MissingOrInvalid,
    Present(Vec<(String, i32)>),
}

/// Decode `SpawnData.LIST_CODEC` one entry at a time. `ListCodec` retains
/// partial element values, so malformed entries are discarded while valid
/// entries remain. A valid SpawnData does not require an entity id.
fn decode_spawn_potentials_state(tag: &CompoundTag) -> SpawnPotentialsDecode {
    let Some(Tag::List(list)) = tag.get("SpawnPotentials") else {
        return SpawnPotentialsDecode::MissingOrInvalid;
    };
    SpawnPotentialsDecode::Present(
        list.list
            .iter()
            .filter_map(|entry| {
                let Tag::Compound(compound) = entry else {
                    return None;
                };
                let weight = compound.get_int("weight")?;
                if weight < 0 {
                    return None;
                }
                let data = decode_spawn_data(compound.get_compound("data")?)?;
                let id = data
                    .get_compound("entity")
                    .and_then(|entity| entity.get_string("id"))
                    .cloned()
                    .unwrap_or_default();
                Some((id, weight))
            })
            .collect(),
    )
}

fn decode_spawn_potentials(tag: &CompoundTag) -> Vec<(String, i32)> {
    match decode_spawn_potentials_state(tag) {
        SpawnPotentialsDecode::MissingOrInvalid => Vec::new(),
        SpawnPotentialsDecode::Present(potentials) => potentials,
    }
}

fn selected_spawn_potential(tag: &CompoundTag, mut roll: i32) -> Option<CompoundTag> {
    let list = tag.get_list("SpawnPotentials")?;
    for entry in &list.list {
        let Tag::Compound(compound) = entry else {
            continue;
        };
        let Some(weight) = compound.get_int("weight").filter(|weight| *weight >= 0) else {
            continue;
        };
        let Some(data) = compound.get_compound("data").and_then(decode_spawn_data) else {
            continue;
        };
        if roll < weight {
            return Some(data);
        }
        roll = roll.wrapping_sub(weight);
    }
    None
}

fn spawn_potential_total(potentials: &[(String, i32)]) -> Option<i32> {
    let total: i64 = potentials
        .iter()
        .filter(|(_, weight)| *weight >= 0)
        .map(|(_, weight)| i64::from(*weight))
        .sum();
    if total > i64::from(i32::MAX) {
        panic!("Sum of weights must be <= 2147483647");
    }
    (total > 0).then_some(total as i32)
}

fn spawn_potential_weight(
    next_spawn: Option<&String>,
    spawn_potentials: &[(String, i32)],
    tag: Option<&CompoundTag>,
) -> Option<i32> {
    if next_spawn.is_some() {
        return None;
    }
    if let Some(tag) = tag
        && spawn_data(tag).is_some()
    {
        return None;
    }
    if !spawn_potentials.is_empty() {
        return spawn_potential_total(spawn_potentials);
    }
    let tag = tag?;
    match decode_spawn_potentials_state(tag) {
        SpawnPotentialsDecode::MissingOrInvalid => Some(1),
        SpawnPotentialsDecode::Present(potentials) => spawn_potential_total(&potentials),
    }
}

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
///
/// Generic over the same value types as [`ChunkAccess`] (`T` the block-state
/// type, `B` the biome type, `S` the caller's structure key) so the worldgen
/// executor can drive a region over its own chunk element types — the
/// `BlockState`/`section_reconstruction::BiomeId` `ProtoChunk`s — while the
/// server's dense `StateId`/`ServerBiomeId` region keeps its block-state
/// methods on the specialized impl. The trait itself is lifetime-free: the
/// borrow-carrying region (`WorldGenRegion<'a, T, B, S>`) stores each holder
/// as `Box<dyn GenerationChunkHolderView<T, B, S> + 'a>`, so a holder that
/// borrows a chunk (the worldgen center `ProtoChunk` the executor already
/// owns) or owns one (the ring chunks it generated) both type-check through
/// the same trait object.
pub trait GenerationChunkHolderView<T, B, S>: Send
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    /// `GenerationChunkHolder.getChunkIfPresentUnchecked(ChunkStatus)` — the
    /// held chunk completed to at least `status`, if any.
    fn get_chunk_if_present_unchecked(&self, status: ChunkStatus) -> Option<&ChunkAccess<T, B, S>>;

    /// `GenerationChunkHolder.getPersistedStatus()` — the held chunk's status,
    /// or `None` for a holder with no chunk (Java null).
    fn get_persisted_status(&self) -> Option<ChunkStatus>;

    /// Rust-only mutable half of
    /// [`get_chunk_if_present_unchecked`](Self::get_chunk_if_present_unchecked)
    /// for the region's `setBlock` chunk write (see the trait doc).
    fn get_chunk_if_present_unchecked_mut(
        &mut self,
        status: ChunkStatus,
    ) -> Option<&mut ChunkAccess<T, B, S>>;
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
            self.actual_status.map_or_else(
                || "[no chunk held]".to_string(),
                |s| s.serialization_name().to_string(),
            )
        };
        let max_allowed = self.max_allowed_status.map_or_else(
            || "null".to_string(),
            |s| s.serialization_name().to_string(),
        );
        let deps = self
            .dependencies
            .iter()
            .map(|s| s.serialization_name())
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
            self.generating_status.serialization_name(),
            self.requested_status.serialization_name(),
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
/// (the chunk view), the generating [`ChunkStep`] (per-ring dependencies +
/// write radius), the center chunk position, and the scalar `ServerLevel` seam
/// values. The value-layer slice implements the ring/status/distance contract,
/// the write-radius gate, and the minimal [`WorldGenLevel`] facade; the heavy
/// server reads defer (see the module doc).
///
/// Generic over the chunk value types `<T, B, S>` plus the holder lifetime
/// `'a`. The pure chunk-view methods — including the write-zone gate the block
/// writes share — live on the generic [`impl<'a, T, B, S>`] (so the worldgen
/// executor's borrow-carrying region can use them); the block-state methods and
/// the [`WorldGenLevel`] impl live on the `StateId`/`ServerBiomeId`/
/// `StructureKey` and `BlockState`/`WorldgenBiomeId`/`StructureKey`
/// specializations. `'a` is the shortest lifetime the cached holders borrow —
/// `'static` for a region over owning holders (the server value layer), a
/// scoped borrow for the executor's center-chunk region.
pub struct WorldGenRegion<'a, T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    /// `cache` — the `StaticCache2D<GenerationChunkHolder>` chunk view.
    cache: StaticCache2D<Box<dyn GenerationChunkHolderView<T, B, S> + 'a>>,
    /// `center` (as `getPos()`) — the generating chunk's position.
    center_pos: ChunkPos,
    /// `centerChunkX` — the center chunk's x.
    center_chunk_x: i32,
    /// `centerChunkZ` — the center chunk's z.
    center_chunk_z: i32,
    /// `generatingStep` — the step whose per-ring dependencies bound chunk
    /// availability and whose `blockStateWriteRadius` bounds writes.
    generating_step: ChunkStep,
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
    /// `level.registryAccess()` — the shared `RegistryAccess` the region
    /// returns from `registry_access` (the `WorldGenLevel` back-reference the
    /// selector/composite features resolve their `Holder<PlacedFeature>`s
    /// through). Owned by value (a cheap `Arc` clone sharing the same frozen
    /// registries); the injected construction mirrors the `ServerLevel` seam
    /// like `uncached_biome_source`.
    registry_access: RegistryAccess,
    /// Live block entities created by this region's block writes. The region
    /// does not infer an entity from a pre-existing block-state id; loading
    /// persisted entities belongs to the chunk/block-entity unit.
    block_entities: HashMap<BlockPos, WorldgenBlockEntity>,
    /// Pending block-entity NBT mirrored into the owning `ChunkAccess`. The
    /// region map is only a typed query cache; dropping the region must not
    /// drop the chunk's DUMMY/materialized payload.
    block_entity_nbts: HashMap<BlockPos, CompoundTag>,
}

impl<'a, T, B, S> WorldGenRegion<'a, T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    /// `new WorldGenRegion(ServerLevel, StaticCache2D, ChunkStep, ChunkAccess)`.
    ///
    /// The `ServerLevel` seam is decomposed into the scalar values the region
    /// reads (`seed`/`min_y`/`height`/`sea_level`) and the injected
    /// `uncached_biome_source` (the `getUncachedNoiseBiome` seam) and
    /// `registry_access` (the `registryAccess()` back-reference); the `center`
    /// `ChunkAccess` is decomposed into its `ChunkPos` (the region reads the
    /// cached chunks through the holder view, never a separate center
    /// reference).
    #[allow(clippy::too_many_arguments)] // mirrors the Java constructor's parameter surface.
    pub fn new(
        cache: StaticCache2D<Box<dyn GenerationChunkHolderView<T, B, S> + 'a>>,
        center_pos: ChunkPos,
        generating_step: ChunkStep,
        seed: i64,
        min_y: i32,
        height: i32,
        sea_level: i32,
        uncached_biome_source: Arc<dyn NoiseBiomeSource>,
        registry_access: RegistryAccess,
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
            registry_access,
            block_entities: HashMap::new(),
            block_entity_nbts: HashMap::new(),
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
        distance < self.generating_step.direct_dependencies().size() as i32
    }

    /// `WorldGenRegion.getChunk(int, int)` — the 2-arg form, targeting
    /// `ChunkStatus.EMPTY`. Panics with the unavailable-chunk diagnostic when
    /// the chunk is not available, exactly as Java throws `ReportedException`.
    pub fn get_chunk(&self, chunk_x: i32, chunk_z: i32) -> &ChunkAccess<T, B, S> {
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
    ) -> Result<&ChunkAccess<T, B, S>, UnavailableChunkDiagnostic> {
        let distance = self
            .center_pos
            .get_chessboard_distance_coords(chunk_x, chunk_z);
        // The per-ring dependency slice is only materialized for the error
        // diagnostic; the happy path reads it by index (no per-access Vec).
        let dependencies = self.generating_step.direct_dependencies();
        let max_allowed_status = if distance >= dependencies.size() as i32 {
            None
        } else {
            Some(dependencies.get(distance as usize))
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
            dependencies: dependencies.as_list().to_vec(),
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
    ) -> Result<&mut ChunkAccess<T, B, S>, UnavailableChunkDiagnostic> {
        let distance = self
            .center_pos
            .get_chessboard_distance_coords(chunk_x, chunk_z);
        // The per-ring dependency slice is scoped so its immutable borrow of
        // `self` ends before the mutable `cache` access below (no per-access
        // Vec on the happy path; the diagnostic re-fetches it).
        let max_allowed_status = {
            let dependencies = self.generating_step.direct_dependencies();
            if distance >= dependencies.size() as i32 {
                None
            } else {
                Some(dependencies.get(distance as usize))
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
            dependencies: self
                .generating_step
                .direct_dependencies()
                .as_list()
                .to_vec(),
            distance,
            generating_chunk: self.center_pos,
        })
    }

    /// `WorldGenRegion.getChunk(int, int)` mutable half — the 2-arg contract
    /// for `setBlock`.
    fn get_chunk_mut(&mut self, chunk_x: i32, chunk_z: i32) -> &mut ChunkAccess<T, B, S> {
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

    fn existing_block_entity_nbt(&self, pos: &BlockPos) -> Option<CompoundTag> {
        self.block_entity_nbts.get(pos).cloned().or_else(|| {
            let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
            let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
            self.try_get_chunk(chunk_x, chunk_z, ChunkStatus::Empty, false)
                .ok()
                .and_then(|chunk| chunk.get_block_entity_nbt(pos).cloned())
        })
    }

    fn persist_block_entity_nbt(&mut self, pos: &BlockPos, tag: CompoundTag) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        let chunk = self.get_chunk_mut(chunk_x, chunk_z);
        chunk.set_block_entity_nbt(tag.clone());
        self.block_entity_nbts.insert(*pos, tag);
    }

    fn remove_persisted_block_entity(&mut self, pos: &BlockPos) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        let chunk = self.get_chunk_mut(chunk_x, chunk_z);
        chunk.remove_block_entity_nbt(pos);
        self.block_entity_nbts.remove(pos);
    }

    /// `LevelReader.getNoiseBiome(int, int, int)` — read the cached BIOMES
    /// chunk selected by `QuartPos.toSection`. Java's `WorldGenRegion` does not
    /// fall back to `ServerLevel.getUncachedNoiseBiome` here: a missing or
    /// under-BIOMES chunk is a world-generation diagnostic.
    fn get_noise_biome_cached(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
    ) -> Result<B, UnavailableChunkDiagnostic> {
        let chunk_x = QuartPos::to_section(quart_x);
        let chunk_z = QuartPos::to_section(quart_z);
        self.try_get_chunk(chunk_x, chunk_z, ChunkStatus::Biomes, false)
            .map(|chunk| chunk.get_noise_biome(quart_x, quart_y, quart_z))
    }

    /// `WorldGenRegion.ensureCanWrite(BlockPos)` — the writability gate every
    /// write checks first.
    ///
    /// Type-agnostic (the write-radius gate reads only the scalar `writeRadius`
    /// and center coordinates), so it lives here for the dense and FEATURES-pass
    /// specializations to share. Inside the write zone the gate is open; Java's
    /// upgrade branch (`center.isUpgrading()` → the generation height-accessor
    /// check) never runs here because `BelowZeroRetrogen` is always null in the
    /// port, so `isUpgrading()` is always false (RivetTodo #185).
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
                self.generating_step.target_status().serialization_name()
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
                "Detected unsafe terrain read during worldgen: reading from chunk [{}, {}] while generating chunk [{}, {}] (distance: {}, write radius: {}, step: {})",
                chunk_x,
                chunk_z,
                self.center_chunk_x,
                self.center_chunk_z,
                read_distance,
                self.write_radius,
                self.generating_step.target_status().serialization_name()
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

/// The dense server specialization — the block-state methods and the
/// [`WorldGenLevel`] facade over the server's dense chunk value types.
///
/// Split from the generic impl because the block-state spine is
/// `StateId`-specific: the region's reads/writes target `StateId`/`ServerBiomeId`
/// sections. The [`WorldGenLevel`] trait is `Send`-bound but NOT `'static`
/// (see the trait doc), so the dense and FEATURES-pass specializations both
/// implement it over every region lifetime. The generic chunk-view methods the
/// executor's borrow-carrying region needs live on
/// [`impl<'a, T, B, S> WorldGenRegion<'a, T, B, S>`](WorldGenRegion).
impl WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> {
    fn materialize_block_entity(
        &mut self,
        pos: &BlockPos,
        state: BlockState,
        old_state: BlockState,
    ) {
        let same_block = old_state.block() == state.block();
        if state.block() != Blocks::CHEST.id() && state.block() != Blocks::SPAWNER.id() {
            self.block_entities.remove(pos);
            self.remove_persisted_block_entity(pos);
            return;
        }
        if !same_block {
            self.block_entities.remove(pos);
            self.remove_persisted_block_entity(pos);
        }
        let tag = self
            .existing_block_entity_nbt(pos)
            .unwrap_or_else(|| dummy_block_entity(pos));
        self.block_entity_nbts.insert(*pos, tag.clone());
        if state.block() == Blocks::CHEST.id() {
            let loot = tag
                .get_long("LootTableSeed")
                .zip(tag.get_string("LootTable").cloned());
            self.block_entities
                .insert(*pos, WorldgenBlockEntity::Chest { loot });
        } else {
            self.block_entities.insert(
                *pos,
                WorldgenBlockEntity::Spawner {
                    next_spawn: spawn_data_id(&tag),
                    spawn_potentials: decode_spawn_potentials(&tag),
                },
            );
        }
        self.persist_block_entity_nbt(pos, tag);
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
        let old_state = write_block(chunk, pos, block_state, persisted_status);
        self.materialize_block_entity(pos, block_state, BlockState::new(old_state));
        true
    }

    /// `WorldGenRegion.removeBlock(BlockPos, boolean)` —
    /// `setBlock(pos, Blocks.AIR.defaultBlockState(), Block.UPDATE_ALL)` with
    /// `Block.UPDATE_LIMIT` from `LevelWriter`'s three-argument overload.
    pub fn remove_block(&mut self, pos: &BlockPos, _moved_by_piston: bool) -> bool {
        self.set_block(pos, BlockState::new(StateId(0)), UPDATE_ALL, UPDATE_LIMIT)
    }

    /// `BlockGetter.getBlockState(BlockPos)` — the gated chunk block read.
    ///
    /// Inherent here (not only on the [`WorldGenLevel`] impl) because dense
    /// methods such as [`is_state_at_position`](Self::is_state_at_position)
    /// read it off a `&WorldGenRegion<'_, …>` whose region lifetime is not
    /// pinned; the trait impl (over every region lifetime, the trait is
    /// `Send`-bound but NOT `'static`) delegates back to this method.
    pub fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.warn_if_read_outside_write_zone(chunk_x, chunk_z);
        let chunk = self.get_chunk(chunk_x, chunk_z);
        chunk_block_state(chunk, pos)
    }
}

impl LevelHeightAccessor for WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> {
    fn get_height(&self) -> i32 {
        self.height
    }

    fn get_min_y(&self) -> i32 {
        self.min_y
    }
}

/// The `WorldGenLevel` facade over the dense specialization. The trait is
/// `Send`-bound but deliberately NOT `'static` (see the trait doc), so this
/// impl covers every region lifetime — the server value layer's owning-holder
/// `'static` region and the executor's borrow-carrying scoped region alike.
impl WorldGenLevel for WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> {
    /// `WorldGenLevel.getSeed()`.
    fn get_seed(&self) -> i64 {
        self.seed
    }

    /// `WorldGenLevel.ensureCanWrite(BlockPos)` — the write-radius gate.
    fn ensure_can_write(&self, pos: &BlockPos) -> bool {
        self.ensure_can_write(pos)
    }

    /// `LevelWriter.setBlock(BlockPos, BlockState, int)` — the 3-arg trait
    /// form, delegating to the 4-arg write with Java's `LevelWriter` default
    /// `updateLimit = Block.UPDATE_LIMIT`.
    ///
    /// `&mut self` is the trait's write contract; the delegated write is the
    /// region's [`set_block`](Self::set_block) (write-radius-gated chunk
    /// section write, with the `UPDATE_*`-gated side-effects deferred).
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        self.set_block(pos, state, flags as i32, UPDATE_LIMIT)
    }

    /// `LevelAccessor.destroyBlock(BlockPos, boolean)` — Java's chain
    /// `destroyBlock(pos, drop)` → `(pos, drop, null)` →
    /// `(pos, drop, null, UPDATE_LIMIT)` ends in
    /// `!getBlockState(pos).isAir() && setBlock(pos, AIR, UPDATE_ALL,
    /// updateLimit)` (WorldGenRegion.java:252). The `dropResources` flag is
    /// unread — the entity/`breakBlock` side-effects defer.
    fn destroy_block(&mut self, pos: &BlockPos, _drop: bool) -> bool {
        !self.get_block_state(pos).is_air()
            && self.set_block(pos, BlockState::new(StateId(0)), UPDATE_ALL, UPDATE_LIMIT)
    }

    /// `LevelReader.isEmptyBlock(BlockPos)` — `getBlockState(pos).isAir()`.
    fn is_empty_block(&self, pos: &BlockPos) -> bool {
        self.get_block_state(pos).is_air()
    }

    /// `WorldGenRegion.registryAccess()` — `level.registryAccess()`, the
    /// injected shared access (a cheap `Arc` clone; see the field doc).
    fn registry_access(&self) -> RegistryAccess {
        self.registry_access.clone()
    }

    fn is_randomizable_container(&self, pos: &BlockPos) -> bool {
        if matches!(
            self.block_entities.get(pos),
            Some(WorldgenBlockEntity::Chest { .. })
        ) {
            return true;
        }
        let Some(tag) = self.existing_block_entity_nbt(pos) else {
            return false;
        };
        tag.get_string("id")
            .is_some_and(|id| id == "minecraft:chest")
            || (tag.get_string("id").is_some_and(|id| id == "DUMMY")
                && self.get_block_state(pos).block() == Blocks::CHEST.id())
    }

    fn set_block_entity_loot_table(&mut self, pos: &BlockPos, seed: i64, loot_table: &str) {
        if !matches!(
            self.block_entities.get(pos),
            Some(WorldgenBlockEntity::Chest { .. })
        ) {
            let state = self.get_block_state(pos);
            if state.block() == Blocks::CHEST.id() {
                self.materialize_block_entity(pos, state, state);
            }
        }
        if let Some(WorldgenBlockEntity::Chest { loot }) = self.block_entities.get_mut(pos) {
            *loot = Some((seed, loot_table.to_string()));
            let mut tag = self
                .existing_block_entity_nbt(pos)
                .unwrap_or_else(|| dummy_block_entity(pos));
            tag.put_string("LootTable", loot_table);
            tag.put_long("LootTableSeed", seed);
            self.persist_block_entity_nbt(pos, tag);
        }
    }

    fn is_spawner_block_entity(&self, pos: &BlockPos) -> bool {
        if matches!(
            self.block_entities.get(pos),
            Some(WorldgenBlockEntity::Spawner { .. })
        ) {
            return true;
        }
        let Some(tag) = self.existing_block_entity_nbt(pos) else {
            return false;
        };
        tag.get_string("id")
            .is_some_and(|id| id == "minecraft:mob_spawner" || id == "DUMMY")
            && (tag
                .get_string("id")
                .is_some_and(|id| id == "minecraft:mob_spawner")
                || self.get_block_state(pos).block() == Blocks::SPAWNER.id())
    }

    fn spawner_potential_weight(&self, pos: &BlockPos) -> Option<i32> {
        let tag = self.existing_block_entity_nbt(pos);
        if let Some(WorldgenBlockEntity::Spawner {
            next_spawn,
            spawn_potentials,
        }) = self.block_entities.get(pos)
        {
            return spawn_potential_weight(next_spawn.as_ref(), spawn_potentials, tag.as_ref());
        }
        let tag = tag.as_ref()?;
        if spawn_data(tag).is_some() {
            return None;
        }
        match decode_spawn_potentials_state(tag) {
            SpawnPotentialsDecode::MissingOrInvalid => Some(1),
            SpawnPotentialsDecode::Present(potentials) => spawn_potential_total(&potentials),
        }
    }

    fn set_spawner_entity(&mut self, pos: &BlockPos, entity_id: &str, potential_roll: Option<i32>) {
        if !matches!(
            self.block_entities.get(pos),
            Some(WorldgenBlockEntity::Spawner { .. })
        ) {
            let state = self.get_block_state(pos);
            if state.block() == Blocks::SPAWNER.id() {
                self.materialize_block_entity(pos, state, state);
            }
        }
        let Some(WorldgenBlockEntity::Spawner {
            next_spawn,
            spawn_potentials,
        }) = self.block_entities.get_mut(pos)
        else {
            return;
        };
        if let Some(mut roll) = potential_roll {
            for (_, weight) in spawn_potentials.iter() {
                if roll < *weight {
                    break;
                }
                roll -= *weight;
            }
        }
        *next_spawn = Some(entity_id.to_string());
        spawn_potentials.clear();

        let mut tag = self
            .existing_block_entity_nbt(pos)
            .unwrap_or_else(|| dummy_block_entity(pos));
        if let Some(decoded) = spawn_data(&tag) {
            tag.put("SpawnData".to_string(), Tag::Compound(decoded));
        } else if let Some(roll) = potential_roll
            && let Some(selected) = selected_spawn_potential(&tag, roll)
        {
            tag.put("SpawnData".to_string(), Tag::Compound(selected));
        }
        let spawn_data = tag.get_compound_or_empty_mut("SpawnData");
        spawn_data
            .get_compound_or_empty_mut("entity")
            .put_string("id", entity_id);
        tag.put("SpawnPotentials".to_string(), Tag::List(ListTag::new()));
        self.persist_block_entity_nbt(pos, tag);
    }

    /// `ChunkAccess.markPosForPostProcessing(BlockPos)` — Java's private
    /// `markPosForPostProcessing` (WorldGenRegion.java:410):
    /// `this.getChunk(blockPos).markPosForPostProcessing(blockPos)`. The
    /// chunk-access hop the trait seam folds in is the `get_chunk` read here;
    /// the base `ChunkAccess` warns and no-ops (`ProtoChunk` overrides it).
    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.get_chunk(chunk_x, chunk_z)
            .mark_pos_for_post_processing(pos);
    }

    /// `BlockGetter.getBlockState(BlockPos)` — the gated chunk block read.
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.get_block_state(pos)
    }

    /// `LevelReader.getBiome(BlockPos)` — `BiomeManager(this,
    /// obfuscateSeed(seed)).getBiome(pos)`. Each corner must read the cached
    /// BIOMES chunk; unavailable corners propagate Java's diagnostic as a
    /// world-generation failure rather than using an uncached source.
    fn get_biome(&self, pos: &BlockPos) -> Holder<BiomeId> {
        self.biome_manager
            .get_biome_with(pos, |quart_x, quart_y, quart_z| {
                self.get_noise_biome_cached(quart_x, quart_y, quart_z)
                    .map(|biome| Holder::direct(BiomeId::from_id(biome.raw())))
                    .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
            })
    }

    /// `LevelReader.getHeight(Heightmap.Types, int, int)` — the gated heightmap
    /// read.
    ///
    /// Java's `WorldGenRegion.getHeight` (WorldGenRegion.java:514) is
    /// `getChunk(...).getHeight(type, x & 15, z & 15) + 1` — the same `+ 1`
    /// `Level.getHeight` applies (Level.java:1289) — i.e. the chunk's
    /// `getFirstAvailable` height, one ABOVE the topmost opaque block.
    /// [`Heightmap::get_height_at`] is the Java `ChunkAccess.getHeight` value
    /// (`getFirstAvailable(x, z) - 1` — the topmost opaque block's Y), so the
    /// port adds `+ 1` to recover the region method's contract.
    ///
    /// The mutable `ChunkAccess::get_height_at` call is intentional: Java
    /// `ChunkAccess.getHeight` primes a missing map in place, so a direct
    /// section write or an untouched dependency chunk is persisted before the
    /// value is returned. An opaque block at Y contributes Y + 1 to this
    /// region method; an all-air column contributes `minY`.
    fn get_height_at(&mut self, ty: Types, x: i32, z: i32) -> i32 {
        let chunk_x = SectionPos::block_to_section_coord(x);
        let chunk_z = SectionPos::block_to_section_coord(z);
        self.warn_if_read_outside_write_zone(chunk_x, chunk_z);
        let chunk = self.get_chunk_mut(chunk_x, chunk_z);
        chunk.get_height_at(ty, x, z) + 1
    }
}

/// The FEATURES-pass specialization — the block-state methods and the
/// [`WorldGenLevel`] facade over the executor's `BlockState`/`WorldgenBiomeId`
/// chunk value types.
///
/// The worldgen executor composes its 3x3 `ProtoChunk`s over `BlockState`
/// sections (the `section_reconstruction` value type) rather than the server's
/// dense `StateId`, so this specialization carries the same block-state surface
/// the dense one does: the gated read/write spine over
/// `ChunkAccess<BlockState, WorldgenBiomeId, StructureKey>` sections, and the
/// [`WorldGenLevel`] facade the feature placement stack runs against. The
/// type-agnostic gate (the generic [`ensure_can_write`]/
/// [`is_within_write_zone`]/`warn_if_read_outside_write_zone`) and the scalar
/// reads live on the generic impl and are shared.
impl WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> {
    fn materialize_block_entity(
        &mut self,
        pos: &BlockPos,
        state: BlockState,
        old_state: BlockState,
    ) {
        let same_block = old_state.block() == state.block();
        if state.block() != Blocks::CHEST.id() && state.block() != Blocks::SPAWNER.id() {
            self.block_entities.remove(pos);
            self.remove_persisted_block_entity(pos);
            return;
        }
        if !same_block {
            self.block_entities.remove(pos);
            self.remove_persisted_block_entity(pos);
        }
        let tag = self
            .existing_block_entity_nbt(pos)
            .unwrap_or_else(|| dummy_block_entity(pos));
        self.block_entity_nbts.insert(*pos, tag.clone());
        if state.block() == Blocks::CHEST.id() {
            let loot = tag
                .get_long("LootTableSeed")
                .zip(tag.get_string("LootTable").cloned());
            self.block_entities
                .insert(*pos, WorldgenBlockEntity::Chest { loot });
        } else {
            self.block_entities.insert(
                *pos,
                WorldgenBlockEntity::Spawner {
                    next_spawn: spawn_data_id(&tag),
                    spawn_potentials: decode_spawn_potentials(&tag),
                },
            );
        }
        self.persist_block_entity_nbt(pos, tag);
    }

    /// `WorldGenRegion.setBlock(BlockPos, BlockState, int updateFlags, int
    /// updateLimit)` — the write-radius-gated block write, mirroring the dense
    /// [`set_block`](WorldGenRegion::set_block) and materializing the live
    /// chest/spawner state the FEATURES features query after their writes.
    ///
    /// The `heightmapsAfter()` update reads the placed state's `StateFlags`
    /// through `resolve_state_flags` (the `BlockState`-typed resolver, the
    /// section_reconstruction analogue of the server's `state_flags`).
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
        // The persisted status threaded from the holder seam — see the dense
        // `set_block`.
        let persisted_status = self.cache.get(chunk_x, chunk_z).get_persisted_status();
        let chunk = self.get_chunk_mut(chunk_x, chunk_z);
        let old_state = write_block_blockstate(chunk, pos, block_state, persisted_status);
        self.materialize_block_entity(pos, block_state, old_state);
        true
    }

    /// `WorldGenRegion.removeBlock(BlockPos, boolean)` — the air-write form.
    pub fn remove_block(&mut self, pos: &BlockPos, _moved_by_piston: bool) -> bool {
        self.set_block(pos, BlockState::new(StateId(0)), UPDATE_ALL, UPDATE_LIMIT)
    }

    /// `BlockGetter.getBlockState(BlockPos)` — the gated chunk block read,
    /// inherent here for the trait impl to delegate (see the dense
    /// [`get_block_state`](WorldGenRegion::get_block_state)).
    pub fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.warn_if_read_outside_write_zone(chunk_x, chunk_z);
        let chunk = self.get_chunk(chunk_x, chunk_z);
        chunk_block_state_blockstate(chunk, pos)
    }
}

impl LevelHeightAccessor for WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> {
    fn get_height(&self) -> i32 {
        self.height
    }

    fn get_min_y(&self) -> i32 {
        self.min_y
    }
}

/// The `WorldGenLevel` facade over the FEATURES-pass specialization — the
/// block-state surface the feature placement stack runs against during
/// decoration. The trait is `Send`-bound but NOT `'static` (see the trait
/// doc), so this impl covers every region lifetime, and the executor's scoped
/// borrow-carrying region implements it directly.
impl WorldGenLevel for WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> {
    /// `WorldGenLevel.getSeed()`.
    fn get_seed(&self) -> i64 {
        self.seed
    }

    /// `WorldGenLevel.ensureCanWrite(BlockPos)` — the write-radius gate.
    fn ensure_can_write(&self, pos: &BlockPos) -> bool {
        self.ensure_can_write(pos)
    }

    /// `LevelWriter.setBlock(BlockPos, BlockState, int)` — the 3-arg trait
    /// form, delegating to the 4-arg write with `LevelWriter`'s default
    /// `updateLimit = Block.UPDATE_LIMIT`.
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        self.set_block(pos, state, flags as i32, UPDATE_LIMIT)
    }

    /// `LevelAccessor.destroyBlock(BlockPos, boolean)` — the
    /// `!getBlockState(pos).isAir() && setBlock(pos, AIR, UPDATE_ALL, ...)`
    /// chain (WorldGenRegion.java:252); the `drop` side-effects defer.
    fn destroy_block(&mut self, pos: &BlockPos, _drop: bool) -> bool {
        !self.get_block_state(pos).is_air()
            && self.set_block(pos, BlockState::new(StateId(0)), UPDATE_ALL, UPDATE_LIMIT)
    }

    /// `LevelReader.isEmptyBlock(BlockPos)` — `getBlockState(pos).isAir()`.
    fn is_empty_block(&self, pos: &BlockPos) -> bool {
        self.get_block_state(pos).is_air()
    }

    /// `WorldGenRegion.registryAccess()` — `level.registryAccess()`, the
    /// injected shared access (a cheap `Arc` clone).
    fn registry_access(&self) -> RegistryAccess {
        self.registry_access.clone()
    }

    fn is_randomizable_container(&self, pos: &BlockPos) -> bool {
        if matches!(
            self.block_entities.get(pos),
            Some(WorldgenBlockEntity::Chest { .. })
        ) {
            return true;
        }
        let Some(tag) = self.existing_block_entity_nbt(pos) else {
            return false;
        };
        tag.get_string("id")
            .is_some_and(|id| id == "minecraft:chest")
            || (tag.get_string("id").is_some_and(|id| id == "DUMMY")
                && self.get_block_state(pos).block() == Blocks::CHEST.id())
    }

    fn set_block_entity_loot_table(&mut self, pos: &BlockPos, seed: i64, loot_table: &str) {
        if !matches!(
            self.block_entities.get(pos),
            Some(WorldgenBlockEntity::Chest { .. })
        ) {
            let state = self.get_block_state(pos);
            if state.block() == Blocks::CHEST.id() {
                self.materialize_block_entity(pos, state, state);
            }
        }
        if let Some(WorldgenBlockEntity::Chest { loot }) = self.block_entities.get_mut(pos) {
            *loot = Some((seed, loot_table.to_string()));
            let mut tag = self
                .existing_block_entity_nbt(pos)
                .unwrap_or_else(|| dummy_block_entity(pos));
            tag.put_string("LootTable", loot_table);
            tag.put_long("LootTableSeed", seed);
            self.persist_block_entity_nbt(pos, tag);
        }
    }

    fn is_spawner_block_entity(&self, pos: &BlockPos) -> bool {
        if matches!(
            self.block_entities.get(pos),
            Some(WorldgenBlockEntity::Spawner { .. })
        ) {
            return true;
        }
        let Some(tag) = self.existing_block_entity_nbt(pos) else {
            return false;
        };
        tag.get_string("id")
            .is_some_and(|id| id == "minecraft:mob_spawner" || id == "DUMMY")
            && (tag
                .get_string("id")
                .is_some_and(|id| id == "minecraft:mob_spawner")
                || self.get_block_state(pos).block() == Blocks::SPAWNER.id())
    }

    fn spawner_potential_weight(&self, pos: &BlockPos) -> Option<i32> {
        let tag = self.existing_block_entity_nbt(pos);
        if let Some(WorldgenBlockEntity::Spawner {
            next_spawn,
            spawn_potentials,
        }) = self.block_entities.get(pos)
        {
            return spawn_potential_weight(next_spawn.as_ref(), spawn_potentials, tag.as_ref());
        }
        let tag = tag.as_ref()?;
        if spawn_data(tag).is_some() {
            return None;
        }
        match decode_spawn_potentials_state(tag) {
            SpawnPotentialsDecode::MissingOrInvalid => Some(1),
            SpawnPotentialsDecode::Present(potentials) => spawn_potential_total(&potentials),
        }
    }

    fn set_spawner_entity(&mut self, pos: &BlockPos, entity_id: &str, potential_roll: Option<i32>) {
        if !matches!(
            self.block_entities.get(pos),
            Some(WorldgenBlockEntity::Spawner { .. })
        ) {
            let state = self.get_block_state(pos);
            if state.block() == Blocks::SPAWNER.id() {
                self.materialize_block_entity(pos, state, state);
            }
        }
        let Some(WorldgenBlockEntity::Spawner {
            next_spawn,
            spawn_potentials,
        }) = self.block_entities.get_mut(pos)
        else {
            return;
        };
        if let Some(mut roll) = potential_roll {
            for (_, weight) in spawn_potentials.iter() {
                if roll < *weight {
                    break;
                }
                roll -= *weight;
            }
        }
        *next_spawn = Some(entity_id.to_string());
        spawn_potentials.clear();

        let mut tag = self
            .existing_block_entity_nbt(pos)
            .unwrap_or_else(|| dummy_block_entity(pos));
        if let Some(decoded) = spawn_data(&tag) {
            tag.put("SpawnData".to_string(), Tag::Compound(decoded));
        } else if let Some(roll) = potential_roll
            && let Some(selected) = selected_spawn_potential(&tag, roll)
        {
            tag.put("SpawnData".to_string(), Tag::Compound(selected));
        }
        let spawn_data = tag.get_compound_or_empty_mut("SpawnData");
        spawn_data
            .get_compound_or_empty_mut("entity")
            .put_string("id", entity_id);
        tag.put("SpawnPotentials".to_string(), Tag::List(ListTag::new()));
        self.persist_block_entity_nbt(pos, tag);
    }

    /// `ChunkAccess.markPosForPostProcessing(BlockPos)` — the chunk-access hop
    /// (`get_chunk` then `mark_pos_for_post_processing`).
    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.get_chunk(chunk_x, chunk_z)
            .mark_pos_for_post_processing(pos);
    }

    /// `BlockGetter.getBlockState(BlockPos)` — the gated chunk block read.
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.get_block_state(pos)
    }

    /// `LevelReader.getBiome(BlockPos)` — `BiomeManager(this,
    /// obfuscateSeed(seed)).getBiome(pos)`. Each corner must read the cached
    /// BIOMES chunk and propagates a missing-chunk diagnostic.
    fn get_biome(&self, pos: &BlockPos) -> Holder<BiomeId> {
        self.biome_manager
            .get_biome_with(pos, |quart_x, quart_y, quart_z| {
                self.get_noise_biome_cached(quart_x, quart_y, quart_z)
                    .map(|biome| Holder::direct(BiomeId::from_id(biome.0)))
                    .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
            })
    }

    /// `LevelReader.getHeight(Heightmap.Types, int, int)` — the gated heightmap
    /// read (`getChunk(...).getHeight(type, x & 15, z & 15) + 1`), mirroring
    /// the dense impl.
    fn get_height_at(&mut self, ty: Types, x: i32, z: i32) -> i32 {
        let chunk_x = SectionPos::block_to_section_coord(x);
        let chunk_z = SectionPos::block_to_section_coord(z);
        self.warn_if_read_outside_write_zone(chunk_x, chunk_z);
        let chunk = self.get_chunk_mut(chunk_x, chunk_z);
        chunk.get_height_at(ty, x, z) + 1
    }

    /// `WorldGenRegion.getSeaLevel()` — `level.getSeaLevel()`.
    fn get_sea_level(&self) -> i32 {
        self.sea_level
    }

    /// `LevelAccessor.scheduleTick(BlockPos, Block, int)` — a documented
    /// no-op. Java's `WorldGenRegion` inherits the `WorldGenLevel` default,
    /// whose body is empty (the real `scheduleTick` lives on the loaded
    /// `ServerLevel`'s tick containers, not the worldgen region); `LakeFeature`
    /// calls it to schedule the placed cave-air, and the scheduled tick is
    /// invisible to the chunk value the executor observes, so the no-op is the
    /// faithful value-layer surface.
    fn schedule_block_tick(
        &mut self,
        _pos: &BlockPos,
        _block: rivet_world::block::Block,
        _delay: i32,
    ) {
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

/// The region's block-state read over the FEATURES-pass `BlockState` value
/// type — air for an out-of-build-height or all-air position, else the section
/// storage read (which returns `BlockState` directly since `T = BlockState`).
fn chunk_block_state_blockstate(
    chunk: &ChunkAccess<BlockState, WorldgenBiomeId, StructureKey>,
    pos: &BlockPos,
) -> BlockState {
    let y = pos.get_y();
    if chunk.is_outside_build_height(y) {
        return BlockState::of(BlockId(794));
    }
    let section_index = chunk.get_section_index(y);
    let section = chunk.get_section(section_index as usize);
    if section.non_empty_block_count() == 0 {
        return BlockState::new(StateId(0));
    }
    section.get_block_state(pos.get_x() & 15, y & 15, pos.get_z() & 15)
}

/// The region's block-state write over the FEATURES-pass `BlockState` value
/// type — the section-level `setBlockState` with the `BlockState` behavior
/// predicates, then the `heightmapsAfter()` update. Mirrors the dense
/// [`write_block`] (Java `ProtoChunk.setBlockState` / `LevelChunk.setBlockState`),
/// with the placed state's `StateFlags` resolved through `resolve_state_flags`
/// (the `BlockState`-typed resolver).
fn write_block_blockstate(
    chunk: &mut ChunkAccess<BlockState, WorldgenBiomeId, StructureKey>,
    pos: &BlockPos,
    block_state: BlockState,
    persisted_status: Option<ChunkStatus>,
) -> BlockState {
    let y = pos.get_y();
    if chunk.is_outside_build_height(y) {
        return BlockState::new(StateId(0));
    }
    let section_index = chunk.get_section_index(y);
    let section = chunk.get_section_mut(section_index as usize);
    let old_state = section.set_block_state(
        pos.get_x() & 15,
        y & 15,
        pos.get_z() & 15,
        block_state,
        &state_is_air_blockstate,
        &state_is_randomly_ticking_blockstate,
        &fluid_is_empty_blockstate,
        &fluid_is_randomly_ticking_blockstate,
        &state_is_special_colliding_blockstate,
    );
    // Java `ProtoChunk.setBlockState`: `getPersistedStatus().heightmapsAfter()`
    // updated unconditionally for every in-build-height write.
    if let Some(status) = persisted_status {
        chunk.update_heightmaps_after(
            status.heightmaps_after(),
            pos.get_x() & 15,
            y,
            pos.get_z() & 15,
            resolve_state_flags(&block_state),
        );
    }
    old_state
}

/// `BlockBehaviour.isAir(state)` — over the `BlockState` value type.
fn state_is_air_blockstate(state: &BlockState) -> bool {
    state.is_air()
}

/// `BlockBehaviour.isRandomlyTicking(state)` — over the `BlockState` value type.
fn state_is_randomly_ticking_blockstate(state: &BlockState) -> bool {
    state.random_ticking()
}

/// `BlockBehaviour.getFluidState(state).isEmpty()` — over the `BlockState`
/// value type.
fn fluid_is_empty_blockstate(state: &BlockState) -> bool {
    state.fluid_empty()
}

/// `getFluidState().isRandomlyTicking()` — false (no fluid-random-tick flag in
/// the generated table; exact for the no-fluid value layer).
fn fluid_is_randomly_ticking_blockstate(_state: &BlockState) -> bool {
    false
}

/// `CollisionUtil.isSpecialCollidingBlock(state)` — false (no special-colliding
/// flag in the generated table; exact for the superflat content).
fn state_is_special_colliding_blockstate(_state: &BlockState) -> bool {
    false
}

/// A [`GenerationChunkHolderView`] that borrows a chunk — the worldgen
/// executor's center-chunk adapter.
///
/// The executor owns the generating `ProtoChunk` and hands its `&mut` to the
/// FEATURES body, which borrows it into a region through this adapter instead
/// of cloning or moving it. `status` is captured at construction: the concrete
/// `ProtoChunk` carries the persisted status the base `ChunkAccess` does not,
/// and the region reads it back from the holder seam.
pub struct CenterHolder<'a, T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    chunk: &'a mut ChunkAccess<T, B, S>,
    status: ChunkStatus,
}

impl<'a, T, B, S> CenterHolder<'a, T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    pub fn new(chunk: &'a mut ChunkAccess<T, B, S>, status: ChunkStatus) -> Self {
        CenterHolder { chunk, status }
    }
}

impl<T, B, S> GenerationChunkHolderView<T, B, S> for CenterHolder<'_, T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    fn get_chunk_if_present_unchecked(&self, status: ChunkStatus) -> Option<&ChunkAccess<T, B, S>> {
        self.status.is_or_after(status).then_some(&*self.chunk)
    }

    fn get_persisted_status(&self) -> Option<ChunkStatus> {
        Some(self.status)
    }

    fn get_chunk_if_present_unchecked_mut(
        &mut self,
        status: ChunkStatus,
    ) -> Option<&mut ChunkAccess<T, B, S>> {
        self.status.is_or_after(status).then_some(&mut *self.chunk)
    }
}

/// A [`GenerationChunkHolderView`] that owns a chunk — the worldgen executor's
/// ring-chunk adapter.
///
/// The executor generates the ring `ProtoChunk`s through CARVERS and moves each
/// base [`ChunkAccess`] in here (the region reads the base only; the concrete
/// chunk stays behind). `status` is captured at construction (see
/// [`CenterHolder`]).
pub struct OwnedHolder<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    chunk: ChunkAccess<T, B, S>,
    status: ChunkStatus,
}

impl<T, B, S> OwnedHolder<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    pub fn new(chunk: ChunkAccess<T, B, S>, status: ChunkStatus) -> Self {
        OwnedHolder { chunk, status }
    }
}

impl<T, B, S> GenerationChunkHolderView<T, B, S> for OwnedHolder<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    fn get_chunk_if_present_unchecked(&self, status: ChunkStatus) -> Option<&ChunkAccess<T, B, S>> {
        self.status.is_or_after(status).then_some(&self.chunk)
    }

    fn get_persisted_status(&self) -> Option<ChunkStatus> {
        Some(self.status)
    }

    fn get_chunk_if_present_unchecked_mut(
        &mut self,
        status: ChunkStatus,
    ) -> Option<&mut ChunkAccess<T, B, S>> {
        self.status.is_or_after(status).then_some(&mut self.chunk)
    }
}

/// A ring holder that preserves the concrete `ProtoChunk` alongside the base
/// view exposed to `WorldGenRegion`. The persisted status and heightmaps stay
/// on the same object the generation helpers produced; the region only borrows
/// its base for feature reads and writes.
pub struct OwnedProtoHolder<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    chunk: ProtoChunk<T, B, S>,
}

impl<T, B, S> OwnedProtoHolder<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    pub fn new(chunk: ProtoChunk<T, B, S>) -> Self {
        OwnedProtoHolder { chunk }
    }
}

impl<T, B, S> GenerationChunkHolderView<T, B, S> for OwnedProtoHolder<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    fn get_chunk_if_present_unchecked(&self, status: ChunkStatus) -> Option<&ChunkAccess<T, B, S>> {
        self.chunk
            .get_persisted_status()
            .is_or_after(status)
            .then_some(self.chunk.base())
    }

    fn get_persisted_status(&self) -> Option<ChunkStatus> {
        Some(self.chunk.get_persisted_status())
    }

    fn get_chunk_if_present_unchecked_mut(
        &mut self,
        status: ChunkStatus,
    ) -> Option<&mut ChunkAccess<T, B, S>> {
        self.chunk
            .get_persisted_status()
            .is_or_after(status)
            .then_some(self.chunk.base_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::core::QuartPos;
    use rivet_registry::root::AnyBox;
    use rivet_registry::{Identifier, ResourceKey};
    use rivet_util::RandomSource;
    use rivet_util::StaticCache2D;
    use rivet_util::random::LegacyRandomSource;
    use rivet_world::block::blocks::Blocks;
    use rivet_world::chunk::status::GENERATION_PYRAMID;
    use rivet_world::chunk::upgrade_data::UpgradeData;
    use rivet_world::level::height_accessor::create as create_accessor;
    use rivet_world::levelgen::feature::registry_keys::PLACED_FEATURE;
    use rivet_world::levelgen::heightmap::FINAL_HEIGHTMAPS;

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

    fn air_chunk(pos: ChunkPos) -> ChunkAccess<StateId, ServerBiomeId, StructureKey> {
        let height_accessor = create_accessor(SUPERFLAT_MIN_Y, SUPERFLAT_HEIGHT);
        ChunkAccess::new(
            pos,
            UpgradeData::empty(height_accessor.get_sections_count() as usize),
            height_accessor,
            &container_factory(),
            0,
            None,
            &|s: &StateId| rivet_world::levelgen::heightmap::StateFlags {
                is_air: s.0 == 0,
                blocks_motion: s.0 != 0,
                has_fluid: false,
                is_leaves: false,
            },
        )
    }

    fn primed_test_chunk(pos: ChunkPos) -> ChunkAccess<StateId, ServerBiomeId, StructureKey> {
        let mut chunk = test_chunk(pos);
        chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
        chunk
    }

    fn region_with_custom_chunks(
        center_chunk: ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        far_chunk: ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        source_biome: BiomeId,
    ) -> WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> {
        let step = GENERATION_PYRAMID
            .get_step_to(ChunkStatus::Features)
            .clone();
        let deps = step.direct_dependencies().as_list().to_vec();
        let mut center_chunk = Some(center_chunk);
        let mut far_chunk = Some(far_chunk);
        let mut holders = Vec::with_capacity(17 * 17);
        for x in -8..=8 {
            for z in -8..=8 {
                let distance = ChunkPos::new(0, 0).get_chessboard_distance_coords(x, z);
                let status = deps[distance.min(deps.len() as i32 - 1) as usize];
                let chunk = if x == 0 && z == 0 {
                    center_chunk.take().expect("center chunk is inserted once")
                } else if x == 8 && z == 0 {
                    far_chunk.take().expect("far chunk is inserted once")
                } else {
                    primed_test_chunk(ChunkPos::new(x, z))
                };
                holders.push(Box::new(OwnedHolder::new(chunk, status))
                    as Box<
                        dyn GenerationChunkHolderView<StateId, ServerBiomeId, StructureKey>,
                    >);
            }
        }
        WorldGenRegion::new(
            StaticCache2D::from_entries(-8, -8, 17, 17, holders),
            ChunkPos::new(0, 0),
            step,
            0,
            SUPERFLAT_MIN_Y,
            SUPERFLAT_HEIGHT,
            -63,
            Arc::new(TestBiomeSource {
                biome: source_biome,
            }),
            RegistryAccess::empty(),
        )
    }

    /// A test holder with no chunk held (Java `GenerationChunkHolder` whose
    /// `getPersistedStatus()` returns null) — exercises the in-ring diagnostic
    /// branch that has no actual status to report.
    struct TestEmptyHolder;

    impl GenerationChunkHolderView<StateId, ServerBiomeId, StructureKey> for TestEmptyHolder {
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

    /// A test noise source — a fixed biome at every quart.
    struct TestBiomeSource {
        biome: BiomeId,
    }

    impl NoiseBiomeSource for TestBiomeSource {
        fn get_noise_biome(&self, _quart_x: i32, _quart_y: i32, _quart_z: i32) -> Holder<BiomeId> {
            Holder::direct(self.biome)
        }
    }

    /// A feature-step region over the injected empty access — a `'static`
    /// owning-holder region (the [`WorldGenLevel`] shape).
    fn feature_region() -> WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> {
        region_with_access(RegistryAccess::empty())
    }

    /// A feature-step region: `generatingStep = getStepTo(FEATURES)` from the
    /// shared generation pyramid — `directDependencies = [CARVERS, CARVERS,
    /// STRUCTURE_STARTS x7]` (rings 0..8), write radius 1 — with every ring
    /// chunk present at its ring's allowed status. `cache` is a
    /// `2 * 8 + 1 = 17`-square centered on (0, 0), covering all nine rings.
    fn region_with_access(
        registry_access: RegistryAccess,
    ) -> WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> {
        let step = GENERATION_PYRAMID
            .get_step_to(ChunkStatus::Features)
            .clone();
        let deps = step.direct_dependencies().as_list().to_vec();
        let cache = StaticCache2D::create(0, 0, 8, &|x, z| {
            let distance = ChunkPos::new(0, 0).get_chessboard_distance_coords(x, z);
            let status = deps[distance.min(deps.len() as i32 - 1) as usize];
            Box::new(OwnedHolder::new(
                primed_test_chunk(ChunkPos::new(x, z)),
                status,
            ))
                as Box<dyn GenerationChunkHolderView<StateId, ServerBiomeId, StructureKey>>
        });
        WorldGenRegion::new(
            cache,
            ChunkPos::new(0, 0),
            step,
            0,
            SUPERFLAT_MIN_Y,
            SUPERFLAT_HEIGHT,
            -63,
            Arc::new(TestBiomeSource {
                biome: BiomeId::from_id(40),
            }),
            registry_access,
        )
    }

    /// A FEATURES region whose center chunk is supplied by the caller, so reads
    /// can distinguish the cached BIOMES value from the uncached source.
    fn region_with_center_chunk(
        center_chunk: ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        source_biome: BiomeId,
    ) -> WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> {
        let step = GENERATION_PYRAMID
            .get_step_to(ChunkStatus::Features)
            .clone();
        let deps = step.direct_dependencies().as_list().to_vec();
        let mut center_chunk = Some(center_chunk);
        let mut holders = Vec::with_capacity(17 * 17);
        for x in -8..=8 {
            for z in -8..=8 {
                let distance = ChunkPos::new(0, 0).get_chessboard_distance_coords(x, z);
                let status = deps[distance.min(deps.len() as i32 - 1) as usize];
                let chunk = if x == 0 && z == 0 {
                    center_chunk.take().expect("center chunk is inserted once")
                } else {
                    primed_test_chunk(ChunkPos::new(x, z))
                };
                holders.push(Box::new(OwnedHolder::new(chunk, status))
                    as Box<
                        dyn GenerationChunkHolderView<StateId, ServerBiomeId, StructureKey>,
                    >);
            }
        }
        WorldGenRegion::new(
            StaticCache2D::from_entries(-8, -8, 17, 17, holders),
            ChunkPos::new(0, 0),
            step,
            0,
            SUPERFLAT_MIN_Y,
            SUPERFLAT_HEIGHT,
            -63,
            Arc::new(TestBiomeSource {
                biome: source_biome,
            }),
            RegistryAccess::empty(),
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
    /// the ring's `CARVERS` allowed status returns it.
    #[test]
    fn center_ring_returns_the_chunk_for_allowed_status() {
        let region = feature_region();
        assert_eq!(
            region
                .try_get_chunk(0, 0, ChunkStatus::Empty, true)
                .expect("center at CARVERS serves EMPTY")
                .get_pos(),
            center()
        );
        assert_eq!(
            region
                .try_get_chunk(0, 0, ChunkStatus::Carvers, true)
                .expect("center at CARVERS serves CARVERS")
                .get_pos(),
            center()
        );
    }

    /// The per-ring allowed status: rings 0..1 allow `CARVERS`, rings 2..8
    /// allow `STRUCTURE_STARTS`; each returns the chunk for a target at or
    /// before the ring's status and diagnoses a target after it.
    #[test]
    fn per_ring_allowed_status_bounds_the_contract() {
        let region = feature_region();
        // Ring 1: CARVERS.
        assert!(
            region
                .try_get_chunk(1, 0, ChunkStatus::Carvers, true)
                .is_ok()
        );
        let diagnostic = region
            .try_get_chunk(1, 0, ChunkStatus::Features, true)
            .err()
            .expect("target after ring-1 CARVERS is unavailable");
        assert_eq!(diagnostic.distance, 1);
        assert_eq!(diagnostic.max_allowed_status, Some(ChunkStatus::Carvers));
        assert_eq!(diagnostic.requested_status, ChunkStatus::Features);

        // Ring 2: STRUCTURE_STARTS.
        assert!(
            region
                .try_get_chunk(2, 0, ChunkStatus::StructureStarts, true)
                .is_ok()
        );
        let diagnostic = region
            .try_get_chunk(2, 0, ChunkStatus::Carvers, true)
            .err()
            .expect("target after ring-2 STRUCTURE_STARTS is unavailable");
        assert_eq!(diagnostic.distance, 2);
        assert_eq!(
            diagnostic.max_allowed_status,
            Some(ChunkStatus::StructureStarts)
        );
        assert_eq!(diagnostic.requested_status, ChunkStatus::Carvers);

        // Ring 8: STRUCTURE_STARTS (chunk (8, 0) is chessboard distance 8).
        assert!(
            region
                .try_get_chunk(8, 0, ChunkStatus::StructureStarts, true)
                .is_ok()
        );
        assert_eq!(
            region
                .try_get_chunk(8, 0, ChunkStatus::StructureStarts, true)
                .expect("ring-8 STRUCTURE_STARTS serves STRUCTURE_STARTS")
                .get_pos(),
            ChunkPos::new(8, 0)
        );
    }

    /// The unavailable-chunk diagnostic: a request beyond the dependency list
    /// (distance 9, outside the 9-ring `[CARVERS, CARVERS, STRUCTURE_STARTS x7]`)
    /// yields `max_allowed_status = None` and the "out of cache bounds" actual
    /// status; a request at distance 1 for a status after the ring carries the
    /// full crash-report surface.
    #[test]
    fn unavailable_chunk_diagnostic_carries_the_request_details() {
        let region = feature_region();

        // Beyond the dependency list: the ring has no allowed status at all.
        let beyond = region
            .try_get_chunk(9, 0, ChunkStatus::Empty, true)
            .err()
            .expect("distance 9 is beyond the 9-ring dependency list");
        assert_eq!(beyond.chunk_x, 9);
        assert_eq!(beyond.chunk_z, 0);
        assert_eq!(beyond.distance, 9);
        assert_eq!(beyond.max_allowed_status, None);
        assert_eq!(beyond.actual_status, None);
        assert_eq!(beyond.generating_status, ChunkStatus::Features);
        assert_eq!(beyond.generating_chunk, center());
        assert_eq!(
            beyond.dependencies,
            vec![
                ChunkStatus::Carvers,
                ChunkStatus::Carvers,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
            ]
        );

        // A ring-1 request past the allowed status: actual status is the held
        // chunk's CARVERS, the max allowed is CARVERS.
        let too_far = region
            .try_get_chunk(1, 0, ChunkStatus::Full, true)
            .err()
            .expect("target after the ring-1 allowed status is unavailable");
        assert_eq!(too_far.actual_status, Some(ChunkStatus::Carvers));
        assert_eq!(too_far.max_allowed_status, Some(ChunkStatus::Carvers));
        assert_eq!(too_far.requested_status, ChunkStatus::Full);

        // The Display message mirrors the crash-report surface (generating
        // status, requested, actual, max allowed, dependencies, distance).
        let message = beyond.to_string();
        assert!(message.contains("Requested chunk unavailable during world generation"));
        assert!(message.contains("requesting chunk [9, 0]"));
        assert!(message.contains("distance: 9"));
        assert!(message.contains("generating status: minecraft:features"));
        assert!(message.contains("requested status: minecraft:empty"));
        assert!(message.contains("actual status: [out of cache bounds]"));
        assert!(message.contains("maximum allowed status: null"));
        assert!(
            message.contains(
                "minecraft:carvers, minecraft:carvers, minecraft:structure_starts, \
                 minecraft:structure_starts, minecraft:structure_starts, minecraft:structure_starts, \
                 minecraft:structure_starts, minecraft:structure_starts, minecraft:structure_starts"
            )
        );
    }

    /// An in-ring request whose holder holds no chunk yet renders "[no chunk
    /// held]" — the branch Java would NPE on (its `getPersistedStatus().getName()`
    /// supplier), rendered honestly instead of conflated with out-of-cache.
    #[test]
    fn in_ring_diagnostic_distinguishes_no_chunk_held_from_out_of_cache() {
        let step = GENERATION_PYRAMID
            .get_step_to(ChunkStatus::Features)
            .clone();
        let cache = StaticCache2D::create(0, 0, 1, &|_x, _z| {
            Box::new(TestEmptyHolder)
                as Box<dyn GenerationChunkHolderView<StateId, ServerBiomeId, StructureKey>>
        });
        let region = WorldGenRegion::new(
            cache,
            ChunkPos::new(0, 0),
            step,
            0,
            SUPERFLAT_MIN_Y,
            SUPERFLAT_HEIGHT,
            -63,
            Arc::new(TestBiomeSource {
                biome: BiomeId::from_id(40),
            }),
            RegistryAccess::empty(),
        );

        // Ring 1 (chunk (1,0)) allows CARVERS; the holder has no chunk, so a
        // request at the ring's allowed status fails with no actual status to
        // report.
        let diagnostic = region
            .try_get_chunk(1, 0, ChunkStatus::Carvers, true)
            .err()
            .expect("in-ring empty holder cannot serve the request");
        assert_eq!(diagnostic.max_allowed_status, Some(ChunkStatus::Carvers));
        assert_eq!(diagnostic.actual_status, None);
        let message = diagnostic.to_string();
        assert!(message.contains("actual status: [no chunk held]"));
        assert!(message.contains("maximum allowed status: minecraft:carvers"));
    }

    /// `hasChunk` is the same distance bound as the ring contract.
    #[test]
    fn has_chunk_matches_the_ring_bound() {
        let region = feature_region();
        for distance in 0..9 {
            // A chunk at (distance, 0) is within the 9-ring dependency list.
            assert!(region.has_chunk(distance, 0), "ring {distance} has a chunk");
        }
        assert!(!region.has_chunk(9, 0));
        assert!(!region.has_chunk(-9, 0));
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

    /// `setBlock` materializes the live block-entity surface used by feature
    /// placement, and replacing an entity block removes the old entity. A block
    /// state alone is never treated as a successful entity query.
    #[test]
    fn block_entity_writes_materialize_and_remove_live_entities() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);

        assert!(region.set_block(&pos, Blocks::CHEST.default_block_state(), UPDATE_ALL, 0,));
        assert!(matches!(
            region.block_entities.get(&pos),
            Some(WorldgenBlockEntity::Chest { .. })
        ));
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_randomizable_container(
            &region, &pos
        ));

        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0,));
        assert!(matches!(
            region.block_entities.get(&pos),
            Some(WorldgenBlockEntity::Spawner { .. })
        ));
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_spawner_block_entity(
            &region, &pos
        ));

        assert!(region.remove_block(&pos, false));
        assert!(!region.block_entities.contains_key(&pos));
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

    /// `getBiome` uses the region's fiddled-distance interpolation and the
    /// cached BIOMES chunks. The uncached source remains available only through
    /// the explicit `get_uncached_noise_biome` seam.
    #[test]
    fn get_biome_routes_through_cached_biomes() {
        let region = feature_region();
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

    #[test]
    #[should_panic(expected = "Requested chunk unavailable during world generation")]
    fn get_biome_propagates_out_of_cache_diagnostic() {
        let region = feature_region();
        region.get_biome(&BlockPos::new(152, 64, 0));
    }

    #[test]
    fn get_biome_prefers_cached_biomes_before_the_uncached_source() {
        let mut center = test_chunk(center());
        for section_index in 0..center.get_sections().len() {
            let section = center.get_section_mut(section_index);
            for quart_x in 0..4 {
                for quart_y in 0..4 {
                    for quart_z in 0..4 {
                        section.set_noise_biome(quart_x, quart_y, quart_z, ServerBiomeId(0));
                    }
                }
            }
        }
        let region = region_with_center_chunk(center, BiomeId::from_id(40));
        // At block (8,64,8) all eight fiddled corners are in the center chunk.
        // The cached id 0 therefore wins over the source's plains id 40.
        assert_eq!(
            region.get_biome(&BlockPos::new(8, 64, 8)),
            Holder::direct(BiomeId::from_id(0)),
        );
    }

    // -----------------------------------------------------------------------
    // Minimal WorldGenLevel facade
    // -----------------------------------------------------------------------

    /// The scalar facade values the region exposes.
    #[test]
    fn facade_exposes_the_scalar_level_values() {
        let mut region = feature_region();
        assert_eq!(region.get_seed(), 0);
        assert_eq!(region.get_min_y(), SUPERFLAT_MIN_Y);
        assert_eq!(region.get_height(), SUPERFLAT_HEIGHT);
        assert_eq!(region.get_sea_level(), -63);
        assert_eq!(region.get_sky_darken(), 0);
        assert!(!region.is_client_side());
        assert_eq!(region.get_center(), center());
        // `getHeight` of the superflat content: the center chunk is persisted
        // at CARVERS (the FEATURES step's ring-0 dependency). The region lazily
        // primes the missing `WorldSurface` map from the existing stone floor;
        // Java's first-available height is one above that floor at `minY`.
        assert_eq!(
            region.get_height_at(Types::WorldSurface, 0, 0),
            SUPERFLAT_MIN_Y + 1
        );
    }

    /// Block writes create live chest/spawner entities; entity queries do not
    /// infer them from a state id. The same test also pins the spawner's
    /// weighted-potential state transition used by `MonsterRoomFeature`.
    #[test]
    fn feature_block_writes_materialize_live_entities() {
        let mut region = feature_region();
        let chest_pos = BlockPos::new(0, 64, 0);
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::set_block(
            &mut region,
            &chest_pos,
            Blocks::CHEST.default_block_state(),
            2,
        ));
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_randomizable_container(
            &region, &chest_pos,
        ));

        let spawner_pos = BlockPos::new(1, 64, 0);
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::set_block(
            &mut region,
            &spawner_pos,
            Blocks::SPAWNER.default_block_state(),
            2,
        ));
        region.block_entities.insert(
            spawner_pos,
            WorldgenBlockEntity::Spawner {
                next_spawn: None,
                spawn_potentials: vec![("minecraft:zombie".to_string(), 2)],
            },
        );
        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region,
                &spawner_pos,
            ),
            Some(2)
        );
        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &spawner_pos,
            "minecraft:skeleton",
            Some(0),
        );
        assert_eq!(
            region.block_entities.get(&spawner_pos),
            Some(&WorldgenBlockEntity::Spawner {
                next_spawn: Some("minecraft:skeleton".to_string()),
                spawn_potentials: Vec::new(),
            })
        );
    }

    #[test]
    fn spawn_potentials_keep_valid_partial_entries_and_reject_invalid_entries() {
        let mut tag = CompoundTag::new();
        let mut list = ListTag::new();

        let mut valid = CompoundTag::new();
        valid.put_int("weight", 2);
        let mut valid_data = CompoundTag::new();
        let mut valid_entity = CompoundTag::new();
        valid_entity.put_string("id", "zombie");
        valid_data.put("entity".to_string(), Tag::Compound(valid_entity));
        valid.put("data".to_string(), Tag::Compound(valid_data));
        list.list.push(Tag::Compound(valid));

        let mut missing_weight = CompoundTag::new();
        let mut missing_weight_data = CompoundTag::new();
        missing_weight_data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
        missing_weight.put("data".to_string(), Tag::Compound(missing_weight_data));
        list.list.push(Tag::Compound(missing_weight));

        let mut negative = CompoundTag::new();
        negative.put_int("weight", -1);
        negative.put("data".to_string(), Tag::Compound(CompoundTag::new()));
        list.list.push(Tag::Compound(negative));

        let mut wrong_data = CompoundTag::new();
        wrong_data.put_int("weight", 4);
        wrong_data.put_string("data", "not a compound");
        list.list.push(Tag::Compound(wrong_data));

        let mut zero = CompoundTag::new();
        zero.put_int("weight", 0);
        let mut zero_data = CompoundTag::new();
        zero_data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
        zero.put("data".to_string(), Tag::Compound(zero_data));
        list.list.push(Tag::Compound(zero));

        tag.put("SpawnPotentials".to_string(), Tag::List(list));
        assert_eq!(
            decode_spawn_potentials(&tag),
            vec![("minecraft:zombie".to_string(), 2), (String::new(), 0)]
        );
        assert_eq!(
            spawn_potential_total(&decode_spawn_potentials(&tag)),
            Some(2)
        );
        assert_eq!(
            spawn_potential_total(&[(String::new(), 0)]),
            None,
            "a zero-total list has no selector and consumes no RNG"
        );
    }

    #[test]
    #[should_panic(expected = "Sum of weights must be <= 2147483647")]
    fn spawn_potential_weight_overflow_raises_like_paper() {
        spawn_potential_total(&[(String::new(), i32::MAX), (String::new(), 1)]);
    }

    #[test]
    fn idless_spawn_data_survives_materialization_without_a_potential_draw() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));

        let mut tag = dummy_block_entity(&pos);
        let mut data = CompoundTag::new();
        data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
        tag.put("SpawnData".to_string(), Tag::Compound(data));
        let mut potentials = ListTag::new();
        let mut potential = CompoundTag::new();
        potential.put_int("weight", 3);
        let mut potential_data = CompoundTag::new();
        potential_data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
        potential.put("data".to_string(), Tag::Compound(potential_data));
        potentials.list.push(Tag::Compound(potential));
        tag.put("SpawnPotentials".to_string(), Tag::List(potentials));
        region.persist_block_entity_nbt(&pos, tag);
        region.materialize_block_entity(
            &pos,
            Blocks::SPAWNER.default_block_state(),
            Blocks::SPAWNER.default_block_state(),
        );

        let mut random = LegacyRandomSource::new(42);
        let mut baseline = LegacyRandomSource::new(42);
        let roll = <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
            &region, &pos,
        )
        .map(|total| random.next_int_bound(total));
        assert_eq!(roll, None);
        assert_eq!(random.next_int_bound(100), baseline.next_int_bound(100));
    }

    #[test]
    fn missing_or_wrong_typed_potentials_use_paper_weight_one_fallback() {
        for wrong_typed_potentials in [false, true] {
            let mut region = feature_region();
            let pos = BlockPos::new(0, 64, 0);
            assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
            let mut tag = dummy_block_entity(&pos);
            let mut malformed_data = CompoundTag::new();
            malformed_data.put_string("entity", "wrong");
            tag.put("SpawnData".to_string(), Tag::Compound(malformed_data));
            if wrong_typed_potentials {
                tag.put_string("SpawnPotentials", "wrong");
            }
            region.persist_block_entity_nbt(&pos, tag);

            let mut random = LegacyRandomSource::new(42);
            let mut baseline = LegacyRandomSource::new(42);
            let total = <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos,
            )
            .expect("Paper's fallback list contains one default SpawnData");
            assert_eq!(total, 1);
            let _ = random.next_int_bound(total);
            assert_ne!(random.next_int_bound(100), baseline.next_int_bound(100));
        }
    }

    #[test]
    fn spawner_partial_spawn_data_controls_selection_and_save_state() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));

        let mut tag = dummy_block_entity(&pos);
        let mut data = CompoundTag::new();
        let mut entity = CompoundTag::new();
        entity.put_int("custom_entity_field", 7);
        data.put("entity".to_string(), Tag::Compound(entity));
        data.put_int("custom_spawn_rules", 1);
        data.put_string("equipment", "malformed");
        tag.put("SpawnData".to_string(), Tag::Compound(data));
        let mut potentials = ListTag::new();
        let mut potential = CompoundTag::new();
        potential.put_int("weight", 3);
        let mut potential_data = CompoundTag::new();
        let mut potential_entity = CompoundTag::new();
        potential_entity.put_string("id", "minecraft:skeleton");
        potential_data.put("entity".to_string(), Tag::Compound(potential_entity));
        potential_data.put_int("selected_field", 11);
        potential.put("data".to_string(), Tag::Compound(potential_data));
        potentials.list.push(Tag::Compound(potential));
        tag.put("SpawnPotentials".to_string(), Tag::List(potentials));
        region.persist_block_entity_nbt(&pos, tag);

        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos,
            ),
            None,
            "a present partial SpawnData must suppress potential selection even without an id"
        );
        let mut random = LegacyRandomSource::new(42);
        let mut baseline = LegacyRandomSource::new(42);
        let roll = <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
            &region, &pos,
        )
        .map(|total| random.next_int_bound(total));
        assert_eq!(
            roll, None,
            "partial SpawnData must not draw from potentials"
        );
        assert_eq!(random.next_int_bound(100), baseline.next_int_bound(100));

        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:zombie",
            Some(0),
        );
        let saved = region
            .existing_block_entity_nbt(&pos)
            .expect("spawner save state");
        let saved_data = saved
            .get_compound("SpawnData")
            .expect("partial SpawnData retained");
        assert!(saved_data.get("custom_spawn_rules").is_none());
        assert!(saved_data.get("equipment").is_none());
        assert_eq!(saved_data.get_int("selected_field"), None);
        assert_eq!(
            saved_data
                .get_compound("entity")
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:zombie")
        );
        assert_eq!(
            saved_data
                .get_compound("entity")
                .and_then(|entity| entity.get_int("custom_entity_field")),
            Some(7)
        );
        assert!(
            saved
                .get_list("SpawnPotentials")
                .is_some_and(ListTag::is_empty)
        );
    }

    #[test]
    fn nested_spawn_data_optionals_are_normalized_before_save() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));

        let mut tag = dummy_block_entity(&pos);
        let mut data = CompoundTag::new();
        data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
        let mut rules = CompoundTag::new();
        rules.put_string("block_light_limit", "malformed");
        data.put("custom_spawn_rules".to_string(), Tag::Compound(rules));
        data.put("equipment".to_string(), Tag::Compound(CompoundTag::new()));
        tag.put("SpawnData".to_string(), Tag::Compound(data));
        region.persist_block_entity_nbt(&pos, tag);

        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:zombie",
            None,
        );
        let saved = region
            .existing_block_entity_nbt(&pos)
            .expect("spawner save state");
        let saved_data = saved.get_compound("SpawnData").expect("SpawnData retained");
        assert_eq!(
            saved_data.get_compound("custom_spawn_rules"),
            Some(&CompoundTag::new())
        );
        assert!(saved_data.get("equipment").is_none());
    }

    #[test]
    fn empty_or_invalid_potentials_do_not_draw_rng() {
        let empty: Vec<(String, i32)> = Vec::new();
        assert_eq!(spawn_potential_total(&empty), None);
        assert_eq!(
            spawn_potential_total(&[(String::new(), -1), (String::new(), 0)]),
            None
        );
        let mut random = LegacyRandomSource::new(99);
        let mut baseline = LegacyRandomSource::new(99);
        let roll = spawn_potential_total(&empty).map(|total| random.next_int_bound(total));
        assert_eq!(roll, None);
        assert_eq!(random.next_int_bound(100), baseline.next_int_bound(100));
    }

    #[test]
    fn valid_potential_draw_consumes_one_rng_value_and_saves_selected_data() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let mut tag = dummy_block_entity(&pos);
        let mut potentials = ListTag::new();
        let mut potential = CompoundTag::new();
        potential.put_int("weight", 2);
        let mut data = CompoundTag::new();
        let mut entity = CompoundTag::new();
        entity.put_string("id", "minecraft:skeleton");
        data.put("entity".to_string(), Tag::Compound(entity));
        data.put_int("selected_field", 11);
        potential.put("data".to_string(), Tag::Compound(data));
        potentials.list.push(Tag::Compound(potential));
        tag.put("SpawnPotentials".to_string(), Tag::List(potentials));
        region.persist_block_entity_nbt(&pos, tag);

        let total = <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
            &region, &pos,
        )
        .expect("valid positive potential total");
        assert_eq!(total, 2);
        let mut random = LegacyRandomSource::new(1234);
        let mut baseline = LegacyRandomSource::new(1234);
        let roll = random.next_int_bound(total);
        assert_eq!(roll, baseline.next_int_bound(total));
        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:zombie",
            Some(roll),
        );
        assert_eq!(random.next_int_bound(100), baseline.next_int_bound(100));
        let saved = region
            .existing_block_entity_nbt(&pos)
            .expect("selected potential save state");
        assert_eq!(
            saved
                .get_compound("SpawnData")
                .unwrap()
                .get_int("selected_field"),
            Some(11)
        );
    }

    /// `set_block` primes and updates the `heightmapsAfter()` entries, so a
    /// written block moves the region `getHeight` to one above its Y — the
    /// `WorldGenRegion.getHeight` `+ 1` (Java `ProtoChunk.setBlockState` runs
    /// the heightmap update unconditionally after every in-build-height write).
    #[test]
    fn set_block_updates_the_worldgen_heightmap() {
        let mut region = feature_region();
        // Write stone at block (0, 0, 0) — chunk (0, 0), inside the write radius.
        let pos = BlockPos::new(0, 0, 0);
        assert!(region.set_block(&pos, BlockState::new(StateId(1)), UPDATE_ALL, 0));
        // The center chunk's persisted status is CARVERS (the FEATURES step's
        // ring-0 dependency), so `heightmaps_after()` returns the
        // FINAL_HEIGHTMAPS types. `getHeight` is first available — one above
        // the topmost block — so the written stone at 0 reads 1 (the floor at
        // -64 is below it).
        assert_eq!(region.get_height_at(Types::WorldSurface, 0, 0), 1);
        // `OceanFloor` (blocks-motion) tracks the same column.
        assert_eq!(region.get_height_at(Types::OceanFloor, 0, 0), 1);
        // The block itself reads back as non-air.
        assert!(!region.get_block_state(&pos).is_air());
        // A column that was never written still reads above the floor's topmost
        // block (`minY + 1`).
        assert_eq!(
            region.get_height_at(Types::WorldSurface, 15, 15),
            SUPERFLAT_MIN_Y + 1
        );
    }

    #[test]
    fn missing_heightmaps_are_lazily_primed_and_persisted_for_center_and_far_chunks() {
        let mut center = air_chunk(center());
        let section_index = center.get_section_index(0) as usize;
        center.get_section_mut(section_index).set_block_state(
            0,
            0,
            0,
            StateId(1),
            &state_is_air,
            &state_is_randomly_ticking,
            &fluid_is_empty,
            &fluid_is_randomly_ticking,
            &state_is_special_colliding,
        );
        let mut far = air_chunk(ChunkPos::new(8, 0));
        let far_section_index = far.get_section_index(100) as usize;
        far.get_section_mut(far_section_index).set_block_state(
            0,
            4,
            0,
            StateId(1),
            &state_is_air,
            &state_is_randomly_ticking,
            &fluid_is_empty,
            &fluid_is_randomly_ticking,
            &state_is_special_colliding,
        );
        let mut region = region_with_custom_chunks(center, far, BiomeId::from_id(40));

        assert_eq!(region.get_height_at(Types::WorldSurface, 0, 0), 1);
        assert_eq!(
            region.get_height_at(Types::WorldSurface, 15, 15),
            SUPERFLAT_MIN_Y
        );
        assert_eq!(region.get_height_at(Types::WorldSurface, 8 * 16, 0), 101);
        assert_eq!(
            region.get_height_at(Types::WorldSurface, 8 * 16 + 15, 15),
            SUPERFLAT_MIN_Y
        );

        assert!(
            region
                .try_get_chunk(0, 0, ChunkStatus::Empty, false)
                .unwrap()
                .has_primed_heightmap(Types::WorldSurface)
        );
        assert!(
            region
                .try_get_chunk(8, 0, ChunkStatus::Empty, false)
                .unwrap()
                .has_primed_heightmap(Types::WorldSurface)
        );
        assert_eq!(region.get_height_at(Types::WorldSurface, 0, 0), 1);
        assert_eq!(region.get_height_at(Types::WorldSurface, 8 * 16, 0), 101);
    }

    // -----------------------------------------------------------------------
    // The WorldGenLevel write/mark/registry seams this slice adds
    // -----------------------------------------------------------------------

    /// The 3-arg `WorldGenLevel::set_block` (the `LevelWriter` form) delegates
    /// to the region's 4-arg write with Java's `Block.UPDATE_LIMIT` default —
    /// a write inside the radius lands, outside is gated.
    ///
    /// The call uses the fully-qualified trait form: the region's inherent
    /// 4-arg `set_block` shadows the trait method by name.
    #[test]
    fn set_block_trait_form_delegates_with_the_level_writer_default() {
        let mut region = feature_region();
        let inside = BlockPos::new(2, 64, 3);
        assert!(
            <WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_block(
                &mut region,
                &inside,
                BlockState::new(StateId(1)),
                UPDATE_ALL as u32
            )
        );
        assert_eq!(
            region.get_block_state(&inside),
            BlockState::new(StateId(1)),
            "the 3-arg trait write landed inside the radius"
        );

        let outside = BlockPos::new(33, 64, 0); // chunk (2, 0), outside the write radius
        assert!(
            !<WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_block(
                &mut region,
                &outside,
                BlockState::new(StateId(1)),
                UPDATE_ALL as u32
            )
        );
        assert_eq!(
            region.get_block_state(&outside),
            BlockState::new(StateId(0)),
            "the gated trait write must not land"
        );
    }

    /// `destroyBlock` (WorldGenRegion.java:252) —
    /// `!getBlockState(pos).isAir() && setBlock(pos, AIR, UPDATE_ALL,
    /// updateLimit)`: a non-air cell is destroyed (reads air after), an
    /// already-air cell reports `false` and stays air.
    #[test]
    fn destroy_block_removes_a_non_air_cell_and_reports_false_for_air() {
        let mut region = feature_region();
        let pos = BlockPos::new(4, 64, 5);
        assert!(region.set_block(&pos, BlockState::new(StateId(1)), UPDATE_ALL, 0));
        assert!(!region.get_block_state(&pos).is_air());

        assert!(region.destroy_block(&pos, true));
        assert_eq!(region.get_block_state(&pos), BlockState::new(StateId(0)));

        // An already-air cell: `!isAir()` is false, so no write and `false`.
        assert!(!region.destroy_block(&pos, false));
        assert_eq!(region.get_block_state(&pos), BlockState::new(StateId(0)));
    }

    /// `isEmptyBlock` — `getBlockState(pos).isAir()`, the write-gated read: an
    /// untouched superflat cell is empty, a written cell is not.
    #[test]
    fn is_empty_block_reflects_the_written_state() {
        let mut region = feature_region();
        let pos = BlockPos::new(6, 64, 7);
        assert!(region.is_empty_block(&pos));
        assert!(region.set_block(&pos, BlockState::new(StateId(1)), UPDATE_ALL, 0));
        assert!(!region.is_empty_block(&pos));
    }

    /// `registryAccess()` — `level.registryAccess()`, the injected access: a
    /// region built over an access carrying the placed-feature registry
    /// resolves it, and the default empty access reports `None`.
    #[test]
    fn registry_access_resolves_the_injected_access() {
        let placed = RegistryBuilder::new(&*PLACED_FEATURE).freeze();
        let access = RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace(
                "worldgen/placed_feature",
            )),
            Box::new(placed) as AnyBox,
        )]);
        let region = region_with_access(access);
        assert!(
            region.registry_access().lookup(&*PLACED_FEATURE).is_some(),
            "the injected access resolves the placed-feature registry"
        );

        let empty = feature_region();
        assert!(
            empty.registry_access().lookup(&*PLACED_FEATURE).is_none(),
            "the default empty access does not resolve the registry"
        );
    }

    /// `markPosForPostProcessing` (WorldGenRegion.java:410) — the private
    /// method routes `this.getChunk(blockPos).markPosForPostProcessing(blockPos)`
    /// through the region's gated chunk read: an in-ring position is served
    /// (the base `ChunkAccess` warns and no-ops; `ProtoChunk` overrides it).
    #[test]
    fn mark_pos_for_post_processing_serves_an_in_ring_position() {
        let mut region = feature_region();
        // Chunk (0, 0) — inside the cache ring, so the read is served.
        region.mark_pos_for_post_processing(&BlockPos::new(8, 64, 9));
    }

    /// A position whose chunk is outside the cache ring fails loudly with the
    /// unavailable-chunk diagnostic — Java throws `ReportedException` from the
    /// same `getChunk` path.
    #[test]
    #[should_panic(expected = "Requested chunk unavailable during world generation")]
    fn mark_pos_for_post_processing_fails_loudly_outside_the_cache_ring() {
        let mut region = feature_region();
        // Block (200, 64, 0) → chunk (12, 0), distance 12 > the 8-ring cache.
        region.mark_pos_for_post_processing(&BlockPos::new(200, 64, 0));
    }
}
