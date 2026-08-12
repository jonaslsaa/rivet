//! Port of `net.minecraft.world.level.levelgen.feature.configurations.UnderwaterMagmaConfiguration`
//! (class, 26.2).
//!
//! Java: a three-field value class (`floorSearchRange`, `placementRadiusAroundFloor`,
//! `placementProbabilityPerValidPosition`) whose `CODEC` is a `RecordCodecBuilder`
//! over the `"floor_search_range"` field (`Codec.intRange(0, 512)`), the
//! `"placement_radius_around_floor"` field (`Codec.intRange(0, 64)`), and the
//! `"placement_probability_per_valid_position"` field (`Codec.floatRange(0.0F,
//! 1.0F)`), all required. DFU `Codec<T>` is `Codec<E, Ops>` in the port, so the
//! static Java constant is exposed as the ops-generic
//! `underwater_magma_configuration_codec::<Ops>()` factory.
//!
//! All three fields are public final in Java (mirrored as `pub` fields). The
//! `intRange`/`floatRange` bounds run on both decode and encode, inclusive on
//! both ends, exactly like Java's `Codec.intRange`/`floatRange` flatXMap.
//! Java does not override `equals` (identity semantics); the port derives
//! value-semantic `PartialEq`, consistent with the other configuration value
//! types (identity is only observable through Java `==`, never in codec
//! behavior).

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.UnderwaterMagmaConfiguration`.
#[derive(Debug, Clone, PartialEq)]
pub struct UnderwaterMagmaConfiguration {
    /// `floorSearchRange` — `[0, 512]`.
    pub floor_search_range: i32,
    /// `placementRadiusAroundFloor` — `[0, 64]`.
    pub placement_radius_around_floor: i32,
    /// `placementProbabilityPerValidPosition` — `[0.0F, 1.0F]`.
    pub placement_probability_per_valid_position: f32,
}

impl UnderwaterMagmaConfiguration {
    /// `new UnderwaterMagmaConfiguration(int, int, float)` — the public
    /// constructor (the codec's `apply` function).
    pub fn new(
        floor_search_range: i32,
        placement_radius_around_floor: i32,
        placement_probability_per_valid_position: f32,
    ) -> Self {
        UnderwaterMagmaConfiguration {
            floor_search_range,
            placement_radius_around_floor,
            placement_probability_per_valid_position,
        }
    }
}

/// `UnderwaterMagmaConfiguration.CODEC` — a record codec over the three
/// required range-bounded fields, as the ops-generic
/// `underwater_magma_configuration_codec::<Ops>()` factory.
pub fn underwater_magma_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<UnderwaterMagmaConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &UnderwaterMagmaConfiguration| c.floor_search_range),
                "floor_search_range".to_string(),
                codec::int_range::<Ops>(0, 512),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &UnderwaterMagmaConfiguration| c.placement_radius_around_floor),
                "placement_radius_around_floor".to_string(),
                codec::int_range::<Ops>(0, 64),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &UnderwaterMagmaConfiguration| {
                    c.placement_probability_per_valid_position
                }),
                "placement_probability_per_valid_position".to_string(),
                codec::float_range::<Ops>(0.0, 1.0),
            ))
            .apply(instance, Arc::new(UnderwaterMagmaConfiguration::new))
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration
    for UnderwaterMagmaConfiguration
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_round_trip() {
        let codec = underwater_magma_configuration_codec::<JsonOps>();
        let config = UnderwaterMagmaConfiguration::new(30, 4, 0.5);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "floor_search_range": 30,
                "placement_radius_around_floor": 4,
                "placement_probability_per_valid_position": 0.5,
            })
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_accepts_inclusive_bounds() {
        // intRange/floatRange are inclusive on both ends; both encode and
        // decode validate.
        let codec = underwater_magma_configuration_codec::<JsonOps>();
        let at_min = UnderwaterMagmaConfiguration::new(0, 0, 0.0);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_min)
                .result()
                .is_some()
        );
        let at_max = UnderwaterMagmaConfiguration::new(512, 64, 1.0);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_max)
                .result()
                .is_some()
        );
    }

    #[test]
    fn codec_rejects_out_of_range_on_decode() {
        let codec = underwater_magma_configuration_codec::<JsonOps>();
        // floor_search_range above 512.
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "floor_search_range": 513,
                        "placement_radius_around_floor": 4,
                        "placement_probability_per_valid_position": 0.5,
                    })
                )
                .is_error()
        );
        // floor_search_range negative.
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "floor_search_range": -1,
                        "placement_radius_around_floor": 4,
                        "placement_probability_per_valid_position": 0.5,
                    })
                )
                .is_error()
        );
        // placement_radius_around_floor above 64.
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "floor_search_range": 10,
                        "placement_radius_around_floor": 65,
                        "placement_probability_per_valid_position": 0.5,
                    })
                )
                .is_error()
        );
        // placement_probability above 1.0.
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "floor_search_range": 10,
                        "placement_radius_around_floor": 4,
                        "placement_probability_per_valid_position": 1.5,
                    })
                )
                .is_error()
        );
    }

    #[test]
    fn codec_rejects_out_of_range_on_encode() {
        let codec = underwater_magma_configuration_codec::<JsonOps>();
        let too_big = UnderwaterMagmaConfiguration::new(600, 4, 0.5);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &too_big)
                .result()
                .is_none()
        );
        let bad_prob = UnderwaterMagmaConfiguration::new(10, 4, -0.1);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &bad_prob)
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_rejects_negative_zero_probability_with_java_message() {
        // The `placementProbabilityPerValidPosition` `floatRange(0.0F, 1.0F)`
        // validates with `Float.compare` total order, so `-0.0` is below the
        // inclusive `0.0` lower bound and must fail decode with Java's
        // `Float.toString` message. serde_json preserves the `-0.0` sign bit.
        let codec = underwater_magma_configuration_codec::<JsonOps>();
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({
                "floor_search_range": 10,
                "placement_radius_around_floor": 4,
                "placement_probability_per_valid_position": -0.0,
            }),
        );
        assert!(result.is_error());
        let error_ref = result.error_ref().expect("error");
        let msg = error_ref.message();
        assert!(
            msg.contains("Value -0.0 outside of range [0.0:1.0]"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn codec_requires_all_fields() {
        let codec = underwater_magma_configuration_codec::<JsonOps>();
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "placement_radius_around_floor": 4,
                        "placement_probability_per_valid_position": 0.5,
                    })
                )
                .is_error()
        );
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "floor_search_range": 10,
                        "placement_radius_around_floor": 4,
                    })
                )
                .is_error()
        );
    }
}
