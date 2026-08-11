//! `net.minecraft.world.level.DataPackConfig` — the enabled/disabled datapack
//! lists (issue #387, the `WorldDataConfiguration` prerequisite for #323).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! DataPackConfig.java`. A two-field value class whose constructor copies both
//! lists into `ImmutableList`s (the port stores `Vec<String>` — Java's
//! `ImmutableList` is an unmodifiable snapshot, the `Vec` is the owned
//! analogue).
//!
//! `DEFAULT` = `new DataPackConfig(["vanilla"], [])`. The `CODEC` is a
//! two-field record codec over `Codec.STRING.listOf()`; the `Enabled`/`Disabled`
//! fields are **mandatory** (a missing field fails the whole decode), which is
//! what makes `DataPackConfig.CODEC.lenientOptionalFieldOf("DataPacks",
//! DataPackConfig.DEFAULT)` in `WorldDataConfiguration.MAP_CODEC` fall back to
//! `DEFAULT` on an absent `DataPacks` compound.
//!
//! Placement: the `mc.world.level` manifest unit owns this file; the port
//! lives in `rivet-world::level`. Ops-generic `codec::<Ops>()` factory, same as
//! `GameType.CODEC`.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{MapCodec, codec_of};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `DataPackConfig` — the `(List<String> enabled, List<String> disabled)` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPackConfig {
    enabled: Vec<String>,
    disabled: Vec<String>,
}

impl DataPackConfig {
    /// `DataPackConfig.DEFAULT` — `new DataPackConfig(ImmutableList.of("vanilla"),
    /// ImmutableList.of())`.
    pub fn default_config() -> Self {
        DataPackConfig::new(vec!["vanilla".to_string()], Vec::new())
    }

    /// `new DataPackConfig(List<String>, List<String>)` — the constructor
    /// (copies both lists; the port owns the `Vec`s).
    pub fn new(enabled: Vec<String>, disabled: Vec<String>) -> Self {
        DataPackConfig { enabled, disabled }
    }

    /// `DataPackConfig.getEnabled()`.
    pub fn get_enabled(&self) -> &[String] {
        &self.enabled
    }

    /// `DataPackConfig.getDisabled()`.
    pub fn get_disabled(&self) -> &[String] {
        &self.disabled
    }
}

/// `DataPackConfig.CODEC` —
/// `RecordCodecBuilder.create(i -> i.group(Codec.STRING.listOf().fieldOf(
/// "Enabled").forGetter(...), Codec.STRING.listOf().fieldOf("Disabled")
/// .forGetter(...)).apply(i, DataPackConfig::new))`.
pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<DataPackConfig, Ops>>
where
    DataPackConfig: 'static,
{
    codec_of(map_codec::<Ops>())
}

/// `DataPackConfig.CODEC` as a `MapCodec` (the `WorldDataConfiguration.MAP_CODEC`
/// composes it via `lenientOptionalFieldOf`).
pub fn map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<DataPackConfig, Ops>>
where
    DataPackConfig: 'static,
{
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &DataPackConfig| c.enabled.clone()),
                "Enabled".to_string(),
                codec::list(codec::string_codec::<Ops>()),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &DataPackConfig| c.disabled.clone()),
                "Disabled".to_string(),
                codec::list(codec::string_codec::<Ops>()),
            ))
            .apply(instance, Arc::new(DataPackConfig::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::pair::Pair;

    #[test]
    fn default_is_vanilla_enabled() {
        let d = DataPackConfig::default_config();
        assert_eq!(d.get_enabled(), &["vanilla".to_string()]);
        assert!(d.get_disabled().is_empty());
    }

    #[test]
    fn codec_round_trips() {
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        let value = DataPackConfig::new(
            vec!["vanilla".to_string(), "a:b".to_string()],
            vec!["c:d".to_string()],
        );
        let encoded = codec
            .encode_start(&ops, &value)
            .get_or_throw("encode")
            .clone();
        // JSON object with "Enabled" then "Disabled".
        assert_eq!(
            encoded,
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
                    ops.create_list(vec![ops.create_string("c:d".to_string())]),
                ),
            ])
        );
        let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
        assert_eq!(decoded.0, value);
    }

    #[test]
    fn missing_field_fails_decode() {
        // Both fields are mandatory: a map without "Enabled" is an error
        // (this is what makes `lenientOptionalFieldOf` fall back to DEFAULT in
        // WorldDataConfiguration).
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        let only_disabled = ops.create_map(vec![Pair::of(
            ops.create_string("Disabled".to_string()),
            ops.create_list(vec![]),
        )]);
        assert!(codec.decode(&ops, &only_disabled).result().is_none());
        let empty = ops.create_map(vec![]);
        assert!(codec.decode(&ops, &empty).result().is_none());
    }

    #[test]
    fn field_order_is_enabled_then_disabled() {
        // The RecordCodecBuilder group order writes "Enabled" first.
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        let value = DataPackConfig::new(vec!["vanilla".to_string()], vec![]);
        let encoded = codec
            .encode_start(&ops, &value)
            .get_or_throw("encode")
            .clone();
        let obj = encoded.as_object().expect("JSON object");
        let keys: Vec<&String> = obj.keys().collect();
        assert_eq!(keys, vec!["Enabled", "Disabled"]);
    }
}
