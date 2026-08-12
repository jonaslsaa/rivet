//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! NoiseBasedStateProvider` (abstract class, 26.2) — the shared noise-backed
//! base of the noise state providers.
//!
//! Java is the abstract base holding the `seed`/`parameters`/`scale` fields and
//! the lazily-built `NormalNoise` (`NormalNoise.create(new WorldgenRandom(new
//! LegacyRandomSource(seed)), parameters)`), plus `getNoiseValue(pos, scale)`.
//!
//! The Rust port provides the shared helpers the concrete noise providers
//! reuse: the Java-exact `POSITIVE_FLOAT`/`Codec.floatRange` validation codecs
//! (built locally because rivet-serialization's `float_range` diverges from
//! Java's message — see [`positive_float`]), the `NormalNoise` construction,
//! and the noise-value read.

use crate::levelgen::synth::normal_noise::{NoiseParameters, NormalNoise};
use rivet_registry::core::BlockPos;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::random::LegacyRandomSource;
use rivet_util::worldgen_random::WorldgenRandom;
use std::sync::Arc;

/// `ExtraCodecs.POSITIVE_FLOAT` — `Codec.FLOAT.validate(value -> value > 0.0F
/// && value <= Float.MAX_VALUE ? success : error("Value must be positive: " +
/// value))`.
///
/// Kept local to this unit per the unit brief: rivet-serialization's
/// `float_range` diverges from Java's message, so the Java-exact validation is
/// rebuilt here with `java_float_to_string` (Java `Float.toString`) for the
/// value in the message.
pub(crate) fn positive_float<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<f32, Ops>> {
    codec::validate(
        codec::float_codec::<Ops>(),
        Arc::new(|value: &f32| {
            if *value > 0.0 && *value <= f32::MAX {
                DataResult::success(*value)
            } else {
                DataResult::error(format!(
                    "Value must be positive: {}",
                    rivet_serialization::float_format::java_float_to_string(*value)
                ))
            }
        }),
    )
}

/// `Codec.floatRange(float, float)` — Java's inclusive-both-ends range
/// validation with the message `"Value must be within range [min;max]: n"`.
///
/// Built locally (same rationale as [`positive_float`]): rivet-serialization's
/// `float_range` uses a different message shape (`"Value {} outside of range
/// [{}:{}]"`), so the Java-exact form is rebuilt here.
pub(crate) fn float_range<Ops: DynamicOps + 'static>(
    min_inclusive: f32,
    max_inclusive: f32,
) -> Arc<dyn Codec<f32, Ops>> {
    codec::validate(
        codec::float_codec::<Ops>(),
        Arc::new(move |value: &f32| {
            if *value >= min_inclusive && *value <= max_inclusive {
                DataResult::success(*value)
            } else {
                DataResult::error(format!(
                    "Value must be within range [{};{}]: {}",
                    rivet_serialization::float_format::java_float_to_string(min_inclusive),
                    rivet_serialization::float_format::java_float_to_string(max_inclusive),
                    rivet_serialization::float_format::java_float_to_string(*value)
                ))
            }
        }),
    )
}

/// `this.noise` — build the `NormalNoise` from the seed: Java
/// `NormalNoise.create(new WorldgenRandom(new LegacyRandomSource(seed)),
/// parameters)`.
pub(crate) fn build_noise(seed: i64, parameters: &NoiseParameters) -> NormalNoise {
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(seed));
    NormalNoise::create(&mut random, parameters.clone())
}

/// `getNoiseValue(BlockPos, double scale)` — `this.noise.getValue(pos.getX() *
/// scale, pos.getY() * scale, pos.getZ() * scale)`.
pub(crate) fn get_noise_value(noise: &NormalNoise, pos: &BlockPos, scale: f64) -> f64 {
    noise.get_value(
        pos.get_x() as f64 * scale,
        pos.get_y() as f64 * scale,
        pos.get_z() as f64 * scale,
    )
}
