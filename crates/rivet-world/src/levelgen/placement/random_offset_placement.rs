//! Port of `net.minecraft.world.level.levelgen.placement.RandomOffsetPlacement`
//! (class, 26.2).
//!
//! Java: a modifier holding two `IntProvider`s (`xzSpread`, `ySpread`) whose
//! `getPositions` offsets the origin by three draws — `origin.getX() +
//! xzSpread.sample(random)`, `origin.getY() + ySpread.sample(random)`,
//! `origin.getZ() + xzSpread.sample(random)` (the XZ provider is sampled twice,
//! once per horizontal axis). Its `CODEC` is the record `{xz_spread,
//! y_spread}` (each `IntProviders.codec(-16, 16)`), and its `type()` is
//! `PlacementModifierType.RANDOM_OFFSET`.

use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypes;
use crate::levelgen::placement::{PlacementContext, PlacementModifier};
use rivet_registry::core::BlockPos;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use rivet_util::valueproviders::constant_int::ConstantInt;
use rivet_util::valueproviders::int_provider::IntProvider;
use rivet_util::valueproviders::int_provider::int_provider_codec_with_bounds;
use rivet_util::valueproviders::trapezoid_int::TrapezoidInt;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.RandomOffsetPlacement`.
#[derive(Debug, Clone, PartialEq)]
pub struct RandomOffsetPlacement {
    /// `this.xzSpread` — the horizontal offset provider (sampled twice).
    xz_spread: IntProvider,
    /// `this.ySpread` — the vertical offset provider.
    y_spread: IntProvider,
}

impl RandomOffsetPlacement {
    /// `of(IntProvider xzSpread, IntProvider ySpread)`.
    pub fn of(xz_spread: IntProvider, y_spread: IntProvider) -> Self {
        RandomOffsetPlacement {
            xz_spread,
            y_spread,
        }
    }

    /// `ofTriangle(int xzRange, int yRange)`.
    pub fn of_triangle(xz_range: i32, y_range: i32) -> Self {
        RandomOffsetPlacement::of(
            TrapezoidInt::triangle(xz_range),
            TrapezoidInt::triangle(y_range),
        )
    }

    /// `vertical(IntProvider ySpread)` — `of(ConstantInt.of(0), ySpread)`.
    pub fn vertical(y_spread: IntProvider) -> Self {
        RandomOffsetPlacement::of(IntProvider::Constant(ConstantInt::of(0)), y_spread)
    }

    /// `horizontal(IntProvider xzSpread)` — `of(xzSpread, ConstantInt.of(0))`.
    pub fn horizontal(xz_spread: IntProvider) -> Self {
        RandomOffsetPlacement::of(xz_spread, IntProvider::Constant(ConstantInt::of(0)))
    }
}

impl PlacementModifier for RandomOffsetPlacement {
    fn get_positions<'a, R: RandomSource>(
        &'a self,
        _context: &PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
        // Java int addition wraps; the XZ provider samples once per axis.
        let scatter_x = origin.get_x().wrapping_add(self.xz_spread.sample(random));
        let scatter_y = origin.get_y().wrapping_add(self.y_spread.sample(random));
        let scatter_z = origin.get_z().wrapping_add(self.xz_spread.sample(random));
        Box::new(std::iter::once(BlockPos::new(
            scatter_x, scatter_y, scatter_z,
        )))
    }

    fn type_id(
        &self,
    ) -> crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId {
        // `PlacementModifierType.RANDOM_OFFSET` is insertion index 13 in
        // `PlacementModifierType.java`'s registration order.
        PlacementModifierTypes::RANDOM_OFFSET
    }
}

/// `RandomOffsetPlacement.CODEC` — the record codec over the two spread fields,
/// as the ops-generic `random_offset_placement_map_codec::<Ops>()` factory.
pub fn random_offset_placement_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<RandomOffsetPlacement, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|c: &RandomOffsetPlacement| c.xz_spread.clone()),
                "xz_spread".to_string(),
                int_provider_codec_with_bounds::<Ops>(-16, 16),
            ))
            .and(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|c: &RandomOffsetPlacement| c.y_spread.clone()),
                "y_spread".to_string(),
                int_provider_codec_with_bounds::<Ops>(-16, 16),
            ))
            .apply(
                instance,
                Arc::new(|xz: IntProvider, y: IntProvider| RandomOffsetPlacement::of(xz, y)),
            )
    })
}

/// `RandomOffsetPlacement.CODEC` as a `Codec` (`MapCodec.codec()`), the shape
/// the `#181` generated dispatch's registration table consumes.
#[allow(dead_code)]
pub fn random_offset_placement_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<RandomOffsetPlacement, Ops>> {
    map_codec::codec_of(random_offset_placement_map_codec::<Ops>())
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

    fn offset_positions(
        modifier: &RandomOffsetPlacement,
        random: &mut LegacyRandomSource,
        origin: &BlockPos,
    ) -> Vec<BlockPos> {
        let mut level = TestLevel(create(-64, 384));
        let generator = NoopGenerator;
        let context = PlacementContext::new(&mut level, &generator, None);
        modifier.get_positions(&context, random, origin).collect()
    }

    #[test]
    fn offsets_each_axis_by_the_spread() {
        // Constant providers: xz=+2, y=-1 -> (x+2, y-1, z+2).
        let modifier = RandomOffsetPlacement::of(
            IntProvider::Constant(ConstantInt::of(2)),
            IntProvider::Constant(ConstantInt::of(-1)),
        );
        let origin = BlockPos::new(10, 20, 30);
        let mut random = LegacyRandomSource::new(0);
        let positions = offset_positions(&modifier, &mut random, &origin);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0], BlockPos::new(12, 19, 32));
    }

    #[test]
    fn triangle_factory_builds_trapezoid_triangles() {
        // `ofTriangle` = `of(TrapezoidInt.triangle(xz), TrapezoidInt.triangle(y))`.
        // TrapezoidInt.triangle(2) is a triangle over [-2, 2] with plateau 0;
        // the golden sample for seed 12345 is pinned from the value-provider
        // unit.
        let modifier = RandomOffsetPlacement::of_triangle(2, 3);
        let origin = BlockPos::new(0, 0, 0);
        let mut random = LegacyRandomSource::new(12345);
        let positions = offset_positions(&modifier, &mut random, &origin);
        assert_eq!(positions.len(), 1);
        // x and z both sample the same triangle(2) provider (two draws).
        assert!(positions[0].get_x() >= -2 && positions[0].get_x() <= 2);
        assert!(positions[0].get_z() >= -2 && positions[0].get_z() <= 2);
        assert!(positions[0].get_y() >= -3 && positions[0].get_y() <= 3);
    }

    #[test]
    fn vertical_keeps_horizontal_zero() {
        let modifier = RandomOffsetPlacement::vertical(IntProvider::Constant(ConstantInt::of(5)));
        let origin = BlockPos::new(7, 8, 9);
        let mut random = LegacyRandomSource::new(0);
        let positions = offset_positions(&modifier, &mut random, &origin);
        assert_eq!(positions[0], BlockPos::new(7, 13, 9));
    }

    #[test]
    fn horizontal_keeps_vertical_zero() {
        let modifier =
            RandomOffsetPlacement::horizontal(IntProvider::Constant(ConstantInt::of(-4)));
        let origin = BlockPos::new(7, 8, 9);
        let mut random = LegacyRandomSource::new(0);
        let positions = offset_positions(&modifier, &mut random, &origin);
        assert_eq!(positions[0], BlockPos::new(3, 8, 5));
    }

    #[test]
    fn random_offset_type_identity_is_reported() {
        // `PlacementModifierType.RANDOM_OFFSET` is insertion index 13.
        let modifier = RandomOffsetPlacement::vertical(IntProvider::Constant(ConstantInt::of(0)));
        assert_eq!(modifier.type_id(), PlacementModifierTypes::RANDOM_OFFSET);
    }

    #[test]
    fn codec_round_trips_the_spreads() {
        let ops = JsonOps::INSTANCE;
        let codec = random_offset_placement_codec::<JsonOps>();
        let modifier = RandomOffsetPlacement::of(
            IntProvider::Constant(ConstantInt::of(2)),
            IntProvider::Constant(ConstantInt::of(-1)),
        );
        let encoded = codec
            .encode_start(&ops, &modifier)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"xz_spread": 2, "y_spread": -1}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, modifier);
    }

    #[test]
    fn codec_rejects_a_spread_out_of_bounds() {
        // `IntProviders.codec(-16, 16)` — a constant 20 is out of range.
        let ops = JsonOps::INSTANCE;
        let codec = random_offset_placement_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({"xz_spread": 20, "y_spread": 0}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.contains("Value provider too high: 16"), "got: {msg}");
    }
}
