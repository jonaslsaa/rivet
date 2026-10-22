use super::*;
use crate::level::{WorldGenLevel, height_accessor::LevelHeightAccessor};
use crate::levelgen::{heightmap::Types, structure::structure_piece::reorient_chest};
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_registry::{block_state::BlockState, generated::blocks::BlockId};
use rivet_util::random_source::XoroshiroRandomSource;
use serde_json::{Value, json};
use std::collections::HashMap;

fn definition(id: &str) -> StructureDefinition {
    StructureDefinition::BuriedTreasure {
        registry_id: id.to_string(),
        terrain_adjustment: if id.ends_with("adjusted_treasure") {
            TerrainAdjustment::BeardThin
        } else {
            TerrainAdjustment::None
        },
    }
}

fn block(name: &str) -> BlockState {
    BlockState::of(BlockId::from_name(name).expect("fixture block"))
}
fn position(value: &Value) -> BlockPos {
    let a: [i32; 3] = serde_json::from_value(value.clone()).unwrap();
    BlockPos::new(a[0], a[1], a[2])
}

struct Authority {
    terrain: String,
    states: HashMap<BlockPos, BlockState>,
    writes: Vec<[i32; 5]>,
    chests: Vec<(BlockPos, BlockState, String, i64)>,
    lifecycle: Vec<&'static str>,
    fault: Option<FailurePoint>,
    active: bool,
    staged_entities: usize,
    committed_entities: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailurePoint {
    BeginError,
    BeginPanic,
    ChestError,
    ChestPanic,
    FinishError,
    FinishPanic,
    FinishPartialError,
}
#[derive(Debug, PartialEq, Eq)]
struct OriginalPanic(FailurePoint);
impl Authority {
    fn new(terrain: &str) -> Self {
        Self {
            terrain: terrain.into(),
            states: HashMap::new(),
            writes: Vec::new(),
            chests: Vec::new(),
            lifecycle: Vec::new(),
            fault: None,
            active: false,
            staged_entities: 0,
            committed_entities: 0,
        }
    }
}
impl LevelHeightAccessor for Authority {
    fn get_min_y(&self) -> i32 {
        -64
    }
    fn get_height(&self) -> i32 {
        384
    }
}
impl WorldGenLevel for Authority {
    fn get_seed(&self) -> i64 {
        42
    }
    fn get_height_at(&mut self, _: Types, _: i32, _: i32) -> i32 {
        70
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.states.get(pos).copied().unwrap_or_else(|| {
            block(if self.terrain == "void" {
                "minecraft:air"
            } else if pos.get_y() < 63 {
                "minecraft:stone"
            } else if pos.get_y() == 63 {
                match self.terrain.as_str() {
                    "water" => "minecraft:water",
                    "lava" => "minecraft:lava",
                    _ => "minecraft:sand",
                }
            } else {
                "minecraft:air"
            })
        })
    }
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        self.states.insert(*pos, state);
        self.writes.push([
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
            i32::from(state.id().0),
            flags as i32,
        ]);
        true
    }
}
impl StructurePlacementAuthority for Authority {
    fn level(&mut self) -> &mut dyn WorldGenLevel {
        self
    }
    fn begin_structure(
        &mut self,
        _: &StructureDefinition,
        _: BoundingBox,
        _: ChunkPos,
    ) -> Result<(), StructureError> {
        self.lifecycle.push("begin");
        self.active = true;
        self.staged_entities = 2;
        if self.fault == Some(FailurePoint::BeginError) {
            return Err(StructureError::PlacementUnavailable(
                "original begin error".into(),
            ));
        }
        if self.fault == Some(FailurePoint::BeginPanic) {
            std::panic::panic_any(OriginalPanic(FailurePoint::BeginPanic));
        }
        Ok(())
    }
    fn finish_structure(&mut self) -> Result<(), StructureError> {
        self.lifecycle.push("finish");
        if self.fault == Some(FailurePoint::FinishPartialError) {
            self.staged_entities -= 1;
            self.committed_entities += 1;
            return Err(StructureError::PlacementUnavailable(
                "original partial finish error".into(),
            ));
        }
        if self.fault == Some(FailurePoint::FinishError) {
            return Err(StructureError::PlacementUnavailable(
                "original finish error".into(),
            ));
        }
        if self.fault == Some(FailurePoint::FinishPanic) {
            std::panic::panic_any(OriginalPanic(FailurePoint::FinishPanic));
        }
        self.committed_entities += self.staged_entities;
        self.staged_entities = 0;
        self.active = false;
        Ok(())
    }
    fn abort_structure(&mut self) {
        self.lifecycle.push("abort");
        self.staged_entities = 0;
        self.active = false;
    }
    fn place_loot_chest(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        loot: &str,
        seed: i64,
    ) -> Result<(), StructureError> {
        self.chests.push((pos, state, loot.into(), seed));
        self.set_block(&pos, state, 2);
        if self.fault == Some(FailurePoint::ChestError) {
            return Err(StructureError::PlacementUnavailable(
                "original chest error".into(),
            ));
        }
        if self.fault == Some(FailurePoint::ChestPanic) {
            std::panic::panic_any(OriginalPanic(FailurePoint::ChestPanic));
        }
        Ok(())
    }
}

