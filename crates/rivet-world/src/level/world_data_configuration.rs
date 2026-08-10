//! `net.minecraft.world.level.WorldDataConfiguration` — the datapack/feature-flag
//! configuration of a world (issue #486, the `WorldData` value-codec slice).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! WorldDataConfiguration.java`. A two-field record over `(DataPackConfig
//! dataPacks, FeatureFlagSet enabledFeatures)`.
//!
//! ## The `MAP_CODEC` lenient-optional default-omit semantics
//!
//! `MAP_CODEC` = `RecordCodecBuilder.mapCodec(i -> i.group(
//! DataPackConfig.CODEC.lenientOptionalFieldOf("DataPacks",
//! DataPackConfig.DEFAULT).forGetter(WorldDataConfiguration::dataPacks),
//! FeatureFlags.CODEC.lenientOptionalFieldOf("enabled_features",
//! FeatureFlags.DEFAULT_FLAGS).forGetter(WorldDataConfiguration::enabledFeatures))
//! .apply(i, WorldDataConfiguration::new))`.
//!
//! In the pinned DFU (10.0.21) `lenientOptionalFieldOf(name, default)` is
//! `optionalField(name, this, true).xmap(o -> o.orElse(default), a ->
//! Objects.equals(a, default) ? Optional.empty() : Optional.of(a))` — verified
//! by disassembling the pinned `datafixerupper-10.0.21.jar`
//! (`Codec.optionalFieldOf(String, A, boolean)`). Three behaviors follow:
//!
//! 1. **Decode — absent field → default.** The underlying `OptionalFieldCodec`
//!    returns `Optional.empty()` and the xmap's decode half promotes it to the
//!    default.
//! 2. **Decode — lenient error → default.** A *present* field whose value fails
//!    to parse (e.g. an unknown `enabled_features` id) is an error; with
//!    `lenient == true` the optional field yields `Optional.empty()` and the
//!    default wins. This is why `WorldDataConfiguration.CODEC` does NOT fail on
//!    a malformed `DataPacks`/`enabled_features` — it silently falls back to
//!    the default.
//! 3. **Encode — value equal to default → omitted.** The xmap's encode half
//!    collapses a value equal to the default to `Optional.empty()`, and the
//!    `OptionalFieldCodec` encoder skips absent optionals. `PrimaryLevelData`
//!    stores the record's dataConfiguration via `MAP_CODEC`, so re-encoding a
//!    world whose configuration equals `DEFAULT` omits both fields.
//!
//! The real 26.2 `New World/level.dat` confirms #3 on the decode side: it
//! stores `DataPacks: {Enabled: ["vanilla"], Disabled: [...]}` (present —
//! non-default) and `enabled_features: []` (the empty list, which decodes to
//! the *empty* set, NOT `DEFAULT_FLAGS` — see below).
//!
//! ## `Objects.equals` and `DataPackConfig`
//!
//! Java's xmap encode check is `Objects.equals(a, default)`. `FeatureFlagSet`
//! overrides `equals` (structural `(universe, mask)`), so `enabled_features`
//! is omitted exactly when the set equals `DEFAULT_FLAGS`. `DataPackConfig` is
//! a plain class with **no `equals` override** — Java compares by reference,
//! so the `DataPacks` field is effectively always encoded (a freshly-built
//! config is never `== DEFAULT`). The port gives `DataPackConfig` value
//! equality (derived `PartialEq`/`Eq`, matching the merged #387 port), so
//! `DataPacks` IS omitted when the config equals `DEFAULT`. This is the only
//! divergence from Java on the save path (which is excluded from this slice);
//! it is value-semantics-consistent with the Rust surface and marked
//! RivetTodo(#486) so the excluded `PrimaryLevelData.write` port re-audits it.
//!
//! ## The `enabled_features: []` vs `DEFAULT_FLAGS` subtlety
//!
//! `FeatureFlags.CODEC` (`REGISTRY.codec()`) decodes a list of ids to the set
//! of *known* ids. `[]` decodes to the **empty** set — which is NOT
//! `DEFAULT_FLAGS` (`{vanilla}`) — so a `New World` with `enabled_features:
//! []` decodes to an empty set, and re-encoding that empty set is NOT
//! `Objects.equals(EMPTY, DEFAULT_FLAGS)` → the field is re-encoded as `[]`.
//! The `WorldDataConfiguration.DEFAULT` config would instead encode an absent
//! `enabled_features` (its `{vanilla}` equals the default). The real level.dat
//! stores `[]` precisely because the launcher created it with an empty feature
//! set, not `DEFAULT`.
//!
//! Placement: `rivet-world::level` (`mc.world.level` unit), next to
//! `DataPackConfig` (#387). `expandFeatures` is the only method beyond the
//! record accessors.
//!
//! Deliberately deferred (blocked by later units; no declarations emitted):
//! RivetTodo(#486) — the `DataPackConfig` reference-identity encode divergence
//! above (re-audit when `PrimaryLevelData.write` lands).

