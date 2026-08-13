//! Port of `net.minecraft.world.level.levelgen.heightproviders.UniformHeight`
//! (class, 26.2).
//!
//! Java: a provider that samples uniformly between a `minInclusive` and
//! `maxInclusive` anchor. `sample` resolves both anchors, warns on an empty
//! range (`min > max`) and returns `min`, otherwise draws
//! `Mth.randomBetweenInclusive(random, min, max)`.
//!
//! The `LongOpenHashSet warnedFor` dedup and the `LOGGER.warn` are dropped: the
//! port's `log_and_pause_if_in_ide` is a documented no-op and the dedup only
//! suppresses duplicate (no-op) warnings — neither affects the sampled value
//! (the same precedent as the dropped IDE-only warnings in `rivet-util::weighted`).

use crate::levelgen::vertical_anchor::VerticalAnchor;
use crate::levelgen::world_generation_context::WorldGenerationContext;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::mth::random_between_inclusive;
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.heightproviders.UniformHeight`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UniformHeight {
    /// `this.minInclusive` — the lower anchor (inclusive).
    min_inclusive: VerticalAnchor,
    /// `this.maxInclusive` — the upper anchor (inclusive).
    max_inclusive: VerticalAnchor,
}

impl UniformHeight {
    /// `UniformHeight.of(VerticalAnchor minInclusive, VerticalAnchor
    /// maxInclusive)`.
    pub const fn of(min_inclusive: VerticalAnchor, max_inclusive: VerticalAnchor) -> UniformHeight {
        UniformHeight {
            min_inclusive,
            max_inclusive,
        }
    }

    /// `UniformHeight.sample(RandomSource, WorldGenerationContext)`.
    ///
    /// ```java
    /// int min = minInclusive.resolveY(context);
    /// int max = maxInclusive.resolveY(context);
    /// if (min > max) { LOGGER.warn(...); return min; }
    /// return Mth.randomBetweenInclusive(random, min, max);
    /// ```
    pub fn sample<R: RandomSource>(&self, random: &mut R, context: &WorldGenerationContext) -> i32 {
        let min = self.min_inclusive.resolve_y(context);
        let max = self.max_inclusive.resolve_y(context);
        if min > max {
            // `LOGGER.warn("Empty height range: {}", this)` — dropped (no-op).
            min
        } else {
            random_between_inclusive(random, min, max)
        }
    }
}

impl fmt::Display for UniformHeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `"[" + minInclusive + "-" + maxInclusive + "]"`.
        write!(f, "[{}-{}]", self.min_inclusive, self.max_inclusive)
    }
}

/// `UniformHeight.CODEC` — a record codec over the two anchor fields, as the
/// ops-generic `uniform_height_map_codec::<Ops>()` factory.
pub fn uniform_height_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<UniformHeight, Ops>>
{
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|u: &UniformHeight| u.min_inclusive),
                // `VerticalAnchor.CODEC.fieldOf("min_inclusive")`.
                codec::field_of::<VerticalAnchor, Ops>(
                    crate::levelgen::vertical_anchor::vertical_anchor_codec::<Ops>(),
                    "min_inclusive".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|u: &UniformHeight| u.max_inclusive),
                codec::field_of::<VerticalAnchor, Ops>(
                    crate::levelgen::vertical_anchor::vertical_anchor_codec::<Ops>(),
                    "max_inclusive".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(|min: VerticalAnchor, max: VerticalAnchor| UniformHeight::of(min, max)),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use rivet_registry::core::BlockPos;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    /// A `ChunkGenerator` double exposing a fixed worldgen window.
    struct TestGenerator {
        min_y: i32,
        depth: i32,
    }
    impl ChunkGenerator for TestGenerator {
        fn create_biomes(&self) {}
        fn apply_carvers(&self) {}
        fn build_surface(&self) {}
        fn spawn_original_mobs(&self) {}
        fn fill_from_noise(&self) {}
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
    fn sample_is_uniform_between_resolved_anchors() {
        let p = UniformHeight::of(VerticalAnchor::absolute(0), VerticalAnchor::absolute(10));
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..8)
            .map(|_| p.sample(&mut random, &overworld()))
            .collect();
        // Mth.randomBetweenInclusive(random, 0, 10), Paper LCG golden.
        assert_eq!(samples, [6, 6, 6, 10, 1, 10, 7, 6]);
    }

    #[test]
    fn sample_empty_range_returns_min_without_consuming_rng() {
        let p = UniformHeight::of(VerticalAnchor::absolute(10), VerticalAnchor::absolute(0));
        let mut random = LegacyRandomSource::new(1);
        assert_eq!(p.sample(&mut random, &overworld()), 10);
        assert_eq!(random.next_int(), -1155869325);
    }

    #[test]
    fn sample_resolves_anchors_against_the_window() {
        // above_bottom(0) -> -64, below_top(0) -> 319: uniform over the whole
        // overworld column.
        let p = UniformHeight::of(
            VerticalAnchor::above_bottom(0),
            VerticalAnchor::below_top(0),
        );
        let mut random = LegacyRandomSource::new(12345);
        let v = p.sample(&mut random, &overworld());
        assert!((-64..=319).contains(&v));
    }

    #[test]
    fn display_matches_java() {
        let p = UniformHeight::of(VerticalAnchor::absolute(1), VerticalAnchor::absolute(9));
        assert_eq!(p.to_string(), "[1 absolute-9 absolute]");
    }

    #[test]
    fn codec_round_trips_and_encodes_field_shape() {
        let codec = rivet_serialization::map_codec::codec_of(uniform_height_map_codec::<JsonOps>());
        let p = UniformHeight::of(VerticalAnchor::absolute(0), VerticalAnchor::absolute(10));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "min_inclusive": {"absolute": 0},
                "max_inclusive": {"absolute": 10}
            })
        );
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(*decoded, p);
    }
}
