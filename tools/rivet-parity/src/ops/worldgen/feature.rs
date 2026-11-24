//! `feature.place`: place one configured feature into a fixed synthetic region.
//!
//! The region contract (identical to `RivetReferenceOracle.featurePlace`):
//!
//! - The region is a 3 x 3 window of proto chunks around chunk (0, 0), status
//!   CARVERS, each a copy of the profile's base fill (by y; the profile is
//!   horizontally uniform except `cave`) with the profile's biome at every
//!   position and block/sky light 0. The centre chunk has its final heightmaps
//!   primed, like `ChunkStatusTasks.generateFeatures`. Rivet's `WorldGenRegion`
//!   (FEATURES step: write radius 1) runs over the window; chunks outside it
//!   are absent, so a read there is the `Requested chunk unavailable during
//!   world generation` crash (answered `threw: true`). y outside [-64, 319]
//!   reads void air.
//! - The feature is placed at the profile's origin with
//!   `XoroshiroRandomSource(seed)`; the region's own random is
//!   `XoroshiroRandomSource(99)`; the sea level and build window are the
//!   profile's noise-settings values.
//! - The answer is `{placed, rng_next, scheduled, blocks[, threw]}`:
//!   `rng_next` the next long of the placement random after the call,
//!   `scheduled` the ticks left in the chunks' tick containers
//!   (`[x, y, z, "block"|"fluid", id]`, sorted), `blocks` every position in
//!   x, z in [-16, 32), y in [-64, 320) whose state differs from the profile's
//!   base fill (`[x, y, z, global state id]`, sorted x, y, z).
//!
//! Profiles (base fill by y): `stone` (stone <= 63), `surface` (stone <= 60,
//! dirt 61-62, grass block 63), `cave` (stone with an air cavity x, z in
//! [-8, 24], y in [30, 44]), `ocean` (stone <= 43, sand 44, water 45-62),
//! `nether` / `nether_ceiling` (netherrack y <= 40 and y >= 120, sea level
//! 32), `end` (end stone <= 62, sea level 0), `void` (all air).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rivet_registry::Identifier;
use rivet_registry::access::RegistryAccess;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::holder::Holder;
use rivet_server::server::level::generated_world::build_feature_access;
use rivet_server::server::level::level_chunk::StructureKey;
use rivet_server::server::level::world_gen_region::{GenerationChunkHolderView, WorldGenRegion};
use rivet_util::RandomSource;
use rivet_util::StaticCache2D;
use rivet_util::random_source::XoroshiroRandomSource;
use rivet_util::worldgen_random::AlgorithmRandomSource;
use rivet_world::biome::biome_manager::NoiseBiomeSource;
use rivet_world::biome::climate::Climate;
use rivet_world::biome::fixed_biome_source::FixedBiomeSource;
use rivet_world::biome::generated_biome_source::dense_biome_id;
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::chunk_generator::ChunkGenerator;
use rivet_world::chunk::proto_chunk::ProtoChunk;
use rivet_world::chunk::status::{ChunkStatus, GENERATION_PYRAMID};
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::chunk::storage::section_reconstruction::{
    BiomeId as WorldgenBiomeId, current_version_container_factory,
};
use rivet_world::chunk::upgrade_data::UpgradeData;
use rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries;
use rivet_world::level::height_accessor::{LevelHeightAccessor, create as create_height_accessor};
use rivet_world::levelgen::feature::registry_keys::CONFIGURED_FEATURE;
use rivet_world::levelgen::heightmap::FINAL_HEIGHTMAPS;
use serde_json::{Map, Value, json};

use super::{is_unported_panic, panic_message, request_i64, request_str};
use crate::ops::compare::Outcome;
use crate::ops::packet;

pub const MIN_Y: i32 = -64;
pub const HEIGHT: i32 = 384;

type Proto = ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>;

/// One synthetic region profile.
pub struct Profile {
    pub name: &'static str,
    pub origin: (i32, i32, i32),
    pub biome: &'static str,
    pub sea_level: i32,
}

