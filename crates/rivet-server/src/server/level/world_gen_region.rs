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

use std::collections::HashSet;
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
use rivet_registry::block_state_properties::BlockStateProperties;
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
use rivet_util::worldgen_random::AlgorithmRandomSource;
use rivet_world::biome::biome::generated_climate_settings;
use rivet_world::biome::biome_manager::{BiomeManager, NoiseBiomeSource};
use rivet_world::biome::generated_biome_source::dense_biome_id;
use rivet_world::block::Block;
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::proto_chunk::ProtoChunk;
use rivet_world::chunk::status::{ChunkStatus, ChunkStep};
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::chunk::storage::section_reconstruction::BiomeId as WorldgenBiomeId;
use rivet_world::level::height_accessor::LevelHeightAccessor;
use rivet_world::level::{WorldBorder, WorldBorderSettings, WorldGenLevel};
use rivet_world::levelgen::heightmap::Types;
use rivet_world::ticks::{SavedTick, ScheduledTick};

use crate::server::level::level_chunk::{BiomeId as ServerBiomeId, StructureKey, state_flags};

fn is_beehive_state(state: BlockState) -> bool {
    matches!(
        state.block().name(),
        "minecraft:bee_nest" | "minecraft:beehive"
    )
}

/// `Block.UPDATE_ALL` — `UPDATE_NEIGHBORS | UPDATE_CLIENTS` (1 | 2), the flag
/// `removeBlock`/`destroyBlock` pass to `setBlock`.
const UPDATE_ALL: i32 = 3;

/// `Block.UPDATE_LIMIT` — the update-limit default `LevelWriter`'s 3-arg
/// `setBlock`/`destroyBlock` pass to the 4-arg form. The value-layer
/// `set_block` ignores it (the update machinery defers), so this is a faithful
/// default, not an operative limit.
const UPDATE_LIMIT: i32 = 512;

/// `Block.UPDATE_KNOWN_SHAPE` — suppresses the automatic post-processing mark
/// in `WorldGenRegion.setBlock` when the caller already knows the new shape.
const UPDATE_KNOWN_SHAPE: i32 = 16;

fn dummy_block_entity(pos: &BlockPos) -> CompoundTag {
    let mut tag = CompoundTag::new();
    tag.put_int("x", pos.get_x());
    tag.put_int("y", pos.get_y());
    tag.put_int("z", pos.get_z());
    tag.put_string("id", "DUMMY");
    tag
}

pub(super) fn structure_spawner_data(
    pos: BlockPos,
    input: &CompoundTag,
) -> Result<CompoundTag, rivet_world::levelgen::structure::StructureError> {
    use super::structure_snapshot_data::{common_data, fresh_snapshot};
    use rivet_world::levelgen::structure::StructureError;
    let state = BlockState::of(Blocks::SPAWNER.id());
    let mut result = fresh_snapshot(pos, state)?
        .block_entity
        .expect("fresh spawner");
    common_data(&mut result, input)?;
    let delay = input.get_int_or("Paper.Delay", i32::from(input.get_short_or("Delay", 20)));
    let min = input.get_int_or(
        "Paper.MinSpawnDelay",
        input.get_int_or("MinSpawnDelay", 200),
    );
    let max = input.get_int_or(
        "Paper.MaxSpawnDelay",
        input.get_int_or("MaxSpawnDelay", 800),
    );
    if delay > i32::from(i16::MAX) {
        result.put_int("Paper.Delay", delay);
    }
    result.put_short("Delay", delay.min(i32::from(i16::MAX)) as i16);
    if min > i32::from(i16::MAX) || max > i32::from(i16::MAX) {
        result.put_int("Paper.MinSpawnDelay", min);
        result.put_int("Paper.MaxSpawnDelay", max);
    }
    result.put_short("MinSpawnDelay", min.min(i32::from(i16::MAX)) as i16);
    result.put_short("MaxSpawnDelay", max.min(i32::from(i16::MAX)) as i16);
    for (key, default) in [
        ("SpawnCount", 4),
        ("MaxNearbyEntities", 6),
        ("RequiredPlayerRange", 16),
        ("SpawnRange", 4),
    ] {
        result.put_short(key, input.get_int_or(key, default) as i16);
    }
    let next = spawn_data(input);
    if let Some(data) = &next {
        result.put("SpawnData".into(), Tag::Compound(data.clone()));
    }
    let mut potentials = ListTag::new();
    let mut sum = 0i64;
    if let Some(list) = input.get_list("SpawnPotentials") {
        for entry in &list.list {
            let Tag::Compound(record) = entry else {
                continue;
            };
            let Some(weight) = record
                .get("weight")
                .and_then(Tag::as_number)
                .map(|n| n.int_value())
                .filter(|weight| *weight >= 0)
            else {
                continue;
            };
            let Some(data) = record.get_compound("data").and_then(decode_spawn_data) else {
                continue;
            };
            sum += i64::from(weight);
            if sum > i64::from(i32::MAX) {
                return Err(StructureError::PlacementUnavailable(
                    "spawner sum of weights must be <= 2147483647".into(),
                ));
            }
            let mut normalized = CompoundTag::new();
            normalized.put_int("weight", weight);
            normalized.put("data".into(), Tag::Compound(data));
            potentials.list.push(Tag::Compound(normalized));
        }
    } else {
        let mut data = next.unwrap_or_else(|| {
            let mut data = CompoundTag::new();
            data.put("entity".into(), Tag::Compound(CompoundTag::new()));
            data
        });
        // SpawnData's constructor normalizes absent/malformed entity ids.
        data = decode_spawn_data(&data).expect("default SpawnData entity compound");
        let mut entry = CompoundTag::new();
        entry.put_int("weight", 1);
        entry.put("data".into(), Tag::Compound(data));
        potentials.list.push(Tag::Compound(entry));
    }
    result.put("SpawnPotentials".into(), Tag::List(potentials));
    Ok(result)
}

fn is_dummy_block_entity(tag: &CompoundTag) -> bool {
    tag.get_string("id").is_some_and(|id| id == "DUMMY")
}

