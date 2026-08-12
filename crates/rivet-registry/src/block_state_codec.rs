//! Port of `BlockState.CODEC` (26.2) — the `"Name"`-dispatch codec for
//! `net.minecraft.world.level.block.state.BlockState` (issue #391; the
//! dependency-clean `block.state` codec leaf the
//! `feature.configurations.blockstate`/`layer`/`spike`/`blockblob` value types
//! reach through `BlockState.CODEC.fieldOf("state")`).
//!
//! Java (BlockState.java:9):
//! ```java
//! public static final Codec<BlockState> CODEC =
//!     codec(BuiltInRegistries.BLOCK.byNameCodec(),
//!           Block::defaultBlockState, Block::getStateDefinition).stable();
//! ```
//! where `StateHolder.codec` (StateHolder.java:188-211) is:
//! ```java
//! return ownerCodec.dispatch("Name", s -> s.owner, o -> {
//!     StateDefinition<O, S> definition = stateDefinition.apply((O)o);
//!     S defaultValue = defaultState.apply((O)o);
//!     return definition.isSingletonState()
//!         ? MapCodec.unit(defaultValue)
//!         : definition.propertiesCodec().codec()
//!             .lenientOptionalFieldOf("Properties")
//!             .xmap(oo -> oo.orElse(defaultValue), Optional::of);
//! });
//! ```
//!
//! So a block state serializes to `{"Properties": {"axis": "x"}, "Name":
//! "minecraft:oak_log"}`: the per-block property codec applies to the whole map
//! first, then the `"Name"` discriminator names the block (unknown name →
//! error). The element-before-key order matches `KeyDispatchCodec.encode`
//! (value first, then the type key), which this port preserves byte-for-byte
//! for the `rivet-parity` oracle. Singleton states (no properties) encode only
//! the name — `MapCodec.unit`.
//!
//! The `"Properties"` field is a **lenient** optional field, so a wrong-typed
//! `Properties` decodes to `None` → the block's default state (matching
//! `NbtUtils.readBlockState`'s recovery). Encode always writes it for
//! non-singleton blocks (`Optional::of`) — even when every value is the
//! default. Note this is deliberately the raw `optional_field(..., true)` +
//! explicit xmap, NOT `lenient_optional_field_of`: the latter's encode omits
//! a value equal to the default, whereas Java's `Optional::of` always emits
//! the `Properties` compound.
//!
//! The per-block properties codec is `StateDefinition.createCodec`: a
//! `MapCodec.unit(default)` folded with
//! `Codec.mapPair(prev, property.valueCodec().fieldOf(name).orElseGet(no-op,
//! () -> property.value(default)))` then `.xmap` collapsing the pair onto the
//! state. The port mirrors that fold with [`codec::map_pair`] (the new
//! `PairMapCodec`) and the existing `MapCodec` combinators, and
//! [`property_value_codec`] is `Property.valueCodec` (`Codec.STRING
//! .comapFlatMap(name -> getValue(name)..., Property::getName)`).

use crate::block_state::BlockState;
use crate::block_state_property::{Property, PropertyValue};
use crate::generated::blocks::BlockId;
use crate::state_definition::StateDefinition;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use std::sync::Arc;

/// `BlockState.CODEC`, as the ops-generic `block_state_codec::<Ops>()` factory.
pub fn block_state_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BlockState, Ops>> {
    codec::stable(dispatch_state_codec())
}

/// `StateHolder.codec`'s `"Name"` dispatch — the `Codec<BlockState>`.
fn dispatch_state_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BlockState, Ops>> {
    let dispatch = rivet_serialization::key_dispatch_codec::dispatch_map::<BlockId, BlockState, Ops>(
        "Name",
        block_by_name_codec::<Ops>(),
        Arc::new(|state: &BlockState| DataResult::success(state.block())),
        Arc::new(state_codec_for_block),
    );
    map_codec::codec_of(dispatch)
}