pub const PROFILES: [Profile; 8] = [
    Profile {
        name: "stone",
        origin: (8, 40, 8),
        biome: "minecraft:plains",
        sea_level: 63,
    },
    Profile {
        name: "surface",
        origin: (8, 64, 8),
        biome: "minecraft:plains",
        sea_level: 63,
    },
    Profile {
        name: "cave",
        origin: (8, 30, 8),
        biome: "minecraft:plains",
        sea_level: 63,
    },
    Profile {
        name: "ocean",
        origin: (8, 45, 8),
        biome: "minecraft:plains",
        sea_level: 63,
    },
    Profile {
        name: "nether",
        origin: (8, 41, 8),
        biome: "minecraft:nether_wastes",
        sea_level: 32,
    },
    Profile {
        name: "nether_ceiling",
        origin: (8, 119, 8),
        biome: "minecraft:nether_wastes",
        sea_level: 32,
    },
    Profile {
        name: "end",
        origin: (8, 63, 8),
        biome: "minecraft:the_end",
        sea_level: 0,
    },
    Profile {
        name: "void",
        origin: (8, 64, 8),
        biome: "minecraft:the_void",
        sea_level: 63,
    },
];

pub fn profile(name: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.name == name)
}

/// The profile a configured feature is placed in: the surroundings its vanilla
/// placement assumes (a kelp forest needs water, a nether ore needs
/// netherrack). Chosen by key first, then by feature type; everything else is
/// placed on the overworld surface.
pub fn region_for(key: &str, feature_type: &str) -> &'static str {
    let name = key.strip_prefix("minecraft:").unwrap_or(key);
    let kind = feature_type
        .strip_prefix("minecraft:")
        .unwrap_or(feature_type);
    match name {
        "weeping_vines" => return "nether_ceiling",
        "crimson_roots"
        | "patch_fire"
        | "patch_soul_fire"
        | "spring_lava_nether"
        | "spring_nether_closed"
        | "spring_nether_open"
        | "ore_ancient_debris_large"
        | "ore_ancient_debris_small"
        | "ore_blackstone"
        | "ore_gravel_nether"
        | "ore_magma"
        | "ore_nether_gold"
        | "ore_quartz"
        | "ore_soul_sand"
        | "basalt_blobs"
        | "blackstone_blobs" => return "nether",
        "spring_water"
        | "spring_lava_overworld"
        | "spring_lava_frozen"
        | "lake_lava"
        | "sulfur_pool"
        | "sulfur_spring"
        | "sulfur_spike"
        | "sulfur_spike_cluster"
        | "dripleaf"
        | "pointed_dripstone"
        | "moss_vegetation"
        | "spore_blossom"
        | "cave_vine"
        | "cave_vine_in_moss"
        | "glow_lichen"
        | "sculk_vein" => return "cave",
        "warm_ocean_vegetation" => return "ocean",
        _ => {}
    }
    match kind {
        "basalt_columns"
        | "basalt_pillar"
        | "delta_feature"
        | "glowstone_blob"
        | "huge_fungus"
        | "nether_forest_vegetation"
        | "netherrack_replace_blobs"
        | "twisting_vines" => "nether",
        "kelp" | "seagrass" | "sea_pickle" | "underwater_magma" | "iceberg" | "blue_ice" => "ocean",
        "chorus_plant" | "end_island" | "end_spike" => "end",
        "end_platform" | "void_start_platform" | "end_gateway" => "void",
        "ore" | "scattered_ore" | "geode" | "fossil" | "monster_room" => "stone",
        "large_dripstone"
        | "speleothem_cluster"
        | "multiface_growth"
        | "sculk_patch"
        | "vegetation_patch"
        | "waterlogged_vegetation_patch"
        | "root_system" => "cave",
        _ => "surface",
    }
}

