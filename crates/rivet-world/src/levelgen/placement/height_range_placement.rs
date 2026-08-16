//! Port of `net.minecraft.world.level.levelgen.placement.HeightRangePlacement`
//! (class, 26.2).
//!
//! Java: a modifier holding a `HeightProvider` whose `getPositions` relocates
//! the origin to the provider's sample at its X/Z — `origin.atY(height.sample(
//! random, context))`. Its `CODEC` is the `"height"` field (`HeightProvider.
//! CODEC`) mapped onto the private constructor, and its `type()` is
//! `PlacementModifierType.HEIGHT_RANGE`.
//!
//! The `HeightProvider` sample takes the `WorldGenerationContext` window; the
//! port passes the composed `world_generation_context` that `PlacementContext`
//! stores (the same value Java's `this.height.sample(random, this)` sees).

use crate::levelgen::heightproviders::height_provider::HeightProvider;
use crate::levelgen::heightproviders::trapezoid_height::TrapezoidHeight;
use crate::levelgen::heightproviders::uniform_height::UniformHeight;
use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypes;
use crate::levelgen::placement::{PlacementContext, PlacementModifier};
use crate::levelgen::vertical_anchor::VerticalAnchor;
use rivet_registry::core::BlockPos;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.HeightRangePlacement`.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightRangePlacement {
    /// `this.height` — the `HeightProvider` sampled per origin.
    height: HeightProvider,
}

impl HeightRangePlacement {
    /// `of(HeightProvider)` — the public factory.
    pub fn of(height: HeightProvider) -> Self {
        HeightRangePlacement { height }
    }

    /// `uniform(VerticalAnchor minInclusive, VerticalAnchor maxInclusive)`.
    pub fn uniform(min_inclusive: VerticalAnchor, max_inclusive: VerticalAnchor) -> Self {
        HeightRangePlacement::of(HeightProvider::Uniform(UniformHeight::of(
            min_inclusive,
            max_inclusive,
        )))
    }

    /// `triangle(VerticalAnchor minInclusive, VerticalAnchor maxInclusive)`.
    pub fn triangle(min_inclusive: VerticalAnchor, max_inclusive: VerticalAnchor) -> Self {
        HeightRangePlacement::of(HeightProvider::Trapezoid(TrapezoidHeight::of_2(
            min_inclusive,
            max_inclusive,
        )))
    }
}

impl PlacementModifier for HeightRangePlacement {
    fn get_positions<'a, R: RandomSource>(
        &'a self,
        context: &mut PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
        // `origin.atY(this.height.sample(random, context))` — the provider
        // samples against the `WorldGenerationContext` window Java holds as
        // its superclass.
        Box::new(std::iter::once(
            origin.at_y(
                self.height
                    .sample(random, context.world_generation_context()),
            ),
        ))
    }

    fn type_id(
        &self,
    ) -> crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId {
        // `PlacementModifierType.HEIGHT_RANGE` is insertion index 11 in
        // `PlacementModifierType.java`'s registration order.
        PlacementModifierTypes::HEIGHT_RANGE
    }
}

/// `HeightRangePlacement.CODEC` — `HeightProvider.CODEC.fieldOf("height").xmap(
/// HeightRangePlacement::new, c -> c.height)`, as the ops-generic
/// `height_range_placement_map_codec::<Ops>()` factory.
pub fn height_range_placement_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<HeightRangePlacement, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|c: &HeightRangePlacement| c.height.clone()),
                "height".to_string(),
                crate::levelgen::heightproviders::height_provider::height_provider_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|height: HeightProvider| HeightRangePlacement::of(height)),
            )
    })
}

/// `HeightRangePlacement.CODEC` as a `Codec` (`MapCodec.codec()`), the shape
/// the `#181` generated dispatch's registration table consumes.
#[allow(dead_code)]
pub fn height_range_placement_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<HeightRangePlacement, Ops>> {
    map_codec::codec_of(height_range_placement_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    /// A `WorldGenLevel` double over the overworld window.
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

    impl ChunkGenerator for NoopGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    fn sample_positions(
        modifier: &HeightRangePlacement,
        random: &mut LegacyRandomSource,
        origin: &BlockPos,
    ) -> Vec<BlockPos> {
        let mut level = TestLevel(create(-64, 384));
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        modifier
            .get_positions(&mut context, random, origin)
            .collect()
    }

    #[test]
    fn relocates_origin_to_the_sampled_height() {
        // `origin.atY(sample)`: X/Z unchanged, Y the provider's sample.
        // Uniform 0..10 with seed 12345 samples 6, 6, 6, 10, ... (pinned
        // golden from the height-provider unit).
        let origin = BlockPos::new(1, 2, 3);
        let modifier = HeightRangePlacement::uniform(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(10),
        );
        let mut random = LegacyRandomSource::new(12345);
        let positions = sample_positions(&modifier, &mut random, &origin);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].get_x(), 1);
        assert_eq!(positions[0].get_z(), 3);
        assert_eq!(positions[0].get_y(), 6);
    }

    #[test]
    fn triangle_factory_uses_trapezoid_height() {
        // `triangle` = `of(TrapezoidHeight.of(min, max))` — with the default
        // plateau 0 (a pure triangle). Seed 12345 triangle 0..9 samples
        // 1, 6, 5, ... (pinned golden from the height-provider unit).
        let origin = BlockPos::new(4, 5, 6);
        let modifier = HeightRangePlacement::triangle(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(9),
        );
        let mut random = LegacyRandomSource::new(12345);
        let positions = sample_positions(&modifier, &mut random, &origin);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].get_y(), 1);
        assert_eq!(positions[0].get_x(), 4);
        assert_eq!(positions[0].get_z(), 6);
    }

    #[test]
    fn height_range_type_identity_is_reported() {
        // `PlacementModifierType.HEIGHT_RANGE` is insertion index 11.
        let modifier = HeightRangePlacement::of(HeightProvider::Constant(
            crate::levelgen::heightproviders::constant_height::ConstantHeight::of(
                VerticalAnchor::absolute(3),
            ),
        ));
        assert_eq!(modifier.type_id(), PlacementModifierTypes::HEIGHT_RANGE);
    }

    #[test]
    fn codec_round_trips_the_height_field() {
        let ops = JsonOps::INSTANCE;
        let codec = height_range_placement_codec::<JsonOps>();
        let modifier = HeightRangePlacement::uniform(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(10),
        );
        let encoded = codec
            .encode_start(&ops, &modifier)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "height": {
                    "min_inclusive": {"absolute": 0},
                    "max_inclusive": {"absolute": 10},
                    "type": "minecraft:uniform"
                }
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, modifier);
    }

    #[test]
    fn codec_missing_height_field_errors() {
        let ops = JsonOps::INSTANCE;
        let codec = height_range_placement_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.contains("No key height"), "got: {msg}");
    }
}
