//! Port of `net.minecraft.world.level.levelgen.feature.configurations.EndSpikeConfiguration`
//! (class, 26.2).
//!
//! Java: a three-field class (`boolean crystalInvulnerable`,
//! `List<EndSpikeFeature.EndSpike> spikes`, `@Nullable BlockPos
//! crystalBeamTarget`) whose `CODEC` is a `RecordCodecBuilder` over the
//! `"crystal_invulnerable"` field (`Codec.BOOL.optionalFieldOf(..., false)` —
//! the NON-lenient with-default optional), the required `"spikes"` field
//! (`EndSpikeFeature.EndSpike.CODEC.listOf()`) and the `"crystal_beam_target"`
//! field (`BlockPos.CODEC.optionalFieldOf(...)` — the NON-lenient absent-ok
//! optional, Java's `optionalFieldOf(name)` two-arg-less form: `Optional`
//! carrier, no default, present-but-invalid is an error). The public
//! constructor takes the nullable target and delegates to the private
//! `Optional`-carrying one. DFU `Codec<T>` is `Codec<E, Ops>` in the port, so
//! the static Java constant is exposed as the ops-generic
//! `end_spike_configuration_codec::<Ops>()` factory.
//!
//! [`EndSpike`] is the out-of-unit value type
//! `net.minecraft.world.level.levelgen.feature.EndSpikeFeature.EndSpike`
//! (owned by the pending `.feature` unit). It is STUB'd here restricted to the
//! surface this configuration consumes (the five `CODEC` fields and the
//! accessors); the top/bottom bounding box and chunk-membership helpers are
//! placement behavior that defers with the owning unit. The `spikes` list is
//! value-semantic (`Vec<EndSpike>`), so the configuration derives
//! `PartialEq`/`Eq` — Java's class does not override `equals`, but the
//! configuration's fields are all value types and the codec round-trip
//! comparison is value-based (consistent with the other configuration value
//! types).

use rivet_registry::core::BlockPos;
use rivet_registry::core::block_pos_codec;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.EndSpikeConfiguration`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndSpikeConfiguration {
    /// `crystalInvulnerable`.
    pub crystal_invulnerable: bool,
    /// `spikes` — the configured end spikes.
    pub spikes: Vec<EndSpike>,
    /// `crystalBeamTarget` — `@Nullable`; `None` when absent.
    pub crystal_beam_target: Option<BlockPos>,
}

impl EndSpikeConfiguration {
    /// `new EndSpikeConfiguration(boolean, List<EndSpike>, @Nullable BlockPos)`
    /// — the public constructor, delegating to the private `Optional` form.
    pub fn new(
        crystal_invulnerable: bool,
        spikes: Vec<EndSpike>,
        crystal_beam_target: Option<BlockPos>,
    ) -> Self {
        EndSpikeConfiguration {
            crystal_invulnerable,
            spikes,
            crystal_beam_target,
        }
    }

    /// `isCrystalInvulnerable()`.
    pub fn is_crystal_invulnerable(&self) -> bool {
        self.crystal_invulnerable
    }

    /// `getSpikes()`.
    pub fn get_spikes(&self) -> &[EndSpike] {
        &self.spikes
    }

    /// `getCrystalBeamTarget()` — `@Nullable`.
    pub fn get_crystal_beam_target(&self) -> Option<&BlockPos> {
        self.crystal_beam_target.as_ref()
    }
}

/// `net.minecraft.world.level.levelgen.feature.EndSpikeFeature.EndSpike`
/// (record, 26.2) — the out-of-unit spike value type this configuration's
/// `"spikes"` field holds.
///
/// STUB(mc.world.level.levelgen.feature.configurations.wave2): owned by the
/// pending `net.minecraft.world.level.levelgen.feature.EndSpikeFeature` unit;
/// this stub carries the value surface this configuration consumes — the five
/// `CODEC` fields (`centerX`, `centerZ`, `radius`, `height` each
/// `Codec.INT.optionalFieldOf(..., 0)`, `guarded`
/// `Codec.BOOL.optionalFieldOf(..., false)`) and their accessors. The
/// `topBoundingBox` (`AABB`) and `isCenterWithinChunk` placement helpers defer
/// with the owning unit (the AABB value type and the `SectionPos`-based chunk
/// membership are placement behavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndSpike {
    /// `centerX`.
    pub center_x: i32,
    /// `centerZ`.
    pub center_z: i32,
    /// `radius`.
    pub radius: i32,
    /// `height`.
    pub height: i32,
    /// `guarded`.
    pub guarded: bool,
}

impl EndSpike {
    /// `new EndSpike(int centerX, int centerZ, int radius, int height, boolean
    /// guarded)` — the record constructor (the codec's `apply` function).
    ///
    /// STUB: Java's constructor also builds `topBoundingBox` (`new AABB(...)`);
    /// that field defers with the owning unit.
    pub fn new(center_x: i32, center_z: i32, radius: i32, height: i32, guarded: bool) -> Self {
        EndSpike {
            center_x,
            center_z,
            radius,
            height,
            guarded,
        }
    }

    /// `getCenterX()`.
    pub fn get_center_x(&self) -> i32 {
        self.center_x
    }

    /// `getCenterZ()`.
    pub fn get_center_z(&self) -> i32 {
        self.center_z
    }

    /// `getRadius()`.
    pub fn get_radius(&self) -> i32 {
        self.radius
    }

    /// `getHeight()`.
    pub fn get_height(&self) -> i32 {
        self.height
    }

    /// `isGuarded()`.
    pub fn is_guarded(&self) -> bool {
        self.guarded
    }
}

