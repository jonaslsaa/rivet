//! Port of `net.minecraft.world.level.levelgen.feature.configurations.LayerConfiguration`
//! (class, 26.2).
//!
//! Java: a two-field value class (`height`, `state`) whose `CODEC` is a
//! `RecordCodecBuilder` over the required `"height"` field
//! (`Codec.intRange(0, DimensionType.Y_SIZE)` — the #388 `Y_SIZE` constant)
//! and the required `"state"` field (`BlockState.CODEC`, the #391 `"Name"`
//! dispatch). DFU `Codec<T>` is `Codec<E, Ops>` in the port, so the static
//! Java constant is exposed as the ops-generic `layer_configuration_codec::<Ops>()`
//! factory. Equality is value-semantic (`PartialEq` on the field pair).

use crate::level::dimension::Y_SIZE;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.LayerConfiguration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerConfiguration {
    /// `height` — the layer thickness in `[0, DimensionType.Y_SIZE]`.
    pub height: i32,
    /// `state` — the layer's block state.
    pub state: BlockState,
}

impl LayerConfiguration {
    /// `new LayerConfiguration(int, BlockState)`.
    pub fn new(height: i32, state: BlockState) -> Self {
        LayerConfiguration { height, state }
    }
}

/// `LayerConfiguration.CODEC` — a record codec over the required `"height"`
/// (int-range `[0, Y_SIZE]`) and `"state"` fields, as the ops-generic
/// `layer_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     Codec.intRange(0, DimensionType.Y_SIZE).fieldOf("height"),
///     BlockState.CODEC.fieldOf("state"))
///     .apply(i, LayerConfiguration::new))
/// ```
pub fn layer_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<LayerConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &LayerConfiguration| c.height),
                "height".to_string(),
                codec::int_range::<Ops>(0, Y_SIZE),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &LayerConfiguration| c.state),
                codec::field_of(block_state_codec::<Ops>(), "state".to_string()),
            ))
            .apply(
                instance,
                Arc::new(|height: i32, state: BlockState| LayerConfiguration::new(height, state)),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for LayerConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_round_trip() {
        let codec = layer_configuration_codec::<JsonOps>();
        let config = LayerConfiguration::new(
            3,
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"height": 3, "state": {"Name": "minecraft:stone"}})
        );
        let decoded = *codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed");
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_requires_both_fields() {
        let codec = layer_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({"state": {"Name": "minecraft:stone"}})
                )
                .is_error()
        );
    }

    #[test]
    fn codec_rejects_height_outside_y_size() {
        // intRange(0, Y_SIZE) is inclusive; 4064 (= Y_SIZE) is valid, Y_SIZE+1
        // is out of range on both decode and encode.
        let codec = layer_configuration_codec::<JsonOps>();
        let at_max = LayerConfiguration::new(
            Y_SIZE,
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_max)
                .result()
                .is_some()
        );
        let over = json!({"height": Y_SIZE + 1, "state": {"Name": "minecraft:stone"}});
        assert!(codec.parse(&JsonOps::INSTANCE, &over).is_error());
        let negative = json!({"height": -1, "state": {"Name": "minecraft:stone"}});
        assert!(codec.parse(&JsonOps::INSTANCE, &negative).is_error());
    }
}
