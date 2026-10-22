use super::*;
use crate::level::WorldGenLevel;
use crate::levelgen::structure::{
    StructureDefinition, StructurePlacementAuthority, StructureStart, TerrainAdjustment,
};
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_registry::block_state::BlockState;
use rivet_util::{random_source::LegacyRandomSource, worldgen_random::WorldgenRandom};

fn definition() -> StructureDefinition {
    StructureDefinition::Mineshaft {
        registry_id: "minecraft:mineshaft".into(),
        terrain_adjustment: TerrainAdjustment::None,
        kind: MineshaftKind::Normal,
    }
}

#[test]
fn room_and_entrance_moves_preserve_actual_java_wrapped_inverted_bounds() {
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/mineshaft-move.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let input = parse_compound_fully(row["input"].as_str().unwrap()).unwrap();
        let mut piece = MineshaftPiece::load(&input, "msroom").unwrap();
        let delta = row["delta"].as_i64().unwrap() as i32;
        piece.move_by(delta, delta, delta);
        assert_eq!(
            piece.create_tag(),
            parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
        );
        assert!(
            piece.data.bounding_box.min_x > piece.data.bounding_box.max_x,
            "move must not normalize wrapped fields"
        );
    }
}

#[test]
fn actual_spawn_window_mineshaft_matches_all_135_ordered_pieces_and_full_saved_start() {
    let rows =
        include_str!("../../../../../tools/rivet-oracle/fixtures/structure-spawn-trace.jsonl");
    let row: serde_json::Value = rows
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|row| row["x"] == 4 && row["z"] == 0)
        .unwrap();
    let expected = parse_compound_fully(row["attempts"][0]["saved"].as_str().unwrap()).unwrap();
    let chunk = ChunkPos::new(4, 0);
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(42, 4, 0);
    let (pieces, _) = generate_pieces(MineshaftKind::Normal, chunk, &mut random, 63, -64, |_| {
        panic!("normal mineshaft does not query mesa surface")
    });
    assert_eq!(pieces.pieces().len(), 135);
    let start = StructureStart::new(definition(), chunk, 0, pieces);
    assert_eq!(start.create_tag(chunk), expected);
    let loaded = StructureStart::load(&expected, |_| Some(definition())).unwrap();
    assert_eq!(
        loaded.create_tag(chunk),
        expected,
        "all per-piece fields survive native decoding"
    );
    assert_eq!(
        loaded.bounding_box().unwrap(),
        BoundingBox::new([-15, 0, -36, 160, 17, 86])
    );
}

struct NoPlacement;
impl StructurePlacementAuthority for NoPlacement {
    fn abort_structure(&mut self) {
        panic!("no scope before capability preflight refusal")
    }
    fn level(&mut self) -> &mut dyn WorldGenLevel {
        panic!("no world access before refusal")
    }
    fn begin_structure(
        &mut self,
        _: &StructureDefinition,
        _: BoundingBox,
        _: ChunkPos,
    ) -> Result<(), StructureError> {
        panic!("no transformer before refusal")
    }
    fn finish_structure(&mut self) -> Result<(), StructureError> {
        panic!("no transformer before refusal")
    }
    fn place_loot_chest(
        &mut self,
        _: BlockPos,
        _: BlockState,
        _: &str,
        _: i64,
    ) -> Result<(), StructureError> {
        panic!("no chest before refusal")
    }
}

#[test]
fn unsupported_postprocess_refuses_before_world_transformer_or_random_side_effects() {
    let chunk = ChunkPos::new(4, 0);
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(42, 4, 0);
    let (pieces, _) = generate_pieces(
        MineshaftKind::Normal,
        chunk,
        &mut random,
        63,
        -64,
        |_| unreachable!(),
    );
    let mut start = StructureStart::new(definition(), chunk, 0, pieces);
    let bounds = start.bounding_box().unwrap();
    let mut placement_random = LegacyRandomSource::new(123);
    let untouched = LegacyRandomSource::new(123).next_long();
    assert!(matches!(
        start.place_in_chunk(&mut NoPlacement, &mut placement_random, bounds, chunk),
        Err(StructureError::PlacementUnavailable(_))
    ));
    assert_eq!(placement_random.next_long(), untouched);
}
