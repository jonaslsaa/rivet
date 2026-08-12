//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! NoiseBasedStateProvider` (abstract class, 26.2) — the shared noise-backed
//! base of the noise state providers.
//!
//! Java is the abstract base holding the `seed`/`parameters`/`scale` fields and
//! the lazily-built `NormalNoise` (`NormalNoise.create(new WorldgenRandom(new
//! LegacyRandomSource(seed)), parameters)`), plus `getNoiseValue(pos, scale)`.
//!
//! The Rust port provides the shared helpers the concrete noise providers
//! reuse: the Java-exact `POSITIVE_FLOAT` validation codec (built locally —
//! rivet-serialization has no `POSITIVE_FLOAT` equivalent), the `NormalNoise`
//! construction, and the noise-value read. `Codec.floatRange` (the
//! `NoiseThresholdProvider` `threshold`/`high_chance` fields) comes straight
//! from `rivet_serialization::codec::float_range`, which since the merged
//! `Codec.floatRange` total-order change (#557) matches DFU exactly.

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
/// Kept local to this unit: rivet-serialization has no `POSITIVE_FLOAT`
/// equivalent, so the Java-exact validation is built here with
/// `java_float_to_string` (Java `Float.toString`) for the value in the message.
/// (`Codec.floatRange`, the distinct DFU range codec the `threshold`/
/// `high_chance` fields use, is not built locally — it comes from
/// `rivet_serialization::codec::float_range`, which is DFU-exact since the
/// merged total-order change.)
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