/// `BuiltInRegistries.BLOCK.byNameCodec()` over the generated block ids — the
/// block is resolved by its namespaced name with the vanilla unknown-key error.
fn block_by_name_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BlockId, Ops>> {
    codec::comap_flat_map::<crate::Identifier, BlockId, Ops>(
        crate::identifier::identifier_codec::<Ops>(),
        Arc::new(
            |name: &crate::Identifier| match BlockId::from_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:block]: {}",
                    name
                )),
            },
        ),
        Arc::new(|id: &BlockId| crate::Identifier::parse(id.name())),
    )
}

/// `o -> isSingletonState() ? unit(default) :
/// propertiesCodec().codec().lenientOptionalFieldOf("Properties").xmap(...)` —
/// the dispatch's per-block `MapCodec<BlockState>`.
fn state_codec_for_block<Ops: DynamicOps + 'static>(
    block: &BlockId,
) -> DataResult<Arc<dyn MapCodec<BlockState, Ops>>> {
    let definition = StateDefinition::for_block(*block);
    if definition.is_singleton_state() {
        let default = definition.any();
        return DataResult::success(map_codec::unit_with(Arc::new(move || default)));
    }

    let default = definition.any();
    let properties_codec = definition.properties_codec::<Ops>();
    let properties_field = codec::optional_field(
        "Properties".to_string(),
        map_codec::codec_of(properties_codec),
        true,
    );
    // `.xmap(oo -> oo.orElse(defaultValue), Optional::of)` — decode falls back
    // to the block's default state (absent OR malformed), encode always writes.
    let decode_default = default;
    DataResult::success(map_codec::xmap(
        properties_field,
        Arc::new(move |o: &Option<BlockState>| o.unwrap_or(decode_default)),
        Arc::new(|s: &BlockState| Some(*s)),
    ))
}

