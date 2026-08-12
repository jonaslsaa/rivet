//! Port of `net.minecraft.world.level.levelgen.placement.InSquarePlacement`
//! (class, 26.2).
//!
//! Java: a stateless singleton modifier whose `getPositions` scatters the
//! origin within the 16x16 chunk column — `random.nextInt(16)` added to the
//! origin's X/Z, Y unchanged. Its `CODEC` is `MapCodec.unit(() -> INSTANCE)`,
//! which encodes the empty map and decodes to the singleton regardless of
//! input, and its `type()` is `PlacementModifierType.IN_SQUARE`.

use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypes;
use crate::levelgen::placement::{PlacementContext, PlacementModifier};
use rivet_registry::core::BlockPos;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.InSquarePlacement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InSquarePlacement;

impl InSquarePlacement {
    /// The private `INSTANCE` — the only value Java ever constructs.
    pub const INSTANCE: InSquarePlacement = InSquarePlacement;

    /// `spread()` — the singleton factory.
    pub fn spread() -> Self {
        InSquarePlacement::INSTANCE
    }
}

impl PlacementModifier for InSquarePlacement {
    fn get_positions<R: RandomSource>(
        &self,
        _context: &PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> Vec<BlockPos> {
        // `random.nextInt(16) + origin.getX()` — Java int addition wraps.
        let x = random.next_int_bound(16).wrapping_add(origin.get_x());
        let z = random.next_int_bound(16).wrapping_add(origin.get_z());
        vec![BlockPos::new(x, origin.get_y(), z)]
    }

    fn type_id(
        &self,
    ) -> crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId {
        // `PlacementModifierType.IN_SQUARE` is insertion index 12 in
        // `PlacementModifierType.java`'s registration order.
        PlacementModifierTypes::IN_SQUARE
    }
}

/// `InSquarePlacement.CODEC` — `MapCodec.unit(() -> INSTANCE)`, as the
/// ops-generic `in_square_placement_map_codec::<Ops>()` factory.
pub fn in_square_placement_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<InSquarePlacement, Ops>> {
    map_codec::unit_with(Arc::new(|| InSquarePlacement::INSTANCE))
}

/// `InSquarePlacement.CODEC` as a `Codec` (`MapCodec.codec()`), the shape the
/// `#181` generated dispatch's registration table consumes.
#[allow(dead_code)]
pub fn in_square_placement_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<InSquarePlacement, Ops>> {
    map_codec::codec_of(in_square_placement_map_codec::<Ops>())
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
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    fn spread_positions(random: &mut LegacyRandomSource, origin: &BlockPos) -> Vec<BlockPos> {
        let mut level = TestLevel(create(-64, 384));
        let generator = NoopGenerator;
        let context = PlacementContext::new(&mut level, &generator, None);
        InSquarePlacement::spread().get_positions(&context, random, origin)
    }

    #[test]
    fn scatters_within_the_16x16_column() {
        // `x = nextInt(16) + origin.getX()`, `z = nextInt(16) + origin.getZ()`,
        // y unchanged — the result is inside the origin's chunk column.
        let origin = BlockPos::new(0, 70, 0);
        let mut random = LegacyRandomSource::new(1);
        for _ in 0..8 {
            let positions = spread_positions(&mut random, &origin);
            assert_eq!(positions.len(), 1);
            let pos = positions[0];
            assert_eq!(pos.get_y(), 70);
            assert!(pos.get_x() >= 0 && pos.get_x() < 16);
            assert!(pos.get_z() >= 0 && pos.get_z() < 16);
        }
    }

    #[test]
    fn spread_offsets_from_the_origin() {
        // An origin at (100, 0, 100): the first two draws of LegacyRandomSource
        // seed 0 are pinned golden values.
        let origin = BlockPos::new(100, 0, 100);
        let mut random = LegacyRandomSource::new(0);
        let positions = spread_positions(&mut random, &origin);
        assert_eq!(positions.len(), 1);
        // Paper `nextInt(16)` after seed 0: first 11, second 13 (verified
        // against `java.util.Random(0L)`).
        assert_eq!(positions[0].get_x(), 111);
        assert_eq!(positions[0].get_z(), 113);
    }

    #[test]
    fn singleton_factory_returns_the_same_value() {
        assert_eq!(InSquarePlacement::spread(), InSquarePlacement::INSTANCE);
    }

    #[test]
    fn in_square_type_identity_is_reported() {
        // `PlacementModifierType.IN_SQUARE` is insertion index 12 in
        // `PlacementModifierType.java`'s registration order.
        let modifier = InSquarePlacement::spread();
        assert_eq!(modifier.type_id(), PlacementModifierTypes::IN_SQUARE);
        assert_eq!(modifier.type_id().location, "minecraft:in_square");
    }

    #[test]
    fn codec_round_trips_the_singleton() {
        // `MapCodec.unit(() -> INSTANCE)`: encode emits the empty map, decode
        // yields the singleton regardless of input.
        let ops = JsonOps::INSTANCE;
        let codec = in_square_placement_codec::<JsonOps>();
        let modifier = InSquarePlacement::spread();
        let encoded = codec
            .encode_start(&ops, &modifier)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .copied()
            .expect("decode should succeed");
        assert_eq!(decoded, modifier);
    }
}
