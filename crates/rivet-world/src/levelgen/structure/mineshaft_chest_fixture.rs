//! Positive full corridor algorithm, native constructor and delayed loot draw.
//! Java admits via actual ProtoChunk; production Rust region storage is tested
//! separately in world_gen_region's center/dependency ownership test.
use super::*;
use crate::entity::chest_minecart::{EntityConstructionContext, NativeChestMinecart};
use rivet_registry::core::Vec3;
use rivet_util::mth::Uuid;

struct NativeChestAuthority {
    ground: Ground,
    entity_random: LegacyRandomSource,
    entities: Vec<NativeChestMinecart>,
}
impl StructurePlacementAuthority for NativeChestAuthority {
    fn level(&mut self) -> &mut dyn WorldGenLevel {
        &mut self.ground
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
    fn abort_structure(&mut self) {}
    fn require_mineshaft_placement(&self) -> Result<(), StructureError> {
        Ok(())
    }
    fn place_loot_chest(
        &mut self,
        _: BlockPos,
        _: BlockState,
        _: &str,
        _: i64,
    ) -> Result<(), StructureError> {
        panic!("no block chest in source mineshaft")
    }
    fn place_loot_minecart<R: RandomSource>(
        &mut self,
        pos: BlockPos,
        loot: &str,
        feature_random: &mut R,
    ) -> Result<(), StructureError> {
        let mut entity = NativeChestMinecart::new(
            EntityConstructionContext {
                entity_id: 101 + self.entities.len() as i32,
                world_uuid: Uuid {
                    most: 0x1234_5678_9abc_def0,
                    least: 0xfedc_ba98_7654_3210_u64 as i64,
                },
                experimental_minecart_movement: false,
            },
            &mut self.entity_random,
        );
        entity.set_initial_pos(Vec3::new(
            f64::from(pos.get_x()) + 0.5,
            f64::from(pos.get_y()) + 0.5,
            f64::from(pos.get_z()) + 0.5,
        ));
        entity.set_loot_table(Identifier::parse(loot), feature_random.next_long());
        self.entities.push(entity);
        Ok(())
    }
}

#[test]
fn genuine_positive_corridor_matches_full_writes_rng_and_saved_chest() {
    let row: serde_json::Value = serde_json::from_str(
        include_str!("../../../../../tools/rivet-oracle/fixtures/mineshaft-chest.jsonl").trim(),
    )
    .unwrap();
    let mut piece =
        StructurePiece::load(&parse_compound_fully(row["input"].as_str().unwrap()).unwrap())
            .unwrap();
    let mut authority = NativeChestAuthority {
        ground: Ground::new(row["terrain"].as_str().unwrap()),
        entity_random: LegacyRandomSource::new(row["entity_seed"].as_i64().unwrap()),
        entities: Vec::new(),
    };
    let mut random = LegacyRandomSource::new(row["seed"].as_i64().unwrap());
    let clip: [i32; 6] = serde_json::from_value(row["clip"].clone()).unwrap();
    piece
        .post_process(&mut authority, &mut random, BoundingBox::new(clip))
        .unwrap();
    let ground = &authority.ground;
    assert_eq!(ground.write_count as u64, row["writes"].as_u64().unwrap());
    assert_eq!(hash(&ground.writes), row["writes_sha256"].as_str().unwrap());
    assert_eq!(ground.states.len() as u64, row["states"].as_u64().unwrap());
    assert_eq!(ground.states_hash(), row["states_sha256"].as_str().unwrap());
    assert_eq!(ground.mark_count as u64, row["marks"].as_u64().unwrap());
    assert_eq!(hash(&ground.marks), row["marks_sha256"].as_str().unwrap());
    assert_eq!(ground.ticks.len() as u64, row["ticks"].as_u64().unwrap());
    assert_eq!(
        hash(ground.ticks.join("\n").as_bytes()),
        row["ticks_sha256"].as_str().unwrap()
    );
    assert_eq!(random.next_long(), row["rng_next"].as_i64().unwrap());
    assert_eq!(
        authority.entity_random.next_long(),
        row["entity_rng_next"].as_i64().unwrap()
    );
    assert_eq!(
        piece.create_tag(),
        parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
    );
    let expected: Vec<_> = row["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| parse_compound_fully(s.as_str().unwrap()).unwrap())
        .collect();
    assert!(
        !expected.is_empty(),
        "actual positive constructor/admission path"
    );
    let actual: Vec<_> = authority
        .entities
        .iter()
        .map(|e| e.save().unwrap())
        .collect();
    assert_eq!(actual, expected);
}