/// `Property.valueCodec` — `Codec.STRING.comapFlatMap(name ->
/// this.getValue(name).map(success).orElseGet(() -> error("Unable to read
/// property: <prop> with value: <name>")), this::getName)`. The property
/// renders via its `Display` impl (`Property.toString()`).
pub fn property_value_codec<Ops: DynamicOps + 'static>(
    prop: Property,
) -> Arc<dyn Codec<PropertyValue, Ops>> {
    let prop_for_error = prop;
    codec::comap_flat_map::<String, PropertyValue, Ops>(
        codec::string_codec::<Ops>(),
        Arc::new(move |name: &String| match prop_for_error.get_value(name) {
            Some(value) => DataResult::success(value),
            None => DataResult::error(format!(
                "Unable to read property: {} with value: {}",
                prop_for_error, name
            )),
        }),
        Arc::new(move |value: &PropertyValue| {
            prop.value_name(*value)
                .expect("encode of an allowed property value")
                .to_string()
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::block_properties::BlockPropertyId;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn codec() -> Arc<dyn Codec<BlockState, JsonOps>> {
        block_state_codec::<JsonOps>()
    }

    #[test]
    fn singleton_state_encodes_name_only() {
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let encoded = codec()
            .encode_start(&JsonOps::INSTANCE, &stone)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"Name": "minecraft:stone"}));
        let decoded = *codec()
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed");
        assert_eq!(decoded, stone);
    }

    #[test]
    fn non_singleton_round_trips_properties() {
        let oak_log = BlockId::from_name("minecraft:oak_log").unwrap();
        let state = BlockState::of(oak_log)
            .set_property(BlockPropertyId::Axis, 0)
            .unwrap();
        let encoded = codec()
            .encode_start(&JsonOps::INSTANCE, &state)
            .result()
            .expect("encode should succeed")
            .clone();
        // Element-first encode order: the property codec writes first, then the
        // "Name" type key (Java `KeyDispatchCodec.encode`). `serde_json`'s Map
        // equality is order-insensitive, so assert the actual key sequence.
        assert_eq!(
            encoded,
            json!({"Properties": {"axis": "x"}, "Name": "minecraft:oak_log"})
        );
        assert_eq!(
            encoded
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["Properties", "Name"]
        );
        assert_eq!(
            encoded["Properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["axis"]
        );
        let decoded = *codec()
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed");
        assert_eq!(decoded, state);
    }

    #[test]
    fn non_singleton_encode_always_writes_properties() {
        // `Optional::of` on encode: even the default oak_log state writes the
        // full `Properties` compound (all values are the defaults).
        let oak_log = BlockState::of(BlockId::from_name("minecraft:oak_log").unwrap());
        let encoded = codec()
            .encode_start(&JsonOps::INSTANCE, &oak_log)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"Properties": {"axis": "y"}, "Name": "minecraft:oak_log"})
        );
        // Element-first: `Properties` before `Name` (see `non_singleton_round_trips_properties`).
        assert_eq!(
            encoded
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["Properties", "Name"]
        );
    }

    #[test]
    fn absent_or_malformed_properties_decodes_to_default_state() {
        // `lenientOptionalFieldOf`: absent → default state.
        let decoded = *codec()
            .parse(&JsonOps::INSTANCE, &json!({"Name": "minecraft:oak_log"}))
            .result()
            .expect("decode should succeed");
        assert_eq!(
            decoded,
            BlockState::of(BlockId::from_name("minecraft:oak_log").unwrap())
        );
        // Wrong-typed `Properties` (non-compound) → `None` → default state.
        let decoded = *codec()
            .parse(
                &JsonOps::INSTANCE,
                &json!({"Name": "minecraft:oak_log", "Properties": "bad"}),
            )
            .result()
            .expect("decode should succeed");
        assert_eq!(
            decoded,
            BlockState::of(BlockId::from_name("minecraft:oak_log").unwrap())
        );
    }

    #[test]
    fn unknown_block_name_errors() {
        let result = codec().parse(&JsonOps::INSTANCE, &json!({"Name": "minecraft:no_such"}));
        assert!(result.is_error());
        let error = result.error_ref().unwrap();
        assert!(
            error
                .message()
                .contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:block]"),
            "unexpected error: {}",
            error.message()
        );
    }

    #[test]
    fn invalid_property_value_falls_back_to_default() {
        // `valueCodec.fieldOf(name).orElseGet(...)`: a value not allowed for
        // the property is a field error → `orElseGet` supplies the default
        // state's value (NOT a decode error).
        let decoded = *codec()
            .parse(
                &JsonOps::INSTANCE,
                &json!({"Name": "minecraft:oak_log", "Properties": {"axis": "sideways"}}),
            )
            .result()
            .expect("orElseGet recovers to the default");
        assert_eq!(
            decoded,
            BlockState::of(BlockId::from_name("minecraft:oak_log").unwrap())
        );
    }

    #[test]
    fn multi_property_round_trip() {
        let oak_leaves = BlockId::from_name("minecraft:oak_leaves").unwrap();
        // Distance values are 1..7 (index 1 → "2"); oak_leaves defaults are
        // distance=7 persistent=false waterlogged=false. Set distance index 1
        // and persistent to true so every field differs from the default.
        let state = BlockState::of(oak_leaves)
            .set_property(BlockPropertyId::Distance, 1)
            .unwrap()
            .set_value(
                crate::block_state_property::Property::from_id(
                    crate::generated::block_properties::BlockPropertyId::Persistent,
                ),
                PropertyValue::Bool(true),
            )
            .unwrap();
        let encoded = codec()
            .encode_start(&JsonOps::INSTANCE, &state)
            .result()
            .expect("encode should succeed")
            .clone();
        // Properties encode in reverse name-sorted order (waterlogged,
        // persistent, distance) — the `PairMapCodec` fold encodes the
        // accumulated `second` first, so the alphabetically-last property
        // (distance, folded last) lands first in the output. Map equality is
        // order-insensitive, so pin the actual key sequence.
        assert_eq!(
            encoded,
            json!({"Properties": {
                "waterlogged": "false", "persistent": "true", "distance": "2"
            }, "Name": "minecraft:oak_leaves"})
        );
        assert_eq!(
            encoded
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["Properties", "Name"]
        );
        assert_eq!(
            encoded["Properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["waterlogged", "persistent", "distance"]
        );
        let decoded = *codec()
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed");
        assert_eq!(decoded, state);
    }
}
