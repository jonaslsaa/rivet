//! Port of `net.minecraft.world.level.levelgen.feature.configurations.CountConfiguration`
//! (class, 26.2).
//!
//! Java: a single-field value class wrapping the `IntProvider` count; its
//! `CODEC` is `IntProviders.codec(0, 256).fieldOf("count").xmap(
//! CountConfiguration::new, CountConfiguration::count).codec()` — the
//! `"count"` field (a required `IntProvider` through the validated
//! `IntProviders.codec(0, 256)` constant-or-dispatch codec, the #181
//! dispatch surface) mapped onto the wrapper value type. The validation runs on
//! both decode and encode, exactly like `Codec.validate`'s `flatXmap`.
//! DFU `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java constant
//! is exposed as the ops-generic `count_configuration_codec::<Ops>()` factory.
//! Equality is value-semantic (`PartialEq` on the wrapped provider, the
//! `IntProvider` enum's documented value-equality convention).

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_util::valueproviders::constant_int::ConstantInt;
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.CountConfiguration`.
#[derive(Debug, Clone, PartialEq)]
pub struct CountConfiguration {
    /// `count` — the configured count provider.
    pub count: IntProvider,
}

impl CountConfiguration {
    /// `new CountConfiguration(IntProvider)` — the provider constructor (the
    /// codec's `apply` function, matching `CountConfiguration::new` in the
    /// `xmap`).
    pub fn new(count: IntProvider) -> Self {
        CountConfiguration { count }
    }

    /// `new CountConfiguration(int)` — wraps the constant via `ConstantInt.of`,
    /// exactly like Java's int constructor.
    pub fn new_with_value(count: i32) -> Self {
        CountConfiguration {
            count: IntProvider::Constant(ConstantInt::of(count)),
        }
    }

    /// `CountConfiguration.count()`.
    pub fn count(&self) -> IntProvider {
        self.count.clone()
    }
}

/// `CountConfiguration.CODEC` — `IntProviders.codec(0, 256)` as the required
/// `"count"` field, mapped onto the wrapper, as the ops-generic
/// `count_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// IntProviders.codec(0, 256).fieldOf("count")
///     .xmap(CountConfiguration::new, CountConfiguration::count)
///     .codec()
/// ```
pub fn count_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<CountConfiguration, Ops>> {
    let count_field: Arc<dyn MapCodec<IntProvider, Ops>> = codec::field_of(
        int_provider_codec_with_bounds::<Ops>(0, 256),
        "count".to_string(),
    );
    map_codec::codec_of(map_codec::xmap(
        count_field,
        Arc::new(|count: &IntProvider| CountConfiguration::new(count.clone())),
        Arc::new(|c: &CountConfiguration| c.count()),
    ))
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for CountConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    #[test]
    fn int_constructor_wraps_constant_int() {
        // `new CountConfiguration(int)` sets `count = ConstantInt.of(count)`.
        let config = CountConfiguration::new_with_value(7);
        assert_eq!(config.count(), IntProvider::Constant(ConstantInt::of(7)));
    }

    #[test]
    fn provider_constructor_keeps_provider() {
        let provider = IntProvider::Uniform(UniformInt::of(0, 3));
        let config = CountConfiguration::new(provider.clone());
        assert_eq!(config.count(), provider);
    }

    #[test]
    fn codec_round_trip_constant_count() {
        // A constant `IntProvider` encodes through the constant-or-dispatch
        // codec as a bare int, inside the `"count"` field.
        let codec = count_configuration_codec::<JsonOps>();
        let config = CountConfiguration::new_with_value(5);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"count": 5}));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_round_trip_dispatch_provider() {
        // A non-constant provider encodes through the discriminated
        // `IntProviders` dispatch inside the `"count"` field.
        let codec = count_configuration_codec::<JsonOps>();
        let config = CountConfiguration::new(IntProvider::Uniform(UniformInt::of(0, 3)));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"count": {"min_inclusive": 0, "max_inclusive": 3,
                              "type": "minecraft:uniform"}})
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_requires_count_field() {
        let codec = count_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
    }

    #[test]
    fn codec_accepts_provider_at_upper_bound() {
        // `IntProviders.codec(0, 256)` is inclusive on both ends.
        let codec = count_configuration_codec::<JsonOps>();
        let input = json!({"count": {"min_inclusive": 0, "max_inclusive": 256,
                                     "type": "minecraft:uniform"}});
        assert!(codec.parse(&JsonOps::INSTANCE, &input).is_success());
    }

    #[test]
    fn codec_rejects_too_high_provider_on_decode() {
        let codec = count_configuration_codec::<JsonOps>();
        let err = codec
            .parse(
                &JsonOps::INSTANCE,
                &json!({"count": {"min_inclusive": 0, "max_inclusive": 300,
                                  "type": "minecraft:uniform"}}),
            )
            .error_ref()
            .map(|e| e.message().to_string());
        assert!(
            err.as_deref()
                .unwrap_or_default()
                .contains("Value provider too high: 256"),
            "decode error should surface the bounds message, got: {err:?}"
        );
    }

    #[test]
    fn codec_rejects_too_low_provider_on_decode() {
        let codec = count_configuration_codec::<JsonOps>();
        let err = codec
            .parse(&JsonOps::INSTANCE, &json!({"count": -1}))
            .error_ref()
            .map(|e| e.message().to_string());
        assert!(
            err.as_deref()
                .unwrap_or_default()
                .contains("Value provider too low: 0"),
            "decode error should surface the bounds message, got: {err:?}"
        );
    }

    #[test]
    fn codec_rejects_out_of_range_on_encode() {
        // `Codec.validate` (via `flatXmap`) validates on encode too.
        let codec = count_configuration_codec::<JsonOps>();
        let too_high = CountConfiguration::new(IntProvider::Uniform(UniformInt::of(0, 300)));
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &too_high)
                .result()
                .is_none()
        );
        let too_low = CountConfiguration::new_with_value(-1);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &too_low)
                .result()
                .is_none()
        );
    }
}