fn base_block(profile: &Profile, x: i32, y: i32, z: i32) -> BlockState {
    if !(MIN_Y..MIN_Y + HEIGHT).contains(&y) {
        return void_air();
    }
    let block = match profile.name {
        "stone" => {
            if y <= 63 {
                Blocks::STONE
            } else {
                Blocks::AIR
            }
        }
        "surface" => match y {
            ..=60 => Blocks::STONE,
            61 | 62 => Blocks::DIRT,
            63 => Blocks::GRASS_BLOCK,
            _ => Blocks::AIR,
        },
        "cave" => {
            if (-8..=24).contains(&x) && (-8..=24).contains(&z) && (30..=44).contains(&y) {
                Blocks::AIR
            } else if y <= 63 {
                Blocks::STONE
            } else {
                Blocks::AIR
            }
        }
        "ocean" => match y {
            ..=43 => Blocks::STONE,
            44 => Blocks::SAND,
            45..=62 => Blocks::WATER,
            _ => Blocks::AIR,
        },
        "nether" | "nether_ceiling" => {
            if y <= 40 || y >= 120 {
                Blocks::NETHERRACK
            } else {
                Blocks::AIR
            }
        }
        "end" => {
            if y <= 62 {
                Blocks::END_STONE
            } else {
                Blocks::AIR
            }
        }
        "void" => Blocks::AIR,
        other => unreachable!("unknown region profile {other}"),
    };
    block.default_block_state()
}

fn void_air() -> BlockState {
    BlockState::of(BlockId(794))
}

fn biome_holder(name: &str) -> Holder<BiomeId> {
    Holder::direct(BiomeId::from_name(name).unwrap_or_else(|| panic!("unknown biome {name}")))
}

fn new_chunk(pos: ChunkPos) -> Proto {
    let height_accessor = create_height_accessor(MIN_Y, HEIGHT);
    ProtoChunk::new(
        pos,
        UpgradeData::empty(height_accessor.get_sections_count() as usize),
        height_accessor,
        &current_version_container_factory(),
        None,
        Blocks::AIR.default_block_state(),
        void_air(),
        &resolve_state_flags,
    )
}

