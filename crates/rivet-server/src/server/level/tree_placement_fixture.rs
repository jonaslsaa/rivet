//! Full-tree differential against pinned Paper, including real edge updates.

use rivet_world::block_entity::beehive_data::BeehiveData;
use super::block_shape_update::update_shape_at_edge;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_registry::access::RegistryAccess;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::fluid_id::FluidId;
use rivet_registry::registry_ops::RegistryOps;
use rivet_serialization::json_ops::JsonOps;
use rivet_util::RandomSource;
use rivet_util::random_source::XoroshiroRandomSource;
use rivet_util::worldgen_random::AlgorithmRandomSource;
use rivet_world::block::Block;
use rivet_world::chunk::chunk_generator::ChunkGenerator;
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::level::{WorldGenLevel, height_accessor::LevelHeightAccessor};
use rivet_world::levelgen::feature::configurations::tree_configuration::tree_configuration_codec;
use rivet_world::levelgen::feature::configurations::vegetation_patch_configuration::vegetation_patch_configuration_codec;
use rivet_world::levelgen::feature::{
    FeatureBehavior, FeaturePlaceContext, TREE, VEGETATION_PATCH,
};
use rivet_world::levelgen::heightmap::{Heightmap, Types};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

struct Ground {
    states: HashMap<BlockPos, BlockState>,
    beehives: HashMap<BlockPos, BeehiveData>,
    materialize_beehives: bool,
    terrain: String,
    registry_access: RegistryAccess,
    pending_hearts: HashMap<BlockPos, CompoundTag>,
    writes: Vec<u8>,
    write_count: u64,
    ticks: Vec<String>,
    random: Option<AlgorithmRandomSource>,
}

impl Ground {
    fn new(registry_access: RegistryAccess) -> Self {
        Self {
            states: HashMap::new(),
            beehives: HashMap::new(),
            materialize_beehives: true,
            terrain: "dry_dirt".to_string(),
            registry_access,
            pending_hearts: HashMap::new(),
            writes: Vec::new(),
            write_count: 0,
            ticks: Vec::new(),
            random: Some(AlgorithmRandomSource::Xoroshiro(
                XoroshiroRandomSource::new(99),
            )),
        }
    }