/// Decode the stored `SpawnData` compound through the partial shape of
/// `SpawnData.CODEC`. The required `entity` field must be a compound; malformed
/// optional fields are dropped from the retained partial value, unknown
/// top-level fields are dropped on re-encode, and valid equipment map entries
/// survive malformed siblings. The record constructor also canonicalizes a
/// valid id and removes an absent or malformed one.
fn decode_spawn_data(spawn_data: &CompoundTag) -> Option<CompoundTag> {
    let entity = spawn_data.get_compound("entity")?.clone();
    let mut decoded = CompoundTag::new();
    decoded.put("entity".to_string(), Tag::Compound(entity));
    if let Some(value) = spawn_data.get("custom_spawn_rules")
        && let Tag::Compound(rules) = value
        && let Ok(normalized) = normalize_custom_spawn_rules(rules)
    {
        decoded.put("custom_spawn_rules".to_string(), Tag::Compound(normalized));
    }
    if let Some(value) = spawn_data.get("equipment")
        && let Tag::Compound(equipment) = value
        && let Some(normalized) = normalize_equipment(equipment)
    {
        decoded.put("equipment".to_string(), Tag::Compound(normalized));
    }
    let Some(Tag::Compound(entity)) = decoded.tags.get_mut("entity") else {
        unreachable!("entity was cloned above")
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
            continue;
        }
        let Some(chance) = chance.as_float() else {
            continue;
        };
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

#[cfg(test)]
fn decode_spawn_potentials(tag: &CompoundTag) -> Vec<(String, i32)> {
    let potentials = match decode_spawn_potentials_state(tag) {
        SpawnPotentialsDecode::MissingOrInvalid => Vec::new(),
        SpawnPotentialsDecode::Present(potentials) => potentials,
    };
    spawn_potential_total(&potentials);
    potentials
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

    /// Consume an owned holder and recover its concrete `ProtoChunk`, when the
    /// holder preserves one. Borrowed and base-only holders return `None`.
    /// This is the ownership seam used to retain successful FEATURES writes to
    /// dependency chunks after the region cache is consumed.
    fn into_owned_proto(self: Box<Self>) -> Option<ProtoChunk<T, B, S>> {
        None
    }

    /// Concrete worldgen entity storage. A base-only ChunkAccess view cannot
    /// pretend to own ProtoChunk's entity list.
    fn get_proto_chunk_if_present_unchecked_mut(
        &mut self,
        _status: ChunkStatus,
    ) -> Option<&mut ProtoChunk<T, B, S>> {
        None
    }
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
    /// Paper's worldgen_region_random factory at the center chunk origin.
    /// Composition must supply the world's actual RandomState-derived stream.
    region_random: Option<AlgorithmRandomSource>,
    /// `level.getMinY()`.
    min_y: i32,
    /// `level.getHeight()`.
    height: i32,
    /// `level.getSeaLevel()`.
    sea_level: i32,
    /// `level.getWorldBorder()` — the generated region keeps the same live
    /// border representation used by `SpawnPlacementTypes.ON_GROUND`.
    /// Worldgen callers default to Paper's default border and may replace it
    /// when composing a region for a configured world.
    world_border: WorldBorder,
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
    /// `Level.getGameTime()` used by `LevelAccessor.createTick`.
    /// Generation has no live ServerLevel clock in this value layer, so it is
    /// explicitly initialized to zero and can be replaced by the owning level
    /// before scheduling.
    game_time: i64,
    /// Monotonic `Level.nextSubTickCount()` replacement for worldgen ticks.
    next_sub_tick_order: i64,
}

impl<T, B, S> WorldGenRegion<'_, T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
    Self: WorldGenLevel,
{
    pub fn structure_fresh_snapshot(
        &mut self,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<
        super::structure_transformer::CraftBlockSnapshot,
        rivet_world::levelgen::structure::StructureError,
    > {
        super::structure_snapshot_data::fresh_snapshot(pos, state)
    }

    pub fn structure_world_snapshot(
        &mut self,
        pos: BlockPos,
    ) -> Result<
        super::structure_transformer::CraftBlockSnapshot,
        rivet_world::levelgen::structure::StructureError,
    > {
        use super::{structure_snapshot_data, structure_transformer::CraftBlockSnapshot};
        use rivet_world::levelgen::structure::StructureError;
        let state = WorldGenLevel::get_block_state(self, &pos);
        if !structure_snapshot_data::has_block_entity(state) {
            return Ok(CraftBlockSnapshot {
                position: pos,
                state,
                block_entity: None,
            });
        }
        let pending = self.existing_block_entity_nbt(&pos).ok_or_else(|| {
            StructureError::PlacementUnavailable(format!(
                "Craft world snapshot at {pos:?} has no actual block entity"
            ))
        })?;
        let data = if is_dummy_block_entity(&pending) {
            structure_snapshot_data::fresh_snapshot(pos, state)?
                .block_entity
                .expect("supported entity snapshot")
        } else {
            match state.block().name() {
                "minecraft:chest" => structure_snapshot_data::chest_data(pos, &pending)?,
                "minecraft:spawner" => structure_spawner_data(pos, &pending)?,
                _ => {
                    return Err(StructureError::PlacementUnavailable(format!(
                        "Craft existing snapshot for {} is unavailable",
                        state.block().name()
                    )));
                }
            }
        };
        self.structure_load_snapshot(pos, &data)?;
        Ok(CraftBlockSnapshot {
            position: pos,
            state,
            block_entity: Some(data),
        })
    }

    pub fn structure_load_snapshot(
        &mut self,
        pos: BlockPos,
        data: &CompoundTag,
    ) -> Result<(), rivet_world::levelgen::structure::StructureError> {
        use super::structure_snapshot_data;
        use rivet_world::levelgen::structure::StructureError;
        let state = WorldGenLevel::get_block_state(self, &pos);
        if !structure_snapshot_data::has_block_entity(state)
            || self.existing_block_entity_nbt(&pos).is_none()
        {
            return Ok(());
        }
        let tag = match state.block().name() {
            "minecraft:chest" => structure_snapshot_data::chest_data(pos, data)?,
            "minecraft:spawner" => structure_spawner_data(pos, data)?,
            _ => {
                return Err(StructureError::PlacementUnavailable(format!(
                    "loadWithComponents for {} is unavailable",
                    state.block().name()
                )));
            }
        };
        self.persist_live_block_entity(&pos, state, &tag);
        Ok(())
    }
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
        Self::new_with_world_border_settings(
            cache,
            center_pos,
            generating_step,
            seed,
            min_y,
            height,
            sea_level,
            uncached_biome_source,
            registry_access,
            WorldBorderSettings::default_settings(),
        )
    }

    /// Construct a region with an owned stationary WorldBorder snapshot. The
    /// caller supplies value settings rather than a live border reference;
    /// normalize them here so an active lerp is never replayed from the
    /// value-layer's zero game time. World-aware callers must use this explicit
    /// constructor; [`Self::new`] is only the Paper-default convenience path.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_world_border_settings(
        cache: StaticCache2D<Box<dyn GenerationChunkHolderView<T, B, S> + 'a>>,
        center_pos: ChunkPos,
        generating_step: ChunkStep,
        seed: i64,
        min_y: i32,
        height: i32,
        sea_level: i32,
        uncached_biome_source: Arc<dyn NoiseBiomeSource>,
        registry_access: RegistryAccess,
        settings: WorldBorderSettings,
    ) -> Self {
        let write_radius = generating_step.block_state_write_radius();
        let biome_manager = BiomeManager::new(
            uncached_biome_source.clone(),
            BiomeManager::obfuscate_seed(seed),
        );
        let border_settings = settings.current_bounds_snapshot();
        let mut world_border = WorldBorder::new(border_settings);
        world_border.apply_initial_settings(0);
        WorldGenRegion {
            center_chunk_x: center_pos.x(),
            center_chunk_z: center_pos.z(),
            cache,
            center_pos,
            generating_step,
            write_radius,
            seed,
            region_random: None,
            min_y,
            height,
            sea_level,
            world_border,
            biome_manager,
            uncached_biome_source,
            registry_access,
            game_time: 0,
            next_sub_tick_order: 0,
        }
    }

    /// Set the world clock used by subsequent `createTick` calls.
    pub fn set_game_time(&mut self, game_time: i64) {
        self.game_time = game_time;
    }

    pub fn set_region_random(&mut self, random: AlgorithmRandomSource) {
        self.region_random = Some(random);
    }

    /// `WorldGenRegion.getCenter()`.
    pub fn get_center(&self) -> ChunkPos {
        self.center_pos
    }

    /// `Level.getWorldBorder()` for the generated region.
    pub fn world_border(&self) -> &WorldBorder {
        &self.world_border
    }

    /// Mutable world-border access for the owning worldgen composition path.
    pub fn world_border_mut(&mut self) -> &mut WorldBorder {
        &mut self.world_border
    }

    /// Consume the region and retain every concrete proto chunk in its cache.
    /// The cache is stored x-major/z-inner, and the returned entries preserve
    /// that canonical order. The borrowed center and any base-only holders are
    /// intentionally omitted.
    pub fn into_owned_proto_entries(self) -> Vec<(ChunkPos, ProtoChunk<T, B, S>)> {
        let radius = self.generating_step.direct_dependencies().size() as i32 - 1;
        let width = radius * 2 + 1;
        let center_pos = self.center_pos;
        self.cache
            .into_entries()
            .into_iter()
            .enumerate()
            .filter_map(|(index, holder)| {
                let dx = index as i32 / width - radius;
                let dz = index as i32 % width - radius;
                let pos = ChunkPos::new(
                    center_pos.x().wrapping_add(dx),
                    center_pos.z().wrapping_add(dz),
                );
                holder.into_owned_proto().map(|chunk| (pos, chunk))
            })
            .collect()
    }

    /// Consume the region and retain the concrete proto chunks in the
    /// FEATURES write zone's distance-1 dependency ring. The returned subset
    /// keeps the canonical x-major/z-inner order.
    pub fn into_distance_one_proto_writebacks(self) -> Vec<(ChunkPos, ProtoChunk<T, B, S>)> {
        let center_pos = self.center_pos;
        self.into_owned_proto_entries()
            .into_iter()
            .filter(|(pos, _)| center_pos.get_chessboard_distance_coords(pos.x(), pos.z()) == 1)
            .collect()
    }

    /// Block ticks owned by the cached chunks, in cache iteration order. This
    /// compatibility view exists only for value-layer tests; scheduling itself
    /// always routes to the chunk containing the scheduled position.
    pub fn scheduled_block_ticks(&self) -> Vec<SavedTick<Block>> {
        let mut ticks = Vec::new();
        self.cache.for_each(|holder| {
            if let Some(status) = holder.get_persisted_status()
                && let Some(chunk) = holder.get_chunk_if_present_unchecked(status)
            {
                ticks.extend(chunk.get_block_ticks().scheduled_ticks());
            }
        });
        ticks
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

    /// Actual WorldGenRegion.addFreshEntity -> ProtoChunk.addEntity path for
    /// the native, nonpassenger chest-minecart constructor slice. Admission is
    /// immediate, with no live ServerLevel entity-manager/event substitution.
    pub fn add_fresh_chest_minecart(
        &mut self,
        entity: rivet_world::entity::chest_minecart::NativeChestMinecart,
    ) -> Result<bool, rivet_world::levelgen::structure::StructureError> {
        let pos = entity.block_position();
        let chunk_x = pos.get_x() >> 4;
        let chunk_z = pos.get_z() >> 4;
        let chunk = self.structure_proto_chunk_mut(ChunkPos::new(chunk_x, chunk_z))?;
        // Source ProtoChunk deliberately ignores Entity.save's bool and still
        // appends the resulting (possibly empty, for discarded entities) tag.
        chunk.add_entity(entity.save().unwrap_or_default());
        Ok(true)
    }

    /// WorldGenRegion.addFreshEntity(...,CHUNK_GEN) directly saves into the
    /// actual ProtoChunk. The source does not install a live entity-manager
    /// entry or synthesize Bukkit/Paper spawn-reason fields on this path.
    /// NativeGenerationCreature currently has no passenger graph; other entity
    /// families must implement that graph before claiming this boundary.
    pub fn add_fresh_generation_creature(
        &mut self,
        entity: rivet_world::entity::generation_creature::NativeGenerationCreature,
    ) -> Result<bool, rivet_world::levelgen::structure::StructureError> {
        let pos = entity.entity().block_position();
        // ProtoChunk.addEntity ignores Entity.save's boolean and appends the
        // built output even when save refuses a removed entity (empty output).
        self.structure_proto_chunk_mut(ChunkPos::new(pos.get_x() >> 4, pos.get_z() >> 4))?
            .add_entity(entity.save().unwrap_or_default());
        Ok(true)
    }

    pub fn structure_chunk_entities_are_empty(
        &mut self,
        pos: ChunkPos,
    ) -> Result<bool, rivet_world::levelgen::structure::StructureError> {
        Ok(self
            .structure_proto_chunk_mut(pos)?
            .get_entities()
            .is_empty())
    }

    /// Source ProtoChunk entity-tag order, copied only for the current loader
    /// invocation. The region/owning chunk remains the sole persistent store.
    pub fn structure_chunk_entity_tags(
        &mut self,
        pos: ChunkPos,
    ) -> Result<Vec<CompoundTag>, rivet_world::levelgen::structure::StructureError> {
        Ok(self.structure_proto_chunk_mut(pos)?.get_entities().to_vec())
    }

    pub fn clear_structure_chunk_entities(
        &mut self,
        pos: ChunkPos,
    ) -> Result<(), rivet_world::levelgen::structure::StructureError> {
        self.structure_proto_chunk_mut(pos)?.clear_entities();
        Ok(())
    }

    fn structure_proto_chunk_mut(
        &mut self,
        pos: ChunkPos,
    ) -> Result<&mut ProtoChunk<T, B, S>, rivet_world::levelgen::structure::StructureError> {
        use rivet_world::levelgen::structure::StructureError;
        let chunk_x = pos.x();
        let chunk_z = pos.z();
        self.try_get_chunk(chunk_x, chunk_z, ChunkStatus::Empty, true)
            .map_err(|e| StructureError::PlacementUnavailable(e.to_string()))?;
        let distance = self
            .center_pos
            .get_chessboard_distance_coords(chunk_x, chunk_z);
        let required = self
            .generating_step
            .direct_dependencies()
            .get(distance as usize);
        self.cache
            .get_mut(chunk_x, chunk_z)
            .get_proto_chunk_if_present_unchecked_mut(required)
            .ok_or_else(|| {
                StructureError::PlacementUnavailable(format!(
                    "chunk [{chunk_x}, {chunk_z}] does not expose actual ProtoChunk entity storage"
                ))
            })
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

    /// `LevelAccessor.createTick` followed by `getChunk(pos).getBlockTicks()`.
    /// The helper is type-agnostic because every `ChunkAccess` owns its block
    /// tick container in this value model.
    fn schedule_block_tick_owner(&mut self, pos: &BlockPos, block: Block, delay: i32) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        let trigger_tick = self.game_time.wrapping_add(delay as i64);
        let sub_tick_order = self.next_sub_tick_order;
        self.next_sub_tick_order = self.next_sub_tick_order.wrapping_add(1);
        self.get_chunk_mut(chunk_x, chunk_z)
            .schedule_block_tick(ScheduledTick::new_normal(
                block,
                *pos,
                trigger_tick,
                sub_tick_order,
            ));
    }

    /// `LevelAccessor.scheduleTick(BlockPos, Fluid, int)` — route through the
    /// owning chunk's `ProtoChunkTicks`, matching the block-tick path above.
    fn schedule_fluid_tick_owner(&mut self, pos: &BlockPos, fluid: FluidId, delay: i32) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        let trigger_tick = self.game_time.wrapping_add(delay as i64);
        let sub_tick_order = self.next_sub_tick_order;
        self.next_sub_tick_order = self.next_sub_tick_order.wrapping_add(1);
        self.get_chunk_mut(chunk_x, chunk_z)
            .schedule_fluid_tick(ScheduledTick::new_normal(
                fluid,
                *pos,
                trigger_tick,
                sub_tick_order,
            ));
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
        let biome = self
            .uncached_biome_source
            .get_noise_biome(quart_x, quart_y, quart_z);
        self.biome_holder_for_id(dense_biome_id(&biome))
    }

    fn biome_holder_for_id(&self, id: u16) -> Holder<BiomeId> {
        if let Some(registry) = self
            .registry_access
            .lookup(&*rivet_registry::registries::BIOME)
        {
            let value = registry
                .by_id(i32::from(id))
                .expect("cached biome must exist in the world's BIOME registry");
            assert_eq!(
                value.id(),
                id,
                "world biome IDs must match generated palette IDs"
            );
            Holder::reference(registry.registry_id(), u32::from(id))
        } else {
            // Registry-less value-layer fixtures have no world owner to claim.
            Holder::direct(BiomeId::from_id(id))
        }
    }

    fn existing_block_entity_nbt(&self, pos: &BlockPos) -> Option<CompoundTag> {
        let chunk = self
            .try_get_chunk(
                SectionPos::block_to_section_coord(pos.get_x()),
                SectionPos::block_to_section_coord(pos.get_z()),
                ChunkStatus::Empty,
                false,
            )
            .ok()?;
        chunk
            .get_block_entity(pos)
            .map(|entity| entity.save_with_full_metadata())
            .or_else(|| chunk.get_block_entity_nbt(pos).cloned())
    }
    fn native_block_entity(
        &self,
        pos: &BlockPos,
    ) -> Option<&rivet_world::block_entity::NativeBlockEntity> {
        self.try_get_chunk(
            SectionPos::block_to_section_coord(pos.get_x()),
            SectionPos::block_to_section_coord(pos.get_z()),
            ChunkStatus::Empty,
            false,
        )
        .ok()?
        .get_block_entity(pos)
    }
    fn native_block_entity_mut(
        &mut self,
        pos: &BlockPos,
    ) -> Option<&mut rivet_world::block_entity::NativeBlockEntity> {
        self.get_chunk_mut(
            SectionPos::block_to_section_coord(pos.get_x()),
            SectionPos::block_to_section_coord(pos.get_z()),
        )
        .get_block_entity_mut(pos)
    }
    fn promote_native_block_entity(&mut self, pos: &BlockPos, state: BlockState) -> bool {
        use rivet_world::block_entity::NativeBlockEntity;
        if self.native_block_entity(pos).is_some() {
            return true;
        }
        let Some(tag) = self.existing_block_entity_nbt(pos) else {
            return false;
        };
        let entity = if is_dummy_block_entity(&tag) {
            Some(NativeBlockEntity::fresh(*pos, state).unwrap_or_else(|error| panic!("{error}")))
        } else {
            NativeBlockEntity::load(*pos, state, &tag).unwrap_or_else(|error| panic!("{error}"))
        };
        let Some(entity) = entity else { return false };
        self.get_chunk_mut(
            SectionPos::block_to_section_coord(pos.get_x()),
            SectionPos::block_to_section_coord(pos.get_z()),
        )
        .set_block_entity(entity);
        true
    }
    fn persist_live_block_entity(&mut self, pos: &BlockPos, state: BlockState, tag: &CompoundTag) {
        use rivet_world::block_entity::NativeBlockEntity;
        let mut normalized = tag.clone();
        if is_dummy_block_entity(tag) {
            let fresh =
                NativeBlockEntity::fresh(*pos, state).unwrap_or_else(|error| panic!("{error}"));
            normalized.put_string(
                "id",
                fresh.save_with_full_metadata().get_string("id").unwrap(),
            );
        }
        let entity = NativeBlockEntity::load(*pos, state, &normalized)
            .unwrap_or_else(|error| panic!("{error}"))
            .expect("matching native block entity");
        let chunk = self.get_chunk_mut(
            SectionPos::block_to_section_coord(pos.get_x()),
            SectionPos::block_to_section_coord(pos.get_z()),
        );
        if let Some(current) = chunk.get_block_entity_mut(pos) {
            *current.data_mut() = entity.data().clone();
        } else {
            chunk.set_block_entity(entity);
        }
    }
    fn materialize_native_block_entity(
        &mut self,
        pos: &BlockPos,
        state: BlockState,
        old_state: BlockState,
    ) {
        if state.block() != old_state.block() {
            self.remove_persisted_block_entity(pos);
        }
        if self.native_block_entity(pos).is_some() {
            return;
        }
        if matches!(
            state.block().name(),
            "minecraft:chest"
                | "minecraft:spawner"
                | "minecraft:beehive"
                | "minecraft:bee_nest"
                | "minecraft:creaking_heart"
        ) {
            self.persist_block_entity_nbt(pos, dummy_block_entity(pos));
        } else {
            self.remove_persisted_block_entity(pos);
        }
    }

    fn has_beehive_for_state(&self, pos: &BlockPos, state: BlockState) -> bool {
        if !is_beehive_state(state) {
            return false;
        }
        if self.native_block_entity(pos).is_some() {
            return true;
        }
        self.existing_block_entity_nbt(pos).is_some_and(|tag| {
            tag.get_string("id").is_some_and(|id| {
                id == "DUMMY"
                    || Identifier::try_parse_result(id)
                        .ok()
                        .flatten()
                        .is_some_and(|id| id == Identifier::with_default_namespace("beehive"))
            })
        })
    }

    fn append_beehive_occupant(&mut self, pos: &BlockPos, state: BlockState, ticks: i32) {
        if !self.load_beehive_for_state(pos, state) {
            return;
        }
        if let Some(entity) = self.native_block_entity_mut(pos)
            && let rivet_world::block_entity::NativeBlockEntityData::Beehive(hive) =
                entity.data_mut()
        {
            hive.store_worldgen_bee(ticks);
        }
    }

    fn load_beehive_for_state(&mut self, pos: &BlockPos, state: BlockState) -> bool {
        self.has_beehive_for_state(pos, state) && self.promote_native_block_entity(pos, state)
    }

    fn persist_block_entity_nbt(&mut self, pos: &BlockPos, tag: CompoundTag) {
        self.get_chunk_mut(
            SectionPos::block_to_section_coord(pos.get_x()),
            SectionPos::block_to_section_coord(pos.get_z()),
        )
        .set_block_entity_nbt(tag);
    }

    fn remove_persisted_block_entity(&mut self, pos: &BlockPos) {
        self.get_chunk_mut(
            SectionPos::block_to_section_coord(pos.get_x()),
            SectionPos::block_to_section_coord(pos.get_z()),
        )
        .remove_block_entity(pos);
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

    /// Paper's `WorldGenRegion.getBrightness` deliberately returns zero until
    /// the target chunk is light-correct. Afterwards its BLOCK listener reads
    /// the visible Starlight nibbles. Never substitute skylight or emission.
    pub fn get_block_brightness(&self, pos: &BlockPos) -> i32 {
        let chunk = self.get_chunk(pos.get_x() >> 4, pos.get_z() >> 4);
        if !chunk.is_light_correct() {
            return 0;
        }
        chunk.get_block_light_at(pos.get_x(), pos.get_y(), pos.get_z())
    }

    /// Read the completed LIGHT values carried by the cached chunk. Paper's
    /// `WorldGenRegion.getRawBrightness` delegates to the level light engine;
    /// this value layer has no engine object in the region, so the equivalent
    /// completed result is the visible Starlight nibble data published on the
    /// cached `ChunkAccess`. Paper's light engine resolves the holder at EMPTY;
    /// SPAWN neighbours may be only BIOMES while their completed light data is
    /// already published. A chunk that is not light-correct or has malformed
    /// light section storage is unavailable and returns `None`; the Starlight
    /// open-sky fallback remains full brightness for null sky data.
    pub fn get_raw_brightness(&self, pos: &BlockPos) -> Option<i32> {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        let chunk = self
            .try_get_chunk(chunk_x, chunk_z, ChunkStatus::Empty, false)
            .ok()?;
        if !chunk.is_light_correct() {
            return None;
        }
        let block = chunk.get_block_light_at(pos.get_x(), pos.get_y(), pos.get_z());
        let sky = chunk.get_sky_light_at(pos.get_x(), pos.get_y(), pos.get_z())?;
        Some(block.max(sky.saturating_sub(self.get_sky_darken())))
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
        self.materialize_native_block_entity(pos, state, old_state);
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
    /// the `oldState.hasBlockEntity()` removal, block-entity unit), defer with
    /// their owning units. The shape post-process mark is retained for the
    /// four vanilla states whose `getPostProcessPos` callback is non-null.
    /// The `updateLimit` is likewise unread by the ported surface.
    pub fn set_block(
        &mut self,
        pos: &BlockPos,
        block_state: BlockState,
        update_flags: i32,
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
        let post_process_pos = (update_flags & UPDATE_KNOWN_SHAPE == 0)
            .then(|| block_state.post_process_pos(pos))
            .flatten();
        let old_state = {
            let chunk = self.get_chunk_mut(chunk_x, chunk_z);
            // `oldState` — the previous state `chunk.setBlockState` returns; the
            // block-entity removal (`oldState.hasBlockEntity()`) and POI update
            // read it, so the write retains it for those deferred seams (#185).
            write_block(chunk, pos, block_state, persisted_status)
        };
        if let Some(post_process_pos) = post_process_pos {
            let post_process_chunk_x = SectionPos::block_to_section_coord(post_process_pos.get_x());
            let post_process_chunk_z = SectionPos::block_to_section_coord(post_process_pos.get_z());
            self.get_chunk_mut(post_process_chunk_x, post_process_chunk_z)
                .mark_pos_for_post_processing(&post_process_pos);
        }
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
pub(super) fn region_can_survive(
    level: &dyn WorldGenLevel,
    state: &BlockState,
    pos: &BlockPos,
) -> bool {
    if let Some(verdict) = super::aquatic_survival::aquatic_can_survive(level, state, pos) {
        return verdict;
    }
    match state.block().name() {
        "minecraft:torch" | "minecraft:soul_torch" | "minecraft:copper_torch" | "minecraft:redstone_torch" => {
            let below = pos.below();
            level.is_face_sturdy_with(&below, &level.get_block_state(&below),
                &rivet_registry::core::Direction::Up,
                rivet_world::level::block_shape_context::SupportType::Center)
                .unwrap_or_else(|error| panic!("torch support shape unavailable: {error}"))
        }
        "minecraft:wall_torch" | "minecraft:soul_wall_torch" | "minecraft:copper_wall_torch" | "minecraft:redstone_wall_torch" => {
            let Some(rivet_registry::block_state_property::PropertyValue::Enum(name)) =
                state.get_value(BlockStateProperties::HORIZONTAL_FACING) else { unreachable!("wall torch facing") };
            let facing = rivet_registry::core::Direction::VALUES.into_iter()
                .find(|direction| direction.get_serialized_name() == name).expect("wall torch direction");
            let support = pos.relative(&facing.get_opposite());
            level.is_face_sturdy(&support, &level.get_block_state(&support), &facing)
        }
        "minecraft:rail" | "minecraft:powered_rail" | "minecraft:detector_rail" | "minecraft:activator_rail" => {
            let below = pos.below();
            level.is_face_sturdy_with(&below, &level.get_block_state(&below),
                &rivet_registry::core::Direction::Up,
                rivet_world::level::block_shape_context::SupportType::Rigid)
                .unwrap_or_else(|error| panic!("rail support shape unavailable: {error}"))
        }
        "minecraft:small_dripleaf" => {
            let below = level.get_block_state(&pos.below());
            if state.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF)
                == Some(rivet_registry::block_state_property::PropertyValue::Enum("upper")) {
                below.block() == state.block() && below.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF)
                    == Some(rivet_registry::block_state_property::PropertyValue::Enum("lower"))
            } else {
                below.is_in_tag("minecraft:supports_small_dripleaf")
                    || (FluidId::from_id(level.get_block_state(pos).fluid_id()) == FluidId::WATER
                        && below.is_in_tag("minecraft:supports_vegetation"))
            }
        }
        "minecraft:big_dripleaf" => {
            let below = level.get_block_state(&pos.below());
            matches!(below.block().name(), "minecraft:big_dripleaf" | "minecraft:big_dripleaf_stem")
                || below.is_in_tag("minecraft:supports_big_dripleaf")
        }
        "minecraft:big_dripleaf_stem" => {
            let below = level.get_block_state(&pos.below());
            let above = level.get_block_state(&pos.above());
            (below.block() == state.block() || below.is_in_tag("minecraft:supports_big_dripleaf"))
                && matches!(above.block().name(), "minecraft:big_dripleaf_stem" | "minecraft:big_dripleaf")
        }
        "minecraft:mangrove_propagule" => {
            if state.get_value(BlockStateProperties::HANGING) == Some(rivet_registry::block_state_property::PropertyValue::Bool(true)) {
                level.get_block_state(&pos.above()).is_in_tag("minecraft:supports_hanging_mangrove_propagule")
            } else {
                level.get_block_state(&pos.below()).is_in_tag("minecraft:supports_mangrove_propagule")
            }
        }
        "minecraft:moss_carpet" => !level.get_block_state(&pos.below()).is_air(),
        "minecraft:pale_moss_carpet" => rivet_world::block::mossy_carpet_block::can_survive(level, *state, pos),
        "minecraft:pale_hanging_moss" => {
            let above_pos = pos.above();
            let above = level.get_block_state(&above_pos);
            level.can_attach_to(&above_pos, &above, &rivet_registry::core::Direction::Down)
                || above.block().name() == "minecraft:pale_hanging_moss"
        }
        "minecraft:tall_grass" | "minecraft:large_fern" => {
            let below = level.get_block_state(&pos.below());
            if state.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF)
                == Some(rivet_registry::block_state_property::PropertyValue::Enum("upper")) {
                below.block() == state.block() && below.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF)
                    == Some(rivet_registry::block_state_property::PropertyValue::Enum("lower"))
            } else { below.is_in_tag("minecraft:supports_vegetation") }
        }
        "minecraft:leaf_litter" => {
            let below_pos = pos.below();
            level.is_face_sturdy(&below_pos, &level.get_block_state(&below_pos), &rivet_registry::core::Direction::Up)
        }
        "minecraft:snow" => {
            let below_pos = pos.below();
            let below = level.get_block_state(&below_pos);
            if below.is_in_tag("minecraft:cannot_support_snow_layer") {
                false
            } else if below.is_in_tag("minecraft:support_override_snow_layer") {
                true
            } else {
                level.is_collision_face_full(
                    &below_pos,
                    &below,
                    &rivet_registry::core::Direction::Up,
                ) || (below.block() == Blocks::SNOW.id()
                    && below.get_value(BlockStateProperties::LAYERS)
                        == Some(rivet_registry::block_state_property::PropertyValue::Int(8)))
            }
        }
        "minecraft:short_grass"
        | "minecraft:fern"
        | "minecraft:firefly_bush"
        // Blocks.java constructs exactly these eight as plain SaplingBlock.
        // SaplingBlock inherits VegetationBlock.canSurvive/mayPlaceOn;
        // MangrovePropaguleBlock overrides both and is deliberately excluded.
        | "minecraft:oak_sapling"
        | "minecraft:spruce_sapling"
        | "minecraft:birch_sapling"
        | "minecraft:jungle_sapling"
        | "minecraft:acacia_sapling"
        | "minecraft:cherry_sapling"
        | "minecraft:dark_oak_sapling"
        | "minecraft:pale_oak_sapling" => level
            .get_block_state(&pos.below())
            .is_in_tag("minecraft:supports_vegetation"),
        other => {
            panic!("WorldGenRegion.canSurvive is not implemented for {other} (RivetTodo #232)")
        }
    }
}

