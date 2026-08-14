//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! ForkingTrunkPlacer` (class, 26.2).
//!
//! `CODEC` is the shared `trunkPlacerParts(i).apply(i, ForkingTrunkPlacer::new)`
//! three-field record. `placeTrunk` places the below-trunk block, then the
//! leaning column (steered by `Direction.Plane.HORIZONTAL.getRandomDirection`),
//! and — when the second random horizontal direction differs — the shorter
//! second branch. Each placed column contributes its top
//! `FoliageAttachment`.

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
use rivet_registry::core::Plane;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.trunkplacers.ForkingTrunkPlacer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkingTrunkPlacer {
    /// `this.baseHeight`.
    base_height: i32,
    /// `this.heightRandA`.
    height_rand_a: i32,
    /// `this.heightRandB`.
    height_rand_b: i32,
}

impl ForkingTrunkPlacer {
    /// `new ForkingTrunkPlacer(int, int, int)`.
    pub fn new(base_height: i32, height_rand_a: i32, height_rand_b: i32) -> ForkingTrunkPlacer {
        ForkingTrunkPlacer {
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

impl TrunkPlacer for ForkingTrunkPlacer {
    fn type_id(&self) -> TrunkPlacerTypeId {
        TrunkPlacerTypes::FORKING_TRUNK_PLACER
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
        let mut attachments = Vec::new();
        let lean_direction = Plane::Horizontal.get_random_direction(random);
        let lean_height = tree_height
            .wrapping_sub(random.next_int_bound(4))
            .wrapping_sub(1);
        let mut lean_steps = 3i32.wrapping_sub(random.next_int_bound(3));
        let mut log_pos = MutableBlockPos::new(0, 0, 0);
        let mut tx = origin.get_x();
        let mut tz = origin.get_z();
        let mut ey: Option<i32> = None;

        for yo in 0..tree_height {
            let yy = origin.get_y().wrapping_add(yo);
            if yo >= lean_height && lean_steps > 0 {
                tx = tx.wrapping_add(lean_direction.step_x());
                tz = tz.wrapping_add(lean_direction.step_z());
                lean_steps = lean_steps.wrapping_sub(1);
            }

            log_pos.set(tx, yy, tz);
            if self.place_log(level, trunk_setter, random, &log_pos.immutable(), config) {
                ey = Some(yy.wrapping_add(1));
            }
        }

        if let Some(ey) = ey {
            attachments.push(FoliageAttachment::new(BlockPos::new(tx, ey, tz), 1, false));
        }

        tx = origin.get_x();
        tz = origin.get_z();
        let branch_direction = Plane::Horizontal.get_random_direction(random);
        if branch_direction != lean_direction {
            let branch_pos = lean_height
                .wrapping_sub(random.next_int_bound(2))
                .wrapping_sub(1);
            let mut branch_steps = 1i32.wrapping_add(random.next_int_bound(3));
            ey = None;

            let mut yo = branch_pos;
            while yo < tree_height && branch_steps > 0 {
                if yo >= 1 {
                    let yy = origin.get_y().wrapping_add(yo);
                    tx = tx.wrapping_add(branch_direction.step_x());
                    tz = tz.wrapping_add(branch_direction.step_z());
                    log_pos.set(tx, yy, tz);
                    if self.place_log(level, trunk_setter, random, &log_pos.immutable(), config) {
                        ey = Some(yy.wrapping_add(1));
                    }
                }

                yo = yo.wrapping_add(1);
                branch_steps = branch_steps.wrapping_sub(1);
            }

            if let Some(ey) = ey {
                attachments.push(FoliageAttachment::new(BlockPos::new(tx, ey, tz), 0, false));
            }
        }

        attachments
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

/// `ForkingTrunkPlacer.CODEC` — the shared three-field trunk-placer record, as
/// the ops-generic `forking_trunk_placer_map_codec::<Ops>()` factory.
#[allow(clippy::type_complexity)]
pub fn forking_trunk_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<ForkingTrunkPlacer, Ops>> {
    record_builder::map_codec::<ForkingTrunkPlacer, Ops>(move |instance| {
        let (base, height_rand_a, height_rand_b) = trunk_placer_parts::<ForkingTrunkPlacer, Ops>(
            Arc::new(|p: &ForkingTrunkPlacer| p.base_height),
            Arc::new(|p: &ForkingTrunkPlacer| p.height_rand_a),
            Arc::new(|p: &ForkingTrunkPlacer| p.height_rand_b),
        );
        instance
            .group(base)
            .and(height_rand_a)
            .and(height_rand_b)
            .apply(instance, Arc::new(ForkingTrunkPlacer::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::map_codec;
    use serde_json::json;

    #[test]
    fn codec_round_trips_the_three_field_record() {
        let codec = map_codec::codec_of(forking_trunk_placer_map_codec::<JsonOps>());
        let input = json!({
            "base_height": 6,
            "height_rand_a": 4,
            "height_rand_b": 2,
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            TrunkPlacer::type_id(decoded),
            TrunkPlacerTypes::FORKING_TRUNK_PLACER
        );
        assert_eq!(decoded.get_base_height(), 6);
        assert_eq!(decoded.height_rand_a(), 4);
        assert_eq!(decoded.height_rand_b(), 2);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_out_of_range_height_rand() {
        // `Codec.intRange(0, 24)` — 25 is a decode error.
        let codec = map_codec::codec_of(forking_trunk_placer_map_codec::<JsonOps>());
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({"base_height": 6, "height_rand_a": 4, "height_rand_b": 25}),
        );
        assert!(result.is_error(), "got: {:?}", result);
    }

    #[test]
    fn place_trunk_places_below_block_and_returns_top_attachments() {
        let placer = ForkingTrunkPlacer::new(1, 0, 0);
        let config = TreeConfiguration::stub();
        let mut random = rivet_util::random::LegacyRandomSource::new(7);
        let origin = BlockPos::new(0, 0, 0);
        let mut placed = Vec::new();
        let mut setter = |pos: &BlockPos, _state: BlockState| {
            placed.push(*pos);
        };
        let attachments = placer.place_trunk(
            &TestLevel::air(),
            &mut setter,
            &mut random,
            5,
            &origin,
            &config,
        );
        // Below-trunk block is always the first write.
        assert_eq!(placed.first(), Some(&BlockPos::new(0, -1, 0)));
        // At least the primary leaning column is placed; the branch is present
        // when the two random directions differ (seed 7: they differ).
        assert!(placed.len() >= 5, "placed: {placed:?}");
        // A fixed seed produces a deterministic attachment set.
        let first = &attachments[0];
        assert_eq!(first.radius_offset, 1);
        // The top attachment sits one above the column's highest placed log.
        let top_y = placed.iter().map(|p| p.get_y()).max().unwrap();
        assert_eq!(first.pos.get_y(), top_y + 1);
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
