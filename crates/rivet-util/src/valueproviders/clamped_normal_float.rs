//! Port of `net.minecraft.util.valueproviders.ClampedNormalFloat` (record, 26.2).

use crate::RandomSource;
use crate::mth;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.ClampedNormalFloat` — a normal-distribution
/// float provider clamped to `[min, max]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClampedNormalFloat {
    /// `this.mean`.
    mean: f32,
    /// `this.deviation`.
    deviation: f32,
    /// `this.min`.
    min: f32,
    /// `this.max`.
    max: f32,
}

impl ClampedNormalFloat {
    /// `ClampedNormalFloat.of(float mean, float deviation, float min, float
    /// max)`.
    pub const fn of(mean: f32, deviation: f32, min: f32, max: f32) -> ClampedNormalFloat {
        ClampedNormalFloat {
            mean,
            deviation,
            min,
            max,
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

    /// `min()` (Java record accessor).
    pub fn min(&self) -> f32 {
        self.min
    }

    /// `max()` (Java record accessor).
    pub fn max(&self) -> f32 {
        self.max
    }

    /// `ClampedNormalFloat.sample(RandomSource)` — the static
    /// `sample(random, mean, deviation, min, max)` overload.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> f32 {
        Self::sample_with(random, self.mean, self.deviation, self.min, self.max)
    }

    /// `ClampedNormalFloat.sample(RandomSource, float mean, float deviation,
    /// float min, float max)` — the public static overload.
    ///
    /// ```java
    /// return Mth.clamp(Mth.normal(random, mean, deviation), min, max);
    /// ```
    pub fn sample_with<R: RandomSource>(
        random: &mut R,
        mean: f32,
        deviation: f32,
        min: f32,
        max: f32,
    ) -> f32 {
        mth::clamp_f32(mth::normal(random, mean, deviation), min, max)
    }
}

impl fmt::Display for ClampedNormalFloat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `"normal(" + mean + ", " + deviation + ") in [" + min +
        // "-" + max + "]"` with Java float stringification.
        write!(
            f,
            "normal({}, {}) in [{}-{}]",
            crate::java_float_format::java_float_to_string(self.mean),
            crate::java_float_format::java_float_to_string(self.deviation),
            crate::java_float_format::java_float_to_string(self.min),
            crate::java_float_format::java_float_to_string(self.max)
        )
    }
}

/// `ClampedNormalFloat.CODEC` — a record codec over `mean`/`deviation`/`min`/
/// `max`, validated, as the ops-generic `clamped_normal_float_map_codec::<Ops>()`
/// factory.
pub fn clamped_normal_float_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<ClampedNormalFloat, Ops>> {
    let inner = record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedNormalFloat| c.mean),
                codec::field_of::<f32, Ops>(codec::float_codec::<Ops>(), "mean".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedNormalFloat| c.deviation),
                codec::field_of::<f32, Ops>(codec::float_codec::<Ops>(), "deviation".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedNormalFloat| c.min),
                codec::field_of::<f32, Ops>(codec::float_codec::<Ops>(), "min".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedNormalFloat| c.max),
                codec::field_of::<f32, Ops>(codec::float_codec::<Ops>(), "max".to_string()),
            ))
            .apply(
                instance,
                Arc::new(|mean: f32, dev: f32, min: f32, max: f32| {
                    ClampedNormalFloat::of(mean, dev, min, max)
                }),
            )
    });
    map_codec::validate(
        inner,
        Arc::new(|c: &ClampedNormalFloat| {
            if c.max < c.min {
                DataResult::error(format!(
                    "Max must be larger than min: [{}, {}]",
                    crate::java_float_format::java_float_to_string(c.min),
                    crate::java_float_format::java_float_to_string(c.max)
                ))
            } else {
                DataResult::success(*c)
            }
        }),
    )
}
