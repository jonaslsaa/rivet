//! Port of `net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorPreset`
//! (record, 26.2).
//!
//! Java: a two-field record `(Holder<Item> displayItem, FlatLevelGeneratorSettings
//! settings)` whose `DIRECT_CODEC` is a `RecordCodecBuilder` over the required
//! `"display"` field (`Item.CODEC` — the item registry's reference codec) and
//! the required `"settings"` field (`FlatLevelGeneratorSettings.CODEC`), and
//! whose `CODEC` wraps that in `RegistryFileCodec.create(
//! Registries.FLAT_LEVEL_GENERATOR_PRESET, DIRECT_CODEC)` — the
//! `Holder<FlatLevelGeneratorPreset>` codec.
//!
//! The `Item` value is the [`ItemStub`] name handle (STUB(mc.world.item)); the
//! display holder is a `Direct(ItemStub)`, so the reference codec (fixed item
//! registry) round-trips its name and `builtInRegistryHolder()` carries it
//! inline.
//!
//! The Java record auto-generates `equals`/`hashCode`/`toString`, but the port
//! does not derive value equality/display: Java compares the `settings` field
//! by reference identity (`FlatLevelGeneratorSettings` has no `equals`
//! override), and the port's `FlatLevelGeneratorSettings` cannot derive
//! `PartialEq` at all (its `PlacedFeature` lake holders hold erased trait
//! objects). No ported surface needs the record's value equality today.

use crate::levelgen::flat::flat_level_generator_settings::flat_level_generator_settings_codec;
use crate::levelgen::flat::{FLAT_LEVEL_GENERATOR_PRESET, ITEM, ItemStub};
use rivet_registry::generated::registries::ITEM_BY_NAME;
use rivet_registry::holder::Holder;
use rivet_registry::registry_file_codec::RegistryFileCodec;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorPreset`.
#[derive(Debug, Clone)]
pub struct FlatLevelGeneratorPreset {
    /// `displayItem` — the preset's display item holder.
    pub display_item: Holder<ItemStub>,
    /// `settings` — the flat generation settings.
    pub settings: crate::levelgen::flat::flat_level_generator_settings::FlatLevelGeneratorSettings,
}

/// `Item.CODEC` over the STUB name handle — the item registry's reference
/// codec, wrapped in Java's air rejection:
/// `holderByNameCodec().validate(item -> item.is(Items.AIR...) ? error("Item
/// must not be minecraft:air") : success(item))`. The STUB has no item-value
/// registry, so the "holder" is a `Direct(ItemStub)` name (a `Reference`
/// carries the same name) and the check compares the registered name — or, for
/// a `Reference`, the element id against the generated `minecraft:air` item id.
///
/// STUB(mc.world.item): replaced by `BuiltInRegistries.ITEM.holderByNameCodec()`
/// (or `RegistryFixedCodec` over a live ITEM registry) when `mc.world.item`
/// lands. The `Reference` element-id check is the decode/encode analogue of
/// Java's `holderByNameCodec().validate`, which rejects air for BOTH holder
/// kinds (the item codec only ever produces ITEM-registry references, so the
/// bare element id — the only comparison available to a lookup-less `validate`
/// predicate — is the faithful check).
pub fn item_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Holder<ItemStub>, Ops>> {
    let codec: Arc<dyn Codec<Holder<ItemStub>, Ops>> = Arc::new(RegistryFileCodec::create(
        &*ITEM,
        identifier_name_codec::<Ops>(),
    ));
    codec::validate(codec, Arc::new(reject_air))
}

/// `validateHeight`-style partial-carrying checker — rejects `minecraft:air`
/// with Java's exact message (`"Item must not be minecraft:air"`), carrying the
/// holder as the partial value.
///
/// Java's `Item.CODEC` (`holderByNameCodec().validate(...)`) rejects air for
/// BOTH holder kinds on encode and decode. `Direct` compares the registered
/// name; a `Reference` compares the element id against the generated
/// `minecraft:air` item id — the `holderByNameCodec` holder is always in the
/// ITEM registry, so the bare element id is the faithful check a lookup-less
/// `validate` predicate can make.
fn reject_air(holder: &Holder<ItemStub>) -> DataResult<Holder<ItemStub>> {
    let is_air = match holder {
        Holder::Direct(item) => item.name() == "minecraft:air",
        // STUB(mc.world.item): the reference codec never produces `Reference`s
        // here (decode yields `Direct`), so this arm is unreachable today. When
        // a live ITEM registry binds, the element-id check rejects air exactly
        // as Java does — the reference codec only yields ITEM-registry
        // references, so a coincident id in another registry cannot occur
        // through it (a hand-built foreign reference could; a live registry's
        // `holderByNameCodec` replacement supersedes this STUB before that
        // matters).
        Holder::Reference { id, .. } => ITEM_BY_NAME
            .get("minecraft:air")
            .is_some_and(|air_id| *id == *air_id as u32),
    };
    if is_air {
        DataResult::error_with_partial("Item must not be minecraft:air", holder.clone())
    } else {
        DataResult::success(holder.clone())
    }
}

/// The element codec — an `ItemStub` by registered name.
///
/// STUB(mc.world.item): `Identifier.CODEC.xmap(Item::byName, Item::getRegisteredName)`
/// — the name handle is encoded/decoded verbatim.
fn identifier_name_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ItemStub, Ops>> {
    let identifier = rivet_registry::identifier::identifier_codec::<Ops>();
    codec::xmap(
        identifier,
        Arc::new(|i: &rivet_registry::Identifier| ItemStub::new(&i.to_string())),
        Arc::new(|s: &ItemStub| {
            // STUB(mc.world.item): `Item.byName` — the name is the registered
            // item name, so parse is infallible here.
            rivet_registry::identifier::Identifier::parse(s.name())
        }),
    )
}

/// `FlatLevelGeneratorPreset.DIRECT_CODEC` — the two-field record codec over
/// the required `"display"` and `"settings"` fields, as the ops-generic
/// `direct_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     Item.CODEC.fieldOf("display").forGetter(e -> e.displayItem),
///     FlatLevelGeneratorSettings.CODEC.fieldOf("settings").forGetter(e -> e.settings))
///     .apply(i, FlatLevelGeneratorPreset::new))
/// ```
pub fn direct_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<FlatLevelGeneratorPreset, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &FlatLevelGeneratorPreset| p.display_item.clone()),
                codec::field_of(item_codec::<Ops>(), "display".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &FlatLevelGeneratorPreset| p.settings.clone()),
                codec::field_of(
                    flat_level_generator_settings_codec::<Ops>(),
                    "settings".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(
                    |display_item: Holder<ItemStub>,
                     settings: crate::levelgen::flat::flat_level_generator_settings::FlatLevelGeneratorSettings| FlatLevelGeneratorPreset {
                        display_item,
                        settings,
                    },
                ),
            )
    })
}