impl WorldGenLevel for WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> {
    fn place_mossy_carpet(&mut self, pos: &BlockPos, flags: u32) {
        let mut random = self
            .region_random
            .take()
            .expect("moss carpet requires region RNG");
        rivet_world::block::mossy_carpet_block::place_at(self, pos, &mut random, flags);
        self.region_random = Some(random);
    }
    fn has_beehive_block_entity(&mut self, pos: &BlockPos) -> bool {
        self.load_beehive_for_state(pos, self.get_block_state(pos))
    }

    fn store_beehive_occupant(&mut self, pos: &BlockPos, ticks: i32) {
        self.append_beehive_occupant(pos, self.get_block_state(pos), ticks);
    }
    fn update_tree_shape_at_edge(&mut self, filled: &HashSet<BlockPos>) {
        let mut random = self
            .region_random
            .take()
            .expect("WorldGenRegion updateShape requires its RandomState-derived region RNG");
        super::block_shape_update::update_shape_at_edge(self, &mut random, filled);
        self.region_random = Some(random);
    }

    /// `WorldGenLevel.getSeed()`.
    fn get_seed(&self) -> i64 {
        self.seed
    }

    fn get_sea_level(&self) -> i32 {
        self.sea_level
    }

    fn get_block_brightness(&self, pos: &BlockPos) -> i32 {
        self.get_block_brightness(pos)
    }

    fn should_freeze(&self, freeze_pos: &BlockPos, check_neighbors: bool) -> bool {
        let biome = self.get_biome(&freeze_pos.above());
        generated_climate_settings(BiomeId::from_id(dense_biome_id(&biome))).should_freeze(
            self,
            freeze_pos,
            check_neighbors,
        )
    }

    fn should_snow(&self, pos: &BlockPos) -> bool {
        let biome = self.get_biome(pos);
        generated_climate_settings(BiomeId::from_id(dense_biome_id(&biome))).should_snow(self, pos)
    }

