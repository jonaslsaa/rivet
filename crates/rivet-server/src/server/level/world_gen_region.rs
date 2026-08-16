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
//! `RivetTodo` pointing at the owning unit. MonsterRoom's chest/spawner writes
//! use the chunk's pending block-entity NBT authority, retaining feature state
//! with the chunk instead of a region-local side map.
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

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_registry::access::RegistryAccess;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};
use rivet_registry::fluid_id::FluidId;
use rivet_registry::generated::block_behaviors::{
    BEHAVIOR_FLAG_FLUID_EMPTY, BEHAVIOR_FLAG_RANDOM_TICKING, behavior_of,
};
use rivet_registry::generated::block_states::StateId;
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::holder::Holder;
use rivet_util::StaticCache2D;
use rivet_util::mth;
use rivet_util::util::log_and_pause_if_in_ide;
use rivet_world::biome::biome_manager::{BiomeManager, NoiseBiomeSource};
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::status::{ChunkStatus, ChunkStep};
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
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

const TRAPPED_CHEST_BLOCK_ID: BlockId = BlockId(470);

fn is_feature_block_entity(state: BlockState) -> bool {
    matches!(
        state.block(),
        id if id == Blocks::CHEST.id()
            || id == TRAPPED_CHEST_BLOCK_ID
            || id == Blocks::SPAWNER.id()
    )
}

fn pending_is_dummy(tag: &CompoundTag) -> bool {
    tag.get_string("id")
        .is_some_and(|id| id == rivet_world::chunk::chunk_access::DUMMY_BLOCK_ENTITY_ID)
}

const LIGHT_RANGE_DEFAULT: (i32, i32) = (0, 15);
const EQUIPMENT_SLOTS: [&str; 8] = [
    "mainhand", "offhand", "feet", "legs", "chest", "head", "body", "saddle",
];

fn normalized_spawn_data(spawn_data: &CompoundTag) -> Option<CompoundTag> {
    let Tag::Compound(entity) = spawn_data.get("entity")? else {
        return None;
    };

    let mut normalized = CompoundTag::new();
    let mut entity = entity.clone();
    normalize_spawn_data_entity_id_in_place(&mut entity);
    normalized.put("entity".to_string(), Tag::Compound(entity));
    if let Some(rules) = spawn_data
        .get("custom_spawn_rules")
        .and_then(normalize_custom_spawn_rules)
    {
        normalized.put("custom_spawn_rules".to_string(), Tag::Compound(rules));
    }
    if let Some(equipment) = spawn_data.get("equipment").and_then(normalize_equipment) {
        normalized.put("equipment".to_string(), Tag::Compound(equipment));
    }
    Some(normalized)
}

fn normalize_spawn_data_entity_id_in_place(entity: &mut CompoundTag) {
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
}

fn normalize_custom_spawn_rules(tag: &Tag) -> Option<CompoundTag> {
    let Tag::Compound(rules) = tag else {
        return None;
    };
    let mut normalized = CompoundTag::new();
    for name in ["block_light_limit", "sky_light_limit"] {
        let Some(raw) = rules.get(name) else {
            continue;
        };
        let value = decoded_light_range(raw)?;
        if value != LIGHT_RANGE_DEFAULT {
            normalized.put(name.to_string(), canonical_light_range(value));
        }
    }
    Some(normalized)
}

fn decoded_light_range(tag: &Tag) -> Option<(i32, i32)> {
    let values = match tag {
        Tag::List(list) if list.list.len() == 2 => list.list[0].as_int().zip(list.list[1].as_int()),
        Tag::Compound(range) => range
            .get_int("min_inclusive")
            .zip(range.get_int("max_inclusive")),
        value => value.as_int().map(|value| (value, value)),
    }?;
    let (min, max) = values;
    ((0..=15).contains(&min) && (0..=15).contains(&max) && min <= max).then_some((min, max))
}

fn canonical_light_range((min, max): (i32, i32)) -> Tag {
    if min == max {
        Tag::Int(rivet_nbt::int_tag::IntTag::value_of(min))
    } else {
        Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![
            Tag::Int(rivet_nbt::int_tag::IntTag::value_of(min)),
            Tag::Int(rivet_nbt::int_tag::IntTag::value_of(max)),
        ]))
    }
}

fn java_float_equals(left: f32, right: f32) -> bool {
    let bits = |value: f32| {
        if value.is_nan() {
            0x7fc0_0000
        } else {
            value.to_bits()
        }
    };
    bits(left) == bits(right)
}

fn normalize_equipment(tag: &Tag) -> Option<CompoundTag> {
    let Tag::Compound(equipment) = tag else {
        return None;
    };
    let loot_table = equipment
        .get_string("loot_table")
        .and_then(|value| Identifier::try_parse_result(value).ok().flatten())?;
    let mut normalized = CompoundTag::new();
    normalized.put_string("loot_table", &loot_table.to_string());

    let Some(slot_drop_chances) = equipment.get("slot_drop_chances") else {
        return Some(normalized);
    };
    if let Some(chance) = slot_drop_chances.as_float() {
        normalized.put_float("slot_drop_chances", chance);
        return Some(normalized);
    }
    let Tag::Compound(chances) = slot_drop_chances else {
        return None;
    };
    let mut values = Vec::with_capacity(chances.tags.len());
    for (slot, chance) in &chances.tags {
        if !EQUIPMENT_SLOTS.contains(&slot.as_str()) {
            return None;
        }
        values.push((slot.clone(), chance.as_float()?));
    }
    if values.len() == EQUIPMENT_SLOTS.len()
        && EQUIPMENT_SLOTS
            .iter()
            .all(|slot| values.iter().any(|(name, _)| name == slot))
        && values
            .iter()
            .all(|(_, chance)| java_float_equals(*chance, values[0].1))
    {
        normalized.put_float("slot_drop_chances", values[0].1);
    } else {
        let mut canonical = CompoundTag::new();
        for (slot, chance) in values {
            canonical.put_float(&slot, chance);
        }
        normalized.put("slot_drop_chances".to_string(), Tag::Compound(canonical));
    }
    Some(normalized)
}

type SpawnPotentialEntry = (CompoundTag, i32);
type DecodedSpawnPotentials = (Vec<SpawnPotentialEntry>, i32);

fn decoded_spawn_potentials(tag: &CompoundTag) -> Result<Option<DecodedSpawnPotentials>, ()> {
    let Some(list) = tag.get_list("SpawnPotentials") else {
        return Ok(None);
    };
    let mut total = 0_i64;
    let mut entries = Vec::with_capacity(list.list.len());
    for raw_entry in &list.list {
        let Tag::Compound(entry) = raw_entry else {
            continue;
        };
        let Some(weight) = entry.get_int("weight") else {
            continue;
        };
        if weight < 0 {
            continue;
        }
        let Some(data) = entry.get_compound("data").and_then(normalized_spawn_data) else {
            continue;
        };
        total = total.checked_add(i64::from(weight)).ok_or(())?;
        if total > i64::from(i32::MAX) {
            return Err(());
        }
        entries.push((data, weight));
    }
    Ok(Some((entries, total as i32)))
}

fn fallback_potential_weight(tag: &CompoundTag) -> Option<i32> {
    if pending_is_dummy(tag) && !tag.contains("SpawnData") && !tag.contains("SpawnPotentials") {
        None
    } else {
        Some(1)
    }
}

fn checked_spawn_potential_weight(tag: &CompoundTag) -> Option<i32> {
    if pending_is_dummy(tag) || normalized_spawn_data_from_tag(tag).is_some() {
        return None;
    }
    match decoded_spawn_potentials(tag) {
        Ok(Some((_, total))) if total > 0 => Some(total),
        Ok(Some((_, _))) => None,
        Ok(None) => fallback_potential_weight(tag),
        Err(()) => None,
    }
}

