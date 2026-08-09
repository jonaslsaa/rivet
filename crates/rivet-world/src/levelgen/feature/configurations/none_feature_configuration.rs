//! Port of `net.minecraft.world.level.levelgen.feature.configurations.NoneFeatureConfiguration`
//! (class, 26.2).
//!
//! Java: a singleton (`INSTANCE`) whose `CODEC` is `MapCodec.unitCodec(INSTANCE)`
//! — a codec that encodes to an empty map/array and always decodes to the
//! singleton. DFU `Codec<T>` is `Codec<E, Ops>` in the port, so the static
//! Java constant is exposed as the ops-generic `none_feature_configuration_codec::<Ops>()`
//! factory — the same shape `Rotations.CODEC` takes in `rivet-registry::core::rotations`.
//! `NoneFeatureConfiguration` is a unit-like value (no fields), so equality is
//! value-semantic (`PartialEq`), matching the singleton's identity in Java.

use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.NoneFeatureConfiguration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoneFeatureConfiguration;

impl NoneFeatureConfiguration {
    /// `NoneFeatureConfiguration.INSTANCE`.
    pub const INSTANCE: NoneFeatureConfiguration = NoneFeatureConfiguration;
}

/// `NoneFeatureConfiguration.CODEC` — `MapCodec.unitCodec(INSTANCE)`, as the
/// ops-generic `none_feature_configuration_codec::<Ops>()` factory.
pub fn none_feature_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<NoneFeatureConfiguration, Ops>> {
    map_codec::unit_codec(NoneFeatureConfiguration::INSTANCE)
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for NoneFeatureConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_encodes_to_empty_map() {
        // `MapCodec.unitCodec(INSTANCE)` encodes to `{}` (empty map form).
        let codec = none_feature_configuration_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &NoneFeatureConfiguration::INSTANCE)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({}));
    }

    #[test]
    fn codec_decodes_empty_map_to_instance() {
        let codec = none_feature_configuration_codec::<JsonOps>();
        let input = json!({});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, NoneFeatureConfiguration::INSTANCE);
    }

    #[test]
    fn codec_rejects_non_map_input() {
        // Java `UnitCodec.decode` checks the input shape via `getMap` (non-
        // compressed ops) and errors on a non-map.
        let codec = none_feature_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!(42)).is_error());
    }
}