/// The generated base fill of one chunk of a profile: built once per
/// (profile, chunk), then copied per input.
pub fn template(profile: &'static Profile, chunk_x: i32, chunk_z: i32) -> Proto {
    thread_local! {
        static TEMPLATES: RefCell<HashMap<(&'static str, i32, i32), Proto>> =
            RefCell::new(HashMap::new());
    }
    TEMPLATES.with(|templates| {
        let mut templates = templates.borrow_mut();
        let source = templates
            .entry((profile.name, chunk_x, chunk_z))
            .or_insert_with(|| build_template(profile, chunk_x, chunk_z));
        copy_chunk(source)
    })
}

/// A fresh base-fill chunk (`pub` for the carver op, which carves the `stone`
/// profile's fill).
pub fn build_template(profile: &'static Profile, chunk_x: i32, chunk_z: i32) -> Proto {
    let pos = ChunkPos::new(chunk_x, chunk_z);
    let mut chunk = new_chunk(pos);
    let air = Blocks::AIR.default_block_state();
    for y in MIN_Y..MIN_Y + HEIGHT {
        for x in 0..16 {
            for z in 0..16 {
                let (wx, wz) = (pos.get_min_block_x() + x, pos.get_min_block_z() + z);
                let state = base_block(profile, wx, y, wz);
                if state != air {
                    chunk.set_block_state(wx, y, wz, state);
                }
            }
        }
    }
    let source = FixedBiomeSource::new(biome_holder(profile.biome));
    chunk.fill_biomes_from_noise(&source, &Climate::empty(), &|holder| {
        WorldgenBiomeId(dense_biome_id(holder))
    });
    chunk
}

/// A deep copy (sections, biomes, heightmaps) of a base-fill chunk.
pub fn copy_chunk(source: &Proto) -> Proto {
    let factory = current_version_container_factory();
    source
        .map_values_ref(
            factory.block_states_strategy().clone(),
            factory.biome_strategy().clone(),
            *factory.default_block_state(),
            void_air(),
            *factory.default_biome(),
            &|state: &BlockState| *state,
            &|biome: &WorldgenBiomeId| *biome,
            &resolve_state_flags,
        )
        .expect("a base-fill proto copies through its own value strategies")
}

/// A window chunk: the proto chunk plus a flag the region's mutable access
/// raises, so the diff only scans chunks that were written.
struct TrackedHolder {
    chunk: Proto,
    dirty: Arc<AtomicBool>,
}

impl GenerationChunkHolderView<BlockState, WorldgenBiomeId, StructureKey> for TrackedHolder {
    fn get_chunk_if_present_unchecked(
        &self,
        status: ChunkStatus,
    ) -> Option<&ChunkAccess<BlockState, WorldgenBiomeId, StructureKey>> {
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
    ) -> Option<&mut ChunkAccess<BlockState, WorldgenBiomeId, StructureKey>> {
        self.dirty.store(true, Ordering::Relaxed);
        self.chunk
            .get_persisted_status()
            .is_or_after(status)
            .then_some(self.chunk.base_mut())
    }

    fn into_owned_proto(self: Box<Self>) -> Option<Proto> {
        Some(self.chunk)
    }

    fn get_proto_chunk_if_present_unchecked_mut(
        &mut self,
        status: ChunkStatus,
    ) -> Option<&mut Proto> {
        self.dirty.store(true, Ordering::Relaxed);
        self.chunk
            .get_persisted_status()
            .is_or_after(status)
            .then_some(&mut self.chunk)
    }
}

/// A position outside the 3 x 3 window: no chunk, so any read is the
/// `Requested chunk unavailable during world generation` diagnostic.
struct AbsentHolder;

impl GenerationChunkHolderView<BlockState, WorldgenBiomeId, StructureKey> for AbsentHolder {
    fn get_chunk_if_present_unchecked(
        &self,
        _status: ChunkStatus,
    ) -> Option<&ChunkAccess<BlockState, WorldgenBiomeId, StructureKey>> {
        None
    }

    fn get_persisted_status(&self) -> Option<ChunkStatus> {
        None
    }

    fn get_chunk_if_present_unchecked_mut(
        &mut self,
        _status: ChunkStatus,
    ) -> Option<&mut ChunkAccess<BlockState, WorldgenBiomeId, StructureKey>> {
        None
    }
}

/// The generator the features read: the profile's sea level and the build window.
pub struct RegionGenerator {
    pub sea_level: i32,
}

impl ChunkGenerator for RegionGenerator {
    fn get_min_y(&self) -> i32 {
        MIN_Y
    }
    fn get_gen_depth(&self) -> i32 {
        HEIGHT
    }
    fn get_sea_level(&self) -> i32 {
        self.sea_level
    }
}

thread_local! {
    static FEATURE_ACCESS: RegistryAccess =
        build_feature_access(&build_worldgen_registries());
}

/// `feature.place`: `{feature, seed, region}` -> `{placed, rng_next, scheduled, blocks[, threw]}`.
pub fn feature_place(request: &Map<String, Value>) -> Outcome {
    let key = match request_str(request, "feature") {
        Ok(key) => key,
        Err(outcome) => return outcome,
    };
    let seed = match request_i64(request, "seed") {
        Ok(seed) => seed,
        Err(outcome) => return outcome,
    };
    let region_name = match request_str(request, "region") {
        Ok(name) => name,
        Err(outcome) => return outcome,
    };
    let Some(profile) = profile(region_name) else {
        return Outcome::Unavailable(format!("unknown region profile {region_name}"));
    };
    FEATURE_ACCESS.with(|access| place(access, key, seed, profile))
}

fn place(access: &RegistryAccess, key: &str, seed: i64, profile: &'static Profile) -> Outcome {
    let Some(registry) = access.lookup(&*CONFIGURED_FEATURE) else {
        return Outcome::Unavailable("the configured feature registry is not built".into());
    };
    let Some(configured) = registry.get_optional(&Identifier::parse(key)) else {
        return Outcome::Unported(format!(
            "{key} is not in Rivet's generated configured-feature closure"
        ));
    };

    let step = GENERATION_PYRAMID
        .get_step_to(ChunkStatus::Features)
        .clone();
    let radius = step.direct_dependencies().size() as i32 - 1;
    let width = radius * 2 + 1;
    let dirty: Vec<Arc<AtomicBool>> = (0..9).map(|_| Arc::new(AtomicBool::new(false))).collect();
    let mut holders: Vec<
        Box<dyn GenerationChunkHolderView<BlockState, WorldgenBiomeId, StructureKey>>,
    > = Vec::with_capacity((width * width) as usize);
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx.abs() <= 1 && dz.abs() <= 1 {
                let mut chunk = template(profile, dx, dz);
                chunk.set_persisted_status(ChunkStatus::Carvers);
                if dx == 0 && dz == 0 {
                    chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
                }
                holders.push(Box::new(TrackedHolder {
                    chunk,
                    dirty: Arc::clone(&dirty[((dx + 1) * 3 + dz + 1) as usize]),
                }));
            } else {
                holders.push(Box::new(AbsentHolder));
            }
        }
    }
    let cache = StaticCache2D::from_entries(-radius, -radius, width, width, holders);
    let biome_source: Arc<dyn NoiseBiomeSource> =
        Arc::new(FixedBiomeSource::new(biome_holder(profile.biome)));
    let mut region = WorldGenRegion::new(
        cache,
        ChunkPos::new(0, 0),
        step,
        seed,
        MIN_Y,
        HEIGHT,
        profile.sea_level,
        biome_source,
        access.clone(),
    );
    region.set_region_random(AlgorithmRandomSource::Xoroshiro(
        XoroshiroRandomSource::new(99),
    ));

    let generator = RegionGenerator {
        sea_level: profile.sea_level,
    };
    let (ox, oy, oz) = profile.origin;
    let origin = BlockPos::new(ox, oy, oz);
    let mut random = XoroshiroRandomSource::new(seed);
    let placed =
        packet::guarded(|| configured.place(&mut region, &generator, &mut random, &origin));
    let (placed, threw) = match placed {
        Ok(placed) => (placed, false),
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            if message.contains("Requested chunk unavailable") {
                (false, true)
            } else if is_unported_panic(&message) {
                return Outcome::Unported(unported_reason(&message));
            } else {
                return Outcome::Unavailable(format!("Rust panic: {message}"));
            }
        }
    };
    let rng_next = random.next_long();

    let mut chunks: Vec<(ChunkPos, Proto)> = region.into_owned_proto_entries();
    chunks.sort_by_key(|(pos, _)| (pos.x(), pos.z()));

    let mut ticks: Vec<(i32, i32, i32, &'static str, &'static str)> = Vec::new();
    let mut changed: Vec<(i32, i32, i32, u16)> = Vec::new();
    for (pos, chunk) in &chunks {
        for tick in chunk.get_block_ticks().scheduled_ticks() {
            ticks.push((
                tick.pos.get_x(),
                tick.pos.get_y(),
                tick.pos.get_z(),
                "block",
                tick.r#type.name(),
            ));
        }
        for tick in chunk.get_fluid_ticks().scheduled_ticks() {
            ticks.push((
                tick.pos.get_x(),
                tick.pos.get_y(),
                tick.pos.get_z(),
                "fluid",
                tick.r#type.name(),
            ));
        }
        let index = ((pos.x() + 1) * 3 + pos.z() + 1) as usize;
        if !dirty[index].load(Ordering::Relaxed) {
            continue;
        }
        for y in MIN_Y..MIN_Y + HEIGHT {
            for lx in 0..16 {
                for lz in 0..16 {
                    let (x, z) = (pos.get_min_block_x() + lx, pos.get_min_block_z() + lz);
                    let state = chunk.get_block_state(x, y, z);
                    if state != base_block(profile, x, y, z) {
                        changed.push((x, y, z, state.id().0));
                    }
                }
            }
        }
    }
    ticks.sort();
    changed.sort();

    let mut result = Map::new();
    result.insert("placed".into(), json!(placed));
    result.insert("rng_next".into(), json!(rng_next));
    result.insert(
        "scheduled".into(),
        Value::Array(
            ticks
                .iter()
                .map(|(x, y, z, kind, name)| json!([x, y, z, kind, name]))
                .collect(),
        ),
    );
    result.insert(
        "blocks".into(),
        Value::Array(
            changed
                .iter()
                .map(|(x, y, z, id)| json!([x, y, z, id]))
                .collect(),
        ),
    );
    if threw {
        result.insert("threw".into(), json!(true));
    }
    Outcome::Accepted(result)
}

/// The panic text, trimmed to the part that names the missing capability.
fn unported_reason(message: &str) -> String {
    const LIMIT: usize = 160;
    let first = message.lines().next().unwrap_or(message);
    let mut text: String = first.chars().take(LIMIT).collect();
    if first.chars().count() > LIMIT {
        text.push_str("...");
    }
    text
}