/// `EndSpikeFeature.EndSpike.CODEC` — the ops-generic `end_spike_codec::<Ops>()`
/// factory (a record codec over the five with-default optional fields).
pub fn end_spike_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<EndSpike, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|s: &EndSpike| s.center_x),
                codec::optional_field_of::<i32, Ops>("centerX", codec::int_codec::<Ops>(), 0),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &EndSpike| s.center_z),
                codec::optional_field_of::<i32, Ops>("centerZ", codec::int_codec::<Ops>(), 0),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &EndSpike| s.radius),
                codec::optional_field_of::<i32, Ops>("radius", codec::int_codec::<Ops>(), 0),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &EndSpike| s.height),
                codec::optional_field_of::<i32, Ops>("height", codec::int_codec::<Ops>(), 0),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &EndSpike| s.guarded),
                codec::optional_field_of::<bool, Ops>("guarded", codec::bool_codec::<Ops>(), false),
            ))
            .apply(
                instance,
                Arc::new(
                    |center_x: i32, center_z: i32, radius: i32, height: i32, guarded: bool| {
                        EndSpike::new(center_x, center_z, radius, height, guarded)
                    },
                ),
            )
    })
}

/// `EndSpikeConfiguration.CODEC` — a record codec over the
/// `"crystal_invulnerable"` (with-default optional), required `"spikes"` and
/// `"crystal_beam_target"` (absent-ok optional) fields, as the ops-generic
/// `end_spike_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     Codec.BOOL.optionalFieldOf("crystal_invulnerable", false),
///     EndSpikeFeature.EndSpike.CODEC.listOf().fieldOf("spikes"),
///     BlockPos.CODEC.optionalFieldOf("crystal_beam_target"))
///     .apply(i, EndSpikeConfiguration::new))
/// ```
pub fn end_spike_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<EndSpikeConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &EndSpikeConfiguration| c.crystal_invulnerable),
                codec::optional_field_of::<bool, Ops>(
                    "crystal_invulnerable",
                    codec::bool_codec::<Ops>(),
                    false,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &EndSpikeConfiguration| c.spikes.clone()),
                codec::field_of(codec::list(end_spike_codec::<Ops>()), "spikes".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &EndSpikeConfiguration| c.crystal_beam_target),
                // Java's `BlockPos.CODEC.optionalFieldOf("crystal_beam_target")`
                // (single-arg) is the NON-lenient absent-ok form: `Optional`
                // carrier, no default, present-but-invalid is a decode error.
                codec::optional_field(
                    "crystal_beam_target".to_string(),
                    block_pos_codec::<Ops>(),
                    false,
                ),
            ))
            .apply(
                instance,
                Arc::new(
                    |crystal_invulnerable: bool,
                     spikes: Vec<EndSpike>,
                     crystal_beam_target: Option<BlockPos>| {
                        EndSpikeConfiguration::new(
                            crystal_invulnerable,
                            spikes,
                            crystal_beam_target,
                        )
                    },
                ),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for EndSpikeConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn sample_spike() -> EndSpike {
        EndSpike::new(0, 0, 2, 76, true)
    }

    #[test]
    fn end_spike_codec_round_trip() {
        let codec = end_spike_codec::<JsonOps>();
        let spike = sample_spike();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &spike)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "radius": 2,
                "height": 76,
                "guarded": true,
            })
        );
        let result = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, spike);
    }

    #[test]
    fn end_spike_codec_defaults_all_fields() {
        // Every field is `optionalFieldOf(..., default)` — an empty map
        // decodes to the all-default spike (0, 0, 0, 0, false).
        let codec = end_spike_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!({}));
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, EndSpike::new(0, 0, 0, 0, false));
    }

    #[test]
    fn end_spike_codec_omits_defaults_on_encode() {
        // `optionalFieldOf(name, default)` omits a value equal to the default
        // on encode (the non-lenient with-default form).
        let codec = end_spike_codec::<JsonOps>();
        let default = EndSpike::new(0, 0, 0, 0, false);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &default)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({}));
    }

    #[test]
    fn end_spike_codec_rejects_present_malformed_optional() {
        // NON-lenient optional: a present-but-wrong-typed field is an error.
        let codec = end_spike_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!({"centerX": "not an int"}));
        assert!(result.is_error());
    }

    #[test]
    fn codec_round_trip_with_beam_target() {
        let config = EndSpikeConfiguration::new(
            true,
            vec![sample_spike(), EndSpike::new(20, -30, 3, 100, false)],
            Some(BlockPos::new(1, 2, 3)),
        );
        let codec = end_spike_configuration_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "crystal_invulnerable": true,
                "spikes": [
                    {"radius": 2, "height": 76, "guarded": true},
                    {"centerX": 20, "centerZ": -30, "radius": 3, "height": 100},
                ],
                "crystal_beam_target": [1, 2, 3],
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
    fn codec_round_trip_omitted_optionals() {
        // `crystal_invulnerable` omitted (default false) and no beam target —
        // both optional fields stay out of the encoded map.
        let config = EndSpikeConfiguration::new(false, vec![], None);
        let codec = end_spike_configuration_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"spikes": []}));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_requires_spikes_field() {
        // `fieldOf("spikes")` is required — a map without it fails.
        let codec = end_spike_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
        let result = codec.parse(&JsonOps::INSTANCE, &json!({"crystal_invulnerable": false}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key spikes"), "got: {msg}");
    }

    #[test]
    fn codec_rejects_present_malformed_beam_target() {
        // `BlockPos.CODEC.optionalFieldOf("crystal_beam_target")` is NON-lenient:
        // a present-but-malformed target is a decode error.
        let codec = end_spike_configuration_codec::<JsonOps>();
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({"spikes": [], "crystal_beam_target": "not a pos"}),
        );
        assert!(result.is_error());
    }

    #[test]
    fn accessors_expose_the_fields() {
        let config =
            EndSpikeConfiguration::new(true, vec![sample_spike()], Some(BlockPos::new(5, 6, 7)));
        assert!(config.is_crystal_invulnerable());
        assert_eq!(config.get_spikes().len(), 1);
        assert_eq!(
            *config.get_crystal_beam_target().unwrap(),
            BlockPos::new(5, 6, 7)
        );
        // The nullable target is `None` when absent.
        assert_eq!(
            EndSpikeConfiguration::new(false, vec![], None).get_crystal_beam_target(),
            None
        );
    }
}