use super::data_pack_config::DataPackConfig;
use crate::flag::{FeatureFlagSet, default_flags, feature_flags::codec as feature_flags_codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `WorldDataConfiguration` — the `(DataPackConfig dataPacks, FeatureFlagSet
/// enabledFeatures)` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldDataConfiguration {
    data_packs: DataPackConfig,
    enabled_features: FeatureFlagSet,
}

impl WorldDataConfiguration {
    /// `WorldDataConfiguration.DEFAULT` — `new WorldDataConfiguration(
    /// DataPackConfig.DEFAULT, FeatureFlags.DEFAULT_FLAGS)`.
    pub fn default_config() -> Self {
        WorldDataConfiguration::new(DataPackConfig::default_config(), default_flags())
    }

    /// The canonical constructor (the record accessor renames are the getters).
    pub fn new(data_packs: DataPackConfig, enabled_features: FeatureFlagSet) -> Self {
        WorldDataConfiguration {
            data_packs,
            enabled_features,
        }
    }

    /// `WorldDataConfiguration.dataPacks()`.
    pub fn data_packs(&self) -> &DataPackConfig {
        &self.data_packs
    }

    /// `WorldDataConfiguration.enabledFeatures()`.
    pub fn enabled_features(&self) -> &FeatureFlagSet {
        &self.enabled_features
    }

    /// `WorldDataConfiguration.expandFeatures(FeatureFlagSet)` —
    /// `new WorldDataConfiguration(this.dataPacks,
    /// this.enabledFeatures.join(newEnabledFeatures))`.
    pub fn expand_features(&self, new_enabled_features: &FeatureFlagSet) -> Self {
        WorldDataConfiguration::new(
            self.data_packs.clone(),
            self.enabled_features.join(new_enabled_features),
        )
    }
}

/// `WorldDataConfiguration.MAP_CODEC` — the two-field record codec over the
/// lenient-optional `DataPacks`/`enabled_features` fields (see the module doc
/// for the default-omit encode and lenient-error decode semantics).
pub fn map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<WorldDataConfiguration, Ops>>
where
    WorldDataConfiguration: 'static,
{
    let dp = DataPackConfig::default_config();
    let ef = default_flags();
    let lenient_data_packs: Arc<dyn MapCodec<DataPackConfig, Ops>> =
        lenient_optional::<DataPackConfig, Ops>(
            "DataPacks".to_string(),
            super::data_pack_config::codec::<Ops>(),
            dp,
        );
    let lenient_features: Arc<dyn MapCodec<FeatureFlagSet, Ops>> =
        lenient_optional::<FeatureFlagSet, Ops>(
            "enabled_features".to_string(),
            feature_flags_codec::<Ops>(),
            ef,
        );
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &WorldDataConfiguration| c.data_packs.clone()),
                lenient_data_packs,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &WorldDataConfiguration| c.enabled_features.clone()),
                lenient_features,
            ))
            .apply(instance, Arc::new(WorldDataConfiguration::new))
    })
}

