//! Port of `net.minecraft.world.level.levelgen.blockpredicates.WouldSurvivePredicate`
//! (class, 26.2).
//!
//! Java: a `BlockPredicate` whose `test` is `this.state.canSurvive(level,
//! origin.offset(this.offset))` and whose `type()` is
//! `BlockPredicateType.WOULD_SURVIVE`. Its `CODEC` is the offset optional field
//! plus the required `"state"` field — `BlockState.CODEC`, ported here as
//! [`block_state_codec`] (the `StateHolder.codec` dispatch over the block's
//! by-name registry key + the `StateDefinition` properties fold).
//!
//! The survival check goes through the [`WorldGenLevel::can_survive`] seam
//! (RivetTodo #399 — unavailable until the world-access lands, then failing
//! explicitly rather than fabricating).
//!
//! ## `BlockState.CODEC` (`StateHolder.codec`)
//!
//! ```text
//! CODEC = BLOCK.byNameCodec().dispatch("Name", BlockState::getBlock, block -> {
//!     definition = block.getStateDefinition(); default = definition.any();
//!     return definition.isSingletonState()
//!         ? MapCodec.unit(default)
//!         : definition.propertiesCodec().codec().lenientOptionalFieldOf("Properties")
//!             .xmap(o -> o.orElse(default), Optional::of);
//! }).stable()
//! ```
//!
//! `propertiesCodec()` is a fold over the block's name-sorted properties,
//! starting at `MapCodec.unit(default)`: for each property,
//! `Codec.mapPair(codec, property.valueCodec().fieldOf(name).orElseGet(() ->
//! property.value(default))).xmap(pair -> pair.first.setValue(property,
//! pair.second.value()), state -> Pair.of(state, property.value(state)))`. The
//! `map_pair`/`PairMapCodec` combinator (added for this fold) threads the
//! partially-built `BlockState` through each property field, applying the
//! decoded value to the state and reading the current value back out on encode.

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::state_testing_predicate::offset_field;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_property::{Property, PropertyValue};
use rivet_registry::core::{BlockPos, Vec3i};
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::state_definition::StateDefinition;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::pair::Pair;
use rivet_serialization::record_builder;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.WouldSurvivePredicate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WouldSurvivePredicate {
    /// `this.offset` — the offset applied to the tested position.
    offset: Vec3i,
    /// `this.state` — the state whose survival is tested.
    state: BlockState,
}

impl WouldSurvivePredicate {
    /// `new WouldSurvivePredicate(Vec3i, BlockState)`.
    pub fn new(offset: Vec3i, state: BlockState) -> Self {
        WouldSurvivePredicate { offset, state }
    }

    /// `this.offset`.
    pub fn offset(&self) -> &Vec3i {
        &self.offset
    }

    /// `this.state`.
    pub fn state(&self) -> BlockState {
        self.state
    }
}

impl BlockPredicate for WouldSurvivePredicate {
    fn test(&self, level: &dyn WorldGenLevel, origin: &BlockPos) -> bool {
        // `this.state.canSurvive(level, origin.offset(this.offset))` — the
        // world-context survival check is the `#399` world-access seam.
        let pos = origin.offset_vec(&self.offset);
        level.can_survive(&self.state, &pos)
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::WOULD_SURVIVE
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `BlockState.CODEC` — the `StateHolder.codec` dispatch (see module docs), as
/// the ops-generic `block_state_codec::<Ops>()` factory. Only the block's
/// by-name registry key is needed (`BLOCK.byNameCodec()`), so this is plain
/// `DynamicOps` (no registry-ops lookup).
pub fn block_state_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BlockState, Ops>> {
    let dispatch = key_dispatch_codec::dispatch_map::<BlockId, BlockState, Ops>(
        "Name",
        block_by_name_codec::<Ops>(),
        Arc::new(|s: &BlockState| DataResult::success(s.block())),
        Arc::new(|block: &BlockId| {
            let definition = StateDefinition::for_block(*block);
            let default = definition.any();
            let value_codec: Arc<dyn MapCodec<BlockState, Ops>> = if definition.is_singleton_state()
            {
                // `MapCodec.unit(defaultValue)` — encodes to an empty map.
                map_codec::unit(default)
            } else {
                let fold = properties_codec::<Ops>(&definition, default);
                let fold_codec = map_codec::codec_of(fold);
                // `.lenientOptionalFieldOf("Properties").xmap(o ->
                // o.orElse(default), Optional::of)` — absent OR malformed
                // falls back to the default state; the default is omitted
                // on encode.
                codec::lenient_optional_field_of::<BlockState, Ops>(
                    "Properties",
                    fold_codec,
                    default,
                )
            };
            DataResult::success(value_codec)
        }),
    );
    let codec = map_codec::codec_of(dispatch);
    codec::stable(codec)
}

/// `BuiltInRegistries.BLOCK.byNameCodec()` — the identifier-by-name lookup over
/// the generated block table, with Paper's exact unknown-key error
/// (`"Unknown registry key in ResourceKey[minecraft:root / minecraft:block]:
/// {name}"`).
pub fn block_by_name_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BlockId, Ops>> {
    codec::comap_flat_map::<rivet_registry::Identifier, BlockId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(
            |name: &rivet_registry::Identifier| match BlockId::from_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:block]: {}",
                    name
                )),
            },
        ),
        Arc::new(|id: &BlockId| rivet_registry::Identifier::parse(id.name())),
    )
}