#[test]
fn native_structure_core_matches_pinned_paper() {
    let mut count = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/structure-core.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        match row["kind"].as_str().unwrap() {
            "piece" => {
                let piece = StructurePiece::load(
                    &parse_compound_fully(row["input"].as_str().unwrap()).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    piece.create_tag(),
                    parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
                );
                assert_eq!(piece.data().bounding_box.center(), position(&row["center"]));
                assert_eq!(piece.data().world_pos(2, -3, 4), position(&row["world"]));
            }
            "start" => {
                let first = StructurePiece::BuriedTreasure(super::super::StructurePieceData {
                    bounding_box: BoundingBox::new([-5, 60, -3, 5, 65, 8]),
                    gen_depth: -7,
                    orientation: None,
                });
                let pieces = PiecesContainer::new(vec![
                    first,
                    StructurePiece::buried_treasure(BlockPos::new(19, 90, -20)),
                ]);
                assert_eq!(
                    pieces.is_inside_piece(&BlockPos::new(5, 65, 8)),
                    row["inside"].as_bool().unwrap()
                );
                let mut start = StructureStart::new(
                    definition(row["id"].as_str().unwrap()),
                    ChunkPos::new(-3, 8),
                    row["references"].as_i64().unwrap() as i32,
                    pieces,
                );
                assert_eq!(
                    start.can_be_referenced(),
                    row["referencable"].as_bool().unwrap()
                );
                start.add_reference();
                assert_eq!(
                    i64::from(start.references()),
                    row["after_reference"].as_i64().unwrap()
                );
                assert_eq!(
                    json!(start.bounding_box().unwrap().to_array()),
                    row["bounds"]
                );
                let saved = start.create_tag(ChunkPos::new(7, -9));
                assert_eq!(
                    saved,
                    parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
                );
                let loaded = StructureStart::load(&saved, |id| Some(definition(id))).unwrap();
                assert_eq!(loaded.chunk_pos(), ChunkPos::new(7, -9));
                assert_eq!(loaded.create_tag(loaded.chunk_pos()), saved);
            }
            "invalid" => {
                let start = StructureStart::invalid();
                assert!(!start.is_valid());
                assert_eq!(start.bounding_box(), Err(StructureError::EmptyPieces));
                assert_eq!(
                    start.create_tag(ChunkPos::new(9, 9)),
                    parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
                );
            }
            "buried_clipped" => {
                let mut start = StructureStart::new(
                    definition("minecraft:buried_treasure"),
                    ChunkPos::ZERO,
                    0,
                    PiecesContainer::new(vec![StructurePiece::buried_treasure(BlockPos::new(
                        0, 90, 0,
                    ))]),
                );
                let mut authority = Authority::new(row["terrain"].as_str().unwrap());
                let mut random = XoroshiroRandomSource::new(42);
                assert_eq!(
                    json!(start.bounding_box().unwrap().to_array()),
                    row["cached_before"]
                );
                start.piece_container.pieces_mut()[0]
                    .post_process(
                        &mut authority,
                        &mut random,
                        BoundingBox::new([-16, -64, -16, 16, 62, 16]),
                    )
                    .unwrap();
                assert_eq!(json!(authority.writes), row["writes"]);
                assert!(authority.chests.is_empty());
                assert_eq!(random.next_long(), row["next_long"].as_i64().unwrap());
                assert_eq!(
                    start.pieces().pieces()[0].create_tag(),
                    parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
                );
                assert_eq!(
                    json!(start.bounding_box().unwrap().to_array()),
                    row["cached_after"],
                    "source retains cached bounds after mutable piece moves"
                );
                assert_eq!(
                    i64::from(reorient_chest(&authority, &BlockPos::new(0, 63, 0)).id().0),
                    row["chest"].as_i64().unwrap()
                );
            }
            kind => panic!("unknown real structure fixture {kind}"),
        }
        count += 1;
    }
    assert_eq!(count, 41);
}

#[test]
fn unsupported_children_refuse_atomically_and_empty_never_becomes_valid() {
    let mut tag = StructurePiece::buried_treasure(BlockPos::new(0, 90, 0)).create_tag();
    tag.put_string("id", "minecraft:jigsaw");
    let children = rivet_nbt::list_tag::ListTag::with_list(vec![
        Tag::Compound(StructurePiece::buried_treasure(BlockPos::ZERO).create_tag()),
        Tag::Compound(tag),
    ]);
    assert!(matches!(
        PiecesContainer::load(&children),
        Err(StructureError::UnsupportedPiece(_))
    ));
    let empty = StructureStart::new(
        definition("minecraft:buried_treasure"),
        ChunkPos::ZERO,
        0,
        PiecesContainer::default(),
    );
    assert!(!empty.is_valid());
    assert_eq!(
        empty.create_tag(ChunkPos::ZERO).get_string_or("id", ""),
        "INVALID"
    );
}