fn normalized_spawn_data_from_tag(tag: &CompoundTag) -> Option<CompoundTag> {
    if pending_is_dummy(tag) {
        return None;
    }
    tag.get("SpawnData")
        .and_then(Tag::as_compound)
        .and_then(normalized_spawn_data)
}

fn selected_spawn_data(tag: &CompoundTag, mut roll: i32) -> CompoundTag {
    let Ok(Some((entries, _))) = decoded_spawn_potentials(tag) else {
        return CompoundTag::new();
    };
    for (data, weight) in entries {
        if roll < weight {
            return data;
        }
        roll -= weight;
    }
    CompoundTag::new()
}

fn persist_chest_tag(
    tag: &CompoundTag,
    pos: &BlockPos,
    seed: i64,
    loot_table: &str,
    fallback_id: &str,
) -> CompoundTag {
    let mut canonical = CompoundTag::new();
    canonical.put_int("x", pos.get_x());
    canonical.put_int("y", pos.get_y());
    canonical.put_int("z", pos.get_z());
    if let Some(id) = tag.get_string("id").filter(|id| *id == fallback_id) {
        canonical.put_string("id", id);
    } else {
        canonical.put_string("id", fallback_id);
    }
    canonical.put_string("LootTable", loot_table);
    if seed != 0 {
        canonical.put_long("LootTableSeed", seed);
    }
    canonical
}

fn spawner_numeric_values(tag: &CompoundTag) -> [i32; 7] {
    [
        tag.get_int_or("Paper.Delay", tag.get_short_or("Delay", 20) as i32),
        tag.get_int_or("Paper.MinSpawnDelay", tag.get_int_or("MinSpawnDelay", 200)),
        tag.get_int_or("Paper.MaxSpawnDelay", tag.get_int_or("MaxSpawnDelay", 800)),
        tag.get_int_or("SpawnCount", 4),
        tag.get_int_or("MaxNearbyEntities", 6),
        tag.get_int_or("RequiredPlayerRange", 16),
        tag.get_int_or("SpawnRange", 4),
    ]
}

fn persist_spawner_tag(
    tag: &CompoundTag,
    pos: &BlockPos,
    entity_id: &str,
    potential_roll: Option<i32>,
) -> CompoundTag {
    let was_dummy = pending_is_dummy(tag);
    let [
        delay,
        min_spawn_delay,
        max_spawn_delay,
        spawn_count,
        max_nearby_entities,
        required_player_range,
        spawn_range,
    ] = if was_dummy {
        [20, 200, 800, 4, 6, 16, 4]
    } else {
        spawner_numeric_values(tag)
    };
    let mut canonical = CompoundTag::new();
    canonical.put_int("x", pos.get_x());
    canonical.put_int("y", pos.get_y());
    canonical.put_int("z", pos.get_z());
    canonical.put_string("id", "minecraft:mob_spawner");
    if delay > i16::MAX as i32 {
        canonical.put_int("Paper.Delay", delay);
    }
    canonical.put_short("Delay", delay.min(i16::MAX as i32) as i16);
    if min_spawn_delay > i16::MAX as i32 || max_spawn_delay > i16::MAX as i32 {
        canonical.put_int("Paper.MinSpawnDelay", min_spawn_delay);
        canonical.put_int("Paper.MaxSpawnDelay", max_spawn_delay);
    }
    canonical.put_short("MinSpawnDelay", min_spawn_delay.min(i16::MAX as i32) as i16);
    canonical.put_short("MaxSpawnDelay", max_spawn_delay.min(i16::MAX as i32) as i16);
    canonical.put_short("SpawnCount", spawn_count as i16);
    canonical.put_short("MaxNearbyEntities", max_nearby_entities as i16);
    canonical.put_short("RequiredPlayerRange", required_player_range as i16);
    canonical.put_short("SpawnRange", spawn_range as i16);

    let mut spawn_data = if was_dummy {
        CompoundTag::new()
    } else {
        normalized_spawn_data_from_tag(tag).unwrap_or_else(|| {
            potential_roll.map_or_else(CompoundTag::new, |roll| selected_spawn_data(tag, roll))
        })
    };
    spawn_data
        .get_compound_or_empty_mut("entity")
        .put_string("id", entity_id);
    canonical.put("SpawnData".to_string(), Tag::Compound(spawn_data));
    canonical.put(
        "SpawnPotentials".to_string(),
        Tag::List(rivet_nbt::list_tag::ListTag::new()),
    );
    canonical
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
/// `'a`. The pure chunk-view methods live on the generic [`impl<'a, T, B, S>`]
/// (so the worldgen executor's borrow-carrying region can use them); the dense
/// block-state methods and the [`WorldGenLevel`] impl live on the
/// `StateId`/`ServerBiomeId`/`StructureKey` specialization. `'a` is the
/// shortest lifetime the cached holders borrow — `'static` for a region over
/// owning holders (the server value layer, and the [`WorldGenLevel`] trait's
/// `'static` bound), a scoped borrow for the executor's center-chunk region.
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
}

fn pending_entity_tag_matches(tag: &CompoundTag, state_matches: bool, entity_id: &str) -> bool {
    if pending_is_dummy(tag) {
        return state_matches;
    }
    if !state_matches {
        return false;
    }
    let Some(actual_id) = tag.get_string("id") else {
        return false;
    };
    let canonical_match = Identifier::try_parse_result(actual_id)
        .ok()
        .flatten()
        .is_some_and(|identifier| identifier.to_string() == entity_id);
    canonical_match
        && (entity_id != "minecraft:mob_spawner" || decoded_spawn_potentials(tag).is_ok())
}

