//! Port of `net.minecraft.util.valueproviders.ClampedNormalInt` (record, 26.2).

use crate::RandomSource;
use crate::mth;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.ClampedNormalInt` — a normal-distribution
/// integer provider clamped to `[min_inclusive, max_inclusive]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClampedNormalInt {
    /// `this.mean`.
    mean: f32,
    /// `this.deviation`.
    deviation: f32,
    /// `this.minInclusive`.
    min_inclusive: i32,
    /// `this.maxInclusive`.
    max_inclusive: i32,
}

impl ClampedNormalInt {
    /// `ClampedNormalInt.of(float mean, float deviation, int minInclusive, int
    /// maxInclusive)`.
    pub const fn of(
        mean: f32,
        deviation: f32,
        min_inclusive: i32,
        max_inclusive: i32,
    ) -> ClampedNormalInt {
        ClampedNormalInt {
            mean,
            deviation,
            min_inclusive,
            max_inclusive,
        }
    }

    /// `mean()` (Java record accessor).
    pub fn mean(&self) -> f32 {
        self.mean
    }

    /// `deviation()` (Java record accessor).
    pub fn deviation(&self) -> f32 {
        self.deviation
    }

    /// `minInclusive()` (Java record accessor).
    pub fn min_inclusive(&self) -> i32 {
        self.min_inclusive
    }

    /// `maxInclusive()` (Java record accessor).
    pub fn max_inclusive(&self) -> i32 {
        self.max_inclusive
    }

    /// `ClampedNormalInt.sample(RandomSource)` — the static
    /// `sample(random, mean, deviation, minInclusive, maxInclusive)` overload.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> i32 {
        Self::sample_with(
            random,
            self.mean,
            self.deviation,
            self.min_inclusive as f32,
            self.max_inclusive as f32,
        )
    }

    /// `ClampedNormalInt.sample(RandomSource, float mean, float deviation,
    /// float minInclusive, float maxInclusive)` — the public static overload,
    /// taking float bounds.
    ///
    /// ```java
    /// return (int)Mth.clamp(Mth.normal(random, mean, deviation), minInclusive, maxInclusive);
    /// ```
    pub fn sample_with<R: RandomSource>(
        random: &mut R,
        mean: f32,
        deviation: f32,
        min_inclusive: f32,
        max_inclusive: f32,
    ) -> i32 {
        let normal = mth::normal(random, mean, deviation);
        let clamped = mth::clamp_f32(normal, min_inclusive, max_inclusive);
        // Java float->int cast saturates and maps NaN to 0 (PORTING.md).
        clamped as i32
    }
}

impl fmt::Display for ClampedNormalInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `"normal(" + mean + ", " + deviation + ") in [" +
        // minInclusive + "-" + maxInclusive + "]"`. Java's float stringification
        // is `Float.toString`.
        write!(
            f,
            "normal({}, {}) in [{}-{}]",
            crate::java_float_format::java_float_to_string(self.mean),
            crate::java_float_format::java_float_to_string(self.deviation),
            self.min_inclusive,
            self.max_inclusive
        )
    }
}

/// `ClampedNormalInt.CODEC` — a record codec over `mean`/`deviation`/the two
/// bounds, validated, as the ops-generic `clamped_normal_int_map_codec::<Ops>()`
/// factory.
pub fn clamped_normal_int_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<ClampedNormalInt, Ops>> {
    let inner = record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedNormalInt| c.mean),
                codec::field_of::<f32, Ops>(codec::float_codec::<Ops>(), "mean".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedNormalInt| c.deviation),
                codec::field_of::<f32, Ops>(codec::float_codec::<Ops>(), "deviation".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedNormalInt| c.min_inclusive),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "min_inclusive".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedNormalInt| c.max_inclusive),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "max_inclusive".to_string()),
            ))
            .apply(
                instance,
                Arc::new(|mean: f32, dev: f32, min: i32, max: i32| {
                    ClampedNormalInt::of(mean, dev, min, max)
                }),
            )
    });
    map_codec::validate(
        inner,
        Arc::new(|c: &ClampedNormalInt| {
            if c.max_inclusive < c.min_inclusive {
                DataResult::error(format!(
                    "Max must be larger than min: [{}, {}]",
                    c.min_inclusive, c.max_inclusive
                ))
            } else {
                DataResult::success(*c)
            }
        }),
    )
}