/// `StateDefinition.propertiesCodec()` — the fold over the block's name-sorted
/// properties (see module docs), as the ops-generic
/// `properties_codec::<Ops>(definition, default)` factory.
fn properties_codec<Ops: DynamicOps + 'static>(
    definition: &StateDefinition,
    default: BlockState,
) -> Arc<dyn MapCodec<BlockState, Ops>> {
    // `MapCodec.unit(defaultSupplier)` — the fold's starting value.
    let mut codec: Arc<dyn MapCodec<BlockState, Ops>> = map_codec::unit(default);
    for property in definition.properties() {
        let value_codec = property_value_codec::<Ops>(property);
        // `property.valueCodec().fieldOf(name)`.
        let field = codec::field_of(value_codec, property.name().to_string());
        // `.orElseGet(var0 -> {}, () -> property.value(defaultSupplier.get()))`
        // — a missing or malformed field recovers to the property's value on
        // the default state, as a clean success (no partial error).
        let default_value = default
            .get_value(property)
            .expect("the block's default state carries every property");
        let field_with_default =
            map_codec::or_else_get(field, Arc::new(|_| {}), Arc::new(move || default_value));
        // `Codec.mapPair(codec, field).xmap(pair -> pair.first.setValue(
        // property, pair.second.value()), state -> Pair.of(state,
        // property.value(state)))`.
        let paired = map_codec::map_pair(codec, field_with_default);
        codec = map_codec::xmap(
            paired,
            Arc::new(move |pair: &Pair<BlockState, PropertyValue>| {
                pair.first
                    .set_value(property, pair.second)
                    .expect("a decoded property value is valid for the property")
            }),
            Arc::new(move |state: &BlockState| {
                Pair::of(
                    *state,
                    state
                        .get_value(property)
                        .expect("a block state carries every property of its block"),
                )
            }),
        );
    }
    codec
}

/// `Property.codec` (via `valueCodec`) — the string-named value codec:
/// `Codec.STRING.comapFlatMap(name -> getValue(name) or error, getName)`.
///
/// The error message renders the property like `Property{...}` (Java's
/// `Property.toString()`); the generated tables don't carry the Java value
/// class (`clazz`), so the message is the port's best-effort form. It is only
/// ever observed inside the fold's `orElseGet` (which turns the error into a
/// clean default), never propagated to a caller.
fn property_value_codec<Ops: DynamicOps + 'static>(
    property: Property,
) -> Arc<dyn Codec<PropertyValue, Ops>> {
    codec::comap_flat_map::<String, PropertyValue, Ops>(
        codec::string_codec::<Ops>(),
        Arc::new(move |name: &String| match property.get_value(name) {
            Some(value) => DataResult::success(value),
            None => DataResult::error(format!(
                "Unable to read property: Property{{name={}, values={:?}}} with value: {}",
                property.name(),
                property.values(),
                name
            )),
        }),
        Arc::new(move |value: &PropertyValue| {
            property
                .value_name(*value)
                .expect("a block state only yields valid property values")
                .to_string()
        }),
    )
}

