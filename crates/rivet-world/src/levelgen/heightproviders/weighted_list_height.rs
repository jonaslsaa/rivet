//! Port of `net.minecraft.world.level.levelgen.heightproviders.WeightedListHeight`
//! (class, 26.2).
//!
//! Java: a provider that holds a `WeightedList<HeightProvider>` and samples by
//! selecting a weighted element (`getRandomOrThrow`) and delegating to its
//! `sample`. `type()` is `HeightProviderType.WEIGHTED_LIST`.
//!
//! `CODEC` is a record codec over the `"distribution"` field,
//! `WeightedList.nonEmptyCodec(HeightProvider.CODEC)` — the recursive element
//! codec is the `HeightProvider.CODEC` `RecursiveSelf` threaded down from the
//! dispatch graph (see `height_provider`). No `toString` in Java (identity-based
//! `Object.toString`), so none is ported.

use crate::levelgen::heightproviders::height_provider::HeightProvider;
use crate::levelgen::world_generation_context::WorldGenerationContext;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::WeightedList;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.heightproviders.WeightedListHeight`.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedListHeight {
    /// `this.distribution` — the weighted provider list.
    distribution: WeightedList<HeightProvider>,
}

impl WeightedListHeight {
    /// `new WeightedListHeight(WeightedList<HeightProvider>)` — the public
    /// constructor.
    pub fn new(distribution: WeightedList<HeightProvider>) -> WeightedListHeight {
        WeightedListHeight { distribution }
    }

    /// `this.distribution`.
    pub fn distribution(&self) -> &WeightedList<HeightProvider> {
        &self.distribution
    }

    /// `WeightedListHeight.sample(RandomSource, WorldGenerationContext)` —
    /// `this.distribution.getRandomOrThrow(random).sample(random, heightAccessor)`.
    pub fn sample<R: RandomSource>(&self, random: &mut R, context: &WorldGenerationContext) -> i32 {
        self.distribution
            .get_random_or_throw(random)
            .sample(random, context)
    }
}

/// `WeightedListHeight.CODEC` — a record codec over the `"distribution"` field
/// (`WeightedList.nonEmptyCodec(HeightProvider.CODEC)`), as the ops-generic
/// `weighted_list_height_map_codec::<Ops>(top)` factory. `top` is the
/// `HeightProvider.CODEC` `RecursiveSelf` from the dispatch graph, so nested
/// weighted lists round-trip through the single recursive codec.
pub fn weighted_list_height_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<HeightProvider, Ops>>,
) -> Arc<dyn MapCodec<WeightedListHeight, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|w: &WeightedListHeight| w.distribution.clone()),
                // `WeightedList.nonEmptyCodec(HeightProvider.CODEC).fieldOf(
                // "distribution")`.
                codec::field_of::<WeightedList<HeightProvider>, Ops>(
                    rivet_util::weighted::weighted_list_non_empty_codec::<HeightProvider, Ops>(top),
                    "distribution".to_string(),
                ),
            ))
            .apply(instance, Arc::new(WeightedListHeight::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use crate::levelgen::heightproviders::constant_height::ConstantHeight;
    use crate::levelgen::heightproviders::height_provider::height_provider_codec;
    use crate::levelgen::vertical_anchor::VerticalAnchor;
    use rivet_registry::core::BlockPos;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::{LegacyRandomSource, XoroshiroRandomSource};
    use rivet_util::{Weighted, WeightedList};
    use serde_json::json;

    /// A `ChunkGenerator` double exposing a fixed worldgen window.
    struct TestGenerator {
        min_y: i32,
        depth: i32,
    }
    impl ChunkGenerator for TestGenerator {
        fn get_min_y(&self) -> i32 {
            self.min_y
        }
        fn get_gen_depth(&self) -> i32 {
            self.depth
        }
    }

    /// A `WorldGenLevel` double over a fixed window.
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
            // RivetTodo(#399): never read here.
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    fn context(min_y: i32, height: i32, gen_depth: i32) -> WorldGenerationContext {
        let level = TestLevel(create(min_y, height));
        let generator = TestGenerator {
            min_y,
            depth: gen_depth,
        };
        WorldGenerationContext::new(&generator, &level)
    }

    fn overworld() -> WorldGenerationContext {
        context(-64, 384, 384)
    }

    #[test]
    fn sample_delegates_to_weighted_element() {
        let distribution = WeightedList::new(&[
            Weighted::new(
                HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::absolute(1))),
                1,
            ),
            Weighted::new(
                HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::absolute(2))),
                3,
            ),
        ]);
        let p = WeightedListHeight::new(distribution);
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..4)
            .map(|_| p.sample(&mut random, &overworld()))
            .collect();
        // total weight 4 (flat): selection 0 -> value 1; 1,2,3 -> value 2.
        assert_eq!(samples, [2, 2, 2, 2]);
        let mut xor = XoroshiroRandomSource::new(12345);
        let xsamples: Vec<i32> = (0..4).map(|_| p.sample(&mut xor, &overworld())).collect();
        assert_eq!(xsamples, [1, 2, 2, 1]);
    }

    #[test]
    #[should_panic(expected = "Weighted list has no elements")]
    fn empty_distribution_panics_on_sample() {
        let p = WeightedListHeight::new(WeightedList::of());
        let mut random = LegacyRandomSource::new(1);
        let _ = p.sample(&mut random, &overworld());
    }

    #[test]
    fn codec_round_trips_through_the_recursive_dispatch() {
        // The top-level `HeightProvider.CODEC` dispatches `weighted_list` and
        // threads the recursive self into the distribution's element codec.
        let codec = height_provider_codec::<JsonOps>();
        let input = json!({
            "type": "minecraft:weighted_list",
            "distribution": [
                {"data": {"type": "minecraft:constant", "value": {"absolute": 1}}, "weight": 1},
                {"data": {"type": "minecraft:uniform",
                           "min_inclusive": {"absolute": 2},
                           "max_inclusive": {"absolute": 3}}, "weight": 2}
            ]
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        // Paper's `CODEC.xmap` special-cases CONSTANT to a bare anchor, and the
        // dispatch `"type"` key encodes last.
        assert_eq!(
            encoded,
            json!({
                "distribution": [
                    {"data": {"absolute": 1}, "weight": 1},
                    {"data": {"min_inclusive": {"absolute": 2},
                              "max_inclusive": {"absolute": 3},
                              "type": "minecraft:uniform"}, "weight": 2}
                ],
                "type": "minecraft:weighted_list"
            })
        );
    }

    #[test]
    fn non_empty_codec_rejects_empty_distribution() {
        // `WeightedList.nonEmptyCodec` validates the decoded list is non-empty
        // with Paper's exact message.
        let codec =
            rivet_serialization::map_codec::codec_of(weighted_list_height_map_codec::<JsonOps>(
                height_provider_codec::<JsonOps>(),
            ));
        let input = json!({"distribution": []});
        let error = codec
            .parse(&JsonOps::INSTANCE, &input)
            .error_ref()
            .map(|e| e.message().to_string());
        assert_eq!(
            error.as_deref(),
            Some("Weighted list must contain at least one entry with non-zero weight")
        );
    }
}
