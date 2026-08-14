//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! StraightTrunkPlacer` (class, 26.2).
//!
//! `CODEC` is the shared `trunkPlacerParts(i).apply(i, StraightTrunkPlacer::new)`
//! three-field record. `placeTrunk` places the below-trunk block and one log
//! per height step, returning the single top `FoliageAttachment`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer::FoliageAttachment;
use crate::levelgen::feature::trunkplacers::trunk_placer::{
    TrunkPlacer, place_below_trunk_block, trunk_placer_parts,
};
use crate::levelgen::feature::trunkplacers::trunk_placer_type::{
    TrunkPlacerTypeId, TrunkPlacerTypes,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.trunkplacers.StraightTrunkPlacer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StraightTrunkPlacer {
    /// `this.baseHeight`.
    base_height: i32,
    /// `this.heightRandA`.
    height_rand_a: i32,
    /// `this.heightRandB`.
    height_rand_b: i32,
}

impl StraightTrunkPlacer {
    /// `new StraightTrunkPlacer(int, int, int)`.
    pub fn new(base_height: i32, height_rand_a: i32, height_rand_b: i32) -> StraightTrunkPlacer {
        StraightTrunkPlacer {
            base_height,
            height_rand_a,
            height_rand_b,
        }
    }

    /// `this.baseHeight`.
    pub fn base_height(&self) -> i32 {
        self.base_height
    }

    /// `this.heightRandA`.
    pub fn height_rand_a(&self) -> i32 {
        self.height_rand_a
    }

    /// `this.heightRandB`.
    pub fn height_rand_b(&self) -> i32 {
        self.height_rand_b
    }
}

impl TrunkPlacer for StraightTrunkPlacer {
    fn type_id(&self) -> TrunkPlacerTypeId {
        TrunkPlacerTypes::STRAIGHT_TRUNK_PLACER
    }

    fn place_trunk<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        tree_height: i32,
        origin: &BlockPos,
        config: &TreeConfiguration,
    ) -> Vec<FoliageAttachment> {
        place_below_trunk_block(level, trunk_setter, random, &origin.below(), config);

        for y in 0..tree_height {
            self.place_log(level, trunk_setter, random, &origin.above_steps(y), config);
        }

        vec![FoliageAttachment::new(
            origin.above_steps(tree_height),
            0,
            false,
        )]
    }

    fn get_base_height(&self) -> i32 {
        self.base_height
    }

    fn base_height(&self) -> i32 {
        self.base_height
    }

    fn height_rand_a(&self) -> i32 {
        self.height_rand_a
    }

    fn height_rand_b(&self) -> i32 {
        self.height_rand_b
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `StraightTrunkPlacer.CODEC` — the shared three-field trunk-placer record, as
/// the ops-generic `straight_trunk_placer_map_codec::<Ops>()` factory.
#[allow(clippy::type_complexity)]
pub fn straight_trunk_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<StraightTrunkPlacer, Ops>> {
    record_builder::map_codec::<StraightTrunkPlacer, Ops>(move |instance| {
        let (base, height_rand_a, height_rand_b) = trunk_placer_parts::<StraightTrunkPlacer, Ops>(
            Arc::new(|p: &StraightTrunkPlacer| p.base_height),
            Arc::new(|p: &StraightTrunkPlacer| p.height_rand_a),
            Arc::new(|p: &StraightTrunkPlacer| p.height_rand_b),
        );
        instance
            .group(base)
            .and(height_rand_a)
            .and(height_rand_b)
            .apply(instance, Arc::new(StraightTrunkPlacer::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::feature::configurations::TreeConfiguration;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::map_codec;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn codec_round_trips_the_three_field_record() {
        let codec = map_codec::codec_of(straight_trunk_placer_map_codec::<JsonOps>());
        let input = json!({
            "base_height": 7,
            "height_rand_a": 2,
            "height_rand_b": 3,
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            TrunkPlacer::type_id(decoded),
            TrunkPlacerTypes::STRAIGHT_TRUNK_PLACER
        );
        assert_eq!(decoded.get_base_height(), 7);
        assert_eq!(decoded.height_rand_a(), 2);
        assert_eq!(decoded.height_rand_b(), 3);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_out_of_range_base_height() {
        // `Codec.intRange(0, 32)` — a negative base height is a decode error.
        let codec = map_codec::codec_of(straight_trunk_placer_map_codec::<JsonOps>());
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({"base_height": -1, "height_rand_a": 2, "height_rand_b": 3}),
        );
        assert!(result.is_error(), "got: {:?}", result);
    }

    #[test]
    fn place_trunk_places_a_vertical_column_and_top_attachment() {
        let placer = StraightTrunkPlacer::new(1, 0, 0);
        let config = TreeConfiguration::stub();
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let origin = BlockPos::new(3, 64, 5);
        let mut placed = BTreeMap::new();
        let mut setter = |pos: &BlockPos, state: BlockState| {
            placed.insert(*pos, state);
        };
        let attachments = placer.place_trunk(
            &TestLevel::air(),
            &mut setter,
            &mut random,
            4,
            &origin,
            &config,
        );
        // Below-trunk block at (3, 63, 5), logs at y 64..=67, top attachment
        // at (3, 68, 5).
        assert_eq!(placed.len(), 5);
        assert!(placed.contains_key(&BlockPos::new(3, 63, 5)));
        for y in 0..4 {
            assert!(placed.contains_key(&BlockPos::new(3, 64 + y, 5)));
        }
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].pos, BlockPos::new(3, 68, 5));
        assert_eq!(attachments[0].radius_offset, 0);
        assert!(!attachments[0].double_trunk);
    }

    /// A world double with a real per-position block map: air by default, with
    /// positions explicitly seeded to another state (a log, stone, leaves, …).
    /// `is_state_at_position` answers from the queried position, so predicates
    /// evaluate the real column instead of a fabricated AIR everywhere; tests
    /// that seed occupied positions exercise `place_log_if_free`'s skip path
    /// and `is_free`'s `LOGS` branch.
    struct TestLevel {
        blocks: std::collections::BTreeMap<BlockPos, BlockState>,
    }

    impl TestLevel {
        fn air() -> TestLevel {
            TestLevel {
                blocks: std::collections::BTreeMap::new(),
            }
        }
    }

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, pos: &BlockPos) -> BlockState {
            self.blocks
                .get(pos)
                .copied()
                .unwrap_or_else(|| crate::block::blocks::Blocks::AIR.default_block_state())
        }

        fn is_state_at_position(&self, pos: &BlockPos, test: &dyn Fn(&BlockState) -> bool) -> bool {
            test(&self.get_block_state(pos))
        }
    }
}