    fn blocks_bytes(&self) -> Vec<u8> {
        let mut positions: Vec<_> = self.states.keys().copied().collect();
        positions.sort_by_key(|pos| (pos.get_x(), pos.get_y(), pos.get_z()));
        let mut bytes = Vec::new();
        for pos in positions {
            for value in [
                pos.get_x(),
                pos.get_y(),
                pos.get_z(),
                self.states[&pos].id().0 as i32,
            ] {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
        }
        bytes
    }

    fn tick(&mut self, pos: &BlockPos, name: &str, delay: i32) {
        self.ticks.push(format!(
            "{},{},{},{},{}",
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
            name,
            delay
        ));
    }

    fn assert_beehives_match(&self, expected: &serde_json::Value, context: &str) {
        let mut positions: Vec<_> = self.beehives.keys().copied().collect();
        positions.sort_by_key(|pos| (pos.get_x(), pos.get_y(), pos.get_z()));
        let expected = expected.as_array().expect("Paper beehives array");
        assert_eq!(positions.len(), expected.len(), "live hive count {context}");
        for (pos, row) in positions.into_iter().zip(expected) {
            assert_eq!(
                serde_json::json!([pos.get_x(), pos.get_y(), pos.get_z()]),
                row["pos"],
                "hive position {context}"
            );
            let tag = self.beehives[&pos].save_with_full_metadata(&pos);
            let java_tag =
                parse_compound_fully(row["snbt"].as_str().expect("actual Paper full hive SNBT"))
                    .expect("valid Paper hive NBT");
            assert_eq!(tag, java_tag, "complete persisted hive NBT {context}");
            assert_eq!(
                tag.get_list("bees").expect("saved hive bee list").size() as u64,
                row["occupants"].as_u64().unwrap(),
                "stored occupants {context}"
            );
        }
    }
}

impl LevelHeightAccessor for Ground {
    fn get_height(&self) -> i32 {
        384
    }
    fn get_min_y(&self) -> i32 {
        -64
    }
}

impl WorldGenLevel for Ground {
    fn get_seed(&self) -> i64 {
        42
    }
    fn get_sea_level(&self) -> i32 {
        63
    }
    fn registry_access(&self) -> RegistryAccess {
        self.registry_access.clone()
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.states.get(pos).copied().unwrap_or_else(|| {
            let y = pos.get_y();
            let block = match self.terrain.as_str() {
                "dry_dirt" => {
                    if y < 64 {
                        "dirt"
                    } else {
                        "air"
                    }
                }
                "dry_mud" => {
                    if y < 63 {
                        "dirt"
                    } else if y < 64 {
                        "mud"
                    } else {
                        "air"
                    }
                }
                "shallow_water" => {
                    if y < 61 {
                        "dirt"
                    } else if y == 61 {
                        "mud"
                    } else if y < 64 {
                        "water"
                    } else {
                        "air"
                    }
                }
                "root_limit_void" => "air",
                "clay_basin" | "clay_basin_leak" => {
                    let inside = pos.get_x().abs() <= 3 && pos.get_z().abs() <= 3;
                    if (self.terrain == "clay_basin_leak"
                        && pos.get_x() == 0
                        && pos.get_z() == 0
                        && y == 60)
                        || y >= 64 - if inside { 2 } else { 0 }
                    {
                        "air"
                    } else {
                        "dirt"
                    }
                }
                "patch_wall" => {
                    if y < 64 {
                        "dirt"
                    } else if (pos.get_x() == 2 && pos.get_z() == 0 && y < 67)
                        || (pos.get_x() == -2 && pos.get_z() == 1 && y < 66)
                    {
                        "stone"
                    } else {
                        "air"
                    }
                }
                other => panic!("unknown Java fixture terrain {other}"),
            };
            BlockState::of(
                rivet_registry::generated::blocks::BlockId::from_name(&format!(
                    "minecraft:{block}"
                ))
                .expect("fixture block exists"),
            )
        })
    }
    fn is_empty_block(&self, pos: &BlockPos) -> bool {
        self.get_block_state(pos).is_air()
    }
    fn can_survive(&self, state: &BlockState, pos: &BlockPos) -> bool {
        super::world_gen_region::region_can_survive(self, state, pos)
    }
    fn get_height_at(&mut self, ty: Types, x: i32, z: i32) -> i32 {
        for y in (self.get_min_y()..=self.get_max_y()).rev() {
            let state = self.get_block_state(&BlockPos::new(x, y, z));
            if Heightmap::is_opaque(ty, resolve_state_flags(&state)) {
                return y + 1;
            }
        }
        self.get_min_y()
    }
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        let old = self.get_block_state(pos);
        self.states.insert(*pos, state);
        if state.block().name() == "minecraft:creaking_heart" {
            let mut tag = CompoundTag::new();
            tag.put_int("x", pos.get_x());
            tag.put_int("y", pos.get_y());
            tag.put_int("z", pos.get_z());
            tag.put_string("id", "DUMMY");
            self.pending_hearts.insert(*pos, tag);
        } else if old.block() != state.block() {
            self.pending_hearts.remove(pos);
        }
        if self.materialize_beehives
            && matches!(
                state.block().name(),
                "minecraft:bee_nest" | "minecraft:beehive"
            )
        {
            if old.block() != state.block() || !self.beehives.contains_key(pos) {
                self.beehives
                    .insert(*pos, BeehiveData::load(&CompoundTag::new()));
            }
        } else if old.block() != state.block() {
            self.beehives.remove(pos);
        }
        self.write_count += 1;
        for value in [
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
            state.id().0 as i32,
            flags as i32,
        ] {
            self.writes.extend_from_slice(&value.to_be_bytes());
        }
        true
    }
    fn has_beehive_block_entity(&mut self, pos: &BlockPos) -> bool {
        self.beehives.contains_key(pos)
    }
    fn store_beehive_occupant(&mut self, pos: &BlockPos, ticks_in_hive: i32) {
        self.beehives
            .get_mut(pos)
            .expect("genuine hive entity required")
            .store_worldgen_bee(ticks_in_hive);
    }
    fn schedule_tick(&mut self, pos: &BlockPos, fluid: FluidId, delay: i32) {
        self.tick(pos, fluid.name(), delay);
    }
    fn schedule_block_tick(&mut self, pos: &BlockPos, block: Block, delay: i32) {
        self.tick(pos, block.name(), delay);
    }
    fn update_tree_shape_at_edge(&mut self, filled: &HashSet<BlockPos>) {
        let mut random = self.random.take().expect("world RNG");
        update_shape_at_edge(self, &mut random, filled);
        self.random = Some(random);
    }
    fn place_mossy_carpet(&mut self, pos: &BlockPos, flags: u32) {
        let mut random = self
            .random
            .take()
            .expect("world RNG for actual moss carpet placement");
        rivet_world::block::mossy_carpet_block::place_at(self, pos, &mut random, flags);
        self.random = Some(random);
    }
}

