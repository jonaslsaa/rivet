//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! MegaJungleTrunkPlacer` (class, 26.2) — `GiantTrunkPlacer` plus the
//! `Mth.cos`/`Mth.sin` branch limbs.
//!
//! The port keeps the Java `extends GiantTrunkPlacer` inheritance as an embedded
//! [`GiantTrunkPlacer`] field (`giant`), exactly like the config-record
//! composition pattern: `placeTrunk` calls `GiantTrunkPlacer::place_trunk` on
//! the embedded giant and appends its own branch limbs; the height accessors
//! delegate to the giant. `CODEC` is the shared three-field record
//! (`trunkPlacerParts(i).apply(i, MegaJungleTrunkPlacer::new)`).

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer::FoliageAttachment;
use crate::levelgen::feature::trunkplacers::giant_trunk_placer::GiantTrunkPlacer;
use crate::levelgen::feature::trunkplacers::trunk_placer::{TrunkPlacer, trunk_placer_parts};
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

/// `net.minecraft.world.level.levelgen.feature.trunkplacers.MegaJungleTrunkPlacer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MegaJungleTrunkPlacer {
    /// The embedded `GiantTrunkPlacer` superclass state (Java `extends`).
    giant: GiantTrunkPlacer,
}

impl MegaJungleTrunkPlacer {
    /// `new MegaJungleTrunkPlacer(int, int, int)` — `super(baseHeight,
    /// heightRandA, heightRandB)`.
    pub fn new(base_height: i32, height_rand_a: i32, height_rand_b: i32) -> MegaJungleTrunkPlacer {
        MegaJungleTrunkPlacer {
            giant: GiantTrunkPlacer::new(base_height, height_rand_a, height_rand_b),
        }
    }
}

impl TrunkPlacer for MegaJungleTrunkPlacer {
    fn type_id(&self) -> TrunkPlacerTypeId {
        TrunkPlacerTypes::MEGA_JUNGLE_TRUNK_PLACER
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
        // `super.placeTrunk(...)` — the embedded giant's 2x2 trunk.
        let mut attachments = GiantTrunkPlacer::place_trunk(
            &self.giant,
            level,
            trunk_setter,
            random,
            tree_height,
            origin,
            config,
        );

        let mut branch_height = tree_height
            .wrapping_sub(2)
            .wrapping_sub(random.next_int_bound(4));
        while branch_height > tree_height / 2 {
            // `random.nextFloat() * (float)(Math.PI * 2)`.
            let angle = random.next_float() * (std::f64::consts::PI * 2.0) as f32;
            let mut bx = 0;
            let mut bz = 0;

            for b in 0..5 {
                // `(int)(1.5F + Mth.cos(angle) * b)` — `Mth.cos` takes the
                // widened `double` of the float angle.
                bx = (1.5f32 + rivet_util::mth::cos(angle as f64) * b as f32) as i32;
                bz = (1.5f32 + rivet_util::mth::sin(angle as f64) * b as f32) as i32;
                let pos = origin.offset(bx, branch_height.wrapping_sub(3).wrapping_add(b / 2), bz);
                self.place_log(level, trunk_setter, random, &pos, config);
            }

            attachments.push(FoliageAttachment::new(
                origin.offset(bx, branch_height, bz),
                -2,
                false,
            ));

            // `branchHeight -= 2 + random.nextInt(4)`.
            branch_height = branch_height
                .wrapping_sub(2)
                .wrapping_sub(random.next_int_bound(4));
        }

        attachments
    }

    fn get_base_height(&self) -> i32 {
        self.giant.get_base_height()
    }

    fn base_height(&self) -> i32 {
        self.giant.base_height()
    }

    fn height_rand_a(&self) -> i32 {
        self.giant.height_rand_a()
    }

    fn height_rand_b(&self) -> i32 {
        self.giant.height_rand_b()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `MegaJungleTrunkPlacer.CODEC` — the shared three-field trunk-placer record,
/// as the ops-generic `mega_jungle_trunk_placer_map_codec::<Ops>()` factory.
#[allow(clippy::type_complexity)]
pub fn mega_jungle_trunk_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<MegaJungleTrunkPlacer, Ops>> {
    record_builder::map_codec::<MegaJungleTrunkPlacer, Ops>(move |instance| {
        let (base, height_rand_a, height_rand_b) = trunk_placer_parts::<MegaJungleTrunkPlacer, Ops>(
            Arc::new(|p: &MegaJungleTrunkPlacer| p.giant.base_height()),
            Arc::new(|p: &MegaJungleTrunkPlacer| p.giant.height_rand_a()),
            Arc::new(|p: &MegaJungleTrunkPlacer| p.giant.height_rand_b()),
        );
        instance
            .group(base)
            .and(height_rand_a)
            .and(height_rand_b)
            .apply(instance, Arc::new(MegaJungleTrunkPlacer::new))
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
        let codec = map_codec::codec_of(mega_jungle_trunk_placer_map_codec::<JsonOps>());
        let input = json!({
            "base_height": 14,
            "height_rand_a": 4,
            "height_rand_b": 5,
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            TrunkPlacer::type_id(decoded),
            TrunkPlacerTypes::MEGA_JUNGLE_TRUNK_PLACER
        );
        assert_eq!(decoded.get_base_height(), 14);
        assert_eq!(decoded.height_rand_a(), 4);
        assert_eq!(decoded.height_rand_b(), 5);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn place_trunk_includes_giant_trunk_and_limbs() {
        let placer = MegaJungleTrunkPlacer::new(1, 0, 0);
        let config = TreeConfiguration::stub();
        let mut random = rivet_util::random::LegacyRandomSource::new(11);
        let origin = BlockPos::new(0, 0, 0);
        let mut placed = BTreeSet::new();
        let mut setter = |pos: &BlockPos, _state: BlockState| {
            placed.insert(*pos);
        };
        let attachments = placer.place_trunk(
            &TestLevel::air(),
            &mut setter,
            &mut random,
            12,
            &origin,
            &config,
        );

        // The giant base (2x2 below-trunk + full 2x2 lower layers) is placed.
        assert!(placed.contains(&BlockPos::new(0, -1, 0)));
        assert!(placed.contains(&BlockPos::new(1, -1, 1)));
        assert!(placed.contains(&BlockPos::new(1, 0, 0)));
        // A fixed seed produces the deterministic limb/branch set; the giant's
        // top attachment is double-trunk.
        assert!(!attachments.is_empty());
        assert!(attachments[0].double_trunk);
        // Limbs reach beyond the 2x2 origin footprint.
        let max_abs = placed
            .iter()
            .map(|p| p.get_x().abs().max(p.get_z().abs()))
            .max()
            .unwrap();
        assert!(
            max_abs >= 2,
            "limbs should extend the footprint, got max {max_abs}"
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
