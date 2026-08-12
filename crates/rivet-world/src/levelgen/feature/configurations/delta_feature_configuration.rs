//! Port of `net.minecraft.world.level.levelgen.feature.configurations.DeltaFeatureConfiguration`
//! (class, 26.2).
//!
//! Java: a four-field value class (`contents`, `rim`, `size`, `rimSize`) whose
//! `CODEC` is a `RecordCodecBuilder` over the required `"contents"` field
//! (`BlockState.CODEC`), the required `"rim"` field (`BlockState.CODEC`), the
//! required `"size"` field (`IntProviders.codec(0, 16)` — the integer provider
//! dispatch codec validated to the inclusive `[0, 16]` range), and the required
//! `"rim_size"` field (`IntProviders.codec(0, 16)` — validated to the same
//! inclusive `[0, 16]` range). DFU `Codec<T>` is `Codec<E, Ops>` in the port,
//! so the static Java constant is exposed as the ops-generic
//! `delta_feature_configuration_codec::<Ops>()` factory — the same shape the
//! other configuration value types take.
//!
//! All four fields are `private final` in Java, exposed through the `contents()`,
//! `rim()`, `size()`, and `rimSize()` accessors, mirrored as methods over
//! private fields. The bounds validation runs on both decode and encode
//! (Java's `IntProviders.codec` is a `.validate(...)` wrapper around the
//! constant-or-dispatch `CODEC`, exactly like the `codec::validate` used for
//! the `[0, 16]` windows here), with Paper's exact `"Value provider too low"` /
//! `"Value provider too high"` messages. Java does not override `equals`
//! (identity semantics); the port derives value-semantic `PartialEq`,
//! consistent with the other configuration value types.

use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.DeltaFeatureConfiguration`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeltaFeatureConfiguration {
    /// `contents` — the interior block state of the delta.
    contents: BlockState,
    /// `rim` — the block state of the delta's rim.
    rim: BlockState,
    /// `size` — an `IntProvider` validated to the inclusive `[0, 16]` range.
    size: IntProvider,
    /// `rimSize` — an `IntProvider` validated to the inclusive `[0, 16]` range.
    rim_size: IntProvider,
}

impl DeltaFeatureConfiguration {
    /// `new DeltaFeatureConfiguration(BlockState, BlockState, IntProvider,
    /// IntProvider)` — the public constructor (the codec's `apply` function).
    pub fn new(
        contents: BlockState,
        rim: BlockState,
        size: IntProvider,
        rim_size: IntProvider,
    ) -> Self {
        DeltaFeatureConfiguration {
            contents,
            rim,
            size,
            rim_size,
        }
    }

    /// `DeltaFeatureConfiguration.contents()`.
    pub fn contents(&self) -> BlockState {
        self.contents
    }

    /// `DeltaFeatureConfiguration.rim()`.
    pub fn rim(&self) -> BlockState {
        self.rim
    }

    /// `DeltaFeatureConfiguration.size()`.
    pub fn size(&self) -> &IntProvider {
        &self.size
    }

    /// `DeltaFeatureConfiguration.rimSize()`.
    pub fn rim_size(&self) -> &IntProvider {
        &self.rim_size
    }
}

