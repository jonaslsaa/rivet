//! Port of `net.minecraft.world.level.levelgen.heightproviders.VeryBiasedToBottomHeight`
//! (class, 26.2).
//!
//! Java: a provider that samples between a `minInclusive` and `maxInclusive`
//! anchor, more strongly biased toward the bottom than
//! `BiasedToBottomHeight` by `inner` (`>= 1`). `sample`:
//!
//! ```java
//! int min = minInclusive.resolveY(context);
//! int max = maxInclusive.resolveY(context);
//! if (max - min - inner + 1 <= 0) { LOGGER.warn(...); return min; }
//! int upperInclusive = Mth.nextInt(random, min + inner, max);
//! int biasedUpperInclusive = Mth.nextInt(random, min, upperInclusive - 1);
//! return Mth.nextInt(random, min, biasedUpperInclusive - 1 + inner);
//! ```
//!
//! All arithmetic is Java-int wrapping; `Mth.nextInt` is `rivet_util::mth::next_int`.
//! The `LOGGER.warn` is dropped (no-op). The `"inner"` codec field is
//! `Codec.intRange(1, Integer.MAX_VALUE)` with a default of `1`.

use crate::levelgen::vertical_anchor::VerticalAnchor;
use crate::levelgen::world_generation_context::WorldGenerationContext;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::mth::next_int;
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.heightproviders.VeryBiasedToBottomHeight`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VeryBiasedToBottomHeight {
    /// `this.minInclusive` — the lower anchor (inclusive).
    min_inclusive: VerticalAnchor,
    /// `this.maxInclusive` — the upper anchor (inclusive).
    max_inclusive: VerticalAnchor,
    /// `this.inner` — the bias offset (`>= 1`).
    inner: i32,
}

impl VeryBiasedToBottomHeight {
    /// `VeryBiasedToBottomHeight.of(VerticalAnchor minInclusive, VerticalAnchor
    /// maxInclusive, int offset)`.
    pub const fn of(
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
        offset: i32,
    ) -> VeryBiasedToBottomHeight {
        VeryBiasedToBottomHeight {
            min_inclusive,
            max_inclusive,
            inner: offset,
        }
    }

    /// `VeryBiasedToBottomHeight.sample(RandomSource, WorldGenerationContext)` —
    /// Java-int wrapping arithmetic (see module doc).
    pub fn sample<R: RandomSource>(&self, random: &mut R, context: &WorldGenerationContext) -> i32 {
        let min = self.min_inclusive.resolve_y(context);
        let max = self.max_inclusive.resolve_y(context);
        if max
            .wrapping_sub(min)
            .wrapping_sub(self.inner)
            .wrapping_add(1)
            <= 0
        {
            // `LOGGER.warn("Empty height range: {}", this)` — dropped (no-op).
            min
        } else {
            let upper_inclusive = next_int(random, min.wrapping_add(self.inner), max);
            let biased_upper_inclusive = next_int(random, min, upper_inclusive.wrapping_sub(1));
            next_int(
                random,
                min,
                biased_upper_inclusive
                    .wrapping_sub(1)
                    .wrapping_add(self.inner),
            )
        }
    }
}

impl fmt::Display for VeryBiasedToBottomHeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `"biased[" + min + "-" + max + " inner: " + inner + "]"`.
        write!(
            f,
            "biased[{}-{} inner: {}]",
            self.min_inclusive, self.max_inclusive, self.inner
        )
    }
}

