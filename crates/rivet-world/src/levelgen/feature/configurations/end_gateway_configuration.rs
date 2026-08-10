//! Port of `net.minecraft.world.level.levelgen.feature.configurations.EndGatewayConfiguration`
//! (class, 26.2).
//!
//! Java: a two-field value class (the exit `Optional<BlockPos>` and the `exact`
//! flag) whose `CODEC` is a `RecordCodecBuilder` over the `"exit"` field
//! (`BlockPos.CODEC.optionalFieldOf("exit")`, absent → empty optional) and the
//! `"exact"` field (`Codec.BOOL`, required). The constructor is `private`;
//! instances are produced by the `knownExit(BlockPos, boolean)` and
//! `delayedExitSearch()` factories. DFU `Codec<T>` is `Codec<E, Ops>` in the
//! port, so the static Java constant is exposed as the ops-generic
//! `end_gateway_configuration_codec::<Ops>()` factory.
//!
//! The `exit` optional field is non-lenient (`optionalFieldOf(String)` is
//! `optionalField(name, false)`), so a present-but-invalid `exit` is an error
//! and an absent `exit` decodes to `None`. Java does not override `equals`
//! (identity semantics); the port derives value-semantic `PartialEq` on the
//! `(exit, exact)` pair, consistent with the other configuration value types
//! (identity is only observable through Java `==`, never in codec behavior).

use rivet_registry::core::BlockPos;
use rivet_registry::core::block_pos_codec;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.EndGatewayConfiguration`.
#[derive(Debug, Clone, PartialEq)]
pub struct EndGatewayConfiguration {
    /// `exit` — the known exit position; `None` when the exit is searched on
    /// placement (`Optional.empty()` in Java).
    pub exit: Option<BlockPos>,
    /// `exact` — whether the exit position is used exactly as given.
    pub exact: bool,
}

impl EndGatewayConfiguration {
    /// `EndGatewayConfiguration.knownExit(BlockPos, boolean)`.
    pub fn known_exit(exit: BlockPos, exact: bool) -> Self {
        EndGatewayConfiguration {
            exit: Some(exit),
            exact,
        }
    }

    /// `EndGatewayConfiguration.delayedExitSearch()` — `new
    /// EndGatewayConfiguration(Optional.empty(), false)`.
    pub fn delayed_exit_search() -> Self {
        EndGatewayConfiguration {
            exit: None,
            exact: false,
        }
    }

    /// `EndGatewayConfiguration.getExit()`.
    pub fn get_exit(&self) -> Option<BlockPos> {
        self.exit
    }

    /// `EndGatewayConfiguration.isExitExact()`.
    pub fn is_exit_exact(&self) -> bool {
        self.exact
    }
}

/// `EndGatewayConfiguration.CODEC` — a record codec over the optional `"exit"`
/// field (`BlockPos.CODEC.optionalFieldOf`, non-lenient) and the required
/// `"exact"` field (`Codec.BOOL`), as the ops-generic
/// `end_gateway_configuration_codec::<Ops>()` factory.
pub fn end_gateway_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<EndGatewayConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &EndGatewayConfiguration| c.exit),
                codec::optional_field(
                    "exit".to_string(),
                    block_pos_codec::<Ops>(),
                    // `optionalFieldOf(String)` is `optionalField(name, false)`.
                    false,
                ),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &EndGatewayConfiguration| c.exact),
                "exact".to_string(),
                codec::bool_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(
                    |exit: Option<BlockPos>, exact: bool| EndGatewayConfiguration { exit, exact },
                ),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for EndGatewayConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn factories_produce_the_expected_values() {
        let pos = BlockPos::new(1, -60, 3);
        let known = EndGatewayConfiguration::known_exit(pos, true);
        assert_eq!(known.get_exit(), Some(pos));
        assert!(known.is_exit_exact());

        let delayed = EndGatewayConfiguration::delayed_exit_search();
        assert_eq!(delayed.get_exit(), None);
        assert!(!delayed.is_exit_exact());
    }

    #[test]
    fn codec_round_trip_known_exit() {
        let codec = end_gateway_configuration_codec::<JsonOps>();
        let config = EndGatewayConfiguration::known_exit(BlockPos::new(1, -60, 3), true);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"exit": [1, -60, 3], "exact": true}));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_round_trip_delayed_exit_search() {
        // `delayedExitSearch()` has `exit = Optional.empty()`, so the optional
        // field is omitted on encode and decodes back to `None`.
        let codec = end_gateway_configuration_codec::<JsonOps>();
        let config = EndGatewayConfiguration::delayed_exit_search();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"exact": false}));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_decodes_absent_exit_to_none() {
        let codec = end_gateway_configuration_codec::<JsonOps>();
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!({"exact": false}))
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, EndGatewayConfiguration::delayed_exit_search());
    }

    #[test]
    fn codec_requires_exact_field() {
        // `exact` is a required field (`Codec.BOOL.fieldOf`); a missing field
        // is an error.
        let codec = end_gateway_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"exit": [1, 2, 3]}))
                .is_error()
        );
    }

    #[test]
    fn codec_rejects_invalid_exit_when_present() {
        // Non-lenient optional field: a present but malformed `exit` is an
        // error, not `None`.
        let codec = end_gateway_configuration_codec::<JsonOps>();
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"exit": [1, 2], "exact": true}))
                .is_error()
        );
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({"exit": "not-a-pos", "exact": true})
                )
                .is_error()
        );
    }

    #[test]
    fn codec_rejects_non_boolean_exact() {
        let codec = end_gateway_configuration_codec::<JsonOps>();
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"exact": "yes"}))
                .is_error()
        );
    }
}