    fn can_survive(&self, state: &BlockState, pos: &BlockPos) -> bool {
        region_can_survive(self, state, pos)
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
            self.native_block_entity(pos).map(|entity| entity.data()),
            Some(rivet_world::block_entity::NativeBlockEntityData::Chest(_))
        ) {
            return true;
        }
        let Some(tag) = self.existing_block_entity_nbt(pos) else {
            return false;
        };
        tag.get_string("id").is_some_and(|id| {
            id == "minecraft:chest"
                || (id == "DUMMY" && self.get_block_state(pos).block() == Blocks::CHEST.id())
        })
    }

    fn set_block_entity_loot_table(&mut self, pos: &BlockPos, seed: i64, loot_table: &str) {
        let state = self.get_block_state(pos);
        if state.block() != Blocks::CHEST.id() || !self.promote_native_block_entity(pos, state) {
            return;
        }
        let mut tag = self.existing_block_entity_nbt(pos).expect("live chest");
        tag.put_string("LootTable", loot_table);
        tag.put_long("LootTableSeed", seed);
        self.persist_live_block_entity(pos, state, &tag);
    }

    fn is_spawner_block_entity(&self, pos: &BlockPos) -> bool {
        if matches!(
            self.native_block_entity(pos).map(|entity| entity.data()),
            Some(rivet_world::block_entity::NativeBlockEntityData::Spawner(_))
        ) {
            return true;
        }
        let Some(tag) = self.existing_block_entity_nbt(pos) else {
            return false;
        };
        tag.get_string("id").is_some_and(|id| {
            id == "minecraft:mob_spawner"
                || (id == "DUMMY" && self.get_block_state(pos).block() == Blocks::SPAWNER.id())
        })
    }

    fn spawner_potential_weight(&self, pos: &BlockPos) -> Option<i32> {
        let tag = self.existing_block_entity_nbt(pos)?;
        if spawn_data(&tag).is_some() || is_dummy_block_entity(&tag) {
            return None;
        }
        match decode_spawn_potentials_state(&tag) {
            SpawnPotentialsDecode::MissingOrInvalid => Some(1),
            SpawnPotentialsDecode::Present(potentials) => spawn_potential_total(&potentials),
        }
    }

    fn set_spawner_entity(&mut self, pos: &BlockPos, entity_id: &str, potential_roll: Option<i32>) {
        let state = self.get_block_state(pos);
        if state.block() != Blocks::SPAWNER.id() || !self.promote_native_block_entity(pos, state) {
            return;
        }
        if let Some(entity) = self.native_block_entity_mut(pos)
            && let rivet_world::block_entity::NativeBlockEntityData::Spawner(data) =
                entity.data_mut()
        {
            data.set_entity_id(&Identifier::parse(entity_id), potential_roll)
                .unwrap_or_else(|error| panic!("{error}"));
        }
    }

    /// `ChunkAccess.markPosForPostProcessing(BlockPos)` — Java's private
    /// `markPosForPostProcessing` (WorldGenRegion.java:410):
    /// `this.getChunk(blockPos).markPosForPostProcessing(blockPos)`. The
    /// chunk-access hop the trait seam folds in is the `get_chunk` read here;
    /// the base `ChunkAccess` warns and no-ops (`ProtoChunk` overrides it).
    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.get_chunk_mut(chunk_x, chunk_z)
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
                    .map(|biome| self.biome_holder_for_id(biome.raw()))
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

    /// `LevelAccessor.scheduleTick(BlockPos, Fluid, int)` — route through the
    /// owning chunk's `ProtoChunkTicks`.
    fn schedule_tick(&mut self, pos: &BlockPos, fluid: FluidId, delay: i32) {
        self.schedule_fluid_tick_owner(pos, fluid, delay);
    }

    /// `LevelAccessor.scheduleTick(BlockPos, Block, int)` — route through the
    /// owning chunk's `ProtoChunkTicks`.
    fn schedule_block_tick(
        &mut self,
        pos: &BlockPos,
        block: rivet_world::block::Block,
        delay: i32,
    ) {
        self.schedule_block_tick_owner(pos, block, delay);
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
        self.materialize_native_block_entity(pos, state, old_state);
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
        update_flags: i32,
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
        let post_process_pos = (update_flags & UPDATE_KNOWN_SHAPE == 0)
            .then(|| block_state.post_process_pos(pos))
            .flatten();
        let old_state = {
            let chunk = self.get_chunk_mut(chunk_x, chunk_z);
            write_block_blockstate(chunk, pos, block_state, persisted_status)
        };
        if let Some(post_process_pos) = post_process_pos {
            let post_process_chunk_x = SectionPos::block_to_section_coord(post_process_pos.get_x());
            let post_process_chunk_z = SectionPos::block_to_section_coord(post_process_pos.get_z());
            self.get_chunk_mut(post_process_chunk_x, post_process_chunk_z)
                .mark_pos_for_post_processing(&post_process_pos);
        }
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
    fn place_mossy_carpet(&mut self, pos: &BlockPos, flags: u32) {
        let mut random = self
            .region_random
            .take()
            .expect("moss carpet requires region RNG");
        rivet_world::block::mossy_carpet_block::place_at(self, pos, &mut random, flags);
        self.region_random = Some(random);
    }
    fn has_beehive_block_entity(&mut self, pos: &BlockPos) -> bool {
        self.load_beehive_for_state(pos, self.get_block_state(pos))
    }

    fn store_beehive_occupant(&mut self, pos: &BlockPos, ticks: i32) {
        self.append_beehive_occupant(pos, self.get_block_state(pos), ticks);
    }
    fn update_tree_shape_at_edge(&mut self, filled: &HashSet<BlockPos>) {
        let mut random = self
            .region_random
            .take()
            .expect("WorldGenRegion updateShape requires its RandomState-derived region RNG");
        super::block_shape_update::update_shape_at_edge(self, &mut random, filled);
        self.region_random = Some(random);
    }

    /// `WorldGenLevel.getSeed()`.
    fn get_seed(&self) -> i64 {
        self.seed
    }

    fn get_block_brightness(&self, pos: &BlockPos) -> i32 {
        self.get_block_brightness(pos)
    }

    fn should_freeze(&self, freeze_pos: &BlockPos, check_neighbors: bool) -> bool {
        let biome = self.get_biome(&freeze_pos.above());
        generated_climate_settings(BiomeId::from_id(dense_biome_id(&biome))).should_freeze(
            self,
            freeze_pos,
            check_neighbors,
        )
    }

    fn should_snow(&self, pos: &BlockPos) -> bool {
        let biome = self.get_biome(pos);
        generated_climate_settings(BiomeId::from_id(dense_biome_id(&biome))).should_snow(self, pos)
    }

    /// `BlockBehaviour.BlockStateBase.canSurvive` for the vegetation states
    /// reached by the seed-42 FEATURES slice, including the stage-0 dark oak
    /// sapling used by `dark_oak_leaf_litter`. Paper's `VegetationBlock` checks
    /// only the block below against `BlockTags.SUPPORTS_VEGETATION`; keeping
    /// this as a world-backed read (rather than a state-only shortcut) is
    /// important because the feature's random offset can land across chunks.
    fn can_survive(&self, state: &BlockState, pos: &BlockPos) -> bool {
        region_can_survive(self, state, pos)
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
            self.native_block_entity(pos).map(|entity| entity.data()),
            Some(rivet_world::block_entity::NativeBlockEntityData::Chest(_))
        ) {
            return true;
        }
        let Some(tag) = self.existing_block_entity_nbt(pos) else {
            return false;
        };
        tag.get_string("id").is_some_and(|id| {
            id == "minecraft:chest"
                || (id == "DUMMY" && self.get_block_state(pos).block() == Blocks::CHEST.id())
        })
    }

    fn set_block_entity_loot_table(&mut self, pos: &BlockPos, seed: i64, loot_table: &str) {
        let state = self.get_block_state(pos);
        if state.block() != Blocks::CHEST.id() || !self.promote_native_block_entity(pos, state) {
            return;
        }
        let mut tag = self.existing_block_entity_nbt(pos).expect("live chest");
        tag.put_string("LootTable", loot_table);
        tag.put_long("LootTableSeed", seed);
        self.persist_live_block_entity(pos, state, &tag);
    }

    fn is_spawner_block_entity(&self, pos: &BlockPos) -> bool {
        if matches!(
            self.native_block_entity(pos).map(|entity| entity.data()),
            Some(rivet_world::block_entity::NativeBlockEntityData::Spawner(_))
        ) {
            return true;
        }
        let Some(tag) = self.existing_block_entity_nbt(pos) else {
            return false;
        };
        tag.get_string("id").is_some_and(|id| {
            id == "minecraft:mob_spawner"
                || (id == "DUMMY" && self.get_block_state(pos).block() == Blocks::SPAWNER.id())
        })
    }

    fn spawner_potential_weight(&self, pos: &BlockPos) -> Option<i32> {
        let tag = self.existing_block_entity_nbt(pos)?;
        if spawn_data(&tag).is_some() || is_dummy_block_entity(&tag) {
            return None;
        }
        match decode_spawn_potentials_state(&tag) {
            SpawnPotentialsDecode::MissingOrInvalid => Some(1),
            SpawnPotentialsDecode::Present(potentials) => spawn_potential_total(&potentials),
        }
    }

    fn set_spawner_entity(&mut self, pos: &BlockPos, entity_id: &str, potential_roll: Option<i32>) {
        let state = self.get_block_state(pos);
        if state.block() != Blocks::SPAWNER.id() || !self.promote_native_block_entity(pos, state) {
            return;
        }
        if let Some(entity) = self.native_block_entity_mut(pos)
            && let rivet_world::block_entity::NativeBlockEntityData::Spawner(data) =
                entity.data_mut()
        {
            data.set_entity_id(&Identifier::parse(entity_id), potential_roll)
                .unwrap_or_else(|error| panic!("{error}"));
        }
    }

    /// `ChunkAccess.markPosForPostProcessing(BlockPos)` — the chunk-access hop
    /// (`get_chunk` then `mark_pos_for_post_processing`).
    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.get_chunk_mut(chunk_x, chunk_z)
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
                    .map(|biome| self.biome_holder_for_id(biome.0))
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

    /// `LevelAccessor.scheduleTick(BlockPos, Fluid, int)` — route through the
    /// owning chunk's `ProtoChunkTicks`.
    fn schedule_tick(&mut self, pos: &BlockPos, fluid: FluidId, delay: i32) {
        self.schedule_fluid_tick_owner(pos, fluid, delay);
    }

    /// `LevelAccessor.scheduleTick(BlockPos, Block, int)` — route through the
    /// owning chunk's `ProtoChunkTicks`.
    fn schedule_block_tick(
        &mut self,
        pos: &BlockPos,
        block: rivet_world::block::Block,
        delay: i32,
    ) {
        self.schedule_block_tick_owner(pos, block, delay);
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

/// `getFluidState().isRandomlyTicking()` — lava fluid states randomly tick;
/// the generated state table carries the fluid registry id even though it does
/// not carry the fluid behavior flag itself.
fn fluid_is_randomly_ticking(state: &StateId) -> bool {
    matches!(BlockState::new(*state).fluid_id(), 3 | 4)
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

/// `getFluidState().isRandomlyTicking()` — lava fluid states randomly tick;
/// the generated state table carries the fluid registry id even though it does
/// not carry the fluid behavior flag itself.
fn fluid_is_randomly_ticking_blockstate(state: &BlockState) -> bool {
    matches!(state.fluid_id(), 3 | 4)
}

/// `CollisionUtil.isSpecialCollidingBlock(state)` — false (no special-colliding
/// flag in the generated table; exact for the superflat content).
fn state_is_special_colliding_blockstate(_state: &BlockState) -> bool {
    false
}

impl WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey> {
    /// The structure pass asks for the center's STRUCTURE_REFERENCES and
    /// traverses this exact fastutil set, not an outer-map iteration order.
    pub(super) fn placement_reference_chunks(
        &self,
        structure: &Identifier,
    ) -> Result<Vec<ChunkPos>, super::structure_placement::StructurePlacementError> {
        let center = self.get_center();
        let chunk = self
            .try_get_chunk(
                center.x(),
                center.z(),
                ChunkStatus::StructureReferences,
                true,
            )
            .map_err(super::structure_placement::StructurePlacementError::RegionUnavailable)?;
        Ok(chunk
            .get_references_for_structure(structure)
            .map(|key| ChunkPos::unpack(*key as i64))
            .collect())
    }

    pub(super) fn placement_start_entry(
        &self,
        source: ChunkPos,
        structure: &Identifier,
    ) -> Result<
        Option<&rivet_world::chunk::structure_access::StructureStartEntry>,
        super::structure_placement::StructurePlacementError,
    > {
        let chunk = self
            .try_get_chunk(source.x(), source.z(), ChunkStatus::StructureStarts, true)
            .map_err(super::structure_placement::StructurePlacementError::RegionUnavailable)?;
        if chunk.has_unkeyed_starts() {
            return Err(
                super::structure_manager::StructureManagerError::UnresolvedUnkeyedStarts {
                    pos: source,
                }
                .into(),
            );
        }
        Ok(chunk.get_start_for_structure(structure))
    }

    /// Keep the key/table slot installed while lending its unique native
    /// value. A reentrant access gets an explicit unavailable entry, never an
    /// invalid/absent start. The scheduler restores this slot on every exit.
    pub(super) fn placement_take_start_entry(
        &mut self,
        source: ChunkPos,
        structure: &Identifier,
    ) -> Result<
        Option<rivet_world::chunk::structure_access::StructureStartEntry>,
        super::structure_placement::StructurePlacementError,
    > {
        use rivet_world::{
            chunk::structure_access::StructureStartEntry, levelgen::structure::StructureError,
        };
        let chunk = self
            .try_get_chunk_mut(source.x(), source.z(), ChunkStatus::StructureStarts, true)
            .map_err(super::structure_placement::StructurePlacementError::RegionUnavailable)?;
        if chunk.has_unkeyed_starts() {
            return Err(
                super::structure_manager::StructureManagerError::UnresolvedUnkeyedStarts {
                    pos: source,
                }
                .into(),
            );
        }
        Ok(chunk.get_start_for_structure_mut(structure).map(|entry| {
            std::mem::replace(
                entry,
                StructureStartEntry::Deferred {
                    tag: Box::new(Tag::Compound(CompoundTag::new())),
                    reason: StructureError::PlacementUnavailable(
                        "canonical structure start is exclusively loaned to placement".into(),
                    ),
                    source_key: None,
                },
            )
        }))
    }

    pub(super) fn placement_return_start_entry(
        &mut self,
        source: ChunkPos,
        structure: &Identifier,
        entry: rivet_world::chunk::structure_access::StructureStartEntry,
    ) {
        // The region's holder cache and metadata map are inaccessible to
        // transformer callbacks. This unique job cannot remove the checked
        // slot or transfer its chunk while a start is loaned.
        let chunk = self
            .try_get_chunk_mut(source.x(), source.z(), ChunkStatus::StructureStarts, true)
            .expect("placement loan retains its canonical source chunk");
        *chunk
            .get_start_for_structure_mut(structure)
            .expect("placement loan retains its key slot") = entry;
    }
}

/// A [`GenerationChunkHolderView`] that borrows the executor's center chunk.
/// Complete ProtoChunk entity ownership instead uses BorrowedProtoHolder.
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

    fn into_owned_proto(self: Box<Self>) -> Option<ProtoChunk<T, B, S>> {
        Some(self.chunk)
    }

    fn get_proto_chunk_if_present_unchecked_mut(
        &mut self,
        status: ChunkStatus,
    ) -> Option<&mut ProtoChunk<T, B, S>> {
        self.chunk
            .get_persisted_status()
            .is_or_after(status)
            .then_some(&mut self.chunk)
    }
}

/// Borrowed center holder retaining the actual ProtoChunk, including its
/// entity list, rather than only borrowing the block-state base.
pub struct BorrowedProtoHolder<'a, T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    chunk: &'a mut ProtoChunk<T, B, S>,
}

