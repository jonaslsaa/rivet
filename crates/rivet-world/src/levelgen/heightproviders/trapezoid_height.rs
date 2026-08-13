//! Port of `net.minecraft.world.level.levelgen.heightproviders.TrapezoidHeight`
//! (class, 26.2).
//!
//! Java: a provider that samples with a trapezoid distribution between a
//! `minInclusive` and `maxInclusive` anchor, with a `plateau` of constant
//! density (`0` -> a triangle). `sample`:
//!
//! ```java
//! int min = minInclusive.resolveY(context);
//! int max = maxInclusive.resolveY(context);
//! if (min > max) { LOGGER.warn(...); return min; }
//! int range = max - min;
//! if (plateau >= range) return Mth.randomBetweenInclusive(random, min, max);
//! int plateauStart = (range - plateau) / 2;
//! int plateauEnd = range - plateauStart;
//! return min + Mth.randomBetweenInclusive(random, 0, plateauEnd)
//!            + Mth.randomBetweenInclusive(random, 0, plateauStart);
//! ```
//!
//! All arithmetic is Java-int wrapping (including the `/ 2` on a possibly-odd
//! `range - plateau`, which is Java's truncating division — `wrapping_div`).
//! The `LOGGER.warn` is dropped (no-op). The `"plateau"` codec field is
//! `Codec.INT` with a default of `0`.

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

/// `net.minecraft.world.level.levelgen.heightproviders.TrapezoidHeight`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrapezoidHeight {
    /// `this.minInclusive` — the lower anchor (inclusive).
    min_inclusive: VerticalAnchor,
    /// `this.maxInclusive` — the upper anchor (inclusive).
    max_inclusive: VerticalAnchor,
    /// `this.plateau` — the width of the constant-density middle (`0` ->
    /// triangle).
    plateau: i32,
}

impl TrapezoidHeight {
    /// `TrapezoidHeight.of(VerticalAnchor minInclusive, VerticalAnchor
    /// maxInclusive, int plateau)`.
    pub const fn of(
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
        plateau: i32,
    ) -> TrapezoidHeight {
        TrapezoidHeight {
            min_inclusive,
            max_inclusive,
            plateau,
        }
    }

    /// `TrapezoidHeight.of(VerticalAnchor, VerticalAnchor)` — the two-arg
    /// overload, `of(minInclusive, maxInclusive, 0)`.
    pub const fn of_2(
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
    ) -> TrapezoidHeight {
        TrapezoidHeight::of(min_inclusive, max_inclusive, 0)
    }

    /// `TrapezoidHeight.sample(RandomSource, WorldGenerationContext)` — Java-int
    /// wrapping arithmetic (see module doc).
    pub fn sample<R: RandomSource>(&self, random: &mut R, context: &WorldGenerationContext) -> i32 {
        let min = self.min_inclusive.resolve_y(context);
        let max = self.max_inclusive.resolve_y(context);
        if min > max {
            // `LOGGER.warn("Empty height range: {}", this)` — dropped (no-op).
            return min;
        }
        let range = max.wrapping_sub(min);
        if self.plateau >= range {
            return random_between_inclusive(random, min, max);
        }
        let plateau_start = range.wrapping_sub(self.plateau).wrapping_div(2);
        let plateau_end = range.wrapping_sub(plateau_start);
        min.wrapping_add(random_between_inclusive(random, 0, plateau_end))
            .wrapping_add(random_between_inclusive(random, 0, plateau_start))
    }
}

impl fmt::Display for TrapezoidHeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `plateau == 0 ? "triangle (" + min + "-" + max + ")"
        // : "trapezoid(" + plateau + ") in [" + min + "-" + max + "]"`.
        if self.plateau == 0 {
            write!(
                f,
                "triangle ({}-{})",
                self.min_inclusive, self.max_inclusive
            )
        } else {
            write!(
                f,
                "trapezoid({}) in [{}-{}]",
                self.plateau, self.min_inclusive, self.max_inclusive
            )
        }
    }
}