struct Generator;
impl ChunkGenerator for Generator {
    fn get_min_y(&self) -> i32 {
        -64
    }
    fn get_gen_depth(&self) -> i32 {
        384
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn full_trees_match_paper_writes_blocks_ticks_and_both_random_streams() {
    let access = super::generated_world::build_feature_access(
        &rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries(),
    );
    let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone());
    let mut cases = 0;
    let mut occupied_successes = HashSet::new();
    let mut missing_hive_cases = 0;
    let mut rooted_successes = HashSet::new();
    let mut root_limit_failures = 0;
    let mut pale_heart_successes = 0;
    let mut moss_level_rng_successes = 0;
    let mut patch_cases = 0;
    let mut clay_cases = 0;
    let mut clay_small = false;
    let mut clay_big = false;
    let mut clay_wet = false;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/tree-placement.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).expect("Paper tree fixture");
        let name = row["feature"].as_str().expect("feature");
        let seed = row["seed"].as_i64().expect("seed");
        let mut ground = Ground::new(access.clone());
        ground.materialize_beehives = row["block_entities"].as_bool().unwrap_or(true);
        ground.terrain = row["terrain"].as_str().unwrap_or("dry_dirt").to_string();
        let mut random = XoroshiroRandomSource::new(seed);
        let result = if row["kind"] == "named_configured" {
            clay_cases += 1;
            access
                .lookup(&*rivet_world::levelgen::feature::registry_keys::CONFIGURED_FEATURE)
                .expect("real configured registry")
                .get_optional(&rivet_registry::Identifier::parse(&format!(
                    "minecraft:{name}"
                )))
                .expect("actual generated configured feature")
                .place(
                    &mut ground,
                    &Generator,
                    &mut random,
                    &BlockPos::new(0, 64, 0),
                )
        } else if row["kind"] == "vegetation_patch" {
            let decoded = vegetation_patch_configuration_codec().parse(&ops, &row["config"]);
            let config = decoded
                .result()
                .unwrap_or_else(|| panic!("{name} patch config: {decoded:?}"));
            patch_cases += 1;
            VEGETATION_PATCH.place(&mut FeaturePlaceContext::new(
                None,
                &mut ground,
                &Generator,
                &mut random,
                &BlockPos::new(0, 64, 0),
                config,
            ))
        } else {
            let decoded = tree_configuration_codec().parse(&ops, &row["config"]);
            let config = decoded
                .result()
                .unwrap_or_else(|| panic!("{name} config: {decoded:?}"));
            TREE.place(&mut FeaturePlaceContext::new(
                None,
                &mut ground,
                &Generator,
                &mut random,
                &BlockPos::new(0, 64, 0),
                config,
            ))
        };
        let context = format!("{name} seed {seed}");
        assert_eq!(result, row["result"].as_bool().unwrap(), "result {context}");
        assert_eq!(
            random.next_long(),
            row["next_long"].as_i64().unwrap(),
            "feature RNG {context}"
        );
        assert_eq!(
            ground.random.as_mut().unwrap().next_long(),
            row["world_next_long"].as_i64().unwrap(),
            "world RNG {context}"
        );
        assert_eq!(
            ground.write_count,
            row["writes"].as_u64().unwrap(),
            "write count {context}"
        );
        assert_eq!(
            hash(&ground.writes),
            row["writes_sha256"].as_str().unwrap(),
            "ordered writes {context}"
        );
        assert_eq!(
            hash(&ground.blocks_bytes()),
            row["blocks_sha256"].as_str().unwrap(),
            "final blocks {context}"
        );
        assert_eq!(
            ground.ticks.len() as u64,
            row["ticks"].as_u64().unwrap(),
            "tick count {context}"
        );
        assert_eq!(
            hash(ground.ticks.join("\n").as_bytes()),
            row["ticks_sha256"].as_str().unwrap(),
            "ordered ticks {context}"
        );
        if let Some(beehives) = row.get("beehives") {
            ground.assert_beehives_match(beehives, &context);
            if !ground.materialize_beehives {
                assert!(
                    ground
                        .states
                        .values()
                        .any(|state| state.block().name() == "minecraft:bee_nest"),
                    "the no-entity branch must still place a real nest {context}"
                );
                assert!(ground.beehives.is_empty());
                missing_hive_cases += 1;
            }
            if row["seed_search_success"] == true {
                assert!(
                    beehives
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|hive| hive["occupants"].as_u64().unwrap() >= 2),
                    "positive hive case {context}"
                );
                occupied_successes.insert(name.to_string());
            }
        }
        if row.get("roots").is_some() {
            let terrain = row["terrain"].as_str().expect("root case terrain");
            let roots = ground
                .states
                .values()
                .filter(|state| state.block().name() == "minecraft:mangrove_roots")
                .count();
            let muddy_roots = ground
                .states
                .values()
                .filter(|state| state.block().name() == "minecraft:muddy_mangrove_roots")
                .count();
            let waterlogged_roots = ground.states.values().filter(|state| state.block().name() == "minecraft:mangrove_roots" && state.get_value(rivet_registry::block_state_properties::BlockStateProperties::WATERLOGGED) == Some(rivet_registry::block_state_property::PropertyValue::Bool(true))).count();
            assert_eq!(
                roots as u64,
                row["roots"].as_u64().unwrap(),
                "root count {context}"
            );
            assert_eq!(
                muddy_roots as u64,
                row["muddy_roots"].as_u64().unwrap(),
                "muddy roots {context}"
            );
            assert_eq!(
                waterlogged_roots as u64,
                row["waterlogged_roots"].as_u64().unwrap(),
                "waterlogged roots {context}"
            );
            if terrain == "root_limit_void" {
                assert!(
                    !result,
                    "root length limit must reject an unsupported root network {context}"
                );
                assert_eq!(ground.write_count, 0);
                assert!(ground.ticks.is_empty());
                root_limit_failures += 1;
            }
            if row["root_seed_search_success"] == true {
                assert!(result && roots > 0);
                if terrain == "dry_mud" {
                    assert!(muddy_roots > 0);
                }
                if terrain == "shallow_water" {
                    assert!(waterlogged_roots > 0);
                }
                rooted_successes.insert((name.to_string(), terrain.to_string()));
            }
        }
        for (field, block) in [
            ("pale_moss_ground", "pale_moss_block"),
            ("pale_moss_carpet", "pale_moss_carpet"),
            ("tall_grass", "tall_grass"),
            ("pale_hanging_moss", "pale_hanging_moss"),
            ("creaking_hearts", "creaking_heart"),
            ("clay", "clay"),
            ("water", "water"),
            ("small_dripleaf", "small_dripleaf"),
            ("big_dripleaf", "big_dripleaf"),
            ("big_dripleaf_stem", "big_dripleaf_stem"),
        ] {
            if let Some(expected) = row[field].as_u64() {
                let name = format!("minecraft:{block}");
                assert_eq!(
                    ground
                        .states
                        .values()
                        .filter(|state| state.block().name() == name)
                        .count() as u64,
                    expected,
                    "actual {field} placement {context}"
                );
            }
        }
        if row["kind"] == "named_configured" {
            use rivet_registry::block_state_properties::BlockStateProperties;
            use rivet_registry::block_state_property::PropertyValue;
            let wet = |state: &&BlockState| {
                state.get_value(BlockStateProperties::WATERLOGGED)
                    == Some(PropertyValue::Bool(true))
            };
            let waterlogged = ground.states.values().filter(wet).count() as u64;
            assert_eq!(
                waterlogged,
                row["waterlogged_dripleaves"].as_u64().unwrap(),
                "waterlogged {context}"
            );
            for half in ["lower", "upper"] {
                let count = ground
                    .states
                    .values()
                    .filter(|state| {
                        state.block().name() == "minecraft:small_dripleaf"
                            && wet(state)
                            && state.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF)
                                == Some(PropertyValue::Enum(half))
                    })
                    .count() as u64;
                assert_eq!(
                    count,
                    row[format!("waterlogged_small_{half}")].as_u64().unwrap(),
                    "small {half} waterlogging {context}"
                );
            }
            clay_small |= row["small_dripleaf"].as_u64().unwrap() > 0;
            clay_big |= row["big_dripleaf"].as_u64().unwrap() > 0;
            clay_wet |= waterlogged > 0;
        }
        if let Some(hearts) = row["pending_heart_entities"].as_array() {
            assert_eq!(
                ground.pending_hearts.len(),
                hearts.len(),
                "pending heart count {context}"
            );
            for heart in hearts {
                let xyz: [i32; 3] = serde_json::from_value(heart["pos"].clone()).unwrap();
                let tag = ground
                    .pending_hearts
                    .get(&BlockPos::new(xyz[0], xyz[1], xyz[2]))
                    .expect("actual pending heart metadata");
                assert_eq!(
                    *tag,
                    parse_compound_fully(heart["snbt"].as_str().unwrap()).unwrap(),
                    "pending heart NBT {context}"
                );
            }
        }
        if row["pale_seed_search_success"] == true {
            assert!(
                row["creaking_hearts"].as_u64().unwrap() > 0
                    && row["pale_moss_ground"].as_u64().unwrap() > 0
            );
            assert!(!ground.pending_hearts.is_empty());
            pale_heart_successes += 1;
        }
        if row["moss_level_rng_success"] == true {
            assert!(
                row["pale_moss_carpet"].as_u64().unwrap() > 0
                    && row["tall_grass"].as_u64().unwrap() > 0
            );
            assert_ne!(
                row["world_next_long"].as_i64().unwrap(),
                1454447627545378826
            );
            moss_level_rng_successes += 1;
        }
        cases += 1;
    }
    assert_eq!(cases, 251);
    assert_eq!(clay_cases, 48);
    assert!(clay_small && clay_big && clay_wet);
    assert_eq!(patch_cases, 13);
    assert_eq!(pale_heart_successes, 1);
    assert_eq!(moss_level_rng_successes, 1);
    assert_eq!(missing_hive_cases, 1);
    assert_eq!(root_limit_failures, 8);
    assert_eq!(
        rooted_successes,
        ["mangrove", "tall_mangrove"]
            .into_iter()
            .flat_map(|name| ["dry_dirt", "dry_mud", "shallow_water"]
                .map(|terrain| (name.to_string(), terrain.to_string())))
            .collect()
    );
    assert_eq!(
        occupied_successes,
        HashSet::from([
            "oak_bees_005".to_string(),
            "oak_bees_002".to_string(),
            "oak_bees_0002_leaf_litter".to_string(),
            "birch_bees_005".to_string(),
            "birch_bees_002".to_string(),
            "birch_bees_0002".to_string(),
            "birch_bees_0002_leaf_litter".to_string(),
            "fancy_oak_bees".to_string(),
            "fancy_oak_bees_005".to_string(),
            "fancy_oak_bees_002".to_string(),
            "fancy_oak_bees_0002_leaf_litter".to_string(),
            "super_birch_bees".to_string(),
            "super_birch_bees_0002".to_string(),
            "cherry_bees_005".to_string(),
        ])
    );
}
