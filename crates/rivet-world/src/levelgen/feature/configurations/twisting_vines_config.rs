//! Port of `net.minecraft.world.level.levelgen.feature.configurations.TwistingVinesConfig`
//! (record, 26.2).
//!
//! Java: a record `record TwistingVinesConfig(int spreadWidth, int spreadHeight,
//! int maxHeight)` whose `CODEC` is a `RecordCodecBuilder` over the
//! `"spread_width"`/`"spread_height"`/`"max_height"` fields, each
//! `ExtraCodecs.POSITIVE_INT` (validated to `[1, MAX]`, error `"Value must be
//! positive: {n}"`), all required. DFU `Codec<T>` is `Codec<E, Ops>` in the
//! port, so the static Java constant is exposed as the ops-generic
//! `twisting_vines_config_codec::<Ops>()` factory.
//!
//! `POSITIVE_INT` lives in `rivet-util::extra_codecs` (`ExtraCodecs` maps to
//! `rivet-util`); its bounds run on both decode and encode. The record
//! accessors (`spreadWidth()`, …) are mirrored as field accessors; equality is
//! value-semantic on the three fields (`PartialEq`), matching the record's
//! `equals`.

use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::positive_int;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.TwistingVinesConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TwistingVinesConfig {
    /// `spreadWidth` — `[1, MAX]`.
    pub spread_width: i32,
    /// `spreadHeight` — `[1, MAX]`.
    pub spread_height: i32,
    /// `maxHeight` — `[1, MAX]`.
    pub max_height: i32,
}

impl TwistingVinesConfig {
    /// `new TwistingVinesConfig(int, int, int)` — the record constructor (the
    /// codec's `apply` function).
    pub fn new(spread_width: i32, spread_height: i32, max_height: i32) -> Self {
        TwistingVinesConfig {
            spread_width,
            spread_height,
            max_height,
        }
    }

    /// `TwistingVinesConfig.spreadWidth()`.
    pub fn spread_width(&self) -> i32 {
        self.spread_width
    }

    /// `TwistingVinesConfig.spreadHeight()`.
    pub fn spread_height(&self) -> i32 {
        self.spread_height
    }

    /// `TwistingVinesConfig.maxHeight()`.
    pub fn max_height(&self) -> i32 {
        self.max_height
    }
}

/// `TwistingVinesConfig.CODEC` — a record codec over the three
/// `POSITIVE_INT`-validated fields, as the ops-generic
/// `twisting_vines_config_codec::<Ops>()` factory.
pub fn twisting_vines_config_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<TwistingVinesConfig, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &TwistingVinesConfig| c.spread_width),
                "spread_width".to_string(),
                positive_int::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &TwistingVinesConfig| c.spread_height),
                "spread_height".to_string(),
                positive_int::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &TwistingVinesConfig| c.max_height),
                "max_height".to_string(),
                positive_int::<Ops>(),
            ))
            .apply(instance, Arc::new(TwistingVinesConfig::new))
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for TwistingVinesConfig {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_round_trip() {
        let codec = twisting_vines_config_codec::<JsonOps>();
        let config = TwistingVinesConfig::new(8, 4, 6);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "spread_width": 8,
                "spread_height": 4,
                "max_height": 6,
            })
        );
        let decoded = *codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed");
        assert_eq!(decoded, config);
    }

    #[test]
    fn accessors_return_the_fields() {
        let config = TwistingVinesConfig::new(8, 4, 6);
        assert_eq!(config.spread_width(), 8);
        assert_eq!(config.spread_height(), 4);
        assert_eq!(config.max_height(), 6);
    }

    #[test]
    fn codec_rejects_non_positive_on_decode() {
        // POSITIVE_INT is `[1, MAX]`: 0 and negative values are errors, with
        // the Java-exact message `"Value must be positive: {n}"`.
        let codec = twisting_vines_config_codec::<JsonOps>();
        let zero = json!({
            "spread_width": 0,
            "spread_height": 4,
            "max_height": 6,
        });
        let result = codec.parse(&JsonOps::INSTANCE, &zero);
        assert!(result.is_error());
        let negative = json!({
            "spread_width": 8,
            "spread_height": -1,
            "max_height": 6,
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &negative).is_error());
    }

    #[test]
    fn codec_rejects_non_positive_on_encode() {
        let codec = twisting_vines_config_codec::<JsonOps>();
        let bad = TwistingVinesConfig::new(0, 4, 6);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &bad)
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_requires_all_fields() {
        let codec = twisting_vines_config_codec::<JsonOps>();
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({"spread_width": 8, "spread_height": 4})
                )
                .is_error()
        );
    }
}
