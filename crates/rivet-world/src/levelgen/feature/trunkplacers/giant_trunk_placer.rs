//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! GiantTrunkPlacer` (class, 26.2).
//!
//! `CODEC` is the shared `trunkPlacerParts(i).apply(i, GiantTrunkPlacer::new)`
//! three-field record. `placeTrunk` places the below-trunk block at the four
//! 2x2 corners, then the 2x2 trunk (`placeLogIfFreeWithOffset`), tapering to
//! the single origin column on the top layer. Returns the single
//! `doubleTrunk` top `FoliageAttachment`.

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
use rivet_registry::core::MutableBlockPos;
use rivet_registry::core::Vec3i;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.trunkplacers.GiantTrunkPlacer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GiantTrunkPlacer {
    /// `this.baseHeight`.
    base_height: i32,
    /// `this.heightRandA`.
    height_rand_a: i32,
    /// `this.heightRandB`.
    height_rand_b: i32,
}

impl GiantTrunkPlacer {
    /// `new GiantTrunkPlacer(int, int, int)`.
    pub fn new(base_height: i32, height_rand_a: i32, height_rand_b: i32) -> GiantTrunkPlacer {
        GiantTrunkPlacer {
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

impl TrunkPlacer for GiantTrunkPlacer {
    fn type_id(&self) -> TrunkPlacerTypeId {
        TrunkPlacerTypes::GIANT_TRUNK_PLACER
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
        let below = origin.below();
        place_below_trunk_block(level, trunk_setter, random, &below, config);
        place_below_trunk_block(level, trunk_setter, random, &below.east(), config);
        place_below_trunk_block(level, trunk_setter, random, &below.south(), config);
        place_below_trunk_block(level, trunk_setter, random, &below.south().east(), config);
        let mut trunk_pos = MutableBlockPos::new(0, 0, 0);

        for hh in 0..tree_height {
            self.place_log_if_free_with_offset(
                level,
                trunk_setter,
                random,
                &mut trunk_pos,
                config,
                origin,
                0,
                hh,
                0,
            );
            if hh < tree_height.wrapping_sub(1) {
                self.place_log_if_free_with_offset(
                    level,
                    trunk_setter,
                    random,
                    &mut trunk_pos,
                    config,
                    origin,
                    1,
                    hh,
                    0,
                );
                self.place_log_if_free_with_offset(
                    level,
                    trunk_setter,
                    random,
                    &mut trunk_pos,
                    config,
                    origin,
                    1,
                    hh,
                    1,
                );
                self.place_log_if_free_with_offset(
                    level,
                    trunk_setter,
                    random,
                    &mut trunk_pos,
                    config,
                    origin,
                    0,
                    hh,
                    1,
                );
            }
        }

        vec![FoliageAttachment::new(
            origin.above_steps(tree_height),
            0,
            true,
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

impl GiantTrunkPlacer {
    /// `GiantTrunkPlacer.placeLogIfFreeWithOffset(...)` (private instance) —
    /// `trunkPos.setWithOffset(treePos, x, y, z)` then `placeLogIfFree`.
    #[allow(clippy::too_many_arguments)] // mirrors Java `placeLogIfFreeWithOffset(WorldGenLevel, Consumer, Random, MutableBlockPos, TreeConfiguration, BlockPos, int, int, int)`.
    fn place_log_if_free_with_offset<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        trunk_pos: &mut MutableBlockPos,
        config: &TreeConfiguration,
        tree_pos: &BlockPos,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let tree_pos_vec = Vec3i::new(tree_pos.get_x(), tree_pos.get_y(), tree_pos.get_z());
        trunk_pos.set_with_offset_xyz(&tree_pos_vec, x, y, z);
        self.place_log_if_free(level, trunk_setter, random, trunk_pos, config);
    }
}

/// `GiantTrunkPlacer.CODEC` — the shared three-field trunk-placer record, as
/// the ops-generic `giant_trunk_placer_map_codec::<Ops>()` factory.
#[allow(clippy::type_complexity)]
pub fn giant_trunk_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<GiantTrunkPlacer, Ops>> {
    record_builder::map_codec::<GiantTrunkPlacer, Ops>(move |instance| {
        let (base, height_rand_a, height_rand_b) = trunk_placer_parts::<GiantTrunkPlacer, Ops>(
            Arc::new(|p: &GiantTrunkPlacer| p.base_height),
            Arc::new(|p: &GiantTrunkPlacer| p.height_rand_a),
            Arc::new(|p: &GiantTrunkPlacer| p.height_rand_b),
        );
        instance
            .group(base)
            .and(height_rand_a)
            .and(height_rand_b)
            .apply(instance, Arc::new(GiantTrunkPlacer::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::map_codec;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn codec_round_trips_the_three_field_record() {
        let codec = map_codec::codec_of(giant_trunk_placer_map_codec::<JsonOps>());
        let input = json!({
            "base_height": 11,
            "height_rand_a": 3,
            "height_rand_b": 0,
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            TrunkPlacer::type_id(decoded),
            TrunkPlacerTypes::GIANT_TRUNK_PLACER
        );
        assert_eq!(decoded.get_base_height(), 11);
        assert_eq!(decoded.height_rand_a(), 3);
        assert_eq!(decoded.height_rand_b(), 0);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn place_trunk_builds_a_2x2_trunk_tapering_to_one_on_top() {
        let placer = GiantTrunkPlacer::new(1, 0, 0);
        let config = TreeConfiguration::stub();
        let mut random = rivet_util::random::LegacyRandomSource::new(3);
        let origin = BlockPos::new(0, 0, 0);
        let mut placed = BTreeSet::new();
        let mut setter = |pos: &BlockPos, _state: BlockState| {
            placed.insert(*pos);
        };
        let attachments = placer.place_trunk(
            &TestLevel::air(),
            &mut setter,
            &mut random,
            4,
            &origin,
            &config,
        );

        // The four below-trunk corners.
        for corner in [
            BlockPos::new(0, -1, 0),
            BlockPos::new(1, -1, 0),
            BlockPos::new(0, -1, 1),
            BlockPos::new(1, -1, 1),
        ] {
            assert!(placed.contains(&corner), "missing {corner:?}");
        }
        // Layers 0..=2 are the full 2x2; layer 3 is only the origin column.
        for hh in 0..3 {
            for (x, z) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                assert!(
                    placed.contains(&BlockPos::new(x, hh, z)),
                    "missing layer {hh} corner ({x},{z})"
                );
            }
        }
        assert!(placed.contains(&BlockPos::new(0, 3, 0)));
        assert!(!placed.contains(&BlockPos::new(1, 3, 0)));
        assert!(!placed.contains(&BlockPos::new(1, 3, 1)));
        assert!(!placed.contains(&BlockPos::new(0, 3, 1)));

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].pos, BlockPos::new(0, 4, 0));
        assert_eq!(attachments[0].radius_offset, 0);
        assert!(attachments[0].double_trunk);
    }

    #[test]
    fn place_trunk_skips_occupied_trunk_positions() {
        // A stone block at the origin trunk column (0,0,0): `placeLogIfFree`
        // sees a non-air, non-LOGS state and must not overwrite it, so the
        // origin column is left out of `placed` while the other corners of the
        // 2x2 trunk are still placed.
        let placer = GiantTrunkPlacer::new(1, 0, 0);
        let config = TreeConfiguration::stub();
        let mut random = rivet_util::random::LegacyRandomSource::new(3);
        let origin = BlockPos::new(0, 0, 0);
        let level = TestLevel::air().with_block(
            BlockPos::new(0, 0, 0),
            crate::block::blocks::Blocks::STONE.default_block_state(),
        );
        let mut placed = BTreeSet::new();
        let mut setter = |pos: &BlockPos, _state: BlockState| {
            placed.insert(*pos);
        };
        let attachments = placer.place_trunk(&level, &mut setter, &mut random, 4, &origin, &config);

        // The below-trunk corners are still placed unconditionally.
        for corner in [
            BlockPos::new(0, -1, 0),
            BlockPos::new(1, -1, 0),
            BlockPos::new(0, -1, 1),
            BlockPos::new(1, -1, 1),
        ] {
            assert!(placed.contains(&corner), "missing {corner:?}");
        }
        // The occupied origin column is skipped; the other corners are not.
        assert!(
            !placed.contains(&BlockPos::new(0, 0, 0)),
            "occupied position must not be overwritten"
        );
        assert!(placed.contains(&BlockPos::new(1, 0, 0)));
        assert!(placed.contains(&BlockPos::new(0, 0, 1)));
        assert_eq!(attachments.len(), 1);
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

        fn with_block(mut self, pos: BlockPos, state: BlockState) -> TestLevel {
            self.blocks.insert(pos, state);
            self
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
