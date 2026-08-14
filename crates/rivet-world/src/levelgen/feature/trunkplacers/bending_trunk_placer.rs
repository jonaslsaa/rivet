//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! BendingTrunkPlacer` (class, 26.2).
//!
//! `CODEC` is `trunkPlacerParts(i).and(i.group(min_height_for_leaves,
//! bend_length)).apply(i, BendingTrunkPlacer::new)` — the shared three-field
//! trunk record plus the nested two-field group (`ExtraCodecs.POSITIVE_INT`
//! optional field defaulting to 1, and `IntProviders.codec(1, 64)`
//! `bend_length`). `placeTrunk` walks the leaning column, then extends the bend
//! horizontally for `bendLength.sample(random)` more steps, adding a foliage
//! point at every step at or above `minHeightForLeaves`.

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
use rivet_registry::core::Direction;
use rivet_registry::core::Plane;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.trunkplacers.BendingTrunkPlacer`.
#[derive(Debug, Clone, PartialEq)]
pub struct BendingTrunkPlacer {
    /// `this.baseHeight`.
    base_height: i32,
    /// `this.heightRandA`.
    height_rand_a: i32,
    /// `this.heightRandB`.
    height_rand_b: i32,
    /// `this.minHeightForLeaves` — the first foliage-point height.
    min_height_for_leaves: i32,
    /// `this.bendLength` — the horizontal extension length.
    bend_length: IntProvider,
}

impl BendingTrunkPlacer {
    /// `new BendingTrunkPlacer(int, int, int, int, IntProvider)`.
    pub fn new(
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
        min_height_for_leaves: i32,
        bend_length: IntProvider,
    ) -> BendingTrunkPlacer {
        BendingTrunkPlacer {
            base_height,
            height_rand_a,
            height_rand_b,
            min_height_for_leaves,
            bend_length,
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

    /// `this.minHeightForLeaves`.
    pub fn min_height_for_leaves(&self) -> i32 {
        self.min_height_for_leaves
    }

    /// `this.bendLength`.
    pub fn bend_length(&self) -> &IntProvider {
        &self.bend_length
    }
}

impl TrunkPlacer for BendingTrunkPlacer {
    fn type_id(&self) -> TrunkPlacerTypeId {
        TrunkPlacerTypes::BENDING_TRUNK_PLACER
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
        let direction = Plane::Horizontal.get_random_direction(random);
        let log_height = tree_height.wrapping_sub(1);
        let mut pos = origin.mutable();
        let below_pos = pos.immutable().below();
        place_below_trunk_block(level, trunk_setter, random, &below_pos, config);
        let mut foliage_points = Vec::new();

        for i in 0..=log_height {
            if i.wrapping_add(1) >= log_height.wrapping_add(random.next_int_bound(2)) {
                pos.move_dir(&direction);
            }

            if self.valid_tree_pos(level, &pos.immutable()) {
                self.place_log(level, trunk_setter, random, &pos.immutable(), config);
            }

            if i >= self.min_height_for_leaves {
                foliage_points.push(FoliageAttachment::new(pos.immutable(), 0, false));
            }

            pos.move_dir(&Direction::Up);
        }

        let dir_length = self.bend_length.sample(random);

        for _i in 0..=dir_length {
            if self.valid_tree_pos(level, &pos.immutable()) {
                self.place_log(level, trunk_setter, random, &pos.immutable(), config);
            }

            foliage_points.push(FoliageAttachment::new(pos.immutable(), 0, false));
            pos.move_dir(&direction);
        }

        foliage_points
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

/// `BendingTrunkPlacer.CODEC` — the shared three-field trunk record combined
/// with the nested `i.group(min_height_for_leaves, bend_length)`, as the
/// ops-generic `bending_trunk_placer_map_codec::<Ops>()` factory.
#[allow(clippy::type_complexity)]
pub fn bending_trunk_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<BendingTrunkPlacer, Ops>> {
    record_builder::map_codec::<BendingTrunkPlacer, Ops>(move |instance| {
        let (base, height_rand_a, height_rand_b) = trunk_placer_parts::<BendingTrunkPlacer, Ops>(
            Arc::new(|p: &BendingTrunkPlacer| p.base_height),
            Arc::new(|p: &BendingTrunkPlacer| p.height_rand_a),
            Arc::new(|p: &BendingTrunkPlacer| p.height_rand_b),
        );
        // `ExtraCodecs.POSITIVE_INT.optionalFieldOf("min_height_for_leaves", 1)`
        // — the NON-lenient optional field (present-but-malformed is an error),
        // defaulting to 1 on decode and omitted on encode when equal to 1.
        let min_height_for_leaves = RecordCodecBuilder::of(
            Arc::new(|p: &BendingTrunkPlacer| p.min_height_for_leaves),
            codec::optional_field_of(
                "min_height_for_leaves",
                rivet_util::extra_codecs::positive_int::<Ops>(),
                1,
            ),
        );
        // `IntProviders.codec(1, 64).fieldOf("bend_length")`.
        let bend_length = RecordCodecBuilder::of(
            Arc::new(|p: &BendingTrunkPlacer| p.bend_length.clone()),
            codec::field_of(
                int_provider_codec_with_bounds::<Ops>(1, 64),
                "bend_length".to_string(),
            ),
        );
        instance
            .group(base)
            .and(height_rand_a)
            .and(height_rand_b)
            .and(min_height_for_leaves)
            .and(bend_length)
            .apply(instance, Arc::new(BendingTrunkPlacer::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::map_codec;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    fn uniform(min: i32, max: i32) -> IntProvider {
        IntProvider::Uniform(UniformInt::of(min, max))
    }

    #[test]
    fn codec_round_trips_the_five_field_record() {
        let codec = map_codec::codec_of(bending_trunk_placer_map_codec::<JsonOps>());
        let input = json!({
            "base_height": 7,
            "height_rand_a": 3,
            "height_rand_b": 2,
            "min_height_for_leaves": 4,
            "bend_length": {"min_inclusive": 2, "max_inclusive": 8, "type": "minecraft:uniform"},
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            TrunkPlacer::type_id(decoded),
            TrunkPlacerTypes::BENDING_TRUNK_PLACER
        );
        assert_eq!(decoded.min_height_for_leaves(), 4);
        assert_eq!(decoded.bend_length().min_inclusive(), 2);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn min_height_for_leaves_defaults_to_one_and_is_omitted_on_encode() {
        // `optionalFieldOf(..., 1)` — absent on decode defaults to 1; encode
        // omits the key when it equals the default.
        let codec = map_codec::codec_of(bending_trunk_placer_map_codec::<JsonOps>());
        let input = json!({
            "base_height": 7,
            "height_rand_a": 3,
            "height_rand_b": 2,
            "bend_length": {"min_inclusive": 2, "max_inclusive": 8, "type": "minecraft:uniform"},
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(decoded.min_height_for_leaves(), 1);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert!(
            encoded.get("min_height_for_leaves").is_none(),
            "default min_height_for_leaves must be omitted on encode: {encoded}"
        );
    }

    #[test]
    fn codec_rejects_non_positive_min_height_for_leaves() {
        // `ExtraCodecs.POSITIVE_INT` — zero is a decode error (the optional
        // field is NON-lenient).
        let codec = map_codec::codec_of(bending_trunk_placer_map_codec::<JsonOps>());
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({
                "base_height": 7,
                "height_rand_a": 3,
                "height_rand_b": 2,
                "min_height_for_leaves": 0,
                "bend_length": {"min_inclusive": 2, "max_inclusive": 8, "type": "minecraft:uniform"},
            }),
        );
        assert!(result.is_error(), "got: {:?}", result);
    }

    #[test]
    fn place_trunk_extends_horizontally_past_the_origin() {
        let placer = BendingTrunkPlacer::new(1, 0, 0, 2, uniform(2, 2));
        let config = TreeConfiguration::stub();
        let mut random = rivet_util::random::LegacyRandomSource::new(5);
        let origin = BlockPos::new(0, 0, 0);
        let mut placed = Vec::new();
        let mut setter = |pos: &BlockPos, _state: BlockState| {
            placed.push(*pos);
        };
        let foliage = placer.place_trunk(
            &TestLevel::air(),
            &mut setter,
            &mut random,
            5,
            &origin,
            &config,
        );
        // Below-trunk block first; the bend extends along the lean direction.
        assert_eq!(placed.first(), Some(&BlockPos::new(0, -1, 0)));
        // The bend extends 2 steps horizontally beyond the column.
        let max_abs = placed
            .iter()
            .map(|p| p.get_x().abs().max(p.get_z().abs()))
            .max()
            .unwrap();
        assert!(
            max_abs >= 2,
            "bend should extend past origin, got max {max_abs}"
        );
        // Foliage points are added every bend step (and every column step at
        // or above min_height_for_leaves).
        assert!(foliage.len() >= 3, "foliage points: {foliage:?}");
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