/// `VeryBiasedToBottomHeight.CODEC` — a record codec over the two anchor fields
/// and the optional `"inner"` field (`Codec.intRange(1, Integer.MAX_VALUE)`,
/// default `1`), as the ops-generic
/// `very_biased_to_bottom_height_map_codec::<Ops>()` factory.
pub fn very_biased_to_bottom_height_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<VeryBiasedToBottomHeight, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|v: &VeryBiasedToBottomHeight| v.min_inclusive),
                codec::field_of::<VerticalAnchor, Ops>(
                    crate::levelgen::vertical_anchor::vertical_anchor_codec::<Ops>(),
                    "min_inclusive".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|v: &VeryBiasedToBottomHeight| v.max_inclusive),
                codec::field_of::<VerticalAnchor, Ops>(
                    crate::levelgen::vertical_anchor::vertical_anchor_codec::<Ops>(),
                    "max_inclusive".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|v: &VeryBiasedToBottomHeight| v.inner),
                // `Codec.intRange(1, Integer.MAX_VALUE).optionalFieldOf("inner", 1)`.
                crate::levelgen::heightproviders::optional_field_of::<i32, Ops>(
                    "inner",
                    codec::int_range::<Ops>(1, i32::MAX),
                    1,
                ),
            ))
            .apply(
                instance,
                Arc::new(|min: VerticalAnchor, max: VerticalAnchor, inner: i32| {
                    VeryBiasedToBottomHeight::of(min, max, inner)
                }),
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
    fn sample_very_biased_golden() {
        let p = VeryBiasedToBottomHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(9),
            1,
        );
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..8)
            .map(|_| p.sample(&mut random, &overworld()))
            .collect();
        assert_eq!(samples, [0, 2, 0, 0, 0, 0, 5, 1]);
    }

    #[test]
    fn sample_empty_range_returns_min() {
        let p = VeryBiasedToBottomHeight::of(
            VerticalAnchor::absolute(5),
            VerticalAnchor::absolute(5),
            1,
        );
        let mut random = LegacyRandomSource::new(1);
        assert_eq!(p.sample(&mut random, &overworld()), 5);
        assert_eq!(random.next_int(), -1155869325);
    }

    #[test]
    fn sample_wraps_on_extreme_bounds() {
        let p = VeryBiasedToBottomHeight::of(
            VerticalAnchor::absolute(i32::MAX),
            VerticalAnchor::absolute(i32::MIN),
            1,
        );
        let mut random = LegacyRandomSource::new(1);
        assert_eq!(p.sample(&mut random, &overworld()), i32::MAX);
    }

    #[test]
    fn display_matches_java() {
        let p = VeryBiasedToBottomHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(9),
            1,
        );
        assert_eq!(p.to_string(), "biased[0 absolute-9 absolute inner: 1]");
    }

    #[test]
    fn codec_round_trips_and_defaults_inner() {
        let codec =
            rivet_serialization::map_codec::codec_of(very_biased_to_bottom_height_map_codec::<
                JsonOps,
            >());
        let p = VeryBiasedToBottomHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(9),
            3,
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "min_inclusive": {"absolute": 0},
                "max_inclusive": {"absolute": 9},
                "inner": 3
            })
        );
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(*decoded, p);
        // The default `inner` is omitted on encode (value-equal to 1).
        let default_p = VeryBiasedToBottomHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(9),
            1,
        );
        let encoded_default = codec
            .encode_start(&JsonOps::INSTANCE, &default_p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded_default,
            json!({
                "min_inclusive": {"absolute": 0},
                "max_inclusive": {"absolute": 9}
            })
        );
        let decoded_default_result = codec.parse(&JsonOps::INSTANCE, &encoded_default);
        let decoded_default = decoded_default_result
            .result()
            .expect("decode should succeed");
        assert_eq!(*decoded_default, default_p);
    }

    #[test]
    fn codec_rejects_zero_inner() {
        // `Codec.intRange(1, Integer.MAX_VALUE)` rejects 0 on both decode and
        // encode.
        let codec =
            rivet_serialization::map_codec::codec_of(very_biased_to_bottom_height_map_codec::<
                JsonOps,
            >());
        let input = json!({
            "min_inclusive": {"absolute": 0},
            "max_inclusive": {"absolute": 9},
            "inner": 0
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &input).is_error());
        let zero_inner = VeryBiasedToBottomHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(9),
            0,
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &zero_inner)
                .is_error()
        );
    }
}
