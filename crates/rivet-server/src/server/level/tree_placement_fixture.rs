//! Full-tree differential against pinned Paper, including real edge updates.

use super::beehive_data::BeehiveData;
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
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::chunk_generator::ChunkGenerator;
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::level::{WorldGenLevel, height_accessor::LevelHeightAccessor};
use rivet_world::levelgen::feature::configurations::tree_configuration::tree_configuration_codec;
use rivet_world::levelgen::feature::{FeatureBehavior, FeaturePlaceContext, TREE};
use rivet_world::levelgen::heightmap::{Heightmap, Types};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

struct Ground {
    states: HashMap<BlockPos, BlockState>,
    beehives: HashMap<BlockPos, BeehiveData>,
    materialize_beehives: bool,
    writes: Vec<u8>,
    write_count: u64,
    ticks: Vec<String>,
    random: Option<AlgorithmRandomSource>,
}

impl Ground {
    fn new() -> Self {
        Self {
            states: HashMap::new(),
            beehives: HashMap::new(),
            materialize_beehives: true,
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
        RegistryAccess::empty()
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.states.get(pos).copied().unwrap_or_else(|| {
            if pos.get_y() < 64 {
                Blocks::DIRT.default_block_state()
            } else {
                Blocks::AIR.default_block_state()
            }
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
        assert_eq!(
            fluid,
            FluidId::WATER,
            "fixture only reaches leaf water ticks"
        );
        self.tick(pos, "minecraft:water", delay);
    }
    fn schedule_block_tick(&mut self, pos: &BlockPos, block: Block, delay: i32) {
        self.tick(pos, block.name(), delay);
    }
    fn update_tree_shape_at_edge(&mut self, filled: &HashSet<BlockPos>) {
        let mut random = self.random.take().expect("world RNG");
        update_shape_at_edge(self, &mut random, filled);
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
    let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
    let mut cases = 0;
    let mut occupied_successes = HashSet::new();
    let mut missing_hive_cases = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/tree-placement.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).expect("Paper tree fixture");
        let name = row["feature"].as_str().expect("feature");
        let seed = row["seed"].as_i64().expect("seed");
        let decoded = tree_configuration_codec().parse(&ops, &row["config"]);
        let config = decoded
            .result()
            .unwrap_or_else(|| panic!("{name} config: {decoded:?}"));
        let mut ground = Ground::new();
        ground.materialize_beehives = row["block_entities"].as_bool().unwrap_or(true);
        let mut random = XoroshiroRandomSource::new(seed);
        let result = TREE.place(&mut FeaturePlaceContext::new(
            None,
            &mut ground,
            &Generator,
            &mut random,
            &BlockPos::new(0, 64, 0),
            config,
        ));
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
        cases += 1;
    }
    assert_eq!(cases, 143);
    assert_eq!(missing_hive_cases, 1);
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
