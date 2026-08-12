//! Port of `net.minecraft.world.level.levelgen.placement.NoiseThresholdCountPlacement`
//! (class, 26.2).
//!
//! A `RepeatingPlacement` whose count is the below/above threshold branch:
//! `Biome.BIOME_INFO_NOISE.getValue(origin.getX() / 200.0, origin.getZ() /
//! 200.0, false) < noiseLevel ? belowNoise : aboveNoise`. The `200.0` divisor
//! is a hardcoded constant (the threshold variant's fixed scale, unlike
//! `NoiseBasedCountPlacement`'s configurable `noiseFactor`).
//!
//! The `CODEC` is a `RecordCodecBuilder` over the required `"noise_level"`
//! (`Codec.DOUBLE`), `"below_noise"` (`Codec.INT`), and `"above_noise"`
//! (`Codec.INT`) fields. The noise draw is read-only — the only world state
//! these modifiers touch — so `count` stays a pure function of `(origin, this)`.

use crate::biome::BIOME_INFO_NOISE;
use crate::levelgen::placement::placement_modifier_type::{
    PlacementModifierTypeId, PlacementModifierTypes,
};
use crate::levelgen::placement::{PlacementContext, PlacementModifier, RepeatingPlacement};
use rivet_registry::core::BlockPos;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.NoiseThresholdCountPlacement`.
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseThresholdCountPlacement {
    /// `noiseLevel`.
    pub noise_level: f64,
    /// `belowNoise`.
    pub below_noise: i32,
    /// `aboveNoise`.
    pub above_noise: i32,
}

impl NoiseThresholdCountPlacement {
    /// `new NoiseThresholdCountPlacement(double, int, int)` — the private
    /// constructor (the codec's `apply` function).
    pub fn new(noise_level: f64, below_noise: i32, above_noise: i32) -> Self {
        NoiseThresholdCountPlacement {
            noise_level,
            below_noise,
            above_noise,
        }
    }

    /// `NoiseThresholdCountPlacement.of(double, int, int)`.
    pub fn of(noise_level: f64, below_noise: i32, above_noise: i32) -> Self {
        NoiseThresholdCountPlacement::new(noise_level, below_noise, above_noise)
    }
}

impl RepeatingPlacement for NoiseThresholdCountPlacement {
    /// `count(RandomSource, BlockPos)` — the below/above threshold branch on
    /// `BIOME_INFO_NOISE` at the fixed `200.0` scale; `random` is unused
    /// exactly as in Java.
    fn count<R: RandomSource>(&self, _random: &mut R, origin: &BlockPos) -> i32 {
        let flower_noise = BIOME_INFO_NOISE.get_value(
            origin.get_x() as f64 / 200.0,
            origin.get_z() as f64 / 200.0,
            false,
        );
        if flower_noise < self.noise_level {
            self.below_noise
        } else {
            self.above_noise
        }
    }
}

impl PlacementModifier for NoiseThresholdCountPlacement {
    /// `getPositions` — the inherited `RepeatingPlacement` shell.
    fn get_positions<R: RandomSource>(
        &self,
        context: &PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> Vec<BlockPos> {
        RepeatingPlacement::get_positions(self, context, random, origin)
    }

    /// `type()` — `PlacementModifierType.NOISE_THRESHOLD_COUNT` (insertion
    /// index 7 in `PlacementModifierType.java`'s registration order).
    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementModifierTypes::NOISE_THRESHOLD_COUNT
    }
}

/// `NoiseThresholdCountPlacement.CODEC` — a record codec over the required
/// `"noise_level"` (`Codec.DOUBLE`), `"below_noise"` (`Codec.INT`), and
/// `"above_noise"` (`Codec.INT`) fields, as the ops-generic
/// `noise_threshold_count_placement_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.mapCodec(i -> i.group(
///     Codec.DOUBLE.fieldOf("noise_level"),
///     Codec.INT.fieldOf("below_noise"),
///     Codec.INT.fieldOf("above_noise"))
///     .apply(i, NoiseThresholdCountPlacement::new))
/// ```
pub fn noise_threshold_count_placement_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<NoiseThresholdCountPlacement, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &NoiseThresholdCountPlacement| c.noise_level),
                "noise_level".to_string(),
                codec::double_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &NoiseThresholdCountPlacement| c.below_noise),
                "below_noise".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &NoiseThresholdCountPlacement| c.above_noise),
                "above_noise".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|noise_level: f64, below_noise: i32, above_noise: i32| {
                    NoiseThresholdCountPlacement::new(noise_level, below_noise, above_noise)
                }),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn type_identity_is_noise_threshold_count() {
        // `PlacementModifierType.NOISE_THRESHOLD_COUNT` — insertion index 7,
        // "minecraft:noise_threshold_count".
        let placement = NoiseThresholdCountPlacement::of(0.0, 2, 4);
        assert_eq!(
            placement.type_id(),
            PlacementModifierTypeId::new(7, "minecraft:noise_threshold_count")
        );
    }

    #[test]
    fn codec_round_trip() {
        let codec =
            rivet_serialization::map_codec::codec_of(noise_threshold_count_placement_codec::<
                JsonOps,
            >());
        let placement = NoiseThresholdCountPlacement::of(-0.8, 2, 4);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &placement)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"noise_level": -0.8, "below_noise": 2, "above_noise": 4})
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, placement);
    }

    #[test]
    fn codec_requires_all_fields() {
        let codec =
            rivet_serialization::map_codec::codec_of(noise_threshold_count_placement_codec::<
                JsonOps,
            >());
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({"below_noise": 2, "above_noise": 4})
                )
                .is_error()
        );
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({"noise_level": -0.8, "above_noise": 4})
                )
                .is_error()
        );
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({"noise_level": -0.8, "below_noise": 2})
                )
                .is_error()
        );
    }
}