/// `FlatLevelGeneratorPreset.CODEC` — `RegistryFileCodec.create(
/// Registries.FLAT_LEVEL_GENERATOR_PRESET, DIRECT_CODEC)`, the
/// `Holder<FlatLevelGeneratorPreset>` codec, as the ops-generic
/// `flat_level_generator_preset_codec::<Ops>()` factory.
pub fn flat_level_generator_preset_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Holder<FlatLevelGeneratorPreset>, Ops>> {
    Arc::new(RegistryFileCodec::create(
        &*FLAT_LEVEL_GENERATOR_PRESET,
        direct_codec::<Ops>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    #[test]
    fn item_codec_rejects_air_on_encode() {
        // The `validate` checker runs on the encode path before the inner codec,
        // so a `Direct` air display holder is rejected without needing a registry.
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = item_codec::<TestOps>();

        let rejected = codec
            .encode_start(&ops, &Holder::direct(ItemStub::new("minecraft:air")))
            .error_ref()
            .expect("air must be rejected on encode")
            .message()
            .to_string();
        assert_eq!(rejected, "Item must not be minecraft:air");

        // Decode-side rejection is deferred with the ITEM registry STUB: with no
        // `minecraft:item` registry bound, the inner `RegistryFileCodec` errors
        // `"Registry does not exist"` before the checker runs, and with a bound
        // registry it resolves to a `Reference` the registry-less checker cannot
        // name. It becomes live with `mc.world.item` (`Item.CODEC` replacement).
        let encoded = codec
            .encode_start(&ops, &Holder::direct(ItemStub::new("minecraft:stone")))
            .result()
            .expect("a non-air item must encode")
            .clone();
        assert_eq!(encoded, json!("minecraft:stone"));
    }
}