/// `WouldSurvivePredicate.CODEC` — the offset optional field (`Vec3i.
/// offsetCodec(16)`, default `Vec3i.ZERO`) plus the required `"state"` field
/// (`BlockState.CODEC`), as the ops-generic
/// `would_survive_predicate_map_codec::<Ops>()` factory.
pub fn would_survive_predicate_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<WouldSurvivePredicate, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(offset_field::<WouldSurvivePredicate, Ops>(Arc::new(
                |p: &WouldSurvivePredicate| p.offset,
            )))
            .and(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|p: &WouldSurvivePredicate| p.state),
                "state".to_string(),
                block_state_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|offset: Vec3i, state: BlockState| {
                    WouldSurvivePredicate::new(offset, state)
                }),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::block_predicate::block_predicate_codec;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::state_definition::StateDefinition;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// The test ops: a `RegistryOps` over JSON — the only ops that implement
    /// `RegistryOpsLookup` (the dispatch's holder-set fields require it). The
    /// would-survive codec's `"state"` field resolves blocks by name over the
    /// generated table (no registry), so an empty access is enough.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn stone() -> BlockState {
        BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:stone").unwrap(),
        )
    }

    fn oak_log_axis_x() -> BlockState {
        // The default oak_log state is `axis=y`; set the axis property to `x`
        // so the properties fold has a non-default value to write.
        let oak_log =
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:oak_log").unwrap();
        let definition = StateDefinition::for_block(oak_log);
        let axis = definition
            .get_property("axis")
            .expect("oak_log has axis property");
        let x = axis.get_value("x").expect("axis value x");
        BlockState::of(oak_log)
            .set_value(axis, x)
            .expect("x is a valid axis value")
    }

    #[test]
    fn singleton_state_encodes_without_properties() {
        // `StateDefinition.isSingletonState()` → `MapCodec.unit(default)`: the
        // singleton stone state encodes as just the `"Name"` dispatch key, no
        // `"Properties"` map.
        let p: Arc<dyn BlockPredicate> = Arc::new(WouldSurvivePredicate::new(Vec3i::ZERO, stone()));
        let codec = block_predicate_codec::<TestOps>();
        let ops = ops();
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "type": "minecraft:would_survive",
                "state": {"Name": "minecraft:stone"}
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            BlockPredicate::type_id(&*decoded),
            BlockPredicateTypes::WOULD_SURVIVE
        );
        let as_survive = decoded
            .as_any()
            .downcast_ref::<WouldSurvivePredicate>()
            .expect("decoded would_survive predicate");
        assert_eq!(as_survive.state(), stone());
    }

    #[test]
    fn multi_property_state_round_trips_through_properties_fold() {
        // A multi-property state (oak_log) encodes through the properties fold
        // under the lenient `"Properties"` optional field, preserving the
        // non-default axis.
        let p: Arc<dyn BlockPredicate> = Arc::new(WouldSurvivePredicate::new(
            Vec3i::new(1, 2, 3),
            oak_log_axis_x(),
        ));
        let codec = block_predicate_codec::<TestOps>();
        let ops = ops();
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "type": "minecraft:would_survive",
                "offset": [1, 2, 3],
                "state": {
                    "Name": "minecraft:oak_log",
                    "Properties": {"axis": "x"}
                }
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        let as_survive = decoded
            .as_any()
            .downcast_ref::<WouldSurvivePredicate>()
            .expect("decoded would_survive predicate");
        assert_eq!(as_survive.state(), oak_log_axis_x());
    }

    #[test]
    fn missing_state_field_errors() {
        let codec = block_predicate_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(&ops, &json!({"type": "minecraft:would_survive"}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key state"), "got: {msg}");
    }

    #[test]
    fn unknown_block_name_errors() {
        let codec = block_predicate_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(
            &ops,
            &json!({
                "type": "minecraft:would_survive",
                "state": {"Name": "minecraft:not_a_block"}
            }),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:block]: minecraft:not_a_block"),
            "got: {msg}"
        );
    }
}
