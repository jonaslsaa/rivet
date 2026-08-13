//! Port of `net.minecraft.world.level.levelgen.GeodeCrackSettings` (26.2) — the
//! `mc.world.level.levelgen.settings` unit.
//!
//! The geode crack three-field record: `generate_crack_chance`
//! (`GeodeConfiguration.CHANCE_RANGE` = `Codec.doubleRange(0.0, 1.0)`, default
//! `1.0`), `base_crack_size` (`Codec.doubleRange(0.0, 5.0)`, default `2.0`),
//! and `crack_point_offset` (`Codec.intRange(0, 10)`, default `2`), all
//! non-lenient optional-with-default fields.
//!
//! `GeodeConfiguration.CHANCE_RANGE` is owned by the pending
//! `mc.world.level.levelgen.feature.configurations.geode` manifest unit (the
//! `GeodeConfiguration` record). This unit only needs the `[0.0, 1.0]` double
//! range, so it is composed inline here; when the geode-config unit lands it
//! should reuse that unit's constant.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.GeodeCrackSettings`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeodeCrackSettings {
    /// `generateCrackChance`.
    pub generate_crack_chance: f64,
    /// `baseCrackSize`.
    pub base_crack_size: f64,
    /// `crackPointOffset`.
    pub crack_point_offset: i32,
}

impl GeodeCrackSettings {
    /// The record constructor (the codec's `apply` function).
    pub const fn new(
        generate_crack_chance: f64,
        base_crack_size: f64,
        crack_point_offset: i32,
    ) -> Self {
        GeodeCrackSettings {
            generate_crack_chance,
            base_crack_size,
            crack_point_offset,
        }
    }
}

/// `GeodeConfiguration.CHANCE_RANGE` — `Codec.doubleRange(0.0, 1.0)` (owned by
/// the pending geode-config unit; composed here until that unit lands).
pub fn chance_range_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<f64, Ops>> {
    codec::double_range::<Ops>(0.0, 1.0)
}

/// `GeodeCrackSettings.CODEC` — the ops-generic
/// `geode_crack_settings_codec::<Ops>()` factory.
pub fn geode_crack_settings_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<GeodeCrackSettings, Ops>> {
    let chance_range = chance_range_codec::<Ops>();
    let base_crack_size = codec::double_range::<Ops>(0.0, 5.0);
    let crack_point_offset = codec::int_range::<Ops>(0, 10);
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|s: &GeodeCrackSettings| s.generate_crack_chance),
                codec::optional_field_of("generate_crack_chance", chance_range, 1.0),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &GeodeCrackSettings| s.base_crack_size),
                codec::optional_field_of("base_crack_size", base_crack_size, 2.0),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &GeodeCrackSettings| s.crack_point_offset),
                codec::optional_field_of("crack_point_offset", crack_point_offset, 2),
            ))
            .apply(instance, Arc::new(GeodeCrackSettings::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_round_trip_and_defaults() {
        let ops = JsonOps::INSTANCE;
        let codec = geode_crack_settings_codec::<JsonOps>();
        let defaults = GeodeCrackSettings::new(1.0, 2.0, 2);
        let encoded = codec
            .encode_start(&ops, &defaults)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, json!({}));
        assert_eq!(
            codec
                .parse(&ops, &encoded)
                .result()
                .expect("decode")
                .clone(),
            defaults
        );

        let custom = GeodeCrackSettings::new(0.5, 3.0, 4);
        let encoded = codec
            .encode_start(&ops, &custom)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"generate_crack_chance": 0.5, "base_crack_size": 3.0, "crack_point_offset": 4})
        );
        assert_eq!(
            codec
                .parse(&ops, &encoded)
                .result()
                .expect("decode")
                .clone(),
            custom
        );
    }

    #[test]
    fn codec_bounds() {
        let ops = JsonOps::INSTANCE;
        let codec = geode_crack_settings_codec::<JsonOps>();
        // `CHANCE_RANGE` = [0.0, 1.0]; `base_crack_size` = [0.0, 5.0];
        // `crack_point_offset` = [0, 10].
        assert!(
            codec
                .parse(&ops, &json!({"generate_crack_chance": 1.5}))
                .result()
                .is_none()
        );
        assert!(
            codec
                .parse(&ops, &json!({"base_crack_size": 5.1}))
                .result()
                .is_none()
        );
        assert!(
            codec
                .parse(&ops, &json!({"crack_point_offset": 11}))
                .result()
                .is_none()
        );
        assert!(
            codec
                .parse(&ops, &json!({"crack_point_offset": -1}))
                .result()
                .is_none()
        );
    }
}
