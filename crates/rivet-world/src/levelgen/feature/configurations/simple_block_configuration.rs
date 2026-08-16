//! Port of `net.minecraft.world.level.levelgen.feature.configurations.
//! SimpleBlockConfiguration` (record, 26.2).
//!
//! Java: a two-field record `record SimpleBlockConfiguration(BlockStateProvider
//! toPlace, boolean scheduleTick)` whose `CODEC` is a `RecordCodecBuilder` over
//! the required `"to_place"` field (`BlockStateProvider.CODEC` — the `"type"`
//! by-name dispatch) and the `"schedule_tick"` field
//! (`Codec.BOOL.optionalFieldOf("schedule_tick", false)` — the NON-lenient
//! with-default optional). There is also a single-argument constructor
//! `SimpleBlockConfiguration(BlockStateProvider)` delegating to the two-argument
//! one with `scheduleTick = false`. DFU `Codec<T>` is `Codec<E, Ops>` in the
//! port, so the static Java constant is exposed as the ops-generic
//! `simple_block_configuration_codec::<Ops>()` factory.
//!
//! The `toPlace` half is held as the erased `Arc<dyn ErasedBlockStateProvider>`
//! carrier — the trait does not extend `PartialEq` (providers are behavior, not
//! values), so the configuration is `Clone`+`Debug` only — no `PartialEq` (the
//! same shape `BlockBlobConfiguration`/`DiskConfiguration` take).
//!
//! ## The `schedule_tick` optional field
//!
//! `Codec.optionalFieldOf(name, default)` (two-arg) is the NON-lenient
//! with-default form (DFU 10.0.21: `optionalField(name, this, false).xmap(o ->
//! o.orElse(default), a -> Objects.equals(a, default) ? Optional.empty() :
//! Optional.of(a))`). Unlike `lenientOptionalFieldOf`, a present-but-malformed
//! `"schedule_tick"` is a decode error; the field is omitted on encode when
//! value-equal to the default `false`. This is exactly the
//! `rivet_serialization::codec::optional_field_of` helper.

use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.SimpleBlockConfiguration`.
///
/// The `toPlace` half is held as the erased `Arc<dyn ErasedBlockStateProvider>`
/// carrier; the trait does not extend `PartialEq`, so the configuration is
/// `Clone`+`Debug` only (the same shape `BlockBlobConfiguration`/
/// `DiskConfiguration` take).
#[derive(Debug, Clone)]
pub struct SimpleBlockConfiguration {
    /// `toPlace` — the block state provider for the placed blocks.
    pub to_place: Arc<dyn ErasedBlockStateProvider>,
    /// `scheduleTick` — whether placed blocks are scheduled for ticking.
    pub schedule_tick: bool,
}

impl SimpleBlockConfiguration {
    /// `new SimpleBlockConfiguration(BlockStateProvider, boolean)` — the
    /// two-argument record constructor (the codec's `apply` function).
    pub fn new(to_place: Arc<dyn ErasedBlockStateProvider>, schedule_tick: bool) -> Self {
        SimpleBlockConfiguration {
            to_place,
            schedule_tick,
        }
    }

    /// `new SimpleBlockConfiguration(BlockStateProvider)` — the single-argument
    /// constructor, delegating to the two-argument one with `scheduleTick =
    /// false`.
    pub fn new_without_schedule_tick(to_place: Arc<dyn ErasedBlockStateProvider>) -> Self {
        SimpleBlockConfiguration::new(to_place, false)
    }

    /// `SimpleBlockConfiguration.toPlace()`.
    pub fn to_place(&self) -> &Arc<dyn ErasedBlockStateProvider> {
        &self.to_place
    }

    /// `SimpleBlockConfiguration.scheduleTick()`.
    pub fn schedule_tick(&self) -> bool {
        self.schedule_tick
    }
}

/// `SimpleBlockConfiguration.CODEC` — a record codec over the required
/// `"to_place"` field and the non-lenient with-default `"schedule_tick"` field,
/// as the ops-generic `simple_block_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     BlockStateProvider.CODEC.fieldOf("to_place").forGetter(c -> c.toPlace),
///     Codec.BOOL.optionalFieldOf("schedule_tick", false).forGetter(c -> c.scheduleTick))
///     .apply(i, SimpleBlockConfiguration::new))
/// ```
pub fn simple_block_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<SimpleBlockConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &SimpleBlockConfiguration| c.to_place.clone()),
                codec::field_of(block_state_provider_codec::<Ops>(), "to_place".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &SimpleBlockConfiguration| c.schedule_tick),
                codec::optional_field_of::<bool, Ops>(
                    "schedule_tick",
                    codec::bool_codec::<Ops>(),
                    false,
                ),
            ))
            .apply(instance, Arc::new(SimpleBlockConfiguration::new))
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for SimpleBlockConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// `block_state_provider_codec` dispatches over the registry, so the codec
    /// requires `RegistryOps` (the `RegistryOpsLookup` ops). An empty access is
    /// enough — the provider here is `simple`.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn stone_provider() -> Arc<dyn ErasedBlockStateProvider> {
        Arc::new(simple(BlockState::of(
            BlockId::from_name("minecraft:stone").unwrap(),
        )))
    }

    fn config(schedule_tick: bool) -> SimpleBlockConfiguration {
        SimpleBlockConfiguration::new(stone_provider(), schedule_tick)
    }

    #[test]
    fn codec_round_trip_with_schedule_tick() {
        let codec = simple_block_configuration_codec::<TestOps>();
        let ops = ops();
        let config = config(true);
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "to_place": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:stone"}},
                "schedule_tick": true,
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert!(decoded.schedule_tick);
        // The provider half is a behavior carrier; equality is by dispatch
        // identity (the `"type"` key), which is what the codec round-trips.
        assert_eq!(
            ErasedBlockStateProvider::type_id(&**decoded.to_place()),
            ErasedBlockStateProvider::type_id(&**config.to_place())
        );
    }

    #[test]
    fn codec_round_trip_without_schedule_tick() {
        // `schedule_tick` is `false` — the default — so the encode OMITS the
        // field (the non-lenient with-default xmap's `Objects.equals(a, default)
        // ? Optional.empty() : Optional.of(a)`).
        let codec = simple_block_configuration_codec::<TestOps>();
        let ops = ops();
        let config = config(false);
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"to_place": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:stone"}}})
        );
        // Decoding the same map without the field yields the default `false`.
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert!(!decoded.schedule_tick);
    }

    #[test]
    fn single_argument_constructor_defaults_schedule_tick() {
        let config = SimpleBlockConfiguration::new_without_schedule_tick(stone_provider());
        assert!(!config.schedule_tick);
        assert!(config.to_place().as_any().is::<crate::levelgen::feature::stateproviders::block_state_provider::SimpleStateProvider>());
    }

    #[test]
    fn codec_requires_the_to_place_field() {
        let codec = simple_block_configuration_codec::<TestOps>();
        let ops = ops();
        // Missing `to_place` is a decode error.
        assert!(codec.parse(&ops, &json!({})).is_error());
        // A wrong-typed `schedule_tick` is a decode error (NOT lenient).
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({"to_place": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:stone"}}, "schedule_tick": "not-a-bool"})
                )
                .is_error()
        );
    }
}
