//! Port of `net.minecraft.util.valueproviders.UniformFloat` (record, 26.2).

use crate::RandomSource;
use crate::mth::random_between;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.UniformFloat` — uniform over
/// `[min, max)` (the max is exclusive, matching Java's `Mth.randomBetween`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UniformFloat {
    /// `this.min`.
    min: f32,
    /// `this.max`.
    max: f32,
}

impl UniformFloat {
    /// `UniformFloat.of(float min, float max)`.
    ///
    /// Java throws `IllegalArgumentException("Max must exceed min")` when
    /// `max <= min`; the panic reproduces the unchecked exception exactly.
    pub fn of(min: f32, max: f32) -> UniformFloat {
        if max <= min {
            panic!("Max must exceed min");
        }
        UniformFloat::new(min, max)
    }

    /// The record canonical constructor `new UniformFloat(float min, float
    /// max)` — no validation. The codec applies this and validates separately,
    /// so a `max <= min` input surfaces as a `DataResult.error`, never a panic.
    fn new(min: f32, max: f32) -> UniformFloat {
        UniformFloat { min, max }
    }

    /// `min()` (Java record accessor).
    pub fn min(&self) -> f32 {
        self.min
    }

    /// `max()` (Java record accessor).
    pub fn max(&self) -> f32 {
        self.max
    }

    /// `UniformFloat.sample(RandomSource)` — `Mth.randomBetween(random, min,
    /// max)`.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> f32 {
        random_between(random, self.min, self.max)
    }
}

impl fmt::Display for UniformFloat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `"[" + min + "-" + max + "]"` with Java float
        // stringification.
        write!(
            f,
            "[{}-{}]",
            crate::java_float_format::java_float_to_string(self.min),
            crate::java_float_format::java_float_to_string(self.max)
        )
    }
}

/// `UniformFloat.CODEC` — a record codec over the `"min_inclusive"` /
/// `"max_exclusive"` fields, validated, as the ops-generic
/// `uniform_float_map_codec::<Ops>()` factory.
pub fn uniform_float_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<UniformFloat, Ops>>
{
    let inner = record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|u: &UniformFloat| u.min),
                codec::field_of::<f32, Ops>(
                    codec::float_codec::<Ops>(),
                    "min_inclusive".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|u: &UniformFloat| u.max),
                codec::field_of::<f32, Ops>(
                    codec::float_codec::<Ops>(),
                    "max_exclusive".to_string(),
                ),
            ))
            .apply(instance, Arc::new(UniformFloat::new))
    });
    map_codec::validate(
        inner,
        Arc::new(|u: &UniformFloat| {
            if u.max <= u.min {
                DataResult::error(format!(
                    "Max must be larger than min, min: {}, max: {}",
                    crate::java_float_format::java_float_to_string(u.min),
                    crate::java_float_format::java_float_to_string(u.max)
                ))
            } else {
                DataResult::success(*u)
            }
        }),
    )
}