/// `TrapezoidHeight.CODEC` — a record codec over the two anchor fields and the
/// optional `"plateau"` field (`Codec.INT`, default `0`), as the ops-generic
/// `trapezoid_height_map_codec::<Ops>()` factory.
pub fn trapezoid_height_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<TrapezoidHeight, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|t: &TrapezoidHeight| t.min_inclusive),
                codec::field_of::<VerticalAnchor, Ops>(
                    crate::levelgen::vertical_anchor::vertical_anchor_codec::<Ops>(),
                    "min_inclusive".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &TrapezoidHeight| t.max_inclusive),
                codec::field_of::<VerticalAnchor, Ops>(
                    crate::levelgen::vertical_anchor::vertical_anchor_codec::<Ops>(),
                    "max_inclusive".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &TrapezoidHeight| t.plateau),
                // `Codec.INT.optionalFieldOf("plateau", 0)`.
                crate::levelgen::heightproviders::optional_field_of::<i32, Ops>(
                    "plateau",
                    codec::int_codec::<Ops>(),
                    0,
                ),
            ))
            .apply(
                instance,
                Arc::new(|min: VerticalAnchor, max: VerticalAnchor, plateau: i32| {
                    TrapezoidHeight::of(min, max, plateau)
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
    fn triangle_sample_golden() {
        // plateau 0 over [0, 9]: triangle. Golden pinned against Paper's LCG.
        let p = TrapezoidHeight::of(VerticalAnchor::absolute(0), VerticalAnchor::absolute(9), 0);
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..8)
            .map(|_| p.sample(&mut random, &overworld()))
            .collect();
        assert_eq!(samples, [1, 6, 5, 3, 5, 7, 5, 6]);
    }

    #[test]
    fn trapezoid_plateau_golden() {
        // plateau 4 over [0, 9]: range 9 > plateau 4, so the trapezoid path.
        let p = TrapezoidHeight::of(VerticalAnchor::absolute(0), VerticalAnchor::absolute(9), 4);
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..8)
            .map(|_| p.sample(&mut random, &overworld()))
            .collect();
        assert_eq!(samples, [3, 7, 7, 2, 1, 2, 4, 6]);
    }

    #[test]
    fn plateau_gte_range_falls_back_to_uniform() {
        // plateau 20 >= range 9 -> Mth.randomBetweenInclusive(0, 9).
        let p = TrapezoidHeight::of(VerticalAnchor::absolute(0), VerticalAnchor::absolute(9), 20);
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..4)
            .map(|_| p.sample(&mut random, &overworld()))
            .collect();
        assert_eq!(samples, [1, 0, 1, 8]);
    }

    #[test]
    fn sample_empty_range_returns_min() {
        let p = TrapezoidHeight::of(VerticalAnchor::absolute(9), VerticalAnchor::absolute(0), 0);
        let mut random = LegacyRandomSource::new(1);
        assert_eq!(p.sample(&mut random, &overworld()), 9);
        assert_eq!(random.next_int(), -1155869325);
    }

    #[test]
    fn sample_large_range_trapezoid_path_does_not_panic() {
        // min=1, max=i32::MAX: range = 2147483646 (does NOT wrap), plateau 0,
        // so plateau_start = plateau_end = 1073741823 and the final
        // min + a + b caps at exactly 1 + 1073741823 + 1073741823 = i32::MAX —
        // no arithmetic wraps here. The test locks that the trapezoid path
        // runs at extreme magnitudes without panicking; wrapping is not
        // exercised because for min <= max the final sum can never overflow
        // (plateau_end + plateau_start = range, so min + a + b <= min + range
        // = max).
        let p = TrapezoidHeight::of(
            VerticalAnchor::absolute(1),
            VerticalAnchor::absolute(i32::MAX),
            0,
        );
        let mut random = LegacyRandomSource::new(1);
        let _ = p.sample(&mut random, &overworld());
    }

    #[test]
    fn display_matches_java() {
        let triangle =
            TrapezoidHeight::of(VerticalAnchor::absolute(0), VerticalAnchor::absolute(9), 0);
        assert_eq!(triangle.to_string(), "triangle (0 absolute-9 absolute)");
        let trap = TrapezoidHeight::of(VerticalAnchor::absolute(0), VerticalAnchor::absolute(9), 4);
        assert_eq!(trap.to_string(), "trapezoid(4) in [0 absolute-9 absolute]");
    }

    #[test]
    fn two_arg_overload_defaults_plateau_zero() {
        let p = TrapezoidHeight::of_2(VerticalAnchor::absolute(0), VerticalAnchor::absolute(9));
        assert_eq!(
            p,
            TrapezoidHeight::of(VerticalAnchor::absolute(0), VerticalAnchor::absolute(9), 0)
        );
    }

    #[test]
    fn codec_round_trips_and_defaults_plateau() {
        let codec =
            rivet_serialization::map_codec::codec_of(trapezoid_height_map_codec::<JsonOps>());
        let p = TrapezoidHeight::of(VerticalAnchor::absolute(0), VerticalAnchor::absolute(9), 4);
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
                "plateau": 4
            })
        );
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(*decoded, p);
        // The default plateau 0 is omitted on encode.
        let default_p =
            TrapezoidHeight::of(VerticalAnchor::absolute(0), VerticalAnchor::absolute(9), 0);
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
}
