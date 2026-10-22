//! Actual Paper postProcess output on the same controlled world storage.
//! Entity/Craft callbacks are deliberately unavailable: the source corpus covers
//! legitimate no-entity paths, not fabricated successful entity construction.
use super::*;
use crate::level::{WorldGenLevel, height_accessor::LevelHeightAccessor};
use crate::levelgen::heightmap::Types;
use crate::levelgen::structure::piece_placement::block;
use crate::levelgen::structure::{StructureDefinition, StructurePiece};
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_registry::{
    Identifier, RegistryAccess, ResourceKey, biome_id::BiomeId, block_state::BlockState,
    fluid_id::FluidId, holder::Holder, holder_lookup::HolderGetter, registries::BIOME,
};
use rivet_util::random_source::LegacyRandomSource;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

struct Ground {
    terrain: String,
    access: RegistryAccess,
    states: HashMap<BlockPos, BlockState>,
    writes: Vec<u8>,
    marks: Vec<u8>,
    ticks: Vec<String>,
    write_count: usize,
    mark_count: usize,
}
impl Ground {
    fn new(terrain: &str) -> Self {
        Self {
            terrain: terrain.into(),
            access: crate::data::worldgen::worldgen_bootstraps::build_worldgen_registries(),
            states: HashMap::new(),
            writes: Vec::new(),
            marks: Vec::new(),
            ticks: Vec::new(),
            write_count: 0,
            mark_count: 0,
        }
    }
    fn states_hash(&self) -> String {
        let mut entries: Vec<_> = self.states.iter().collect();
        entries.sort_by_key(|(p, _)| (p.get_x(), p.get_y(), p.get_z()));
        let mut bytes = Vec::new();
        for (p, s) in entries {
            append(
                &mut bytes,
                &[p.get_x(), p.get_y(), p.get_z(), i32::from(s.id().0)],
            );
        }
        hash(&bytes)
    }
}
fn append(bytes: &mut Vec<u8>, ints: &[i32]) {
    for i in ints {
        bytes.extend_from_slice(&i.to_be_bytes());
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct CaptureAuthority {
    ground: Ground,
    entry: Option<(&'static str, BlockPos, u32)>,
}
impl StructurePlacementAuthority for CaptureAuthority {
    fn abort_structure(&mut self) {} // Direct piece fixture owns no transformer scope.
    fn level(&mut self) -> &mut dyn WorldGenLevel {
        &mut self.ground
    }
    fn require_mineshaft_placement(&self) -> Result<(), StructureError> {
        Ok(())
    }
    fn begin_structure(
        &mut self,
        _: &StructureDefinition,
        _: BoundingBox,
        _: rivet_registry::core::ChunkPos,
    ) -> Result<(), StructureError> {
        panic!("direct piece oracle does not invoke structure events")
    }
    fn finish_structure(&mut self) -> Result<(), StructureError> {
        panic!("direct piece oracle does not invoke structure events")
    }
    fn place_loot_chest(
        &mut self,
        _: BlockPos,
        _: BlockState,
        _: &str,
        _: i64,
    ) -> Result<(), StructureError> {
        panic!("mineshaft does not place a block chest")
    }
    fn place_cave_spider_spawner(
        &mut self,
        pos: BlockPos,
        flags: u32,
    ) -> Result<(), StructureError> {
        self.entry = Some(("craft_spawner", pos, flags));
        Err(StructureError::PlacementUnavailable(
            "captured authority entry".into(),
        ))
    }
    fn place_loot_minecart<R: RandomSource>(
        &mut self,
        pos: BlockPos,
        loot: &str,
        _: &mut R,
    ) -> Result<(), StructureError> {
        assert_eq!(loot, "minecraft:chests/abandoned_mineshaft");
        self.entry = Some(("chest_minecart", pos, 2));
        Err(StructureError::PlacementUnavailable(
            "captured authority entry".into(),
        ))
    }
}

#[test]
fn positive_authority_entry_matches_actual_paper_before_unimplemented_external_bridge() {
    let mut count = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/mineshaft-authority.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let boundary = row["boundary"].as_str().unwrap();
        let mut piece =
            StructurePiece::load(&parse_compound_fully(row["input"].as_str().unwrap()).unwrap())
                .unwrap();
        let mut authority = CaptureAuthority {
            ground: Ground::new(row["terrain"].as_str().unwrap()),
            entry: None,
        };
        let mut random = LegacyRandomSource::new(row["seed"].as_i64().unwrap());
        let clip: [i32; 6] = serde_json::from_value(row["clip"].clone()).unwrap();
        let error = piece
            .post_process(&mut authority, &mut random, BoundingBox::new(clip))
            .unwrap_err();
        assert_eq!(
            error,
            StructureError::PlacementUnavailable("captured authority entry".into())
        );
        let (actual, pos, flags) = authority.entry.expect("positive authority entry reached");
        assert_eq!(actual, boundary);
        assert_eq!(
            serde_json::json!([pos.get_x(), pos.get_y(), pos.get_z()]),
            row["position"]
        );
        assert_eq!(u64::from(flags), row["flags"].as_u64().unwrap());
        let ground = &authority.ground;
        assert_eq!(
            ground.write_count as u64,
            row["writes"].as_u64().unwrap(),
            "{boundary} writes before entry"
        );
        assert_eq!(
            hash(&ground.writes),
            row["writes_sha256"].as_str().unwrap(),
            "{boundary} ordered writes before entry"
        );
        assert_eq!(
            ground.states.len() as u64,
            row["states"].as_u64().unwrap(),
            "{boundary} state count"
        );
        assert_eq!(
            ground.states_hash(),
            row["states_sha256"].as_str().unwrap(),
            "{boundary} final states before entry"
        );
        assert_eq!(
            ground.mark_count as u64,
            row["marks"].as_u64().unwrap(),
            "{boundary} marks"
        );
        assert_eq!(
            hash(&ground.marks),
            row["marks_sha256"].as_str().unwrap(),
            "{boundary} mark order"
        );
        assert_eq!(
            ground.ticks.len() as u64,
            row["ticks"].as_u64().unwrap(),
            "{boundary} ticks"
        );
        assert_eq!(
            hash(ground.ticks.join("\n").as_bytes()),
            row["ticks_sha256"].as_str().unwrap(),
            "{boundary} tick order"
        );
        assert_eq!(
            random.next_long(),
            row["rng_next"].as_i64().unwrap(),
            "{boundary} feature RNG before handoff"
        );
        assert_eq!(
            piece.create_tag(),
            parse_compound_fully(row["saved"].as_str().unwrap()).unwrap(),
            "{boundary} hps flag/NBT before handoff"
        );
        count += 1;
    }
    assert_eq!(count, 2);
}

impl LevelHeightAccessor for Ground {
    fn get_min_y(&self) -> i32 {
        -64
    }
    fn get_height(&self) -> i32 {
        384
    }
}
impl WorldGenLevel for Ground {
    fn get_seed(&self) -> i64 {
        42
    }
    fn get_block_state(&self, p: &BlockPos) -> BlockState {
        if let Some(s) = self.states.get(p) {
            return *s;
        }
        if self.terrain == "void" {
            return block("minecraft:air");
        }
        if self.terrain == "chain_roof" || self.terrain == "falling_roof" {
            return block(if p.get_y() == 53 {
                if self.terrain == "chain_roof" {
                    "minecraft:stone"
                } else {
                    "minecraft:sand"
                }
            } else {
                "minecraft:air"
            });
        }
        if self.terrain == "water" && *p == BlockPos::new(-1, 50, 0) {
            return block("minecraft:water");
        }
        block(if p.get_y() <= 70 {
            "minecraft:stone"
        } else {
            "minecraft:air"
        })
    }
    fn get_biome(&self, _: &BlockPos) -> Holder<BiomeId> {
        self.access
            .lookup(&BIOME)
            .unwrap()
            .get_or_throw(&ResourceKey::create(
                &BIOME,
                Identifier::with_default_namespace(if self.terrain == "blocked" {
                    "deep_dark"
                } else {
                    "plains"
                }),
            ))
    }
    fn registry_access(&self) -> RegistryAccess {
        self.access.clone()
    }
    fn get_height_at(&mut self, ty: Types, x: i32, z: i32) -> i32 {
        assert_eq!(ty, Types::OceanFloorWg);
        for y in (-64..=319).rev() {
            if self
                .get_block_state(&BlockPos::new(x, y, z))
                .blocks_motion()
            {
                return y + 1;
            }
        }
        -64
    }
    fn set_block(&mut self, p: &BlockPos, s: BlockState, flags: u32) -> bool {
        self.states.insert(*p, s);
        append(
            &mut self.writes,
            &[
                p.get_x(),
                p.get_y(),
                p.get_z(),
                i32::from(s.id().0),
                flags as i32,
            ],
        );
        self.write_count += 1;
        true
    }
    fn schedule_tick(&mut self, p: &BlockPos, fluid: FluidId, delay: i32) {
        self.ticks.push(format!(
            "{},{},{},{},{}",
            p.get_x(),
            p.get_y(),
            p.get_z(),
            fluid.name(),
            delay
        ));
    }
    fn mark_pos_for_post_processing(&mut self, p: &BlockPos) {
        append(&mut self.marks, &[p.get_x(), p.get_y(), p.get_z()]);
        self.mark_count += 1;
    }
}
impl StructurePlacementAuthority for Ground {
    fn abort_structure(&mut self) {} // Direct fixture has no staged resources.
    fn level(&mut self) -> &mut dyn WorldGenLevel {
        self
    }
    fn begin_structure(
        &mut self,
        _: &StructureDefinition,
        _: BoundingBox,
        _: rivet_registry::core::ChunkPos,
    ) -> Result<(), StructureError> {
        Ok(())
    }
    fn finish_structure(&mut self) -> Result<(), StructureError> {
        Ok(())
    }
    fn place_loot_chest(
        &mut self,
        _: BlockPos,
        _: BlockState,
        _: &str,
        _: i64,
    ) -> Result<(), StructureError> {
        panic!("mineshaft does not place block chests")
    }
    fn require_mineshaft_placement(&self) -> Result<(), StructureError> {
        Ok(())
    }
    // These test-world callbacks deliberately retain their default refusal. Java
    // likewise refuses getLevel: corpus cases must not reach either callback.
}
#[test]
fn all_mineshaft_piece_placements_match_genuine_paper_writes_states_marks_and_rng() {
    let mut count = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/mineshaft-placement.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let context = row["case"].as_str().unwrap();
        let mut piece =
            StructurePiece::load(&parse_compound_fully(row["input"].as_str().unwrap()).unwrap())
                .unwrap();
        let mut ground = Ground::new(row["terrain"].as_str().unwrap());
        let mut random = LegacyRandomSource::new(row["seed"].as_i64().unwrap());
        let clip: [i32; 6] = serde_json::from_value(row["clip"].clone()).unwrap();
        piece
            .post_process(&mut ground, &mut random, BoundingBox::new(clip))
            .unwrap_or_else(|e| panic!("{context}: {e}"));
        assert_eq!(
            ground.write_count as u64,
            row["writes"].as_u64().unwrap(),
            "{context} write count"
        );
        assert_eq!(
            hash(&ground.writes),
            row["writes_sha256"].as_str().unwrap(),
            "{context} ordered writes"
        );
        assert_eq!(
            ground.states.len() as u64,
            row["states"].as_u64().unwrap(),
            "{context} state count"
        );
        assert_eq!(
            ground.states_hash(),
            row["states_sha256"].as_str().unwrap(),
            "{context} final states"
        );
        assert_eq!(
            ground.mark_count as u64,
            row["marks"].as_u64().unwrap(),
            "{context} marks count"
        );
        assert_eq!(
            hash(&ground.marks),
            row["marks_sha256"].as_str().unwrap(),
            "{context} ordered marks"
        );
        assert_eq!(
            ground.ticks.len() as u64,
            row["ticks"].as_u64().unwrap(),
            "{context} tick count"
        );
        assert_eq!(
            hash(ground.ticks.join("\n").as_bytes()),
            row["ticks_sha256"].as_str().unwrap(),
            "{context} tick order"
        );
        assert_eq!(
            random.next_long(),
            row["rng_next"].as_i64().unwrap(),
            "{context} RNG continuation"
        );
        assert_eq!(
            piece.create_tag(),
            parse_compound_fully(row["saved"].as_str().unwrap()).unwrap(),
            "{context} mutated native piece data"
        );
        count += 1;
    }
    assert_eq!(count, 20);
}