impl<'a, T, B, S> BorrowedProtoHolder<'a, T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    pub fn new(chunk: &'a mut ProtoChunk<T, B, S>) -> Self {
        Self { chunk }
    }
}

impl<T, B, S> GenerationChunkHolderView<T, B, S> for BorrowedProtoHolder<'_, T, B, S>
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
    fn get_proto_chunk_if_present_unchecked_mut(
        &mut self,
        status: ChunkStatus,
    ) -> Option<&mut ProtoChunk<T, B, S>> {
        self.chunk
            .get_persisted_status()
            .is_or_after(status)
            .then_some(&mut *self.chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::core::QuartPos;
    use rivet_registry::root::AnyBox;
    use rivet_registry::{HolderGetter, Identifier, ResourceKey};
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

    #[test]
    fn real_clay_pool_matches_paper_in_chunk_backed_region() {
        use rivet_util::random_source::XoroshiroRandomSource;
        use rivet_util::worldgen_random::AlgorithmRandomSource;
        use rivet_world::chunk::chunk_generator::ChunkGenerator;
        use rivet_world::levelgen::feature::registry_keys::CONFIGURED_FEATURE;
        use sha2::{Digest, Sha256};

        struct Generator;
        impl ChunkGenerator for Generator {
            fn get_min_y(&self) -> i32 {
                SUPERFLAT_MIN_Y
            }
            fn get_gen_depth(&self) -> i32 {
                SUPERFLAT_HEIGHT
            }
        }
        let row = include_str!("../../../../../tools/rivet-oracle/fixtures/tree-placement.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|row| {
                row["feature"] == "clay_pool_with_dripleaves"
                    && row["terrain"] == "dry_dirt"
                    && row["seed"] == 0
            })
            .expect("actual Paper pool fixture with both dripleaf families");
        let access = super::super::generated_world::build_feature_access(
            &rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries(),
        );
        let mut region = region_with_access(access.clone());
        region.set_region_random(AlgorithmRandomSource::Xoroshiro(
            XoroshiroRandomSource::new(99),
        ));
        // Match the oracle's flat dirt around the entire patch, with a margin
        // beyond its maximum radius and depth. Writes go to real chunk palettes.
        for x in -12..=12 {
            for z in -12..=12 {
                for y in 56..64 {
                    assert!(region.set_block(
                        &BlockPos::new(x, y, z),
                        Blocks::DIRT.default_block_state(),
                        2,
                        UPDATE_LIMIT
                    ));
                }
            }
        }
        let registry = access.lookup(&*CONFIGURED_FEATURE).unwrap();
        let feature = registry
            .get_optional(&Identifier::parse("minecraft:clay_pool_with_dripleaves"))
            .unwrap();
        let mut random = XoroshiroRandomSource::new(0);
        assert_eq!(
            feature.place(
                &mut region,
                &Generator,
                &mut random,
                &BlockPos::new(0, 64, 0)
            ),
            row["result"].as_bool().unwrap()
        );
        let mut hash = Sha256::new();
        for x in -12..=12 {
            for y in 56..=85 {
                for z in -12..=12 {
                    let state = region.get_block_state(&BlockPos::new(x, y, z));
                    let original = if y < 64 {
                        Blocks::DIRT.default_block_state()
                    } else {
                        Blocks::AIR.default_block_state()
                    };
                    if state != original {
                        for value in [x, y, z, i32::from(state.id().0)] {
                            hash.update(value.to_be_bytes());
                        }
                    }
                }
            }
        }
        assert_eq!(
            format!("{:x}", hash.finalize()),
            row["blocks_sha256"].as_str().unwrap()
        );
        assert_eq!(random.next_long(), row["next_long"].as_i64().unwrap());
        assert_eq!(
            region.region_random.as_mut().unwrap().next_long(),
            row["world_next_long"].as_i64().unwrap()
        );
    }

    #[test]
    fn real_bee_tree_persists_occupants_through_chunk_payload_reload_and_replacement() {
        use rivet_nbt::{nbt_accounter::NbtAccounter, nbt_io};
        use rivet_registry::registry_ops::RegistryOps;
        use rivet_serialization::json_ops::JsonOps;
        use rivet_util::random_source::XoroshiroRandomSource;
        use rivet_util::worldgen_random::AlgorithmRandomSource;
        use rivet_world::chunk::chunk_generator::ChunkGenerator;
        use rivet_world::chunk::storage::serializable_chunk_data::parse_block_entities;
        use rivet_world::levelgen::feature::configurations::tree_configuration::tree_configuration_codec;
        use rivet_world::levelgen::feature::{FeatureBehavior, FeaturePlaceContext, TREE};

        struct Generator;
        impl ChunkGenerator for Generator {
            fn get_min_y(&self) -> i32 {
                SUPERFLAT_MIN_Y
            }
            fn get_gen_depth(&self) -> i32 {
                SUPERFLAT_HEIGHT
            }
        }
        let row = include_str!("../../../../../tools/rivet-oracle/fixtures/tree-placement.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|row| row["feature"] == "oak_bees_005" && row["seed"] == 20)
            .expect("positive actual Paper bee-tree fixture");
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let decoded = tree_configuration_codec().parse(&ops, &row["config"]);
        let config = decoded.result().expect("actual bee tree config");
        let mut region = feature_region();
        region.set_region_random(AlgorithmRandomSource::Xoroshiro(
            XoroshiroRandomSource::new(99),
        ));
        let origin = BlockPos::new(8, 64, 8);
        region.set_block(
            &origin.below(),
            Blocks::DIRT.default_block_state(),
            2,
            UPDATE_LIMIT,
        );
        let mut random = XoroshiroRandomSource::new(20);
        assert!(TREE.place(&mut FeaturePlaceContext::new(
            None,
            &mut region,
            &Generator,
            &mut random,
            &origin,
            config
        )));
        let hive_pos = (-1..=1)
            .find_map(|x| {
                (-1..=1).find_map(|z| {
                    region
                        .get_chunk(x, z)
                        .block_entities()
                        .iter()
                        .find_map(|(pos, entity)| {
                            matches!(
                                entity.data(),
                                rivet_world::block_entity::NativeBlockEntityData::Beehive(_)
                            )
                            .then_some(*pos)
                        })
                })
            })
            .expect("tree must create a canonical typed hive");
        let hive_state = region.get_block_state(&hive_pos);
        assert!(is_beehive_state(hive_state));
        let chunk_x = hive_pos.get_x() >> 4;
        let chunk_z = hive_pos.get_z() >> 4;
        let saved = region
            .existing_block_entity_nbt(&hive_pos)
            .expect("canonical live occupants");
        assert!(
            region
                .get_chunk(chunk_x, chunk_z)
                .get_block_entity_nbt(&hive_pos)
                .is_none(),
            "promotion removes pending tag"
        );
        let original_bees = saved.get_list("bees").expect("saved occupants").clone();
        assert!((2..=3).contains(&original_bees.list.len()));
        // Round-trip genuine pending chunk block_entities through binary NBT
        // and the normal chunk payload parser. No proto is falsely made FULL.
        let mut chunk_payload = CompoundTag::new();
        chunk_payload.put(
            "block_entities".to_string(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(saved)])),
        );
        let mut bytes = Vec::new();
        nbt_io::write_compressed(&chunk_payload, &mut bytes).unwrap();
        let loaded =
            nbt_io::read_compressed(bytes.as_slice(), &mut NbtAccounter::unlimited_heap()).unwrap();
        let loaded_tags = parse_block_entities(&loaded);
        assert_eq!(loaded_tags.len(), 1);
        drop(region);

        let mut reloaded_region = feature_region();
        reloaded_region.set_block(&hive_pos, hive_state, 2, UPDATE_LIMIT);
        // Hand the reconstructed pending payload to its actual owner, with
        // no cached live entity. Typed query/append must load this payload.
        reloaded_region
            .get_chunk_mut(chunk_x, chunk_z)
            .set_block_entity_nbt(loaded_tags[0].clone());
        assert!(WorldGenLevel::has_beehive_block_entity(
            &mut reloaded_region,
            &hive_pos
        ));
        let same_block = hive_state
            .set_value(BlockStateProperties::LEVEL_HONEY, 2)
            .unwrap();
        reloaded_region.set_block(&hive_pos, same_block, 2, UPDATE_LIMIT);
        assert_eq!(
            reloaded_region
                .existing_block_entity_nbt(&hive_pos)
                .unwrap()
                .get_list("bees")
                .unwrap(),
            &original_bees,
            "lazy typed query must mark the entity live before a same-block write"
        );
        WorldGenLevel::store_beehive_occupant(&mut reloaded_region, &hive_pos, 77);
        let appended = reloaded_region
            .existing_block_entity_nbt(&hive_pos)
            .unwrap();
        let bees = appended.get_list("bees").unwrap();
        assert_eq!(
            &bees.list[..original_bees.list.len()],
            original_bees.list.as_slice()
        );
        assert_eq!(bees.list.len(), original_bees.list.len() + 1);
        let Tag::Compound(last) = bees.list.last().unwrap() else {
            panic!("occupant compound")
        };
        assert_eq!(last.get_int("ticks_in_hive"), Some(77));
        assert_eq!(last.get_int("min_ticks_in_hive"), Some(600));
        let same_block = hive_state
            .set_value(BlockStateProperties::LEVEL_HONEY, 3)
            .unwrap();
        reloaded_region.set_block(&hive_pos, same_block, 2, UPDATE_LIMIT);
        assert_eq!(
            reloaded_region
                .existing_block_entity_nbt(&hive_pos)
                .unwrap()
                .get_list("bees")
                .unwrap()
                .list
                .len(),
            original_bees.list.len() + 1
        );
        reloaded_region.set_block(
            &hive_pos,
            Blocks::STONE.default_block_state(),
            2,
            UPDATE_LIMIT,
        );
        assert!(!WorldGenLevel::has_beehive_block_entity(
            &mut reloaded_region,
            &hive_pos
        ));
        assert!(
            reloaded_region
                .get_chunk(chunk_x, chunk_z)
                .get_block_entity_nbt(&hive_pos)
                .is_none()
        );
    }

    #[test]
    fn canonical_live_pending_transitions_match_actual_paper_region() {
        use rivet_nbt::tag_parser::parse_compound_fully;
        let pos = BlockPos::new(8, 64, 8);
        let mut region = feature_region();
        let mut state = Blocks::AIR.default_block_state();
        let mut identity = None;
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/block-entity-lifecycle.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let family = row["family"].as_str().unwrap();
            let phase = row["phase"].as_str().unwrap();
            match phase {
                "pending" => {
                    region = feature_region();
                    identity = None;
                    state = BlockState::new(StateId(row["state"].as_u64().unwrap() as u16));
                    assert!(region.set_block(&pos, state, UPDATE_KNOWN_SHAPE, 0));
                }
                "live" => {
                    assert!(region.promote_native_block_entity(&pos, state));
                    match family {
                        "chest" => WorldGenLevel::set_block_entity_loot_table(
                            &mut region,
                            &pos,
                            42,
                            "minecraft:chests/abandoned_mineshaft",
                        ),
                        "beehive" => WorldGenLevel::store_beehive_occupant(&mut region, &pos, 77),
                        "spawner" | "heart" => {}
                        _ => panic!("fixture family"),
                    }
                    identity = region.native_block_entity(&pos).map(std::ptr::from_ref);
                }
                "same_block" => {
                    state = BlockState::new(StateId(row["state"].as_u64().unwrap() as u16));
                    assert!(region.set_block(&pos, state, UPDATE_KNOWN_SHAPE, 0));
                }
                "pending_write_guard" => {
                    let mut tag = dummy_block_entity(&pos);
                    tag.put_int("unexpected", 9);
                    region.persist_block_entity_nbt(&pos, tag);
                }
                "removed" => {
                    assert!(region.set_block(
                        &pos,
                        Blocks::STONE.default_block_state(),
                        UPDATE_KNOWN_SHAPE,
                        0
                    ));
                }
                _ => panic!("fixture phase"),
            }
            let actual = region.native_block_entity(&pos);
            assert_eq!(
                actual.map(|entity| u64::from(entity.state().id().0)),
                row["cached_state"].as_u64(),
                "cached state {family}/{phase}"
            );
            assert_eq!(
                actual.is_some(),
                row["live"].as_bool().unwrap(),
                "{family}/{phase}"
            );
            let same = actual.is_some() && actual.map(std::ptr::from_ref) == identity;
            assert_eq!(
                same,
                row["same_identity"].as_bool().unwrap(),
                "{family}/{phase}"
            );
            assert_eq!(
                u64::from(region.get_block_state(&pos).id().0),
                row["state"].as_u64().unwrap()
            );
            let pending = row["pending"]
                .as_str()
                .map(|s| parse_compound_fully(s).unwrap());
            assert_eq!(
                region.get_chunk(0, 0).get_block_entity_nbt(&pos),
                pending.as_ref(),
                "{family}/{phase}"
            );
            let saved = row["saved"]
                .as_str()
                .map(|s| parse_compound_fully(s).unwrap());
            assert_eq!(
                region.existing_block_entity_nbt(&pos),
                saved,
                "{family}/{phase}"
            );
        }
    }

    #[test]
    fn ordinary_saplings_inherit_vegetation_support_without_including_mangrove() {
        let mut region = feature_region();
        let pos = BlockPos::new(8, 64, 8);
        for (support, survives, litter_survives) in [
            (Blocks::DIRT.default_block_state(), true, true),
            (Blocks::STONE.default_block_state(), false, true),
            (Blocks::AIR.default_block_state(), false, false),
        ] {
            region.set_block(&pos.below(), support, 2, UPDATE_LIMIT);
            let litter = BlockState::of(BlockId::from_name("minecraft:leaf_litter").unwrap());
            assert_eq!(
                WorldGenLevel::can_survive(&region, &litter, &pos),
                litter_survives,
                "leaf litter uses sturdy UP, not the sapling support tag"
            );
            for name in [
                "oak", "spruce", "birch", "jungle", "acacia", "cherry", "dark_oak", "pale_oak",
            ] {
                let sapling = BlockState::of(
                    BlockId::from_name(&format!("minecraft:{name}_sapling")).unwrap(),
                );
                for stage in [0, 1] {
                    let sapling = sapling
                        .set_value(BlockStateProperties::STAGE, stage)
                        .unwrap();
                    assert_eq!(
                        WorldGenLevel::can_survive(&region, &sapling, &pos),
                        survives,
                        "{name} stage {stage} on {}",
                        support.block().name()
                    );
                }
            }
        }
        let mangrove = BlockState::of(BlockId::from_name("minecraft:mangrove_propagule").unwrap());
        region.set_block(
            &pos.below(),
            BlockState::of(BlockId::from_name("minecraft:clay").unwrap()),
            2,
            UPDATE_LIMIT,
        );
        assert!(WorldGenLevel::can_survive(&region, &mangrove, &pos));
        let oak = BlockState::of(BlockId::from_name("minecraft:oak_sapling").unwrap());
        assert!(
            !WorldGenLevel::can_survive(&region, &oak, &pos),
            "mangrove's clay support is not plain vegetation support"
        );
        let hanging = mangrove
            .set_value(BlockStateProperties::HANGING, true)
            .unwrap();
        assert!(!WorldGenLevel::can_survive(&region, &hanging, &pos));
        region.set_block(
            &pos.above(),
            BlockState::of(BlockId::from_name("minecraft:mangrove_leaves").unwrap()),
            2,
            UPDATE_LIMIT,
        );
        assert!(
            WorldGenLevel::can_survive(&region, &hanging, &pos),
            "hanging propagules use the block above, not their floor"
        );
    }

    #[test]
    fn native_minecart_admission_retains_actual_center_and_dependency_entity_storage() {
        use rivet_nbt::tag_parser::parse_compound_fully;
        use rivet_registry::core::Vec3;
        use rivet_util::{mth::Uuid, random_source::LegacyRandomSource};
        use rivet_world::entity::chest_minecart::{EntityConstructionContext, NativeChestMinecart};
        fn proto(
            pos: ChunkPos,
            status: ChunkStatus,
        ) -> ProtoChunk<StateId, ServerBiomeId, StructureKey> {
            let accessor = create_accessor(SUPERFLAT_MIN_Y, SUPERFLAT_HEIGHT);
            let mut chunk = ProtoChunk::new(
                pos,
                UpgradeData::empty(accessor.get_sections_count() as usize),
                accessor,
                &container_factory(),
                None,
                Blocks::AIR.default_block_state().id(),
                BlockState::of(BlockId::from_name("minecraft:void_air").unwrap()).id(),
                &|state: &StateId| resolve_state_flags(&BlockState::new(*state)),
            );
            chunk.set_persisted_status(status);
            chunk
        }
        fn entity(pos: Vec3) -> NativeChestMinecart {
            let mut entity = NativeChestMinecart::new(
                EntityConstructionContext {
                    entity_id: 101,
                    world_uuid: Uuid {
                        most: 0x1234_5678_9abc_def0,
                        least: 0xfedc_ba98_7654_3210_u64 as i64,
                    },
                    experimental_minecart_movement: false,
                },
                &mut LegacyRandomSource::new(42),
            );
            entity.set_initial_pos(pos);
            entity
        }
        let center_pos = ChunkPos::new(-1, 2);
        let mut center = proto(center_pos, ChunkStatus::Carvers);
        let step = GENERATION_PYRAMID
            .get_step_to(ChunkStatus::Features)
            .clone();
        let deps = step.direct_dependencies().as_list();
        let mut holders: Vec<
            Box<dyn GenerationChunkHolderView<StateId, ServerBiomeId, StructureKey>>,
        > = Vec::new();
        for x in -9_i32..=7 {
            for z in -6_i32..=10 {
                if ChunkPos::new(x, z) == center_pos {
                    continue;
                }
                let distance = center_pos.get_chessboard_distance_coords(x, z);
                holders.push(Box::new(OwnedProtoHolder::new(proto(
                    ChunkPos::new(x, z),
                    deps[distance as usize],
                ))));
            }
        }
        // Explicit lifetime: only this holder borrows a real center ProtoChunk.
        let mut holders: Vec<
            Box<dyn GenerationChunkHolderView<StateId, ServerBiomeId, StructureKey> + '_>,
        > = holders;
        holders.insert(8 * 17 + 8, Box::new(BorrowedProtoHolder::new(&mut center)));
        let mut region = WorldGenRegion::new(
            StaticCache2D::from_entries(-9, -6, 17, 17, holders),
            center_pos,
            step,
            42,
            SUPERFLAT_MIN_Y,
            SUPERFLAT_HEIGHT,
            -63,
            Arc::new(TestBiomeSource {
                biome: BiomeId::from_id(40),
            }),
            RegistryAccess::empty(),
        );
        let center_entity = entity(Vec3::new(-0.5, 50.5, 32.5));
        let center_saved = center_entity.save().unwrap();
        assert!(region.add_fresh_chest_minecart(center_entity).unwrap());
        assert!(
            !region
                .structure_chunk_entities_are_empty(center_pos)
                .unwrap()
        );
        let row: serde_json::Value = serde_json::from_str(
            include_str!("../../../../../tools/rivet-oracle/fixtures/chest-minecart.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let expected = parse_compound_fully(row["saved"].as_str().unwrap()).unwrap();
        assert!(
            region
                .add_fresh_chest_minecart(entity(Vec3::new(-16.5, 50.5, 32.5)))
                .unwrap()
        );
        assert!(
            !region
                .structure_chunk_entities_are_empty(ChunkPos::new(-2, 2))
                .unwrap()
        );
        let owned = region.into_owned_proto_entries();
        assert_eq!(center.get_entities(), &[center_saved]);
        let ring = owned
            .into_iter()
            .find(|(pos, _)| *pos == ChunkPos::new(-2, 2))
            .unwrap()
            .1;
        assert_eq!(ring.get_entities(), &[expected]);
        // Base-only views cannot silently serialize an entity into lost storage.
        let mut base_only = feature_region();
        assert!(
            base_only
                .add_fresh_chest_minecart(entity(Vec3::new(0.5, 50.5, 0.5)))
                .is_err()
        );
    }

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

    #[test]
    fn structure_craft_snapshots_persist_actual_paper_payloads_and_survive_same_block_writes() {
        let pos = BlockPos::new(8, 64, 8);
        let mut rows = 0;
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/structure-snapshots.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["name"] == "block_entity_types" {
                continue;
            }
            let mut region = feature_region();
            let state = BlockState::new(StateId(row["state"].as_u64().unwrap() as u16));
            let expected =
                rivet_nbt::tag_parser::parse_compound_fully(row["nbt"].as_str().unwrap()).unwrap();
            let data = row["input"]
                .as_str()
                .map(|input| rivet_nbt::tag_parser::parse_compound_fully(input).unwrap())
                .unwrap_or_else(|| expected.clone());
            WorldGenLevel::set_block(&mut region, &pos, state, 2);
            region.structure_load_snapshot(pos, &data).unwrap();
            assert_eq!(
                region.existing_block_entity_nbt(&pos),
                Some(expected.clone()),
                "{}",
                row["name"]
            );
            // Source getBlockEntity promotes the pending entity into the live
            // map. A same-block set must preserve its loaded snapshot data.
            WorldGenLevel::set_block(&mut region, &pos, state, 2);
            let actual = region.structure_world_snapshot(pos).unwrap();
            assert_eq!(actual.state, state);
            assert_eq!(actual.block_entity, Some(expected));
            rows += 1;
        }
        assert_eq!(rows, 7);
    }

    fn snowy_feature_region() -> WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> {
        let mut region = feature_region();
        let snowy = ServerBiomeId(BiomeId::from_name("minecraft:snowy_plains").unwrap().id());
        // BiomeManager samples quart corners in adjacent chunks. Populate the
        // actual cached section palettes, not just its uncached source.
        for x in -1..=1 {
            for z in -1..=1 {
                let chunk = region.get_chunk_mut(x, z);
                for section in 0..chunk.get_sections().len() {
                    for qx in 0..4 {
                        for qy in 0..4 {
                            for qz in 0..4 {
                                chunk
                                    .get_section_mut(section)
                                    .set_noise_biome(qx, qy, qz, snowy);
                            }
                        }
                    }
                }
            }
        }
        region
    }

    #[test]
    fn freezing_and_snow_use_paper_pre_light_zero_then_visible_block_nibbles() {
        use rivet_world::lighting::swmr_nibble_array::SwmrNibbleArray;
        let mut region = snowy_feature_region();
        let water = BlockPos::new(8, 0, 8);
        let snow = BlockPos::new(9, 1, 8);
        region.set_block(&water, Blocks::WATER.default_block_state(), 2, UPDATE_LIMIT);
        region.set_block(
            &snow.below(),
            Blocks::STONE.default_block_state(),
            2,
            UPDATE_LIMIT,
        );
        region.set_block(&snow, Blocks::AIR.default_block_state(), 2, UPDATE_LIMIT);
        assert!(!region.get_chunk(0, 0).is_light_correct());
        assert!(WorldGenLevel::should_freeze(&region, &water, false));
        assert!(WorldGenLevel::should_snow(&region, &snow));

        // Paper ignores even published light until isLightCorrect. Once the
        // engine publishes correctness, 9 permits both and 10 rejects both.
        for (brightness, allowed) in [(9, true), (10, false)] {
            let chunk = region.get_chunk_mut(0, 0);
            let mut nibbles: Vec<_> = chunk.block_nibbles().to_vec();
            let light_section =
                (water.get_y() >> 4) - chunk.height_accessor().get_min_section_y() + 1;
            let nibble = &mut nibbles[light_section as usize];
            *nibble = SwmrNibbleArray::new();
            nibble.set(water.get_x(), water.get_y(), water.get_z(), brightness);
            nibble.set(snow.get_x(), snow.get_y(), snow.get_z(), brightness);
            nibble.update_visible();
            chunk.set_block_nibbles(nibbles);
            chunk.set_light_correct(false);
            assert_eq!(region.get_block_brightness(&water), 0);
            assert!(WorldGenLevel::should_freeze(&region, &water, false));
            assert!(WorldGenLevel::should_snow(&region, &snow));
            region.get_chunk_mut(0, 0).set_light_correct(true);
            assert_eq!(region.get_block_brightness(&water), brightness);
            assert_eq!(
                WorldGenLevel::should_freeze(&region, &water, false),
                allowed
            );
            assert_eq!(WorldGenLevel::should_snow(&region, &snow), allowed);
        }
    }

    #[test]
    fn freezing_requires_source_liquid_water_and_optional_exposed_edge() {
        let mut region = snowy_feature_region();
        let pos = BlockPos::new(8, 0, 8);
        for neighbor in [pos, pos.west(), pos.east(), pos.north(), pos.south()] {
            region.set_block(
                &neighbor,
                Blocks::WATER.default_block_state(),
                2,
                UPDATE_LIMIT,
            );
        }
        assert!(WorldGenLevel::should_freeze(&region, &pos, false));
        assert!(!WorldGenLevel::should_freeze(&region, &pos, true));
        region.set_block(
            &pos.west(),
            Blocks::AIR.default_block_state(),
            2,
            UPDATE_LIMIT,
        );
        assert!(WorldGenLevel::should_freeze(&region, &pos, true));
        let flowing = Blocks::WATER
            .default_block_state()
            .set_value(BlockStateProperties::LEVEL, 1)
            .unwrap();
        region.set_block(&pos, flowing, 2, UPDATE_LIMIT);
        assert!(!WorldGenLevel::should_freeze(&region, &pos, false));
        region.set_block(
            &pos,
            Blocks::SEAGRASS.default_block_state(),
            2,
            UPDATE_LIMIT,
        );
        assert!(!WorldGenLevel::should_freeze(&region, &pos, false));
    }

    #[test]
    fn freeze_samples_top_biome_but_evaluates_water_below() {
        let mut region = snowy_feature_region();
        let warm = ServerBiomeId(BiomeId::from_name("minecraft:plains").unwrap().id());
        for x in -1..=1 {
            for z in -1..=1 {
                let chunk = region.get_chunk_mut(x, z);
                for section in 0..chunk.get_sections().len() {
                    for qx in 0..4 {
                        for qy in [0, 2] {
                            for qz in 0..4 {
                                chunk
                                    .get_section_mut(section)
                                    .set_noise_biome(qx, qy, qz, warm);
                            }
                        }
                    }
                }
            }
        }
        let sampled_cold = |region: &WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey>,
                            pos: &BlockPos| {
            generated_climate_settings(BiomeId::from_id(dense_biome_id(&WorldGenLevel::get_biome(
                region, pos,
            ))))
            .temperature
                == 0.0
        };
        // Find an actual BiomeManager interpolation boundary; sampling fixed
        // quart ids directly would miss the topPos/belowPos caller contract.
        for top_is_cold in [true, false] {
            let pos = (-32..32)
                .map(|y| BlockPos::new(8, y, 8))
                .find(|pos| {
                    sampled_cold(&region, &pos.above()) == top_is_cold
                        && sampled_cold(&region, pos) != top_is_cold
                })
                .expect("alternating quart layers produce both biome boundaries");
            region.set_block(&pos, Blocks::WATER.default_block_state(), 2, UPDATE_LIMIT);
            assert_eq!(
                WorldGenLevel::should_freeze(&region, &pos, false),
                top_is_cold
            );
        }
    }

    #[test]
    fn snow_survival_uses_collision_face_tags_and_full_snow_layers() {
        let mut region = snowy_feature_region();
        let pos = BlockPos::new(8, 1, 8);
        for (support, survives) in [
            (Blocks::STONE.default_block_state(), true),
            (Blocks::ICE.default_block_state(), false),
            (Blocks::HONEY_BLOCK.default_block_state(), true),
            (Blocks::AIR.default_block_state(), false),
            (Blocks::OAK_LEAVES.default_block_state(), true),
            (
                Blocks::SNOW
                    .default_block_state()
                    .set_value(BlockStateProperties::LAYERS, 7)
                    .unwrap(),
                false,
            ),
            (
                Blocks::SNOW
                    .default_block_state()
                    .set_value(BlockStateProperties::LAYERS, 8)
                    .unwrap(),
                true,
            ),
        ] {
            region.set_block(&pos.below(), support, 2, UPDATE_LIMIT);
            region.set_block(&pos, Blocks::AIR.default_block_state(), 2, UPDATE_LIMIT);
            assert_eq!(
                WorldGenLevel::should_snow(&region, &pos),
                survives,
                "support {support:?}"
            );
        }
        region.set_block(
            &pos.below(),
            Blocks::STONE.default_block_state(),
            2,
            UPDATE_LIMIT,
        );
        region.set_block(&pos, Blocks::SNOW.default_block_state(), 2, UPDATE_LIMIT);
        assert!(WorldGenLevel::should_snow(&region, &pos));
        region.set_block(
            &pos,
            BlockState::of(BlockId::from_name("minecraft:short_grass").unwrap()),
            2,
            UPDATE_LIMIT,
        );
        assert!(!WorldGenLevel::should_snow(&region, &pos));
        assert!(!WorldGenLevel::should_snow(
            &region,
            &BlockPos::new(8, region.get_min_y() - 1, 8)
        ));
    }

    #[test]
    fn freeze_and_snow_match_live_paper_fixture() {
        use rivet_world::biome::biome::{ClimateSettings, TemperatureModifier};
        struct ProbeLevel {
            pos: BlockPos,
            target: BlockState,
            support: BlockState,
            neighbors: BlockState,
            brightness: i32,
        }
        impl LevelHeightAccessor for ProbeLevel {
            fn get_min_y(&self) -> i32 {
                -64
            }
            fn get_height(&self) -> i32 {
                384
            }
        }
        impl WorldGenLevel for ProbeLevel {
            fn get_seed(&self) -> i64 {
                42
            }
            fn get_sea_level(&self) -> i32 {
                63
            }
            fn get_block_brightness(&self, _: &BlockPos) -> i32 {
                self.brightness
            }
            fn get_block_state(&self, pos: &BlockPos) -> BlockState {
                if *pos == self.pos {
                    self.target
                } else if *pos == self.pos.below() {
                    self.support
                } else {
                    self.neighbors
                }
            }
            fn can_survive(&self, state: &BlockState, pos: &BlockPos) -> bool {
                region_can_survive(self, state, pos)
            }
        }
        fn state(text: &str) -> BlockState {
            let (name, value) = text
                .split_once(':')
                .map_or((text, None), |(name, value)| (name, Some(value)));
            let state = BlockState::of(BlockId::from_name(&format!("minecraft:{name}")).unwrap());
            value.map_or(state, |value| {
                state
                    .set_value(
                        if name == "snow" {
                            BlockStateProperties::LAYERS
                        } else {
                            BlockStateProperties::LEVEL
                        },
                        value.parse::<i32>().unwrap(),
                    )
                    .unwrap()
            })
        }
        let fixture = include_str!("../../../../../tools/rivet-oracle/fixtures/freeze-snow.tsv");
        let mut rows = 0;
        for row in fixture
            .lines()
            .filter(|row| !row.starts_with('#') && !row.is_empty())
        {
            let fields: Vec<_> = row.split('\t').collect();
            assert_eq!(fields.len(), 10);
            let climate = ClimateSettings {
                temperature: fields[0].parse().unwrap(),
                has_precipitation: fields[1].parse().unwrap(),
                temperature_modifier: TemperatureModifier::None,
                downfall: 0.5,
            };
            let level = ProbeLevel {
                pos: BlockPos::new(0, fields[3].parse().unwrap(), 0),
                target: state(fields[4]),
                support: state(fields[5]),
                neighbors: state(fields[6]),
                brightness: fields[2].parse().unwrap(),
            };
            assert_eq!(
                climate.should_freeze(&level, &level.pos, fields[7].parse().unwrap()),
                fields[8].parse::<bool>().unwrap(),
                "freeze: {row}"
            );
            assert_eq!(
                climate.should_snow(&level, &level.pos),
                fields[9].parse::<bool>().unwrap(),
                "snow: {row}"
            );
            rows += 1;
        }
        assert_eq!(rows, 23);
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

    /// A FEATURES region whose holder at `missing` has no chunk value, so a
    /// BIOMES read exercises the typed unavailable diagnostic.
    fn region_with_missing_holder(
        missing: ChunkPos,
    ) -> WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> {
        let step = GENERATION_PYRAMID
            .get_step_to(ChunkStatus::Features)
            .clone();
        let deps = step.direct_dependencies().as_list().to_vec();
        let cache = StaticCache2D::create(0, 0, 8, &|x, z| {
            let pos = ChunkPos::new(x, z);
            if pos == missing {
                Box::new(TestEmptyHolder)
                    as Box<dyn GenerationChunkHolderView<StateId, ServerBiomeId, StructureKey>>
            } else {
                let distance = ChunkPos::new(0, 0).get_chessboard_distance_coords(x, z);
                let status = deps[distance.min(deps.len() as i32 - 1) as usize];
                Box::new(OwnedHolder::new(test_chunk(pos), status))
                    as Box<dyn GenerationChunkHolderView<StateId, ServerBiomeId, StructureKey>>
            }
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
            RegistryAccess::empty(),
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
        assert!(region.native_block_entity(&pos).is_none());
        assert!(region.get_chunk(0, 0).get_block_entity_nbt(&pos).is_some());
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_randomizable_container(
            &region, &pos
        ));

        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0,));
        assert!(region.native_block_entity(&pos).is_none());
        assert!(region.get_chunk(0, 0).get_block_entity_nbt(&pos).is_some());
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_spawner_block_entity(
            &region, &pos
        ));

        assert!(region.remove_block(&pos, false));
        assert!(region.native_block_entity(&pos).is_none());
        assert!(region.get_chunk(0, 0).get_block_entity_nbt(&pos).is_none());
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
    fn cached_and_uncached_biomes_share_the_world_registry_holder_identity() {
        let access = rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries();
        let registry = access.lookup_or_throw(&*rivet_registry::registries::BIOME);
        let expected = registry.get_or_throw(&rivet_registry::ResourceKey::create(
            &*rivet_registry::registries::BIOME,
            rivet_registry::Identifier::parse("minecraft:plains"),
        ));
        let tag = rivet_registry::HolderGetter::get_tag_or_throw(
            registry,
            &rivet_registry::TagKey::create(
                &*rivet_registry::registries::BIOME,
                rivet_registry::Identifier::parse("minecraft:is_overworld"),
            ),
        );
        let region = region_with_access(access.clone());
        let cached = region.get_biome(&BlockPos::new(0, 64, 0));
        let uncached = region.get_uncached_noise_biome(0, 16, 0);
        assert_eq!(cached, expected);
        assert_eq!(uncached, expected);
        assert!(cached.can_serialize_in(registry));
        assert!(tag.contains(&cached));
        assert!(tag.contains(&uncached));
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

    /// A BIOMES read from a ring-2 STRUCTURE_STARTS holder with no chunk must
    /// preserve Paper's typed unavailable diagnostic instead of falling back to
    /// the uncached source. The `Result` form makes the diagnostic explicit.
    #[test]
    fn get_noise_biome_rejects_missing_structure_starts_holder() {
        let region = region_with_missing_holder(ChunkPos::new(2, 0));
        // Quart x=8 maps to chunk x=2; the FEATURES dependency allows only
        // STRUCTURE_STARTS there, so the diagnostic names that ring status.
        let diagnostic = region
            .get_noise_biome_cached(8, 0, 0)
            .expect_err("the in-ring empty holder must reject the BIOMES read");
        assert_eq!(
            diagnostic.max_allowed_status,
            Some(ChunkStatus::StructureStarts)
        );
        assert_eq!(diagnostic.actual_status, None);
        let message = diagnostic.to_string();
        assert!(message.contains("requesting chunk [2, 0]"));
        assert!(message.contains("maximum allowed status: minecraft:structure_starts"));
    }

    /// A biome interpolation that reaches past the 9-ring FEATURES window must
    /// fail with the unavailable-chunk diagnostic rather than use the uncached
    /// source and hide the dependency-window violation.
    #[test]
    #[should_panic(expected = "actual status: [out of cache bounds]")]
    fn get_biome_rejects_out_of_window_cached_read() {
        let region = feature_region();
        region.get_biome(&BlockPos::new(160, 64, 0));
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
        let mut tag = dummy_block_entity(&spawner_pos);
        tag.put_string("id", "minecraft:mob_spawner");
        let mut data = CompoundTag::new();
        let mut entity = CompoundTag::new();
        entity.put_string("id", "minecraft:zombie");
        data.put("entity".into(), Tag::Compound(entity));
        let mut potential = CompoundTag::new();
        potential.put_int("weight", 2);
        potential.put("data".into(), Tag::Compound(data));
        tag.put(
            "SpawnPotentials".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(potential)])),
        );
        region.persist_block_entity_nbt(&spawner_pos, tag);
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
        let saved = region.existing_block_entity_nbt(&spawner_pos).unwrap();
        assert_eq!(
            saved
                .get_compound("SpawnData")
                .unwrap()
                .get_compound("entity")
                .unwrap()
                .get_string("id")
                .map(String::as_str),
            Some("minecraft:skeleton")
        );
        assert!(saved.get_list("SpawnPotentials").unwrap().list.is_empty());
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
    fn dummy_spawner_payload_is_ignored_without_a_potential_draw() {
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
        let materialized = region
            .existing_block_entity_nbt(&pos)
            .expect("materialized DUMMY spawner");
        assert!(materialized.get("SpawnData").is_none());
        assert!(materialized.get("SpawnPotentials").is_none());

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
    fn generated_heart_keeps_pending_entity_in_owning_chunk_until_replaced() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        let heart = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:creaking_heart")
                .unwrap(),
        );
        assert!(region.set_block(&pos, heart, UPDATE_ALL, 0));
        let pending = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("pending heart belongs to chunk");
        assert_eq!(pending, &dummy_block_entity(&pos));
        assert!(region.native_block_entity(&pos).is_none());
        let mut bytes = Vec::new();
        rivet_nbt::nbt_io::write_compressed(pending, &mut bytes).unwrap();
        let loaded = rivet_nbt::nbt_io::read_compressed(
            bytes.as_slice(),
            &mut rivet_nbt::nbt_accounter::NbtAccounter::unlimited_heap(),
        )
        .unwrap();
        assert_eq!(loaded, *pending);
        assert!(region.set_block(&pos, Blocks::STONE.default_block_state(), UPDATE_ALL, 0));
        assert!(region.get_chunk(0, 0).get_block_entity_nbt(&pos).is_none());
    }

    #[test]
    fn same_block_pending_proto_entities_are_replaced_but_live_entities_are_preserved() {
        let mut region = feature_region();
        let chest_pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(
            &chest_pos,
            Blocks::CHEST.default_block_state(),
            UPDATE_ALL,
            0
        ));
        let mut pending_chest = dummy_block_entity(&chest_pos);
        pending_chest.put_string("id", "minecraft:chest");
        pending_chest.put_string("LootTable", "minecraft:stale");
        region.persist_block_entity_nbt(&chest_pos, pending_chest);
        region.materialize_block_entity(
            &chest_pos,
            Blocks::CHEST.default_block_state(),
            Blocks::CHEST.default_block_state(),
        );
        let replaced_chest = region
            .existing_block_entity_nbt(&chest_pos)
            .expect("fresh chest payload");
        assert_eq!(
            replaced_chest.get_string("id").map(String::as_str),
            Some("DUMMY")
        );
        assert!(replaced_chest.get("LootTable").is_none());

        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_block_entity_loot_table(
            &mut region,
            &chest_pos,
            7,
            "minecraft:live",
        );
        region.materialize_block_entity(
            &chest_pos,
            Blocks::CHEST.default_block_state(),
            Blocks::CHEST.default_block_state(),
        );
        let preserved_chest = region
            .existing_block_entity_nbt(&chest_pos)
            .expect("live chest payload");
        assert_eq!(
            preserved_chest.get_string("LootTable").map(String::as_str),
            Some("minecraft:live")
        );

        let spawner_pos = BlockPos::new(1, 64, 0);
        assert!(region.set_block(
            &spawner_pos,
            Blocks::SPAWNER.default_block_state(),
            UPDATE_ALL,
            0,
        ));
        let mut pending_spawner = dummy_block_entity(&spawner_pos);
        pending_spawner.put_string("id", "minecraft:mob_spawner");
        let mut spawn_data = CompoundTag::new();
        spawn_data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
        pending_spawner.put("SpawnData".to_string(), Tag::Compound(spawn_data));
        region.persist_block_entity_nbt(&spawner_pos, pending_spawner);
        region.materialize_block_entity(
            &spawner_pos,
            Blocks::SPAWNER.default_block_state(),
            Blocks::SPAWNER.default_block_state(),
        );
        let replaced_spawner = region
            .existing_block_entity_nbt(&spawner_pos)
            .expect("fresh spawner payload");
        assert_eq!(
            replaced_spawner.get_string("id").map(String::as_str),
            Some("DUMMY")
        );
        assert!(replaced_spawner.get("SpawnData").is_none());
    }

    #[test]
    #[should_panic(expected = "sum of weights must be <= 2147483647")]
    fn spawn_potential_overflow_is_rejected_during_native_promotion() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let mut tag = dummy_block_entity(&pos);
        tag.put_string("id", "minecraft:mob_spawner");
        let mut potentials = ListTag::new();
        for weight in [i32::MAX, 1] {
            let mut potential = CompoundTag::new();
            potential.put_int("weight", weight);
            let mut data = CompoundTag::new();
            data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
            potential.put("data".to_string(), Tag::Compound(data));
            potentials.list.push(Tag::Compound(potential));
        }
        tag.put("SpawnPotentials".to_string(), Tag::List(potentials));
        region.persist_block_entity_nbt(&pos, tag);
        region.promote_native_block_entity(&pos, Blocks::SPAWNER.default_block_state());
    }

    #[test]
    fn missing_or_wrong_typed_potentials_use_paper_weight_one_fallback() {
        for wrong_typed_potentials in [false, true] {
            let mut region = feature_region();
            let pos = BlockPos::new(0, 64, 0);
            assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
            let mut tag = dummy_block_entity(&pos);
            tag.put_string("id", "minecraft:mob_spawner");
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
    fn native_spawner_set_entity_id_retains_wide_numeric_state_like_actual_paper() {
        use rivet_nbt::tag_parser::parse_compound_fully;
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/spawner-entity-id.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let mut region = feature_region();
            let pos = BlockPos::new(8, 64, 8);
            assert!(region.set_block(
                &pos,
                Blocks::SPAWNER.default_block_state(),
                UPDATE_KNOWN_SHAPE,
                0
            ));
            let mut input = parse_compound_fully(row["input"].as_str().unwrap()).unwrap();
            input.put_int("x", 8);
            input.put_int("y", 64);
            input.put_int("z", 8);
            region.persist_block_entity_nbt(&pos, input);
            let mut random = LegacyRandomSource::new(42);
            let roll = WorldGenLevel::spawner_potential_weight(&region, &pos)
                .map(|total| random.next_int_bound(total));
            WorldGenLevel::set_spawner_entity(&mut region, &pos, "minecraft:zombie", roll);
            let entity = region.native_block_entity(&pos).unwrap();
            let rivet_world::block_entity::NativeBlockEntityData::Spawner(data) = entity.data()
            else {
                panic!("typed spawner")
            };
            assert_eq!(
                i64::from(data.spawn_count()),
                row["spawn_count"].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(data.min_spawn_delay()),
                row["min_delay"].as_i64().unwrap()
            );
            assert_eq!(
                entity.save_with_full_metadata(),
                parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
            );
            assert_eq!(random.next_long(), row["world_next_long"].as_i64().unwrap());
        }
    }

    #[test]
    fn spawner_partial_spawn_data_controls_selection_and_save_state() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));

        let mut tag = dummy_block_entity(&pos);
        tag.put_string("id", "minecraft:mob_spawner");
        let mut data = CompoundTag::new();
        let mut entity = CompoundTag::new();
        entity.put_int("custom_entity_field", 7);
        data.put("entity".to_string(), Tag::Compound(entity));
        data.put_int("custom_spawn_rules", 1);
        data.put_string("equipment", "malformed");
        data.put_int("unknown_spawn_data_field", 9);
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
        assert_eq!(saved_data.get_int("unknown_spawn_data_field"), None);
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
        tag.put_string("id", "minecraft:mob_spawner");
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
    fn spawner_equipment_keeps_valid_partial_slot_drop_chances() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));

        let mut tag = dummy_block_entity(&pos);
        tag.put_string("id", "minecraft:mob_spawner");
        let mut data = CompoundTag::new();
        data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
        let mut equipment = CompoundTag::new();
        equipment.put_string("loot_table", "minecraft:chest");
        let mut chances = CompoundTag::new();
        chances.put_float("mainhand", 0.5);
        chances.put_string("offhand", "malformed");
        chances.put_float("bogus", 0.75);
        equipment.put("slot_drop_chances".to_string(), Tag::Compound(chances));
        data.put("equipment".to_string(), Tag::Compound(equipment));
        tag.put("SpawnData".to_string(), Tag::Compound(data));
        region.persist_block_entity_nbt(&pos, tag);

        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:zombie",
            None,
        );
        let saved_data = region
            .existing_block_entity_nbt(&pos)
            .and_then(|tag| tag.get_compound("SpawnData").cloned())
            .expect("SpawnData retained");
        let chances = saved_data
            .get_compound("equipment")
            .and_then(|equipment| equipment.get_compound("slot_drop_chances"))
            .expect("valid partial chance map retained");
        assert_eq!(chances.get_float("mainhand"), Some(0.5));
        assert!(chances.get("offhand").is_none());
        assert!(chances.get("bogus").is_none());
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
        tag.put_string("id", "minecraft:mob_spawner");
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
            None,
            "SpawnData.CODEC drops unknown top-level fields during re-encode"
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
    fn set_block_marks_only_paper_post_process_states() {
        let mut region = feature_region();
        let pos = BlockPos::new(4, 64, 5);
        let section_index = region.get_chunk(0, 0).get_section_index(pos.get_y()) as usize;
        let packed_self =
            ((pos.get_x() & 15) | ((pos.get_y() & 15) << 4) | ((pos.get_z() & 15) << 8)) as i16;
        let above = pos.above();
        let packed_above = ((above.get_x() & 15)
            | ((above.get_y() & 15) << 4)
            | ((above.get_z() & 15) << 8)) as i16;

        let brown = BlockState::of(BlockId::from_name("minecraft:brown_mushroom").unwrap());
        assert!(region.set_block(&pos, brown, 0, 0));
        assert!(region.get_chunk(0, 0).get_post_processing()[section_index].contains(&packed_self));

        let soul_sand = BlockState::of(BlockId::from_name("minecraft:soul_sand").unwrap());
        assert!(region.set_block(&pos, soul_sand, 0, 0));
        assert!(
            region.get_chunk(0, 0).get_post_processing()[section_index].contains(&packed_above)
        );

        let marked_before_known_shape = region
            .get_chunk(0, 0)
            .get_post_processing()
            .iter()
            .map(Vec::len)
            .sum::<usize>();
        let red = BlockState::of(BlockId::from_name("minecraft:red_mushroom").unwrap());
        assert!(region.set_block(&pos, red, UPDATE_KNOWN_SHAPE, 0));
        let marked_after_known_shape = region
            .get_chunk(0, 0)
            .get_post_processing()
            .iter()
            .map(Vec::len)
            .sum::<usize>();
        assert_eq!(marked_after_known_shape, marked_before_known_shape);
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

    /// `scheduleTick(BlockPos, Block, int)` retains a zero-delay stored tick
    /// and applies ProtoChunkTicks' `(type, position)` deduplication.
    #[test]
    fn schedule_block_tick_is_retained_and_deduplicated() {
        let mut region = feature_region();
        let pos = BlockPos::new(8, 64, 9);
        let block = Blocks::CAVE_AIR;
        <WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::schedule_block_tick(
            &mut region,
            &pos,
            block,
            0,
        );
        <WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::schedule_block_tick(
            &mut region,
            &pos,
            block,
            0,
        );
        let ticks = region.scheduled_block_ticks();
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].r#type, block);
        assert_eq!(ticks[0].pos, pos);
        assert_eq!(ticks[0].delay, 0);
    }

    /// `scheduleTick(BlockPos, Fluid, int)` retains the zero-delay fluid tick
    /// in the owning chunk and applies `ProtoChunkTicks` deduplication.
    #[test]
    fn schedule_fluid_tick_is_retained_and_deduplicated() {
        let mut region = feature_region();
        let pos = BlockPos::new(8, 64, 9);
        let fluid = FluidId(4);
        <WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::schedule_tick(
            &mut region,
            &pos,
            fluid,
            0,
        );
        <WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::schedule_tick(
            &mut region,
            &pos,
            fluid,
            0,
        );
        let ticks = region.get_chunk(0, 0).get_fluid_ticks().scheduled_ticks();
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].r#type, fluid);
        assert_eq!(ticks[0].pos, pos);
        assert_eq!(ticks[0].delay, 0);
    }

    /// `markPosForPostProcessing` (WorldGenRegion.java:410) — the private
    /// method routes `this.getChunk(blockPos).markPosForPostProcessing(blockPos)`
    /// through the region's gated chunk view and mutates that chunk's section
    /// offset list.
    #[test]
    fn mark_pos_for_post_processing_serves_an_in_ring_position() {
        let mut region = feature_region();
        let pos = BlockPos::new(8, 64, 9);
        // Chunk (0, 0) — inside the cache ring, so the write is served.
        region.mark_pos_for_post_processing(&pos);
        let chunk = region.get_chunk(0, 0);
        let offsets = chunk.get_post_processing();
        let section_index = chunk.get_section_index(pos.get_y()) as usize;
        let packed =
            ((pos.get_x() & 15) | ((pos.get_y() & 15) << 4) | ((pos.get_z() & 15) << 8)) as i16;
        assert_eq!(offsets[section_index], vec![packed]);
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

    /// Paper's `FluidState.isRandomlyTicking` is true for both flowing and
    /// source lava. The generated block-state table carries the fluid id but
    /// not this fluid behavior flag, so exercise every reachable lava state
    /// rather than only the source default.
    #[test]
    fn lava_states_are_randomly_ticking_fluids() {
        let mut found_flowing = false;
        let mut found_source = false;
        for raw in 0..rivet_registry::generated::block_states::BLOCK_STATE_COUNT {
            let state = BlockState::new(StateId(raw));
            match state.fluid_id() {
                3 => {
                    found_flowing = true;
                    assert!(!state.fluid_empty());
                    assert!(fluid_is_randomly_ticking(&StateId(raw)));
                    assert!(fluid_is_randomly_ticking_blockstate(&state));
                }
                4 => {
                    found_source = true;
                    assert!(!state.fluid_empty());
                    assert!(fluid_is_randomly_ticking(&StateId(raw)));
                    assert!(fluid_is_randomly_ticking_blockstate(&state));
                }
                _ => {}
            }
        }
        assert!(found_flowing, "the generated table must carry flowing lava");
        assert!(found_source, "the generated table must carry source lava");
    }
}
