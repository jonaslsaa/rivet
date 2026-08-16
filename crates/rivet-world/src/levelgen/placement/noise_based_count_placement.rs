//! Port of `net.minecraft.world.level.levelgen.placement.NoiseBasedCountPlacement`
//! (class, 26.2).
//!
//! A `RepeatingPlacement` whose count is
//! `(int)Math.ceil((Biome.BIOME_INFO_NOISE.getValue(origin.getX() /
//! noiseFactor, origin.getZ() / noiseFactor, false) + noiseOffset) *
//! noiseToCountRatio)` — the sampled `PerlinSimplexNoise` value scaled by the
//! count ratio. The `CODEC` is a `RecordCodecBuilder` over the required
//! `"noise_to_count_ratio"` (`Codec.INT`), `"noise_factor"` (`Codec.DOUBLE`),
//! and the optional-with-default `"noise_offset"` (`Codec.DOUBLE.optionalFieldOf(
//! "noise_offset", 0.0)` — Java's STRICT two-arg form: absent decodes to `0.0`,
//! a present-but-malformed value propagates the element parse error, and only a
//! value bit-equal to `0.0` (via `doubleToLongBits`; `-0.0` is distinct) is
//! omitted on encode).
//!
//! `noiseFactor` is a `double` in Java, so `origin.getX() / noiseFactor` is a
//! `double` division (no int truncation); `noiseToCountRatio` is an `int`
//! multiplied into the `double` sum before `Math.ceil`'s exact f64 `(int)`
//! cast (Java's float/double→int casts saturate, matching Rust's `as`).
//! The noise draw is read-only — the only world state these modifiers touch —
//! so `count` stays a pure function of `(origin, this)`. The
//! optional-with-default `"noise_offset"` field is the serialization crate's
//! generic `Codec.optionalFieldOf` capability
//! (`rivet_serialization::codec::optional_field_of`), not a
//! placement-modifier concern.

use crate::biome::biome::BIOME_INFO_NOISE;
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

/// `net.minecraft.world.level.levelgen.placement.NoiseBasedCountPlacement`.
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseBasedCountPlacement {
    /// `noiseToCountRatio`.
    pub noise_to_count_ratio: i32,
    /// `noiseFactor`.
    pub noise_factor: f64,
    /// `noiseOffset`.
    pub noise_offset: f64,
}

impl NoiseBasedCountPlacement {
    /// `new NoiseBasedCountPlacement(int, double, double)` — the private
    /// constructor (the codec's `apply` function).
    pub fn new(noise_to_count_ratio: i32, noise_factor: f64, noise_offset: f64) -> Self {
        NoiseBasedCountPlacement {
            noise_to_count_ratio,
            noise_factor,
            noise_offset,
        }
    }

    /// `NoiseBasedCountPlacement.of(int, double, double)`.
    pub fn of(noise_to_count_ratio: i32, noise_factor: f64, noise_offset: f64) -> Self {
        NoiseBasedCountPlacement::new(noise_to_count_ratio, noise_factor, noise_offset)
    }
}

impl RepeatingPlacement for NoiseBasedCountPlacement {
    /// `count(RandomSource, BlockPos)` — the `BIOME_INFO_NOISE`-sampled count;
    /// `random` is unused exactly as in Java.
    fn count<R: RandomSource>(&self, _random: &mut R, origin: &BlockPos) -> i32 {
        let flower_noise = BIOME_INFO_NOISE.get_value(
            origin.get_x() as f64 / self.noise_factor,
            origin.get_z() as f64 / self.noise_factor,
            false,
        );
        ((flower_noise + self.noise_offset) * self.noise_to_count_ratio as f64).ceil() as i32
    }
}

impl PlacementModifier for NoiseBasedCountPlacement {
    /// `getPositions` — the inherited `RepeatingPlacement` shell (lazy).
    fn get_positions<'a, R: RandomSource>(
        &'a self,
        context: &mut PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
        RepeatingPlacement::get_positions(self, context, random, origin)
    }

    /// `type()` — `PlacementModifierType.NOISE_BASED_COUNT` (insertion index 6
    /// in `PlacementModifierType.java`'s registration order).
    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementModifierTypes::NOISE_BASED_COUNT
    }
}