#[test]
fn start_dispatches_intersecting_piece_and_real_loot_rng_through_authority() {
    let mut start = StructureStart::new(
        definition("minecraft:buried_treasure"),
        ChunkPos::ZERO,
        0,
        StructureDefinition::buried_treasure_pieces(ChunkPos::ZERO),
    );
    let mut authority = Authority::new("sand");
    let mut random = XoroshiroRandomSource::new(42);
    let mut expected = XoroshiroRandomSource::new(42);
    start
        .place_in_chunk(
            &mut authority,
            &mut random,
            BoundingBox::new([0, -64, 0, 15, 319, 15]),
            ChunkPos::ZERO,
        )
        .unwrap();
    assert_eq!(authority.lifecycle, ["begin", "finish"]);
    assert!(!authority.active);
    assert_eq!(authority.staged_entities, 0);
    assert_eq!(authority.committed_entities, 2);
    assert_eq!(authority.chests.len(), 1);
    assert_eq!(authority.chests[0].0, BlockPos::new(9, 63, 9));
    assert_eq!(authority.chests[0].2, "minecraft:chests/buried_treasure");
    assert_eq!(authority.chests[0].3, expected.next_long());
    assert_eq!(random.next_long(), expected.next_long());
    // This adapter is an explicit native storage authority, not evidence for
    // Bukkit event handling. No default production adapter is installed.
    let mut outside = Authority::new("sand");
    start
        .place_in_chunk(
            &mut outside,
            &mut random,
            BoundingBox::new([32, -64, 32, 47, 319, 47]),
            ChunkPos::new(2, 2),
        )
        .unwrap();
    assert!(outside.lifecycle.is_empty() && outside.writes.is_empty());
}

#[test]
fn failed_structure_scope_aborts_without_committing_or_rolling_back_effects() {
    for fault in [
        FailurePoint::BeginError,
        FailurePoint::BeginPanic,
        FailurePoint::ChestError,
        FailurePoint::ChestPanic,
        FailurePoint::FinishError,
        FailurePoint::FinishPanic,
        FailurePoint::FinishPartialError,
    ] {
        let mut start = StructureStart::new(
            definition("minecraft:buried_treasure"),
            ChunkPos::ZERO,
            0,
            PiecesContainer::new(vec![StructurePiece::buried_treasure(BlockPos::new(
                8, 90, 8,
            ))]),
        );
        let mut authority = Authority::new("sand");
        authority.fault = Some(fault);
        let mut random = XoroshiroRandomSource::new(42);
        let mut expected = XoroshiroRandomSource::new(42);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            start.place_in_chunk(
                &mut authority,
                &mut random,
                BoundingBox::new([0, -64, 0, 15, 319, 15]),
                ChunkPos::ZERO,
            )
        }));
        match fault {
            FailurePoint::BeginPanic | FailurePoint::ChestPanic | FailurePoint::FinishPanic => {
                let payload = result.expect_err("the original panic must resume after cleanup");
                assert_eq!(
                    payload.downcast_ref::<OriginalPanic>(),
                    Some(&OriginalPanic(fault))
                );
            }
            _ => {
                let message = match fault {
                    FailurePoint::BeginError => "original begin error",
                    FailurePoint::ChestError => "original chest error",
                    FailurePoint::FinishError => "original finish error",
                    FailurePoint::FinishPartialError => "original partial finish error",
                    _ => unreachable!(),
                };
                assert_eq!(
                    result.expect("ordinary errors must not become panics"),
                    Err(StructureError::PlacementUnavailable(message.into())),
                );
            }
        }
        assert!(!authority.active, "{fault:?}");
        assert_eq!(authority.staged_entities, 0, "{fault:?}");
        assert_eq!(
            authority.committed_entities,
            usize::from(fault == FailurePoint::FinishPartialError),
            "abort retains entities already committed by a partial finish, but commits no more",
        );
        let failed_begin = matches!(fault, FailurePoint::BeginError | FailurePoint::BeginPanic);
        let failed_finish = matches!(
            fault,
            FailurePoint::FinishError
                | FailurePoint::FinishPanic
                | FailurePoint::FinishPartialError
        );
        assert_eq!(
            authority.lifecycle,
            if failed_finish {
                vec!["begin", "finish", "abort"]
            } else {
                vec!["begin", "abort"]
            },
            "{fault:?}",
        );
        assert_eq!(
            authority.chests.len(),
            usize::from(!failed_begin),
            "{fault:?}"
        );
        if failed_begin {
            assert!(authority.writes.is_empty());
        } else {
            assert!(!authority.writes.is_empty());
            assert_eq!(
                authority.get_block_state(&BlockPos::new(8, 63, 8)).block(),
                BlockId::from_name("minecraft:chest").unwrap()
            );
            assert_eq!(authority.chests[0].3, expected.next_long());
        }
        assert_eq!(
            start.pieces().pieces()[0].data().bounding_box.min_y,
            if failed_begin { 90 } else { 63 },
            "piece mutations are not rolled back on failure",
        );
        assert_eq!(random.next_long(), expected.next_long(), "{fault:?}");
    }
}