impl<'a, B, S> WorldGenRegion<'a, BlockState, B, S>
where
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    fn pending_block_entity_matches(&self, pos: &BlockPos, entity_id: &str) -> bool {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        let Some(tag) = self.get_chunk(chunk_x, chunk_z).get_block_entity_nbt(pos) else {
            return false;
        };
        let block = self.get_block_state_worldgen(pos).block();
        let state_matches = match entity_id {
            "minecraft:chest" => block == Blocks::CHEST.id(),
            "minecraft:trapped_chest" => block == TRAPPED_CHEST_BLOCK_ID,
            "minecraft:mob_spawner" => block == Blocks::SPAWNER.id(),
            _ => false,
        };
        pending_entity_tag_matches(tag, state_matches, entity_id)
    }

    fn update_pending_block_entity(
        &mut self,
        pos: &BlockPos,
        update: impl FnOnce(CompoundTag) -> CompoundTag,
    ) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        let Some(tag) = self
            .get_chunk(chunk_x, chunk_z)
            .get_block_entity_nbt(pos)
            .cloned()
        else {
            return;
        };
        self.get_chunk_mut(chunk_x, chunk_z)
            .set_block_entity_nbt(update(tag));
    }

    fn is_within_write_zone_worldgen(&self, pos: &BlockPos) -> bool {
        mth::abs_i32(
            self.center_chunk_x
                .wrapping_sub(SectionPos::block_to_section_coord(pos.get_x())),
        ) <= self.write_radius
            && mth::abs_i32(
                self.center_chunk_z
                    .wrapping_sub(SectionPos::block_to_section_coord(pos.get_z())),
            ) <= self.write_radius
    }

    fn ensure_can_write_worldgen(&self, pos: &BlockPos) -> bool {
        if !self.is_within_write_zone_worldgen(pos) {
            let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
            let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
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

    fn warn_if_read_outside_write_zone_worldgen(&self, chunk_x: i32, chunk_z: i32) {
        if (self.center_chunk_x != chunk_x || self.center_chunk_z != chunk_z)
            && (mth::abs_i32(self.center_chunk_x.wrapping_sub(chunk_x)) > self.write_radius
                || mth::abs_i32(self.center_chunk_z.wrapping_sub(chunk_z)) > self.write_radius)
        {
            let read_distance = mth::abs_max(
                mth::abs_i32(self.center_chunk_x.wrapping_sub(chunk_x)),
                mth::abs_i32(self.center_chunk_z.wrapping_sub(chunk_z)),
            );
            log_and_pause_if_in_ide(&format!(
                "Detected unsafe terrain read during worldgen: reading from chunk [{}, {}] while generating chunk [{}, {}] (distance: {}, write radius: {}), step: {}",
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

    fn set_block_worldgen(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        if !self.ensure_can_write_worldgen(pos) {
            return false;
        }
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        let old_state = self.get_block_state_worldgen(pos);
        let persisted_status = self.cache.get(chunk_x, chunk_z).get_persisted_status();
        let chunk = self.get_chunk_mut(chunk_x, chunk_z);
        let in_build_height = !chunk.is_outside_build_height(pos.get_y());
        if in_build_height {
            let section_index = chunk.get_section_index(pos.get_y());
            let section = chunk.get_section_mut(section_index as usize);
            section.set_block_state(
                pos.get_x() & 15,
                pos.get_y() & 15,
                pos.get_z() & 15,
                state,
                &|state| resolve_state_flags(state).is_air,
                &|state| state.is_in_tag("minecraft:randomly_ticking"),
                &|state| state.fluid_id() == 0,
                &|_| false,
                &|_| false,
            );
            if let Some(status) = persisted_status {
                chunk.update_heightmaps_after(
                    status.heightmaps_after(),
                    pos.get_x() & 15,
                    pos.get_y(),
                    pos.get_z() & 15,
                    resolve_state_flags(&state),
                );
            }
        }
        let chunk = self.get_chunk_mut(chunk_x, chunk_z);
        if is_feature_block_entity(state) {
            chunk.set_dummy_block_entity_nbt(pos);
        } else if is_feature_block_entity(old_state) {
            chunk.remove_block_entity_nbt(pos);
        }
        let _ = flags;
        true
    }

    fn get_block_state_worldgen(&self, pos: &BlockPos) -> BlockState {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.warn_if_read_outside_write_zone_worldgen(chunk_x, chunk_z);
        let chunk = self.get_chunk(chunk_x, chunk_z);
        if chunk.is_outside_build_height(pos.get_y()) {
            return BlockState::of(BlockId(794));
        }
        let section = chunk.get_section(chunk.get_section_index(pos.get_y()) as usize);
        if section.non_empty_block_count() == 0 {
            return BlockState::of(BlockId(0));
        }
        section.get_block_state(pos.get_x() & 15, pos.get_y() & 15, pos.get_z() & 15)
    }
}

impl<B, S> LevelHeightAccessor for WorldGenRegion<'_, BlockState, B, S>
where
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    fn get_height(&self) -> i32 {
        self.height
    }

    fn get_min_y(&self) -> i32 {
        self.min_y
    }
}

/// The dense server specialization — the block-state methods and the
/// [`WorldGenLevel`] facade over the server's dense chunk value types.
///
/// Split from the generic impl because the block-state spine is
/// `StateId`-specific: the region's reads/writes target `StateId`/`ServerBiomeId`
/// sections. The generic `BlockState` facade above serves the executor's
/// borrow-carrying region, while this specialization serves dense server
/// holders.
impl WorldGenRegion<'_, StateId, ServerBiomeId, StructureKey> {
    fn pending_block_entity_matches(&self, pos: &BlockPos, entity_id: &str) -> bool {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        let Some(tag) = self.get_chunk(chunk_x, chunk_z).get_block_entity_nbt(pos) else {
            return false;
        };
        let block = self.get_block_state(pos).block();
        let state_matches = match entity_id {
            "minecraft:chest" => block == Blocks::CHEST.id(),
            "minecraft:trapped_chest" => block == TRAPPED_CHEST_BLOCK_ID,
            "minecraft:mob_spawner" => block == Blocks::SPAWNER.id(),
            _ => false,
        };
        pending_entity_tag_matches(tag, state_matches, entity_id)
    }

    fn update_pending_block_entity(
        &mut self,
        pos: &BlockPos,
        update: impl FnOnce(CompoundTag) -> CompoundTag,
    ) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        let Some(tag) = self
            .get_chunk(chunk_x, chunk_z)
            .get_block_entity_nbt(pos)
            .cloned()
        else {
            return;
        };
        self.get_chunk_mut(chunk_x, chunk_z)
            .set_block_entity_nbt(update(tag));
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
        let old_state = self.get_block_state(pos);
        // The base `ChunkAccess` carries no persisted status (the concrete chunk
        // types do), so the `heightmapsAfter()` set the write must update is
        // threaded from the holder seam (Java's `ProtoChunk.setBlockState` reads
        // `getPersistedStatus().heightmapsAfter()`).
        let persisted_status = self.cache.get(chunk_x, chunk_z).get_persisted_status();
        let chunk = self.get_chunk_mut(chunk_x, chunk_z);
        // `oldState` — the previous state `chunk.setBlockState` returns; the
        // block-entity removal (`oldState.hasBlockEntity()`) and POI update
        // read it, so the write retains it for those deferred seams (#185).
        write_block(chunk, pos, block_state, persisted_status);
        let chunk = self.get_chunk_mut(chunk_x, chunk_z);
        if is_feature_block_entity(block_state) {
            chunk.set_dummy_block_entity_nbt(pos);
        } else if is_feature_block_entity(old_state) {
            chunk.remove_block_entity_nbt(pos);
        }
        true
    }

    /// `WorldGenRegion.removeBlock(BlockPos, boolean)` —
    /// `setBlock(pos, Blocks.AIR.defaultBlockState(), Block.UPDATE_ALL)` with
    /// `Block.UPDATE_LIMIT` from `LevelWriter`'s three-argument overload.
    pub fn remove_block(&mut self, pos: &BlockPos, _moved_by_piston: bool) -> bool {
        self.set_block(pos, BlockState::new(StateId(0)), UPDATE_ALL, UPDATE_LIMIT)
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
                "Detected unsafe terrain read during worldgen: reading from chunk [{}, {}] while generating chunk [{}, {}] (distance: {}, write radius: {}), step: {}",
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

    /// `BlockGetter.getBlockState(BlockPos)` — the gated chunk block read.
    ///
    /// Inherent here (not only on the [`WorldGenLevel`] impl) because dense
    /// methods such as [`is_state_at_position`](Self::is_state_at_position)
    /// read it off a `&WorldGenRegion<'_, …>` whose region lifetime is not
    /// `'static`; the trait impl (pinned to `WorldGenRegion<'static, …>`)
    /// delegates back to this method.
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

/// The worldgen `WorldGenLevel` facade over the composed region. The trait no
/// longer carries a `'static` bound: Java's FEATURES call operates on the
/// executor-scoped center-chunk borrow, and the Rust trait now follows that
/// lifetime instead of excluding the production composition.
impl<'a, B, S> WorldGenLevel for WorldGenRegion<'a, BlockState, B, S>
where
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    fn get_seed(&self) -> i64 {
        self.seed
    }

    fn ensure_can_write(&self, pos: &BlockPos) -> bool {
        self.ensure_can_write_worldgen(pos)
    }

    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        self.set_block_worldgen(pos, state, flags)
    }

    fn destroy_block(&mut self, pos: &BlockPos, _drop: bool) -> bool {
        !self.get_block_state_worldgen(pos).is_air()
            && self.set_block_worldgen(pos, BlockState::of(BlockId(0)), UPDATE_ALL as u32)
    }

    fn registry_access(&self) -> RegistryAccess {
        self.registry_access.clone()
    }

    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.get_block_state_worldgen(pos)
    }

    fn get_biome(&self, pos: &BlockPos) -> Holder<BiomeId> {
        self.get_biome_manager().get_biome(pos)
    }

    fn get_height_at(&self, ty: Types, x: i32, z: i32) -> i32 {
        let chunk_x = SectionPos::block_to_section_coord(x);
        let chunk_z = SectionPos::block_to_section_coord(z);
        self.warn_if_read_outside_write_zone_worldgen(chunk_x, chunk_z);
        let chunk = self.get_chunk(chunk_x, chunk_z);
        match chunk.heightmaps()[ty as usize].as_ref() {
            Some(heightmap) => heightmap.get_height_at(x & 15, z & 15, chunk.get_min_y()) + 1,
            None => chunk.get_min_y() + 1,
        }
    }

    fn is_empty_block(&self, pos: &BlockPos) -> bool {
        self.get_block_state_worldgen(pos).is_air()
    }

    fn get_sea_level(&self) -> i32 {
        self.sea_level
    }

    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.get_chunk_mut(chunk_x, chunk_z)
            .mark_pos_for_post_processing(pos);
    }

    fn is_randomizable_container(&self, pos: &BlockPos) -> bool {
        self.pending_block_entity_matches(pos, "minecraft:chest")
            || self.pending_block_entity_matches(pos, "minecraft:trapped_chest")
    }

    fn set_block_entity_loot_table(&mut self, pos: &BlockPos, seed: i64, loot_table: &str) {
        let fallback_id = if self.get_block_state_worldgen(pos).block() == TRAPPED_CHEST_BLOCK_ID {
            "minecraft:trapped_chest"
        } else {
            "minecraft:chest"
        };
        self.update_pending_block_entity(pos, |tag| {
            persist_chest_tag(&tag, pos, seed, loot_table, fallback_id)
        });
    }

    fn is_spawner_block_entity(&self, pos: &BlockPos) -> bool {
        self.pending_block_entity_matches(pos, "minecraft:mob_spawner")
    }

    fn spawner_potential_weight(&self, pos: &BlockPos) -> Option<i32> {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.get_chunk(chunk_x, chunk_z)
            .get_block_entity_nbt(pos)
            .and_then(checked_spawn_potential_weight)
    }

    fn set_spawner_entity(&mut self, pos: &BlockPos, entity_id: &str, potential_roll: Option<i32>) {
        self.update_pending_block_entity(pos, |tag| {
            persist_spawner_tag(&tag, pos, entity_id, potential_roll)
        });
    }
}

