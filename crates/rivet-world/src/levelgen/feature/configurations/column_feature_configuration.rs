//! Port of `net.minecraft.world.level.levelgen.feature.configurations.ColumnFeatureConfiguration`
//! (class, 26.2).
//!
//! Java: a two-field value class (`reach`, `height`) whose `CODEC` is a
//! `RecordCodecBuilder` over the required `"reach"` field
//! (`IntProviders.codec(0, 3)` — the integer provider dispatch codec validated
//! to the inclusive `[0, 3]` range) and the required `"height"` field
//! (`IntProviders.codec(1, 10)` — validated to the inclusive `[1, 10]` range).
//! DFU `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java constant
//! is exposed as the ops-generic `column_feature_configuration_codec::<Ops>()`
//! factory — the same shape the other configuration value types take.
//!
//! Both fields are `private final` in Java, exposed through the `reach()` and
//! `height()` accessors, mirrored as `reach()`/`height()` methods over private
//! fields. The bounds validation runs on both decode and encode (Java's
//! `IntProviders.codec` is a `.validate(...)` wrapper around the
//! constant-or-dispatch `CODEC`, exactly like the `codec::validate` used for
//! the `[0, 3]`/`[1, 10]` windows here), with Paper's exact
//! `"Value provider too low"` / `"Value provider too high"` messages. Java does
//! not override `equals` (identity semantics); the port derives value-semantic
//! `PartialEq`, consistent with the other configuration value types.

use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.ColumnFeatureConfiguration`.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnFeatureConfiguration {
    /// `reach` — an `IntProvider` validated to the inclusive `[0, 3]` range.
    reach: IntProvider,
    /// `height` — an `IntProvider` validated to the inclusive `[1, 10]` range.
    height: IntProvider,
}

impl ColumnFeatureConfiguration {
    /// `new ColumnFeatureConfiguration(IntProvider, IntProvider)` — the public
    /// constructor (the codec's `apply` function).
    pub fn new(reach: IntProvider, height: IntProvider) -> Self {
        ColumnFeatureConfiguration { reach, height }
    }

    /// `ColumnFeatureConfiguration.reach()`.
    pub fn reach(&self) -> &IntProvider {
        &self.reach
    }

    /// `ColumnFeatureConfiguration.height()`.
    pub fn height(&self) -> &IntProvider {
        &self.height
    }
}

/// `ColumnFeatureConfiguration.CODEC` — a record codec over the two required
/// bound-validated integer provider fields, as the ops-generic
/// `column_feature_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     IntProviders.codec(0, 3).fieldOf("reach"),
///     IntProviders.codec(1, 10).fieldOf("height"))
///     .apply(i, ColumnFeatureConfiguration::new))
/// ```
pub fn column_feature_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ColumnFeatureConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &ColumnFeatureConfiguration| c.reach.clone()),
                "reach".to_string(),
                int_provider_codec_with_bounds::<Ops>(0, 3),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &ColumnFeatureConfiguration| c.height.clone()),
                "height".to_string(),
                int_provider_codec_with_bounds::<Ops>(1, 10),
            ))
            .apply(
                instance,
                Arc::new(|reach: IntProvider, height: IntProvider| {
                    ColumnFeatureConfiguration::new(reach, height)
                }),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for ColumnFeatureConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    #[test]
    fn codec_round_trip() {
        let codec = column_feature_configuration_codec::<JsonOps>();
        // Paper's NetherFeatures.SMALL_BASALT_COLUMNS:
        // `new ColumnFeatureConfiguration(ConstantInt.of(1), UniformInt.of(1, 4))`.
        let config = ColumnFeatureConfiguration::new(
            IntProvider::Constant(ConstantInt::of(1)),
            IntProvider::Uniform(UniformInt::of(1, 4)),
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"reach": 1, "height": {"type": "minecraft:uniform", "min_inclusive": 1, "max_inclusive": 4}})
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn accessors_return_the_fields() {
        let config = ColumnFeatureConfiguration::new(
            IntProvider::Constant(ConstantInt::of(1)),
            IntProvider::Uniform(UniformInt::of(1, 4)),
        );
        assert_eq!(*config.reach(), IntProvider::Constant(ConstantInt::of(1)));
        assert_eq!(*config.height(), IntProvider::Uniform(UniformInt::of(1, 4)));
    }

    #[test]
    fn codec_accepts_providers_within_bounds() {
        // The validated window is inclusive on both ends: `reach` in [0, 3],
        // `height` in [1, 10].
        let codec = column_feature_configuration_codec::<JsonOps>();
        let at_min = ColumnFeatureConfiguration::new(
            IntProvider::Constant(ConstantInt::of(0)),
            IntProvider::Constant(ConstantInt::of(1)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_min)
                .result()
                .is_some()
        );
        let at_max = ColumnFeatureConfiguration::new(
            IntProvider::Constant(ConstantInt::of(3)),
            IntProvider::Constant(ConstantInt::of(10)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_max)
                .result()
                .is_some()
        );
    }

    #[test]
    fn codec_rejects_out_of_bounds_on_encode() {
        let codec = column_feature_configuration_codec::<JsonOps>();
        // reach above 3.
        let reach_too_high = ColumnFeatureConfiguration::new(
            IntProvider::Constant(ConstantInt::of(4)),
            IntProvider::Constant(ConstantInt::of(2)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &reach_too_high)
                .result()
                .is_none()
        );
        // reach below 0.
        let reach_too_low = ColumnFeatureConfiguration::new(
            IntProvider::Constant(ConstantInt::of(-1)),
            IntProvider::Constant(ConstantInt::of(2)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &reach_too_low)
                .result()
                .is_none()
        );
        // height above 10.
        let height_too_high = ColumnFeatureConfiguration::new(
            IntProvider::Constant(ConstantInt::of(1)),
            IntProvider::Constant(ConstantInt::of(11)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &height_too_high)
                .result()
                .is_none()
        );
        // height below 1.
        let height_too_low = ColumnFeatureConfiguration::new(
            IntProvider::Constant(ConstantInt::of(1)),
            IntProvider::Constant(ConstantInt::of(0)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &height_too_low)
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_rejects_out_of_bounds_on_decode() {
        let codec = column_feature_configuration_codec::<JsonOps>();
        // A bare constant `reach` of 4 is out of [0, 3].
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"reach": 4, "height": 2}))
                .is_error()
        );
        // A bare constant `height` of 11 is out of [1, 10].
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"reach": 1, "height": 11}))
                .is_error()
        );
    }

    #[test]
    fn codec_requires_all_fields() {
        let codec = column_feature_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"reach": 1}))
                .is_error()
        );
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"height": 2}))
                .is_error()
        );
    }
}