/// `NoiseBasedCountPlacement.CODEC` — a record codec over the required
/// `"noise_to_count_ratio"` (`Codec.INT`), required `"noise_factor"`
/// (`Codec.DOUBLE`), and the optional-with-default `"noise_offset"`
/// (`Codec.DOUBLE.optionalFieldOf("noise_offset", 0.0)` via the serialization
/// crate's generic `Codec.optionalFieldOf` — see
/// `rivet_serialization::codec::optional_field_of`), as the ops-generic
/// `noise_based_count_placement_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.mapCodec(i -> i.group(
///     Codec.INT.fieldOf("noise_to_count_ratio"),
///     Codec.DOUBLE.fieldOf("noise_factor"),
///     Codec.DOUBLE.optionalFieldOf("noise_offset", 0.0))
///     .apply(i, NoiseBasedCountPlacement::new))
/// ```
pub fn noise_based_count_placement_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<NoiseBasedCountPlacement, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &NoiseBasedCountPlacement| c.noise_to_count_ratio),
                "noise_to_count_ratio".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &NoiseBasedCountPlacement| c.noise_factor),
                "noise_factor".to_string(),
                codec::double_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &NoiseBasedCountPlacement| c.noise_offset),
                // `Codec.DOUBLE.optionalFieldOf("noise_offset", 0.0)` — the
                // STRICT two-arg form: absent decodes to 0.0, a
                // present-but-malformed value propagates the parse error, and
                // only a `doubleToLongBits`-equal 0.0 (not -0.0) is omitted on
                // encode.
                codec::optional_field_of("noise_offset", codec::double_codec::<Ops>(), 0.0),
            ))
            .apply(
                instance,
                Arc::new(
                    |noise_to_count_ratio: i32, noise_factor: f64, noise_offset: f64| {
                        NoiseBasedCountPlacement::new(
                            noise_to_count_ratio,
                            noise_factor,
                            noise_offset,
                        )
                    },
                ),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn type_identity_is_noise_based_count() {
        // `PlacementModifierType.NOISE_BASED_COUNT` — insertion index 6,
        // "minecraft:noise_based_count".
        let placement = NoiseBasedCountPlacement::of(10, 0.01, 0.0);
        assert_eq!(
            placement.type_id(),
            PlacementModifierTypeId::new(6, "minecraft:noise_based_count")
        );
    }

    #[test]
    fn count_matches_paper_goldens() {
        // `count(RandomSource, BlockPos)` — the full `(int)Math.ceil((
        // BIOME_INFO_NOISE.getValue(x / noiseFactor, z / noiseFactor, false) +
        // noiseOffset) * noiseToCountRatio)` arithmetic. `random` is unused.
        // The expected values are frozen snapshots captured from the pinned
        // Paper 26.2 runtime (`26.2-DEV-main@0a99345`) with an ad-hoc
        // development-time `PlacementCountProbe` (the runtime's protected
        // `count` invoked reflectively) that was NOT retained in the repo.
        // They pin the f64-division, `ceil`, and saturating-cast behavior
        // against real Paper but cannot be regenerated or independently
        // re-verified from the tree.
        // RivetTodo(#567): commit a reproducible oracle probe for these goldens.
        let cases: [(i32, i32, i32, f64, f64, i32); 10] = [
            // (x, z, noiseToCountRatio, noiseFactor, noiseOffset, expected)
            (8, 8, 10, 0.01, 0.0, -8),
            (8, 8, 10, 3.0, 0.0, -9),
            (-8, -8, 10, 0.01, 0.0, 3),
            (8, 8, -3, 0.01, 0.0, 3),
            (8, 8, 0, 0.01, 0.0, 0),
            (8, 8, 5, 0.01, -5.0, -29),
            (8, 8, 10, 0.01, 0.5, -3),
            (8, 8, 10, 1.0e6, 0.0, 1),
            (123, -456, 7, 3.5, -0.25, -3),
            // Java's `(int)` cast saturates: `(flowerNoise + 2.0) * i32::MAX`
            // overflows the f64→i32 range, so Paper yields i32::MAX.
            (8, 8, i32::MAX, 0.01, 2.0, i32::MAX),
        ];
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        for (x, z, ratio, factor, offset, expected) in cases {
            let placement = NoiseBasedCountPlacement::of(ratio, factor, offset);
            let origin = BlockPos::new(x, 0, z);
            assert_eq!(
                placement.count(&mut random, &origin),
                expected,
                "count at ({x}, 0, {z}) with ratio {ratio}, factor {factor}, offset {offset}"
            );
        }
    }

    #[test]
    fn codec_round_trip_all_fields() {
        let codec = rivet_serialization::map_codec::codec_of(noise_based_count_placement_codec::<
            JsonOps,
        >());
        let placement = NoiseBasedCountPlacement::of(10, 0.05, 0.5);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &placement)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"noise_to_count_ratio": 10, "noise_factor": 0.05, "noise_offset": 0.5})
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, placement);
    }

    #[test]
    fn codec_defaults_noise_offset_to_zero() {
        // The strict `optionalFieldOf("noise_offset", 0.0)` — absent decodes
        // to 0.0 (identical in strict and lenient forms).
        let codec = rivet_serialization::map_codec::codec_of(noise_based_count_placement_codec::<
            JsonOps,
        >());
        let decoded = codec
            .parse(
                &JsonOps::INSTANCE,
                &json!({"noise_to_count_ratio": 10, "noise_factor": 0.05}),
            )
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.noise_offset, 0.0);
    }

    #[test]
    fn codec_omits_zero_noise_offset_on_encode() {
        // The strict optional-with-default xmap omits the field when it is
        // bit-equal to the default (`doubleToLongBits`; only `0.0` exactly).
        let codec = rivet_serialization::map_codec::codec_of(noise_based_count_placement_codec::<
            JsonOps,
        >());
        let placement = NoiseBasedCountPlacement::of(10, 0.05, 0.0);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &placement)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"noise_to_count_ratio": 10, "noise_factor": 0.05})
        );
    }

    #[test]
    fn codec_rejects_malformed_noise_offset_on_decode() {
        // The STRICT `optionalFieldOf("noise_offset", 0.0)` — a present-but-
        // malformed value propagates the `Codec.DOUBLE` parse error (the
        // lenient form would silently fall back to 0.0).
        let codec = rivet_serialization::map_codec::codec_of(noise_based_count_placement_codec::<
            JsonOps,
        >());
        let input = json!({"noise_to_count_ratio": 10, "noise_factor": 0.05,
                           "noise_offset": "not-a-double"});
        assert!(
            codec.parse(&JsonOps::INSTANCE, &input).is_error(),
            "malformed noise_offset must fail strict decode"
        );
    }

    #[test]
    fn codec_keeps_negative_zero_noise_offset_on_encode() {
        // Java's `Objects.equals(Double, Double)` uses `doubleToLongBits`, so
        // `-0.0` is distinct from the `0.0` default and is NOT omitted on
        // encode — a Java round-trip of `-0.0` preserves `-0.0`.
        let codec = rivet_serialization::map_codec::codec_of(noise_based_count_placement_codec::<
            JsonOps,
        >());
        let placement = NoiseBasedCountPlacement::of(10, 0.05, -0.0);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &placement)
            .result()
            .expect("encode should succeed")
            .clone();
        // `serde_json::Value` equality treats `-0.0 == 0.0`, so comparing the
        // whole map cannot detect sign loss — the field value itself must keep
        // the `-0.0` sign bit.
        let encoded_offset = &encoded["noise_offset"];
        assert!(
            encoded_offset.as_f64().is_some_and(f64::is_sign_negative),
            "encode must preserve the -0.0 noise_offset sign bit, got {encoded_offset:?}"
        );
        // The other two required fields are unaffected by the sign check.
        assert_eq!(encoded["noise_to_count_ratio"], json!(10));
        assert_eq!(encoded["noise_factor"], json!(0.05));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.noise_offset.to_bits(), (-0.0f64).to_bits());
    }

    #[test]
    fn codec_requires_noise_to_count_ratio_and_factor() {
        let codec = rivet_serialization::map_codec::codec_of(noise_based_count_placement_codec::<
            JsonOps,
        >());
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"noise_factor": 0.05}))
                .is_error()
        );
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"noise_to_count_ratio": 10}))
                .is_error()
        );
    }
}