/// The `WorldGenLevel` facade over the dense specialization. The trait is
/// implemented for owning server holders as well as the scoped worldgen region
/// above; the two value types remain separate because the server chunk map uses
/// dense `StateId` sections while the executor composes `BlockState` sections.
impl WorldGenLevel for WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> {
    /// `WorldGenLevel.getSeed()`.
    fn get_seed(&self) -> i64 {
        self.seed
    }

    /// `WorldGenLevel.ensureCanWrite(BlockPos)` — the write-radius gate.
    fn ensure_can_write(&self, pos: &BlockPos) -> bool {
        WorldGenRegion::ensure_can_write(self, pos)
    }

    /// `LevelWriter.setBlock(BlockPos, BlockState, int)` — the 3-arg trait
    /// form, delegating to the 4-arg write with Java's `LevelWriter` default
    /// `updateLimit = Block.UPDATE_LIMIT`.
    ///
    /// `&mut self` is the trait's write contract; the delegated write is the
    /// region's [`set_block`](Self::set_block) (write-radius-gated chunk
    /// section write, with the `UPDATE_*`-gated side-effects deferred).
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        WorldGenRegion::set_block(self, pos, state, flags as i32, UPDATE_LIMIT)
    }

    /// `LevelAccessor.destroyBlock(BlockPos, boolean)` — Java's chain
    /// `destroyBlock(pos, drop)` → `(pos, drop, null)` →
    /// `(pos, drop, null, UPDATE_LIMIT)` ends in
    /// `!getBlockState(pos).isAir() && setBlock(pos, AIR, UPDATE_ALL,
    /// updateLimit)` (WorldGenRegion.java:252). The `dropResources` flag is
    /// unread — the entity/`breakBlock` side-effects defer.
    fn destroy_block(&mut self, pos: &BlockPos, _drop: bool) -> bool {
        !self.get_block_state(pos).is_air()
            && WorldGenRegion::set_block(
                self,
                pos,
                BlockState::new(StateId(0)),
                UPDATE_ALL,
                UPDATE_LIMIT,
            )
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
        self.pending_block_entity_matches(pos, "minecraft:chest")
            || self.pending_block_entity_matches(pos, "minecraft:trapped_chest")
    }

    fn set_block_entity_loot_table(&mut self, pos: &BlockPos, seed: i64, loot_table: &str) {
        let fallback_id = if self.get_block_state(pos).block() == TRAPPED_CHEST_BLOCK_ID {
            "minecraft:trapped_chest"
        } else {
            "minecraft:chest"
        };
        self.update_pending_block_entity(pos, |tag| {
            persist_chest_tag(&tag, pos, seed, loot_table, fallback_id)
        });
    }

    fn is_spawner_block_entity(&self, pos: &BlockPos) -> bool {
        self.pending_block_entity_matches(pos, "minecraft:mob_spawner")
    }

    fn spawner_potential_weight(&self, pos: &BlockPos) -> Option<i32> {
        let chunk_x = SectionPos::block_to_section_coord(pos.get_x());
        let chunk_z = SectionPos::block_to_section_coord(pos.get_z());
        self.get_chunk(chunk_x, chunk_z)
            .get_block_entity_nbt(pos)
            .and_then(checked_spawn_potential_weight)
    }

    fn set_spawner_entity(&mut self, pos: &BlockPos, entity_id: &str, potential_roll: Option<i32>) {
        self.update_pending_block_entity(pos, |tag| {
            persist_spawner_tag(&tag, pos, entity_id, potential_roll)
        });
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
        WorldGenRegion::get_block_state(self, pos)
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
    /// Java's `WorldGenRegion.getHeight` (WorldGenRegion.java:514) is
    /// `getChunk(...).getHeight(type, x & 15, z & 15) + 1` — the same `+ 1`
    /// `Level.getHeight` applies (Level.java:1289) — i.e. the chunk's
    /// `getFirstAvailable` height, one ABOVE the topmost opaque block.
    /// [`Heightmap::get_height_at`] is the Java `ChunkAccess.getHeight` value
    /// (`getFirstAvailable(x, z) - 1` — the topmost opaque block's Y), so the
    /// port adds `+ 1` to recover the region method's contract.
    ///
    /// When the entry is absent the port cannot prime it here —
    /// `ChunkAccess::prime_heightmaps` takes `&mut` (`ChunkAccess::get_height_at`
    /// is the `&mut`-typed half) — so it returns the value the chunk's primed
    /// heightmap would carry: `minY + 1` for the superflat floor whose topmost
    /// block sits at `minY` (first available = `minY + 1`). A genuinely all-air
    /// column would read `minY`, deferred with the `&mut` seam (RivetTodo
    /// #228). Since `write_block` primes and updates the `heightmapsAfter()`
    /// entries on every write, the None branch is only a never-written chunk;
    /// written chunks return the real post-write height.
    fn get_height_at(&self, ty: Types, x: i32, z: i32) -> i32 {
        let chunk_x = SectionPos::block_to_section_coord(x);
        let chunk_z = SectionPos::block_to_section_coord(z);
        self.warn_if_read_outside_write_zone(chunk_x, chunk_z);
        let chunk = self.get_chunk(chunk_x, chunk_z);
        match chunk.heightmaps()[ty as usize].as_ref() {
            Some(heightmap) => heightmap.get_height_at(x & 15, z & 15, chunk.get_min_y()) + 1,
            None => chunk.get_min_y() + 1,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::tag::Tag;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::core::QuartPos;
    use rivet_registry::root::AnyBox;
    use rivet_registry::{Identifier, ResourceKey};
    use rivet_util::StaticCache2D;
    use rivet_world::block::blocks::Blocks;
    use rivet_world::chunk::status::GENERATION_PYRAMID;
    use rivet_world::chunk::upgrade_data::UpgradeData;
    use rivet_world::level::height_accessor::create as create_accessor;
    use rivet_world::levelgen::feature::registry_keys::PLACED_FEATURE;

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

    fn spawner_tag_mut<'a>(
        region: &'a mut WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey>,
        pos: &BlockPos,
    ) -> &'a mut CompoundTag {
        let tag = region
            .get_chunk_mut(0, 0)
            .get_block_entity_nbt_mut(pos)
            .expect("spawner block entity tag");
        tag.put_string("id", "minecraft:mob_spawner");
        tag
    }

    fn set_spawn_potentials(
        region: &mut WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey>,
        pos: &BlockPos,
        weights: &[i32],
    ) {
        let entries = weights
            .iter()
            .map(|weight| {
                let mut data = CompoundTag::new();
                let mut entity = CompoundTag::new();
                entity.put_string("id", "minecraft:zombie");
                data.put("entity".to_string(), Tag::Compound(entity));
                let mut entry = CompoundTag::new();
                entry.put("data".to_string(), Tag::Compound(data));
                entry.put_int("weight", *weight);
                Tag::Compound(entry)
            })
            .collect();
        spawner_tag_mut(region, pos).put(
            "SpawnPotentials".to_string(),
            Tag::List(rivet_nbt::list_tag::ListTag::with_list(entries)),
        );
    }

    fn set_spawn_data(
        region: &mut WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey>,
        pos: &BlockPos,
        spawn_data: Tag,
    ) {
        spawner_tag_mut(region, pos).put("SpawnData".to_string(), spawn_data);
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
            Box::new(OwnedHolder::new(test_chunk(ChunkPos::new(x, z)), status))
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

    /// `setBlock` installs Paper's lazy DUMMY tag in the ChunkAccess authority;
    /// replacing an entity block removes that pending tag.
    #[test]
    fn block_entity_writes_install_and_remove_lazy_entities() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);

        assert!(region.set_block(&pos, Blocks::CHEST.default_block_state(), UPDATE_ALL, 0,));
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_randomizable_container(
            &region, &pos
        ));
        assert_eq!(
            region
                .get_chunk(0, 0)
                .get_block_entity_nbt(&pos)
                .and_then(|tag| tag.get_string("id"))
                .map(String::as_str),
            Some("DUMMY")
        );

        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0,));
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_spawner_block_entity(
            &region, &pos
        ));

        assert!(region.remove_block(&pos, false));
        assert!(region.get_chunk(0, 0).get_block_entity_nbt(&pos).is_none());

        // `ChunkAccess.setBlockState` rejects out-of-height writes, but Paper
        // still installs the pending block-entity tag after that return.
        let outside = BlockPos::new(0, SUPERFLAT_MIN_Y - 1, 0);
        assert!(region.set_block(
            &outside,
            Blocks::SPAWNER.default_block_state(),
            UPDATE_ALL,
            0,
        ));
        assert_eq!(
            region
                .get_chunk(0, 0)
                .get_block_entity_nbt(&outside)
                .and_then(|tag| tag.get_string("id"))
                .map(String::as_str),
            Some("DUMMY")
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
        // `getHeight` of the superflat content: the center chunk is persisted
        // at CARVERS (the FEATURES step's ring-0 dependency) but no block was
        // ever written, so `WorldSurface` is never primed and the None fallback
        // returns `minY + 1` — Java's region `getHeight` for the stone floor
        // whose topmost block sits at `minY` (first available = `minY + 1`).
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
        assert_eq!(
            region
                .get_chunk(0, 0)
                .get_block_entity_nbt(&chest_pos)
                .and_then(|tag| tag.get_string("id"))
                .map(String::as_str),
            Some("DUMMY")
        );
        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_block_entity_loot_table(
            &mut region,
            &chest_pos,
            17,
            "minecraft:chests/simple_dungeon",
        );
        let chest_tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&chest_pos)
            .expect("chest DUMMY materializes lazily into persisted NBT");
        assert_eq!(
            chest_tag.get_string("id").map(String::as_str),
            Some("minecraft:chest")
        );
        assert_eq!(chest_tag.get_long_or("LootTableSeed", 0), 17);

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
        let spawner_tag = region
            .get_chunk_mut(0, 0)
            .get_block_entity_nbt_mut(&spawner_pos)
            .expect("spawner DUMMY tag");
        spawner_tag.put_string("id", "minecraft:mob_spawner");
        spawner_tag.put(
            "SpawnPotentials".to_string(),
            Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![{
                let mut entry = CompoundTag::new();
                entry.put_int("weight", 2);
                entry.put(
                    "data".to_string(),
                    Tag::Compound({
                        let mut data = CompoundTag::new();
                        let mut entity = CompoundTag::new();
                        entity.put_string("id", "minecraft:zombie");
                        data.put("entity".to_string(), Tag::Compound(entity));
                        let mut custom_rules = CompoundTag::new();
                        custom_rules.put_int("block_light_limit", 3);
                        data.put(
                            "custom_spawn_rules".to_string(),
                            Tag::Compound(custom_rules),
                        );
                        data
                    }),
                );
                Tag::Compound(entry)
            }])),
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
        let spawner_tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&spawner_pos)
            .expect("spawner DUMMY materializes lazily into persisted NBT");
        assert_eq!(
            spawner_tag.get_string("id").map(String::as_str),
            Some("minecraft:mob_spawner")
        );
        assert_eq!(
            spawner_tag
                .get_compound("SpawnData")
                .and_then(|data| data.get_compound("entity"))
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:skeleton")
        );
        assert_eq!(
            spawner_tag
                .get_compound("SpawnData")
                .and_then(|data| data.get_compound("custom_spawn_rules"))
                .and_then(|rules| rules.get_int("block_light_limit")),
            Some(3)
        );
        assert!(
            spawner_tag
                .get_list("SpawnPotentials")
                .is_some_and(|list| list.list.is_empty())
        );
    }

    #[test]
    fn fresh_spawner_materialization_saves_paper_defaults_and_empty_potentials() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            None,
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("spawner tag remains present");
        assert_eq!(
            tag.get_string("id").map(String::as_str),
            Some("minecraft:mob_spawner")
        );
        assert_eq!(tag.get_int("x"), Some(0));
        assert_eq!(tag.get_int("y"), Some(64));
        assert_eq!(tag.get_int("z"), Some(0));
        assert_eq!(tag.get_short("Delay"), Some(20));
        assert_eq!(tag.get_short("MinSpawnDelay"), Some(200));
        assert_eq!(tag.get_short("MaxSpawnDelay"), Some(800));
        assert_eq!(tag.get_short("SpawnCount"), Some(4));
        assert_eq!(tag.get_short("MaxNearbyEntities"), Some(6));
        assert_eq!(tag.get_short("RequiredPlayerRange"), Some(16));
        assert_eq!(tag.get_short("SpawnRange"), Some(4));
        assert!(tag.get("Paper.Delay").is_none());
        assert!(tag.get("Paper.MinSpawnDelay").is_none());
        assert!(tag.get("Paper.MaxSpawnDelay").is_none());
        assert!(
            tag.get_list("SpawnPotentials")
                .is_some_and(|list| list.list.is_empty())
        );
        assert_eq!(
            tag.get_compound("SpawnData")
                .and_then(|data| data.get_compound("entity"))
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:skeleton")
        );
    }

    #[test]
    fn dummy_spawner_payload_is_ignored_like_a_fresh_block_entity() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let tag = region
            .get_chunk_mut(0, 0)
            .get_block_entity_nbt_mut(&pos)
            .expect("spawner DUMMY tag");
        tag.put_short("Delay", 37);
        tag.put_int("Paper.Delay", 70_000);
        tag.put_int("MinSpawnDelay", 41);
        tag.put_int("Paper.MinSpawnDelay", 70_001);
        tag.put_int("MaxSpawnDelay", 97);
        tag.put_int("Paper.MaxSpawnDelay", 70_002);
        tag.put_int("SpawnCount", 2);
        tag.put_int("MaxNearbyEntities", 11);
        tag.put_int("RequiredPlayerRange", 23);
        tag.put_int("SpawnRange", 7);
        tag.put_string("UnknownField", "drop-me");
        tag.put(
            "SpawnData".to_string(),
            Tag::Compound({
                let mut spawn_data = CompoundTag::new();
                spawn_data.put(
                    "entity".to_string(),
                    Tag::Compound({
                        let mut entity = CompoundTag::new();
                        entity.put_string("id", "minecraft:zombie");
                        entity
                    }),
                );
                spawn_data.put_int("unknown", 7);
                spawn_data.put(
                    "custom_spawn_rules".to_string(),
                    Tag::Compound({
                        let mut rules = CompoundTag::new();
                        rules.put_int("block_light_limit", 3);
                        rules
                    }),
                );
                spawn_data
            }),
        );
        tag.put(
            "SpawnPotentials".to_string(),
            Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![
                Tag::Compound({
                    let mut entry = CompoundTag::new();
                    entry.put_int("weight", 1);
                    entry.put(
                        "data".to_string(),
                        Tag::Compound({
                            let mut data = CompoundTag::new();
                            data.put(
                                "entity".to_string(),
                                Tag::Compound({
                                    let mut entity = CompoundTag::new();
                                    entity.put_string("id", "minecraft:creeper");
                                    entity
                                }),
                            );
                            data
                        }),
                    );
                    entry
                }),
            ])),
        );

        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_spawner_block_entity(
            &region, &pos,
        ));
        assert_eq!(<WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
            &region, &pos,
        ), None);
        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            Some(0),
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("spawner tag remains present");
        assert_eq!(
            tag.get_string("id").map(String::as_str),
            Some("minecraft:mob_spawner")
        );
        assert_eq!(tag.get_short("Delay"), Some(20));
        assert_eq!(tag.get_short("MinSpawnDelay"), Some(200));
        assert_eq!(tag.get_short("MaxSpawnDelay"), Some(800));
        assert_eq!(tag.get_short("SpawnCount"), Some(4));
        assert_eq!(tag.get_short("MaxNearbyEntities"), Some(6));
        assert_eq!(tag.get_short("RequiredPlayerRange"), Some(16));
        assert_eq!(tag.get_short("SpawnRange"), Some(4));
        assert!(matches!(tag.get("Delay"), Some(Tag::Short(_))));
        assert!(matches!(tag.get("MinSpawnDelay"), Some(Tag::Short(_))));
        assert!(matches!(tag.get("MaxSpawnDelay"), Some(Tag::Short(_))));
        assert!(matches!(tag.get("SpawnCount"), Some(Tag::Short(_))));
        assert!(matches!(tag.get("MaxNearbyEntities"), Some(Tag::Short(_))));
        assert!(matches!(
            tag.get("RequiredPlayerRange"),
            Some(Tag::Short(_))
        ));
        assert!(matches!(tag.get("SpawnRange"), Some(Tag::Short(_))));
        assert!(tag.get("Paper.Delay").is_none());
        assert!(tag.get("Paper.MinSpawnDelay").is_none());
        assert!(tag.get("Paper.MaxSpawnDelay").is_none());
        assert!(tag.get("UnknownField").is_none());
        assert!(
            tag.get_list("SpawnPotentials")
                .is_some_and(|list| list.list.is_empty())
        );
        let spawn_data = tag.get_compound("SpawnData").expect("SpawnData remains");
        assert!(spawn_data.get("unknown").is_none());
        assert!(spawn_data.get("custom_spawn_rules").is_none());
        assert_eq!(
            spawn_data
                .get_compound("entity")
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:skeleton")
        );
    }

    #[test]
    fn block_entity_type_uses_identifier_decode_and_block_state_validation() {
        let mut region = feature_region();
        let spawner_pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(
            &spawner_pos,
            Blocks::SPAWNER.default_block_state(),
            UPDATE_ALL,
            0,
        ));
        spawner_tag_mut(&mut region, &spawner_pos).put_string("id", "mob_spawner");
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_spawner_block_entity(
            &region, &spawner_pos,
        ));

        let chest_pos = BlockPos::new(1, 64, 0);
        assert!(region.set_block(
            &chest_pos,
            Blocks::CHEST.default_block_state(),
            UPDATE_ALL,
            0,
        ));
        let chest_tag = region
            .get_chunk_mut(0, 0)
            .get_block_entity_nbt_mut(&chest_pos)
            .expect("chest DUMMY tag");
        chest_tag.put_string("id", "minecraft:mob_spawner");
        assert!(!<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_spawner_block_entity(
            &region, &chest_pos,
        ));

        let spawner_tag = region
            .get_chunk_mut(0, 0)
            .get_block_entity_nbt_mut(&spawner_pos)
            .expect("spawner tag");
        spawner_tag.put_string("id", "minecraft:chest");
        assert!(!<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_randomizable_container(
            &region,
            &spawner_pos,
        ));

        let trapped_pos = BlockPos::new(2, 64, 0);
        assert!(region.set_block(&trapped_pos, BlockState::new(StateId(11208)), UPDATE_ALL, 0,));
        region
            .get_chunk_mut(0, 0)
            .get_block_entity_nbt_mut(&trapped_pos)
            .expect("trapped chest tag")
            .put_string("id", "minecraft:chest");
        assert!(!<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_randomizable_container(
            &region,
            &trapped_pos,
        ));
        region
            .get_chunk_mut(0, 0)
            .get_block_entity_nbt_mut(&trapped_pos)
            .expect("trapped chest tag")
            .put_string("id", "minecraft:trapped_chest");
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_randomizable_container(
            &region,
            &trapped_pos,
        ));
    }

    #[test]
    fn trapped_chest_entity_and_loot_survive_a_blocked_safe_set() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        let trapped_chest = BlockState::new(StateId(11208));
        assert!(region.set_block(&pos, trapped_chest, UPDATE_ALL, 0));
        assert_eq!(
            region
                .get_chunk(0, 0)
                .get_block_entity_nbt(&pos)
                .and_then(|tag| tag.get_string("id"))
                .map(String::as_str),
            Some("DUMMY")
        );
        assert!(<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_randomizable_container(
            &region, &pos
        ));

        let tag = region
            .get_chunk_mut(0, 0)
            .get_block_entity_nbt_mut(&pos)
            .expect("trapped chest DUMMY tag");
        tag.put_string("unknown", "drop-me");
        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_block_entity_loot_table(
            &mut region,
            &pos,
            0,
            "minecraft:chests/simple_dungeon",
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("trapped chest tag remains present");
        assert_eq!(
            tag.get_string("id").map(String::as_str),
            Some("minecraft:trapped_chest")
        );
        assert_eq!(
            tag.get_string("LootTable").map(String::as_str),
            Some("minecraft:chests/simple_dungeon")
        );
        assert!(tag.get("LootTableSeed").is_none());
        assert!(tag.get("unknown").is_none());
        assert_eq!(tag.get_int("x"), Some(0));
        assert_eq!(tag.get_int("y"), Some(64));
        assert_eq!(tag.get_int("z"), Some(0));
    }

    #[test]
    fn spawn_data_codec_defaults_and_canonicalizes_nested_fields() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let mut entity = CompoundTag::new();
        entity.put_string("id", "zombie");
        let mut rules = CompoundTag::new();
        rules.put_int("block_light_limit", 20);
        let mut equipment = CompoundTag::new();
        equipment.put_string("loot_table", "chest");
        let mut slot_drop_chances = CompoundTag::new();
        for slot in EQUIPMENT_SLOTS {
            slot_drop_chances.put_float(slot, 0.5);
        }
        equipment.put(
            "slot_drop_chances".to_string(),
            Tag::Compound(slot_drop_chances),
        );
        let mut spawn_data = CompoundTag::new();
        spawn_data.put("entity".to_string(), Tag::Compound(entity));
        spawn_data.put("custom_spawn_rules".to_string(), Tag::Compound(rules));
        spawn_data.put("equipment".to_string(), Tag::Compound(equipment));
        spawn_data.put_int("unknown", 7);
        set_spawn_data(&mut region, &pos, Tag::Compound(spawn_data));

        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            None,
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("spawner tag remains present");
        let spawn_data = tag.get_compound("SpawnData").expect("SpawnData remains");
        assert!(spawn_data.get("unknown").is_none());
        assert_eq!(
            spawn_data
                .get_compound("entity")
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:skeleton")
        );
        assert!(spawn_data.get("custom_spawn_rules").is_none());
        let equipment = spawn_data
            .get_compound("equipment")
            .expect("equipment remains");
        assert_eq!(
            equipment.get_string("loot_table").map(String::as_str),
            Some("minecraft:chest")
        );
        assert_eq!(equipment.get_float("slot_drop_chances"), Some(0.5));
    }

    #[test]
    fn malformed_custom_spawn_rules_drop_the_optional_as_a_whole() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let mut entity = CompoundTag::new();
        entity.put_string("id", "minecraft:zombie");
        let mut rules = CompoundTag::new();
        rules.put_int("block_light_limit", 3);
        rules.put_int("sky_light_limit", 20);
        let mut spawn_data = CompoundTag::new();
        spawn_data.put("entity".to_string(), Tag::Compound(entity));
        spawn_data.put("custom_spawn_rules".to_string(), Tag::Compound(rules));
        set_spawn_data(&mut region, &pos, Tag::Compound(spawn_data));

        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            None,
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("spawner tag remains present");
        assert!(
            tag.get_compound("SpawnData")
                .is_some_and(|data| data.get("custom_spawn_rules").is_none())
        );
    }

    #[test]
    fn all_eight_nan_equipment_chances_use_the_scalar_codec_form() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let mut entity = CompoundTag::new();
        entity.put_string("id", "minecraft:zombie");
        let mut equipment = CompoundTag::new();
        equipment.put_string("loot_table", "minecraft:chests/simple_dungeon");
        let mut chances = CompoundTag::new();
        let nan = f32::from_bits(0x7fc0_1234);
        for slot in EQUIPMENT_SLOTS {
            chances.put_float(slot, nan);
        }
        equipment.put("slot_drop_chances".to_string(), Tag::Compound(chances));
        let mut spawn_data = CompoundTag::new();
        spawn_data.put("entity".to_string(), Tag::Compound(entity));
        spawn_data.put("equipment".to_string(), Tag::Compound(equipment));
        set_spawn_data(&mut region, &pos, Tag::Compound(spawn_data));

        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            None,
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("spawner tag remains present");
        let equipment = tag
            .get_compound("SpawnData")
            .and_then(|data| data.get_compound("equipment"))
            .expect("equipment remains");
        let chance = equipment
            .get("slot_drop_chances")
            .and_then(Tag::as_float)
            .expect("equal NaN chances use the scalar form");
        assert!(chance.is_nan());
        assert_eq!(chance.to_bits(), nan.to_bits());
    }

    #[test]
    fn existing_spawner_set_entity_id_preserves_loaded_fields() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let tag = spawner_tag_mut(&mut region, &pos);
        tag.put_string("id", "minecraft:mob_spawner");
        tag.put_short("Delay", 37);
        tag.put_short("MinSpawnDelay", 41);
        tag.put_short("MaxSpawnDelay", 97);
        tag.put_short("SpawnCount", 2);
        tag.put_short("MaxNearbyEntities", 11);
        tag.put_short("RequiredPlayerRange", 23);
        tag.put_short("SpawnRange", 7);
        tag.put_int("UnrelatedField", 1234);
        let mut entity = CompoundTag::new();
        entity.put_string("id", "minecraft:zombie");
        let mut spawn_data = CompoundTag::new();
        spawn_data.put("entity".to_string(), Tag::Compound(entity));
        tag.put("SpawnData".to_string(), Tag::Compound(spawn_data));

        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            None,
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("spawner remains materialized");
        assert_eq!(tag.get_short("Delay"), Some(37));
        assert_eq!(tag.get_short("MinSpawnDelay"), Some(41));
        assert_eq!(tag.get_short("MaxSpawnDelay"), Some(97));
        assert_eq!(tag.get_short("SpawnCount"), Some(2));
        assert_eq!(tag.get_short("MaxNearbyEntities"), Some(11));
        assert_eq!(tag.get_short("RequiredPlayerRange"), Some(23));
        assert_eq!(tag.get_short("SpawnRange"), Some(7));
        assert!(tag.get("UnrelatedField").is_none());
        assert_eq!(
            tag.get_compound("SpawnData")
                .and_then(|data| data.get_compound("entity"))
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:skeleton")
        );
    }

    #[test]
    fn spawner_paper_numeric_precedence_and_saved_types_match_paper() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let tag = spawner_tag_mut(&mut region, &pos);
        tag.put_short("Delay", 37);
        tag.put_int("Paper.Delay", 70_000);
        tag.put_int("MinSpawnDelay", 41);
        tag.put_int("Paper.MinSpawnDelay", 70_000);
        tag.put_int("MaxSpawnDelay", 97);
        tag.put_int("SpawnCount", 70_000);
        tag.put_int("MaxNearbyEntities", -70_000);
        tag.put_string("RequiredPlayerRange", "malformed");
        tag.put_int("SpawnRange", 7);
        tag.put_string("unknown", "drop-me");

        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            None,
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("spawner tag remains present");
        assert_eq!(tag.get_int("Paper.Delay"), Some(70_000));
        assert_eq!(tag.get_short("Delay"), Some(i16::MAX));
        assert_eq!(tag.get_int("Paper.MinSpawnDelay"), Some(70_000));
        assert_eq!(tag.get_int("Paper.MaxSpawnDelay"), Some(97));
        assert_eq!(tag.get_short("MinSpawnDelay"), Some(i16::MAX));
        assert_eq!(tag.get_short("MaxSpawnDelay"), Some(97));
        assert_eq!(tag.get_short("SpawnCount"), Some(70_000_i32 as i16));
        assert_eq!(
            tag.get_short("MaxNearbyEntities"),
            Some((-70_000_i32) as i16)
        );
        assert_eq!(tag.get_short("RequiredPlayerRange"), Some(16));
        assert_eq!(tag.get_short("SpawnRange"), Some(7));
        assert!(tag.get("unknown").is_none());
    }

    #[test]
    fn malformed_spawn_data_uses_valid_potentials_for_the_weighted_draw() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        set_spawn_data(
            &mut region,
            &pos,
            Tag::Int(rivet_nbt::int_tag::IntTag::value_of(7)),
        );
        set_spawn_potentials(&mut region, &pos, &[2]);
        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos
            ),
            Some(2)
        );
        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            Some(1),
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("malformed SpawnData is replaced by selected potential");
        assert_eq!(
            tag.get_compound("SpawnData")
                .and_then(|data| data.get_compound("entity"))
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:skeleton")
        );
        assert!(
            tag.get_list("SpawnPotentials")
                .is_some_and(|list| list.list.is_empty())
        );
    }

    #[test]
    fn malformed_optional_spawn_data_fields_are_dropped() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let mut entity = CompoundTag::new();
        entity.put_string("id", "minecraft:zombie");
        let mut spawn_data = CompoundTag::new();
        spawn_data.put("entity".to_string(), Tag::Compound(entity));
        spawn_data.put_int("custom_spawn_rules", 7);
        spawn_data.put_string("equipment", "malformed");
        set_spawn_data(&mut region, &pos, Tag::Compound(spawn_data));
        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            None,
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("spawner tag remains present");
        let spawn_data = tag.get_compound("SpawnData").expect("SpawnData remains");
        assert!(spawn_data.get("custom_spawn_rules").is_none());
        assert!(spawn_data.get("equipment").is_none());
    }

    #[test]
    fn valid_spawn_data_optional_payloads_are_preserved() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let mut entity = CompoundTag::new();
        entity.put_string("id", "minecraft:zombie");
        let mut rules = CompoundTag::new();
        rules.put_int("block_light_limit", 3);
        rules.put_int("sky_light_limit", 15);
        let mut equipment = CompoundTag::new();
        equipment.put_string("loot_table", "minecraft:chests/simple_dungeon");
        equipment.put_float("slot_drop_chances", 0.5);
        let mut spawn_data = CompoundTag::new();
        spawn_data.put("entity".to_string(), Tag::Compound(entity));
        spawn_data.put("custom_spawn_rules".to_string(), Tag::Compound(rules));
        spawn_data.put("equipment".to_string(), Tag::Compound(equipment));
        set_spawn_data(&mut region, &pos, Tag::Compound(spawn_data));
        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            None,
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("spawner tag remains present");
        let spawn_data = tag.get_compound("SpawnData").expect("SpawnData remains");
        assert_eq!(
            spawn_data
                .get_compound("custom_spawn_rules")
                .and_then(|rules| rules.get_int("block_light_limit")),
            Some(3)
        );
        assert_eq!(
            spawn_data
                .get_compound("equipment")
                .and_then(|equipment| equipment.get_string("loot_table"))
                .map(String::as_str),
            Some("minecraft:chests/simple_dungeon")
        );
    }

    #[test]
    fn zero_weight_spawn_potentials_are_empty_without_a_rng_bound() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        set_spawn_potentials(&mut region, &pos, &[0, 0]);
        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos
            ),
            None
        );
        <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::set_spawner_entity(
            &mut region,
            &pos,
            "minecraft:skeleton",
            None,
        );
        let tag = region
            .get_chunk(0, 0)
            .get_block_entity_nbt(&pos)
            .expect("zero-total potentials materialize an empty SpawnData");
        assert_eq!(
            tag.get_compound("SpawnData")
                .and_then(|data| data.get_compound("entity"))
                .and_then(|entity| entity.get_string("id"))
                .map(String::as_str),
            Some("minecraft:skeleton")
        );
        assert!(
            tag.get_list("SpawnPotentials")
                .is_some_and(|list| list.list.is_empty())
        );
    }

    #[test]
    fn malformed_spawn_potential_weight_is_dropped_without_panicking() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        set_spawn_potentials(&mut region, &pos, &[-1]);
        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos
            ),
            None
        );
    }

    #[test]
    fn overflowing_spawn_potential_weight_rejects_the_spawner_without_fallback() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        set_spawn_potentials(&mut region, &pos, &[i32::MAX, 1]);
        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos
            ),
            None
        );
        assert!(!<WorldGenRegion<
            'static,
            StateId,
            ServerBiomeId,
            StructureKey,
        > as WorldGenLevel>::is_spawner_block_entity(
            &region, &pos,
        ));
    }

    #[test]
    fn non_compound_spawn_potential_entry_is_dropped_without_panicking() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        spawner_tag_mut(&mut region, &pos).put(
            "SpawnPotentials".to_string(),
            Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![Tag::Int(
                rivet_nbt::int_tag::IntTag::value_of(1),
            )])),
        );
        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos
            ),
            None
        );
    }

    #[test]
    fn wrong_typed_spawn_potentials_use_paper_singleton_fallback() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        spawner_tag_mut(&mut region, &pos).put_int("SpawnPotentials", 1);
        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos
            ),
            Some(1)
        );
    }

    #[test]
    fn malformed_spawn_potential_entries_keep_valid_partial_list() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let mut valid = CompoundTag::new();
        valid.put_int("weight", 2);
        valid.put(
            "data".to_string(),
            Tag::Compound({
                let mut data = CompoundTag::new();
                data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
                data
            }),
        );
        spawner_tag_mut(&mut region, &pos).put(
            "SpawnPotentials".to_string(),
            Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![
                Tag::Int(rivet_nbt::int_tag::IntTag::value_of(1)),
                Tag::Compound(valid),
            ])),
        );
        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos
            ),
            Some(2)
        );
    }

    #[test]
    fn missing_spawn_potential_weight_is_dropped_without_panicking() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let mut entry = CompoundTag::new();
        entry.put(
            "data".to_string(),
            Tag::Compound({
                let mut data = CompoundTag::new();
                data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
                data
            }),
        );
        spawner_tag_mut(&mut region, &pos).put(
            "SpawnPotentials".to_string(),
            Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![
                Tag::Compound(entry),
            ])),
        );
        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos
            ),
            None
        );
    }

    #[test]
    fn malformed_spawn_potential_data_is_dropped_without_panicking() {
        let mut region = feature_region();
        let pos = BlockPos::new(0, 64, 0);
        assert!(region.set_block(&pos, Blocks::SPAWNER.default_block_state(), UPDATE_ALL, 0));
        let mut entry = CompoundTag::new();
        entry.put_int("weight", 1);
        entry.put(
            "data".to_string(),
            Tag::Int(rivet_nbt::int_tag::IntTag::value_of(2)),
        );
        spawner_tag_mut(&mut region, &pos).put(
            "SpawnPotentials".to_string(),
            Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![
                Tag::Compound(entry),
            ])),
        );
        assert_eq!(
            <WorldGenRegion<'static, StateId, ServerBiomeId, StructureKey> as WorldGenLevel>::spawner_potential_weight(
                &region, &pos
            ),
            None
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
