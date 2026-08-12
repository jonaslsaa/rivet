//! Port of `net.minecraft.world.level.levelgen.feature.configurations.ReplaceSphereConfiguration`
//! (class, 26.2).
//!
//! Java: a three-field value class (`targetState`, `replaceState`, `radius`)
//! whose `CODEC` is a `RecordCodecBuilder` over the required `"target"` field
//! (`BlockState.CODEC`), the required `"state"` field (`BlockState.CODEC`), and
//! the required `"radius"` field (`IntProviders.codec(0, 12)` — the integer
//! provider dispatch codec validated to the inclusive `[0, 12]` range). DFU
//! `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java constant is
//! exposed as the ops-generic `replace_sphere_configuration_codec::<Ops>()`
//! factory — the same shape the other configuration value types take.
//!
//! `targetState` and `replaceState` are `public final` in Java, mirrored as
//! public fields; `radius` is `private final`, exposed through the `radius()`
//! accessor, mirrored as a `radius()` method over a private field. The bounds
//! validation runs on both decode and encode (Java's `IntProviders.codec` is a
//! `.validate(...)` wrapper around the constant-or-dispatch `CODEC`, exactly
//! like the `codec::validate` used for the `[0, 12]` window here), with Paper's
//! exact `"Value provider too low"` / `"Value provider too high"` messages.
//! Java does not override `equals` (identity semantics); the port derives
//! value-semantic `PartialEq`, consistent with the other configuration value
//! types.

use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.ReplaceSphereConfiguration`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaceSphereConfiguration {
    /// `targetState` — the block state to target for replacement.
    pub target_state: BlockState,
    /// `replaceState` — the block state to replace with.
    pub replace_state: BlockState,
    /// `radius` — an `IntProvider` validated to the inclusive `[0, 12]` range.
    radius: IntProvider,
}

impl ReplaceSphereConfiguration {
    /// `new ReplaceSphereConfiguration(BlockState, BlockState, IntProvider)` —
    /// the public constructor (the codec's `apply` function).
    pub fn new(target_state: BlockState, replace_state: BlockState, radius: IntProvider) -> Self {
        ReplaceSphereConfiguration {
            target_state,
            replace_state,
            radius,
        }
    }

    /// `ReplaceSphereConfiguration.radius()`.
    pub fn radius(&self) -> &IntProvider {
        &self.radius
    }
}

/// `ReplaceSphereConfiguration.CODEC` — a record codec over the two required
/// `BlockState` fields and the required bound-validated integer provider
/// `"radius"` field, as the ops-generic
/// `replace_sphere_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     BlockState.CODEC.fieldOf("target").forGetter(c -> c.targetState),
///     BlockState.CODEC.fieldOf("state").forGetter(c -> c.replaceState),
///     IntProviders.codec(0, 12).fieldOf("radius").forGetter(c -> c.radius))
///     .apply(i, ReplaceSphereConfiguration::new))
/// ```
pub fn replace_sphere_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ReplaceSphereConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &ReplaceSphereConfiguration| c.target_state),
                "target".to_string(),
                block_state_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &ReplaceSphereConfiguration| c.replace_state),
                "state".to_string(),
                block_state_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &ReplaceSphereConfiguration| c.radius.clone()),
                "radius".to_string(),
                int_provider_codec_with_bounds::<Ops>(0, 12),
            ))
            .apply(
                instance,
                Arc::new(
                    |target: BlockState, state: BlockState, radius: IntProvider| {
                        ReplaceSphereConfiguration::new(target, state, radius)
                    },
                ),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for ReplaceSphereConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:stone").unwrap())
    }

    fn oak_log() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:oak_log").unwrap())
    }

    #[test]
    fn codec_round_trip() {
        let codec = replace_sphere_configuration_codec::<JsonOps>();
        let config = ReplaceSphereConfiguration::new(
            stone(),
            BlockState::of(BlockId::from_name("minecraft:air").unwrap()),
            IntProvider::Constant(ConstantInt::of(3)),
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "target": {"Name": "minecraft:stone"},
                "state": {"Name": "minecraft:air"},
                "radius": 3
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
    fn codec_round_trip_uniform_radius_and_property_state() {
        let codec = replace_sphere_configuration_codec::<JsonOps>();
        // oak_log's default state carries its axis property (the default axis
        // is "y" in the generated table); a uniform radius dispatches through
        // the integer-provider type codec.
        let config = ReplaceSphereConfiguration::new(
            oak_log(),
            stone(),
            IntProvider::Uniform(UniformInt::of(1, 4)),
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "target": {"Name": "minecraft:oak_log", "Properties": {"axis": "y"}},
                "state": {"Name": "minecraft:stone"},
                "radius": {"type": "minecraft:uniform", "min_inclusive": 1, "max_inclusive": 4}
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
        let config = ReplaceSphereConfiguration::new(
            stone(),
            oak_log(),
            IntProvider::Constant(ConstantInt::of(5)),
        );
        assert_eq!(config.target_state, stone());
        assert_eq!(config.replace_state, oak_log());
        assert_eq!(*config.radius(), IntProvider::Constant(ConstantInt::of(5)));
    }

    #[test]
    fn codec_accepts_providers_within_bounds() {
        // The validated window is inclusive on both ends: `radius` in [0, 12].
        let codec = replace_sphere_configuration_codec::<JsonOps>();
        let at_min = ReplaceSphereConfiguration::new(
            stone(),
            stone(),
            IntProvider::Constant(ConstantInt::of(0)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_min)
                .result()
                .is_some()
        );
        let at_max = ReplaceSphereConfiguration::new(
            stone(),
            stone(),
            IntProvider::Constant(ConstantInt::of(12)),
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
        let codec = replace_sphere_configuration_codec::<JsonOps>();
        // radius above 12.
        let radius_too_high = ReplaceSphereConfiguration::new(
            stone(),
            stone(),
            IntProvider::Constant(ConstantInt::of(13)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &radius_too_high)
                .result()
                .is_none()
        );
        // radius below 0.
        let radius_too_low = ReplaceSphereConfiguration::new(
            stone(),
            stone(),
            IntProvider::Constant(ConstantInt::of(-1)),
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &radius_too_low)
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_rejects_out_of_bounds_on_decode() {
        let codec = replace_sphere_configuration_codec::<JsonOps>();
        // A bare constant `radius` of 13 is out of [0, 12].
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "target": {"Name": "minecraft:stone"},
                        "state": {"Name": "minecraft:stone"},
                        "radius": 13
                    })
                )
                .is_error()
        );
        // A bare constant `radius` of -1 is out of [0, 12].
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "target": {"Name": "minecraft:stone"},
                        "state": {"Name": "minecraft:stone"},
                        "radius": -1
                    })
                )
                .is_error()
        );
    }

    #[test]
    fn codec_requires_all_fields() {
        let codec = replace_sphere_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({"target": {"Name": "minecraft:stone"}})
                )
                .is_error()
        );
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "target": {"Name": "minecraft:stone"},
                        "state": {"Name": "minecraft:stone"}
                    })
                )
                .is_error()
        );
    }
}