/// `DeltaFeatureConfiguration.CODEC` — a record codec over the two required
/// `BlockState` fields and the two required bound-validated integer provider
/// fields, as the ops-generic `delta_feature_configuration_codec::<Ops>()`
/// factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     BlockState.CODEC.fieldOf("contents").forGetter(c -> c.contents),
///     BlockState.CODEC.fieldOf("rim").forGetter(c -> c.rim),
///     IntProviders.codec(0, 16).fieldOf("size").forGetter(c -> c.size),
///     IntProviders.codec(0, 16).fieldOf("rim_size").forGetter(c -> c.rimSize))
///     .apply(i, DeltaFeatureConfiguration::new))
/// ```
pub fn delta_feature_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<DeltaFeatureConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &DeltaFeatureConfiguration| c.contents),
                "contents".to_string(),
                block_state_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &DeltaFeatureConfiguration| c.rim),
                "rim".to_string(),
                block_state_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &DeltaFeatureConfiguration| c.size.clone()),
                "size".to_string(),
                int_provider_codec_with_bounds::<Ops>(0, 16),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &DeltaFeatureConfiguration| c.rim_size.clone()),
                "rim_size".to_string(),
                int_provider_codec_with_bounds::<Ops>(0, 16),
            ))
            .apply(
                instance,
                Arc::new(
                    |contents: BlockState,
                     rim: BlockState,
                     size: IntProvider,
                     rim_size: IntProvider| {
                        DeltaFeatureConfiguration::new(contents, rim, size, rim_size)
                    },
                ),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for DeltaFeatureConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    fn lava() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:lava").unwrap())
    }

    fn magma_block() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:magma_block").unwrap())
    }

    #[test]
    fn codec_round_trip() {
        // Paper's NetherFeatures.DELTA:
        // `new DeltaFeatureConfiguration(Blocks.LAVA.defaultBlockState(),
        //  Blocks.MAGMA_BLOCK.defaultBlockState(), UniformInt.of(3, 7),
        //  UniformInt.of(0, 2))`.
        let codec = delta_feature_configuration_codec::<JsonOps>();
        let config = DeltaFeatureConfiguration::new(
            lava(),
            magma_block(),
            IntProvider::Uniform(UniformInt::of(3, 7)),
            IntProvider::Uniform(UniformInt::of(0, 2)),
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        // `lava` is a non-singleton block (its `level` property), so the
        // `BlockState` codec emits `Properties` for it; `magma_block` is a
        // singleton and encodes as just the name.
        assert_eq!(
            encoded,
            json!({
                "contents": {"Properties": {"level": "0"}, "Name": "minecraft:lava"},
                "rim": {"Name": "minecraft:magma_block"},
                "size": {"type": "minecraft:uniform", "min_inclusive": 3, "max_inclusive": 7},
                "rim_size": {"type": "minecraft:uniform", "min_inclusive": 0, "max_inclusive": 2}
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
    fn accessors_return_the_fields() {
        let config = DeltaFeatureConfiguration::new(
            lava(),
            magma_block(),
            IntProvider::Uniform(UniformInt::of(3, 7)),
            IntProvider::Uniform(UniformInt::of(0, 2)),
        );
        assert_eq!(config.contents(), lava());
        assert_eq!(config.rim(), magma_block());
        assert_eq!(*config.size(), IntProvider::Uniform(UniformInt::of(3, 7)));
        assert_eq!(
            *config.rim_size(),
            IntProvider::Uniform(UniformInt::of(0, 2))
        );
    }

    #[test]
    fn codec_accepts_providers_within_bounds() {
        // The validated window is inclusive on both ends: `size` and `rim_size`
        // in [0, 16].
        let codec = delta_feature_configuration_codec::<JsonOps>();
        let at_min = DeltaFeatureConfiguration::new(
            lava(),
            magma_block(),
            IntProvider::Uniform(UniformInt::of(0, 0)),
            IntProvider::Uniform(UniformInt::of(0, 0)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_min)
                .result()
                .is_some()
        );
        let at_max = DeltaFeatureConfiguration::new(
            lava(),
            magma_block(),
            IntProvider::Uniform(UniformInt::of(16, 16)),
            IntProvider::Uniform(UniformInt::of(16, 16)),
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
        let codec = delta_feature_configuration_codec::<JsonOps>();
        // size above 16.
        let size_too_high = DeltaFeatureConfiguration::new(
            lava(),
            magma_block(),
            IntProvider::Uniform(UniformInt::of(17, 17)),
            IntProvider::Uniform(UniformInt::of(0, 0)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &size_too_high)
                .result()
                .is_none()
        );
        // rim_size below 0.
        let rim_size_too_low = DeltaFeatureConfiguration::new(
            lava(),
            magma_block(),
            IntProvider::Uniform(UniformInt::of(0, 0)),
            IntProvider::Uniform(UniformInt::of(-1, -1)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &rim_size_too_low)
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_rejects_out_of_bounds_on_decode() {
        let codec = delta_feature_configuration_codec::<JsonOps>();
        // A bare constant `size` of 17 is out of [0, 16].
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "contents": {"Name": "minecraft:lava"},
                        "rim": {"Name": "minecraft:magma_block"},
                        "size": 17,
                        "rim_size": 0
                    })
                )
                .is_error()
        );
        // A bare constant `rim_size` of -1 is out of [0, 16].
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "contents": {"Name": "minecraft:lava"},
                        "rim": {"Name": "minecraft:magma_block"},
                        "size": 3,
                        "rim_size": -1
                    })
                )
                .is_error()
        );
    }

    #[test]
    fn codec_requires_all_fields() {
        let codec = delta_feature_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "contents": {"Name": "minecraft:lava"},
                        "rim": {"Name": "minecraft:magma_block"}
                    })
                )
                .is_error()
        );
        assert!(codec
            .parse(
                &JsonOps::INSTANCE,
                &json!({
                    "contents": {"Name": "minecraft:lava"},
                    "rim": {"Name": "minecraft:magma_block"},
                    "size": {"type": "minecraft:uniform", "min_inclusive": 3, "max_inclusive": 7}
                })
            )
            .is_error());
    }
}
