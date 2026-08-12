//! Port of `net.minecraft.world.level.levelgen.feature.configurations.ProbabilityFeatureConfiguration`
//! (class, 26.2).
//!
//! Java: a single-field value class whose `CODEC` is a `RecordCodecBuilder` over
//! the `"probability"` field, `Codec.floatRange(0.0F, 1.0F)`. DFU `Codec<T>` is
//! `Codec<E, Ops>` in the port, so the static Java constant is exposed as the
//! ops-generic `probability_feature_configuration_codec::<Ops>()` factory — the
//! same shape `Rotations.CODEC` takes in `rivet-registry::core::rotations`. The
//! `probability` field is `float` (f32); the `floatRange` bounds check runs on
//! both decode and encode exactly like Java's `Codec.floatRange` flatXMap.
//! Equality is value-semantic (`PartialEq` on the f32 field).

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.ProbabilityFeatureConfiguration`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbabilityFeatureConfiguration {
    /// `probability` — the spawn/density probability in `[0.0, 1.0]`.
    pub probability: f32,
}

impl ProbabilityFeatureConfiguration {
    /// `new ProbabilityFeatureConfiguration(float)`.
    pub fn new(probability: f32) -> Self {
        ProbabilityFeatureConfiguration { probability }
    }
}

/// `ProbabilityFeatureConfiguration.CODEC` — a record codec over the
/// `"probability"` field, `Codec.floatRange(0.0F, 1.0F)`, as the ops-generic
/// `probability_feature_configuration_codec::<Ops>()` factory.
pub fn probability_feature_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ProbabilityFeatureConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &ProbabilityFeatureConfiguration| c.probability),
                "probability".to_string(),
                codec::float_range::<Ops>(0.0, 1.0),
            ))
            .apply(instance, Arc::new(ProbabilityFeatureConfiguration::new))
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration
    for ProbabilityFeatureConfiguration
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_round_trip_within_range() {
        let codec = probability_feature_configuration_codec::<JsonOps>();
        let config = ProbabilityFeatureConfiguration::new(0.5);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"probability": 0.5}));
        let input = json!({"probability": 0.5});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, config);
    }

    #[test]
    fn codec_encodes_bounds() {
        // floatRange is inclusive on both ends; both encode and decode validate.
        let codec = probability_feature_configuration_codec::<JsonOps>();
        let at_min = ProbabilityFeatureConfiguration::new(0.0);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_min)
                .result()
                .is_some()
        );
        let at_max = ProbabilityFeatureConfiguration::new(1.0);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_max)
                .result()
                .is_some()
        );
    }

    #[test]
    fn codec_rejects_out_of_range_on_decode() {
        let codec = probability_feature_configuration_codec::<JsonOps>();
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"probability": 1.5}))
                .is_error()
        );
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"probability": -0.1}))
                .is_error()
        );
    }

    #[test]
    fn codec_rejects_out_of_range_on_encode() {
        let codec = probability_feature_configuration_codec::<JsonOps>();
        let too_high = ProbabilityFeatureConfiguration::new(1.1);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &too_high)
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_rejects_negative_zero_probability_with_java_message() {
        // `Codec.floatRange` validates with `Float.compare` total order, so
        // `-0.0` is below the inclusive `0.0` lower bound and must be rejected
        // on both decode and encode; the diagnostic uses Java's
        // `Float.toString` ("-0.0", "0.0", "1.0"). serde_json preserves the
        // `-0.0` sign bit.
        let codec = probability_feature_configuration_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!({"probability": -0.0}));
        assert!(result.is_error());
        let error_ref = result.error_ref().expect("error");
        let msg = error_ref.message();
        assert!(
            msg.contains("Value -0.0 outside of range [0.0:1.0]"),
            "unexpected message: {msg}"
        );
        let neg_zero = ProbabilityFeatureConfiguration::new(-0.0);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &neg_zero)
                .result()
                .is_none(),
            "-0.0 probability must fail the encode-side range check"
        );
    }
}