/// `WorldDataConfiguration.CODEC` — `MAP_CODEC.codec()`.
pub fn codec<Ops: DynamicOps + 'static>()
-> Arc<dyn rivet_serialization::Codec<WorldDataConfiguration, Ops>>
where
    WorldDataConfiguration: 'static,
{
    map_codec::codec_of(map_codec::<Ops>())
}

/// `Codec.lenientOptionalFieldOf(name, default)` — `optionalField(name, codec,
/// true).xmap(o -> o.orElse(default), a -> Objects.equals(a, default) ?
/// Optional.empty() : Optional.of(a))` in the pinned DFU 10.0.21. See the
/// module doc for the exact encode/decode behavior this reproduces.
fn lenient_optional<A, Ops: DynamicOps + 'static>(
    name: String,
    element_codec: Arc<dyn rivet_serialization::Codec<A, Ops>>,
    default: A,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: Clone + Send + Sync + PartialEq + 'static,
{
    let field: Arc<dyn MapCodec<Option<A>, Ops>> =
        rivet_serialization::codec::optional_field(name, element_codec, true);
    let default_dec = default.clone();
    let default_enc = default.clone();
    map_codec::xmap(
        field,
        // Decode half: `o -> o.orElse(default)`.
        Arc::new(move |opt: &Option<A>| match opt {
            Some(v) => v.clone(),
            None => default_dec.clone(),
        }),
        // Encode half: `a -> Objects.equals(a, default) ? empty : of(a)`.
        Arc::new(move |a: &A| {
            if *a == default_enc {
                None
            } else {
                Some(a.clone())
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flag::feature_flags::{TRADE_REBALANCE, VANILLA};
    use rivet_serialization::Decoder;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::pair::Pair;

    fn default() -> WorldDataConfiguration {
        WorldDataConfiguration::default_config()
    }

    #[test]
    fn default_is_data_pack_default_and_vanilla() {
        let d = default();
        assert_eq!(
            d.data_packs(),
            &DataPackConfig::new(vec!["vanilla".to_string()], Vec::new())
        );
        assert_eq!(
            d.enabled_features(),
            &crate::flag::feature_flags::vanilla_set()
        );
    }

    #[test]
    fn expand_features_joins_sets() {
        let d = default();
        let exp = d.expand_features(&crate::flag::FeatureFlagSet::of_flag(&TRADE_REBALANCE));
        assert_eq!(
            exp.enabled_features(),
            &crate::flag::FeatureFlagSet::of_flags(&VANILLA, &[&TRADE_REBALANCE])
        );
        // dataPacks untouched.
        assert_eq!(exp.data_packs(), d.data_packs());
    }

    #[test]
    fn encode_omits_default_fields() {
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        // DEFAULT encodes to an EMPTY map — the lenient-optional xmap collapses
        // both fields (Objects.equals(a, default)).
        let encoded = codec
            .encode_start(&ops, &default())
            .get_or_throw("encode")
            .clone();
        assert_eq!(encoded, ops.create_map(vec![]));
    }

    #[test]
    fn encode_present_when_non_default() {
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        // A config with a non-default data pack config: `DataPacks` is encoded
        // (the whole `DataPackConfig.CODEC` map), and `enabled_features` is
        // still omitted (the set is the default).
        let config = WorldDataConfiguration::new(
            DataPackConfig::new(vec!["vanilla".to_string(), "a:b".to_string()], vec![]),
            default_flags(),
        );
        let encoded = codec
            .encode_start(&ops, &config)
            .get_or_throw("encode")
            .clone();
        let obj = encoded.as_object().expect("object");
        // Only "DataPacks" — enabled_features omitted.
        assert_eq!(obj.keys().collect::<Vec<_>>(), vec!["DataPacks"]);
        let data_packs = obj.get("DataPacks").expect("DataPacks present");
        assert_eq!(
            *data_packs,
            ops.create_map(vec![
                Pair::of(
                    ops.create_string("Enabled".to_string()),
                    ops.create_list(vec![
                        ops.create_string("vanilla".to_string()),
                        ops.create_string("a:b".to_string()),
                    ]),
                ),
                Pair::of(
                    ops.create_string("Disabled".to_string()),
                    ops.create_list(vec![]),
                ),
            ])
        );
    }

    #[test]
    fn encode_empty_features_is_present_because_not_default() {
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        // The real 26.2 New World stores enabled_features: [] (the EMPTY set).
        // EMPTY != DEFAULT_FLAGS, so the field IS re-encoded as [].
        let config = WorldDataConfiguration::new(
            DataPackConfig::default_config(),
            crate::flag::FeatureFlagSet::of(),
        );
        let encoded = codec
            .encode_start(&ops, &config)
            .get_or_throw("encode")
            .clone();
        let obj = encoded.as_object().expect("object");
        assert_eq!(obj.keys().collect::<Vec<_>>(), vec!["enabled_features"]);
        assert_eq!(
            *obj.get("enabled_features").expect("present"),
            ops.create_list(vec![])
        );
    }

    #[test]
    fn decode_absent_fields_uses_defaults() {
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        let empty = ops.create_map(vec![]);
        let decoded = codec.decode(&ops, &empty).get_or_throw("decode").clone();
        assert_eq!(decoded.0, default());
    }

    #[test]
    fn decode_present_non_default_fields() {
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        // Match the real New World level.dat: DataPacks present + enabled_features [].
        let input = ops.create_map(vec![
            Pair::of(
                ops.create_string("DataPacks".to_string()),
                ops.create_map(vec![
                    Pair::of(
                        ops.create_string("Enabled".to_string()),
                        ops.create_list(vec![ops.create_string("vanilla".to_string())]),
                    ),
                    Pair::of(
                        ops.create_string("Disabled".to_string()),
                        ops.create_list(vec![]),
                    ),
                ]),
            ),
            Pair::of(
                ops.create_string("enabled_features".to_string()),
                ops.create_list(vec![]),
            ),
        ]);
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(
            decoded.0.data_packs(),
            &DataPackConfig::new(vec!["vanilla".to_string()], Vec::new())
        );
        // [] decodes to the EMPTY set (NOT the default vanilla set).
        assert_eq!(
            decoded.0.enabled_features(),
            &crate::flag::FeatureFlagSet::of()
        );
    }

    #[test]
    fn decode_lenient_malformed_feature_list_falls_back_to_default() {
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        // An unknown feature id makes FeatureFlags.CODEC error (with a partial);
        // the lenient optional field swallows it and yields DEFAULT_FLAGS.
        let input = ops.create_map(vec![Pair::of(
            ops.create_string("enabled_features".to_string()),
            ops.create_list(vec![ops.create_string("minecraft:bogus".to_string())]),
        )]);
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0.enabled_features(), &default_flags());
        assert_eq!(decoded.0.data_packs(), &DataPackConfig::default_config());
    }

    #[test]
    fn decode_lenient_malformed_data_packs_falls_back_to_default() {
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        // A DataPacks compound missing the mandatory "Enabled" field errors;
        // the lenient field falls back to DataPackConfig.DEFAULT.
        let input = ops.create_map(vec![Pair::of(
            ops.create_string("DataPacks".to_string()),
            ops.create_map(vec![]),
        )]);
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0.data_packs(), &DataPackConfig::default_config());
        assert_eq!(decoded.0.enabled_features(), &default_flags());
    }

    #[test]
    fn round_trip_preserves_non_default() {
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        let config = WorldDataConfiguration::new(
            DataPackConfig::new(
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string()],
            ),
            crate::flag::FeatureFlagSet::of_flags(&VANILLA, &[&TRADE_REBALANCE]),
        );
        let encoded = codec
            .encode_start(&ops, &config)
            .get_or_throw("encode")
            .clone();
        let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
        assert_eq!(decoded.0, config);
    }
}
