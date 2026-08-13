//! Port of `net.minecraft.world.level.levelgen.GeodeLayerSettings` (26.2) — the
//! `mc.world.level.levelgen.settings` unit.
//!
//! The four geode layer thicknesses (`filling`/`inner_layer`/`middle_layer`/
//! `outer_layer`), each a `LAYER_RANGE` = `Codec.doubleRange(0.01, 50.0)`
//! non-lenient optional-with-default field.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.GeodeLayerSettings`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeodeLayerSettings {
    /// `filling`.
    pub filling: f64,
    /// `innerLayer`.
    pub inner_layer: f64,
    /// `middleLayer`.
    pub middle_layer: f64,
    /// `outerLayer`.
    pub outer_layer: f64,
}

impl GeodeLayerSettings {
    /// The record constructor (the codec's `apply` function).
    pub const fn new(filling: f64, inner_layer: f64, middle_layer: f64, outer_layer: f64) -> Self {
        GeodeLayerSettings {
            filling,
            inner_layer,
            middle_layer,
            outer_layer,
        }
    }
}

/// `GeodeLayerSettings.CODEC` — the ops-generic
/// `geode_layer_settings_codec::<Ops>()` factory.
///
/// All four fields are `LAYER_RANGE` (`Codec.doubleRange(0.01, 50.0)`)
/// non-lenient optional-with-default fields (`filling` 1.7, `inner_layer` 2.2,
/// `middle_layer` 3.2, `outer_layer` 4.2). The `f64` `JavaEquals` comparison on
/// encode matches Java's `Objects.equals` bit equality (so `-0.0` differs from
/// `0.0`).
pub fn geode_layer_settings_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<GeodeLayerSettings, Ops>> {
    let layer_range = codec::double_range::<Ops>(0.01, 50.0);
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|s: &GeodeLayerSettings| s.filling),
                codec::optional_field_of("filling", layer_range.clone(), 1.7),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &GeodeLayerSettings| s.inner_layer),
                codec::optional_field_of("inner_layer", layer_range.clone(), 2.2),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &GeodeLayerSettings| s.middle_layer),
                codec::optional_field_of("middle_layer", layer_range.clone(), 3.2),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &GeodeLayerSettings| s.outer_layer),
                codec::optional_field_of("outer_layer", layer_range.clone(), 4.2),
            ))
            .apply(instance, Arc::new(GeodeLayerSettings::new))
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
        let codec = geode_layer_settings_codec::<JsonOps>();
        let settings = GeodeLayerSettings::new(1.7, 2.2, 3.2, 4.2);
        // Encode: equal-to-default fields are omitted (`optionalFieldOf`).
        let encoded = codec
            .encode_start(&ops, &settings)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, json!({}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, settings);

        let custom = GeodeLayerSettings::new(1.0, 2.5, 3.0, 5.0);
        let encoded = codec
            .encode_start(&ops, &custom)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"filling": 1.0, "inner_layer": 2.5, "middle_layer": 3.0, "outer_layer": 5.0})
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
    fn codec_rejects_out_of_range() {
        let ops = JsonOps::INSTANCE;
        let codec = geode_layer_settings_codec::<JsonOps>();
        // `doubleRange(0.01, 50.0)` — 0.005 is below the bound.
        assert!(
            codec
                .parse(&ops, &json!({"filling": 0.005}))
                .result()
                .is_none()
        );
        assert!(
            codec
                .parse(&ops, &json!({"outer_layer": 50.1}))
                .result()
                .is_none()
        );
        // A present-but-malformed value is a decode error (non-lenient).
        assert!(
            codec
                .parse(&ops, &json!({"middle_layer": "x"}))
                .result()
                .is_none()
        );
    }
}
