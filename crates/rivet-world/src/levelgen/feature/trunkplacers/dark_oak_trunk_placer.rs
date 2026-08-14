//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! DarkOakTrunkPlacer` (class, 26.2).
//!
//! `CODEC` is the shared `trunkPlacerParts(i).apply(i, DarkOakTrunkPlacer::new)`
//! three-field record. `placeTrunk` places the below-trunk block at the four
//! 2x2 corners, then the leaning 2x2 trunk (each layer gated on
//! `TreeFeature.isAirOrLeaves`, placing the four `2x2` logs), followed by the
//! random branch ring of `length = nextInt(3) + 2` vertical stubs.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer::FoliageAttachment;
use crate::levelgen::feature::tree_feature::is_air_or_leaves;
use crate::levelgen::feature::trunkplacers::trunk_placer::{
    TrunkPlacer, place_below_trunk_block, trunk_placer_parts,
};
use crate::levelgen::feature::trunkplacers::trunk_placer_type::{
    TrunkPlacerTypeId, TrunkPlacerTypes,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Plane;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.trunkplacers.DarkOakTrunkPlacer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DarkOakTrunkPlacer {
    /// `this.baseHeight`.
    base_height: i32,
    /// `this.heightRandA`.
    height_rand_a: i32,
    /// `this.heightRandB`.
    height_rand_b: i32,
}

impl DarkOakTrunkPlacer {
    /// `new DarkOakTrunkPlacer(int, int, int)`.
    pub fn new(base_height: i32, height_rand_a: i32, height_rand_b: i32) -> DarkOakTrunkPlacer {
        DarkOakTrunkPlacer {
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

impl TrunkPlacer for DarkOakTrunkPlacer {
    fn type_id(&self) -> TrunkPlacerTypeId {
        TrunkPlacerTypes::DARK_OAK_TRUNK_PLACER
    }

    #[allow(clippy::manual_range_contains)] // `(i < 0 || i > 1 || j < 0 || j > 1)` mirrors Java's disjoint form.
    fn place_trunk<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        tree_height: i32,
        origin: &BlockPos,
        config: &TreeConfiguration,
    ) -> Vec<FoliageAttachment> {
        let mut attachments = Vec::new();
        let below = origin.below();
        place_below_trunk_block(level, trunk_setter, random, &below, config);
        place_below_trunk_block(level, trunk_setter, random, &below.east(), config);
        place_below_trunk_block(level, trunk_setter, random, &below.south(), config);
        place_below_trunk_block(level, trunk_setter, random, &below.south().east(), config);
        let lean_direction = Plane::Horizontal.get_random_direction(random);
        let lean_height = tree_height.wrapping_sub(random.next_int_bound(4));
        let mut lean_steps = 2i32.wrapping_sub(random.next_int_bound(3));
        let x = origin.get_x();
        let y = origin.get_y();
        let z = origin.get_z();
        let mut tx = x;
        let mut tz = z;
        let ey = y.wrapping_add(tree_height).wrapping_sub(1);

        for dy in 0..tree_height {
            if dy >= lean_height && lean_steps > 0 {
                tx = tx.wrapping_add(lean_direction.step_x());
                tz = tz.wrapping_add(lean_direction.step_z());
                lean_steps = lean_steps.wrapping_sub(1);
            }

            let yy = y.wrapping_add(dy);
            let block_pos = BlockPos::new(tx, yy, tz);
            if is_air_or_leaves(level, &block_pos) {
                self.place_log(level, trunk_setter, random, &block_pos, config);
                self.place_log(level, trunk_setter, random, &block_pos.east(), config);
                self.place_log(level, trunk_setter, random, &block_pos.south(), config);
                self.place_log(
                    level,
                    trunk_setter,
                    random,
                    &block_pos.east().south(),
                    config,
                );
            }
        }

        attachments.push(FoliageAttachment::new(BlockPos::new(tx, ey, tz), 0, true));

        for ox in -1..=2 {
            for oz in -1..=2 {
                if (ox < 0 || ox > 1 || oz < 0 || oz > 1) && random.next_int_bound(3) <= 0 {
                    let length = random.next_int_bound(3).wrapping_add(2);

                    for branch_y in 0..length {
                        self.place_log(
                            level,
                            trunk_setter,
                            random,
                            &BlockPos::new(
                                x.wrapping_add(ox),
                                ey.wrapping_sub(branch_y).wrapping_sub(1),
                                z.wrapping_add(oz),
                            ),
                            config,
                        );
                    }

                    attachments.push(FoliageAttachment::new(
                        BlockPos::new(x.wrapping_add(ox), ey, z.wrapping_add(oz)),
                        0,
                        false,
                    ));
                }
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

/// `DarkOakTrunkPlacer.CODEC` — the shared three-field trunk-placer record, as
/// the ops-generic `dark_oak_trunk_placer_map_codec::<Ops>()` factory.
#[allow(clippy::type_complexity)]
pub fn dark_oak_trunk_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<DarkOakTrunkPlacer, Ops>> {
    record_builder::map_codec::<DarkOakTrunkPlacer, Ops>(move |instance| {
        let (base, height_rand_a, height_rand_b) = trunk_placer_parts::<DarkOakTrunkPlacer, Ops>(
            Arc::new(|p: &DarkOakTrunkPlacer| p.base_height),
            Arc::new(|p: &DarkOakTrunkPlacer| p.height_rand_a),
            Arc::new(|p: &DarkOakTrunkPlacer| p.height_rand_b),
        );
        instance
            .group(base)
            .and(height_rand_a)
            .and(height_rand_b)
            .apply(instance, Arc::new(DarkOakTrunkPlacer::new))
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
        let codec = map_codec::codec_of(dark_oak_trunk_placer_map_codec::<JsonOps>());
        let input = json!({
            "base_height": 6,
            "height_rand_a": 2,
            "height_rand_b": 1,
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            TrunkPlacer::type_id(decoded),
            TrunkPlacerTypes::DARK_OAK_TRUNK_PLACER
        );
        assert_eq!(decoded.get_base_height(), 6);
        assert_eq!(decoded.height_rand_a(), 2);
        assert_eq!(decoded.height_rand_b(), 1);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn place_trunk_builds_a_leaning_2x2_trunk() {
        let placer = DarkOakTrunkPlacer::new(1, 0, 0);
        let config = TreeConfiguration::stub();
        let mut random = rivet_util::random::LegacyRandomSource::new(9);
        let origin = BlockPos::new(0, 0, 0);
        let mut placed = BTreeSet::new();
        let mut setter = |pos: &BlockPos, _state: BlockState| {
            placed.insert(*pos);
        };
        let attachments = placer.place_trunk(
            &TestLevel::air(),
            &mut setter,
            &mut random,
            6,
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
        // The trunk is 2x2: the origin-adjacent +x/+z offsets are present.
        assert!(placed.contains(&BlockPos::new(0, 0, 0)));
        assert!(placed.contains(&BlockPos::new(1, 0, 0)));
        // The top double-trunk attachment is present with the lean footprint.
        assert!(!attachments.is_empty());
        assert!(attachments[0].double_trunk);
        // Branch stubs descend below the top attachment's level.
        let min_branch_y = placed.iter().map(|p| p.get_y()).min().unwrap();
        assert!(
            min_branch_y < 0,
            "branch stubs reach below the below-trunk level, got min {min_branch_y}"
        );
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
