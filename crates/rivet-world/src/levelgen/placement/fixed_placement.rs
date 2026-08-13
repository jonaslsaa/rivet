//! Port of `net.minecraft.world.level.levelgen.placement.FixedPlacement`
//! (class, 26.2).
//!
//! Java: a modifier holding a fixed `List<BlockPos>` whose `getPositions`
//! emits the positions lying in the origin's chunk column (`SectionPos.
//! blockToSectionCoord` match on X and Z). Its `CODEC` is the `"positions"`
//! field (`BlockPos.CODEC.listOf()`) mapped onto the private constructor, and
//! its `type()` is `PlacementModifierType.FIXED_PLACEMENT`.

use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypes;
use crate::levelgen::placement::{PlacementContext, PlacementModifier};
use rivet_registry::core::BlockPos;
use rivet_registry::core::SectionPos;
use rivet_registry::core::block_pos_codec;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.FixedPlacement`.
#[derive(Debug, Clone, PartialEq)]
pub struct FixedPlacement {
    /// `this.positions` — the fixed positions, in `List.of(...)` order.
    positions: Vec<BlockPos>,
}

impl FixedPlacement {
    /// `of(BlockPos...)` — the varargs factory (`List.of(pos)`).
    pub fn of(pos: &[BlockPos]) -> Self {
        FixedPlacement {
            positions: pos.to_vec(),
        }
    }
}

impl PlacementModifier for FixedPlacement {
    fn get_positions<'a, R: RandomSource>(
        &'a self,
        _context: &PlacementContext,
        _random: &mut R,
        origin: &BlockPos,
    ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
        let chunk_x = SectionPos::block_to_section_coord(origin.get_x());
        let chunk_z = SectionPos::block_to_section_coord(origin.get_z());
        let has_positions = self
            .positions
            .iter()
            .any(|p| Self::is_same_chunk(chunk_x, chunk_z, p));
        if !has_positions {
            return Box::new(std::iter::empty());
        }
        Box::new(
            self.positions
                .iter()
                // `move`: capture the Copy `chunk_x`/`chunk_z` by value so the
                // returned `+ 'a` iterator (tied to `&'a self`) does not borrow
                // locals that die at the end of this function.
                .filter(move |p| Self::is_same_chunk(chunk_x, chunk_z, p))
                .copied(),
        )
    }

    fn type_id(
        &self,
    ) -> crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId {
        // `PlacementModifierType.FIXED_PLACEMENT` is insertion index 14 in
        // `PlacementModifierType.java`'s registration order.
        PlacementModifierTypes::FIXED_PLACEMENT
    }
}

impl FixedPlacement {
    /// `isSameChunk(int chunkX, int chunkZ, BlockPos)`.
    fn is_same_chunk(chunk_x: i32, chunk_z: i32, position: &BlockPos) -> bool {
        chunk_x == SectionPos::block_to_section_coord(position.get_x())
            && chunk_z == SectionPos::block_to_section_coord(position.get_z())
    }
}

/// `FixedPlacement.CODEC` — `BlockPos.CODEC.listOf().fieldOf("positions")`, as
/// the ops-generic `fixed_placement_map_codec::<Ops>()` factory.
pub fn fixed_placement_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<FixedPlacement, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|c: &FixedPlacement| c.positions.clone()),
                "positions".to_string(),
                codec::list::<BlockPos, Ops>(block_pos_codec::<Ops>()),
            ))
            .apply(
                instance,
                Arc::new(|positions: Vec<BlockPos>| FixedPlacement { positions }),
            )
    })
}

/// `FixedPlacement.CODEC` as a `Codec` (`MapCodec.codec()`), the shape the
/// `#181` generated dispatch's registration table consumes.
#[allow(dead_code)]
pub fn fixed_placement_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<FixedPlacement, Ops>> {
    map_codec::codec_of(fixed_placement_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    /// A minimal `WorldGenLevel` double over the overworld window.
    struct TestLevel(SimpleLevelHeightAccessor);

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.0.get_height()
        }

        fn get_min_y(&self) -> i32 {
            self.0.get_min_y()
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    struct NoopGenerator;

    impl crate::chunk::ChunkGenerator for NoopGenerator {
        fn create_biomes(&self) {}
        fn apply_carvers(&self) {}
        fn build_surface(&self) {}
        fn spawn_original_mobs(&self) {}
        fn fill_from_noise(&self) {}
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    fn fixed_positions(modifier: &FixedPlacement, origin: &BlockPos) -> Vec<BlockPos> {
        let mut level = TestLevel(create(-64, 384));
        let generator = NoopGenerator;
        let context = PlacementContext::new(&mut level, &generator, None);
        let mut random = LegacyRandomSource::new(0);
        modifier
            .get_positions(&context, &mut random, origin)
            .collect()
    }

    #[test]
    fn emits_positions_in_the_origin_chunk_column() {
        // Origin at (0, 0, 0) is chunk (0, 0); positions with x,z in [0,16)
        // are in that chunk. (16, 0, 0) is chunk (1, 0) and is excluded.
        let positions = [
            BlockPos::new(0, 70, 0),
            BlockPos::new(15, -10, 15),
            BlockPos::new(16, 70, 0),
        ];
        let modifier = FixedPlacement::of(&positions);
        let result = fixed_positions(&modifier, &BlockPos::new(0, 0, 0));
        assert_eq!(
            result,
            vec![BlockPos::new(0, 70, 0), BlockPos::new(15, -10, 15)]
        );
    }

    #[test]
    fn empty_when_no_position_is_in_the_origin_chunk() {
        // Origin chunk (3, 3); all positions elsewhere.
        let positions = [BlockPos::new(0, 70, 0), BlockPos::new(16, 70, 16)];
        let modifier = FixedPlacement::of(&positions);
        let result = fixed_positions(&modifier, &BlockPos::new(48, 0, 48));
        assert!(result.is_empty());
    }

    #[test]
    fn negative_coordinates_floor_into_the_chunk() {
        // `blockToSectionCoord` is `>> 4` (floor): (-1, 0) is chunk (-1, 0).
        let positions = [BlockPos::new(-1, 5, 0)];
        let modifier = FixedPlacement::of(&positions);
        // Origin (-1, 0, 0) is chunk (-1, 0) — the position matches.
        let result = fixed_positions(&modifier, &BlockPos::new(-1, 0, 0));
        assert_eq!(result, vec![BlockPos::new(-1, 5, 0)]);
        // Origin (0, 0, 0) is chunk (0, 0) — the position does not.
        let result = fixed_positions(&modifier, &BlockPos::new(0, 0, 0));
        assert!(result.is_empty());
    }

    #[test]
    fn fixed_placement_type_identity_is_reported() {
        // `PlacementModifierType.FIXED_PLACEMENT` is insertion index 14.
        let modifier = FixedPlacement::of(&[]);
        assert_eq!(modifier.type_id(), PlacementModifierTypes::FIXED_PLACEMENT);
    }

    #[test]
    fn codec_round_trips_the_positions() {
        let ops = JsonOps::INSTANCE;
        let codec = fixed_placement_codec::<JsonOps>();
        let modifier = FixedPlacement::of(&[BlockPos::new(1, 2, 3), BlockPos::new(4, 5, 6)]);
        let encoded = codec
            .encode_start(&ops, &modifier)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"positions": [[1, 2, 3], [4, 5, 6]]}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, modifier);
    }

    #[test]
    fn codec_rejects_a_non_three_int_list() {
        // `BlockPos.CODEC` is the fixed-size-3 int stream.
        let ops = JsonOps::INSTANCE;
        let codec = fixed_placement_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({"positions": [[1, 2]]}));
        assert!(result.is_error());
    }
}
