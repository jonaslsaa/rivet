//! Port of `net.minecraft.world.level.levelgen.presets.WorldPreset` (class,
//! 26.2) — the `mc.world.level.levelgen.presets` unit.
//!
//! The world preset value: the `Map<ResourceKey<LevelStem>, LevelStem>`
//! dimension map, its ordered `"dimensions"` codec (`Codec.unboundedMap(
//! ResourceKey.codec(LEVEL_STEM), LevelStem.CODEC).fieldOf("dimensions")`) with
//! the missing-overworld `validate` (Java `requireOverworld`), and the
//! `worldgen/world_preset` `RegistryFileCodec` ([`world_preset_codec`]) that
//! resolves a `Holder<WorldPreset>` by identifier.
//!
//! ### The codec seam
//!
//! `WorldPreset.DIRECT_CODEC` is structured faithfully over `LevelStem.CODEC` —
//! the same poison leaf the `WorldDimensions` codec inherits (see
//! `level_stem::level_stem_codec`): the map round-trip errors until the
//! `mc.world.level.dimension`/`mc.world.level.chunk.generator` units land. The
//! `requireOverworld` check is tested before the leaf is reached (an empty stem
//! map decodes/encodes without touching a stem, so the missing-overworld error
//! fires; a map with stems errors through the leaf).

use crate::levelgen::settings::level_stem::{self, LevelStem};
use crate::levelgen::settings::world_dimensions::WorldDimensions;
use rivet_registry::holder::Holder;
use rivet_registry::registry_file_codec::RegistryFileCodec;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_registry::resource_key::resource_key_codec;
use rivet_registry::{ResourceKey, registries};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.presets.WorldPreset`.
#[derive(Debug, Clone)]
pub struct WorldPreset {
    /// `dimensions` — the level-stem map.
    dimensions: HashMap<ResourceKey<registries::LevelStem>, LevelStem>,
}

impl WorldPreset {
    /// The record constructor — the codec's `apply` function. Java's record
    /// does not validate here (`requireOverworld` is the codec's `validate`),
    /// so a map without the overworld stem constructs fine and only the
    /// decode/encode check rejects it.
    pub fn new(dimensions: HashMap<ResourceKey<registries::LevelStem>, LevelStem>) -> Self {
        WorldPreset { dimensions }
    }

    /// `dimensionsInOrder()` — the ordered `(key, stem)` pairs over
    /// `WorldDimensions.keysInOrder(this.dimensions.keySet())`, skipping null
    /// stems (unreachable in the port: a `LevelStem` is never null).
    ///
    /// Java builds an `ImmutableMap` (insertion-ordered) here and hands it to
    /// `createWorldDimensions()`. The port returns the same keysInOrder
    /// sequence as ordered pairs, but [`Self::create_world_dimensions`] folds
    /// them into a `HashMap` before the `WorldDimensions` record is built, so
    /// the deterministic overworld/nether/end ordering Java preserves is lost
    /// in-unit — matching the settings unit's current `HashMap` storage.
    /// Re-establishing order is the shared `mc.world.level.levelgen.settings`
    /// ordered-storage/LevelStem registry seam: `WorldDimensions` stores the
    /// level-stem map as a `HashMap` (see `world_dimensions`), so Java's
    /// insertion-ordered semantics are dropped until the settings unit adopts
    /// ordered storage (no owning issue yet).
    fn dimensions_in_order(&self) -> Vec<(ResourceKey<registries::LevelStem>, LevelStem)> {
        let keys: HashSet<ResourceKey<registries::LevelStem>> =
            self.dimensions.keys().cloned().collect();
        WorldDimensions::keys_in_order(&keys)
            .into_iter()
            .filter_map(|key| {
                self.dimensions
                    .get(&key)
                    .map(|level_stem| (key, level_stem.clone()))
            })
            .collect()
    }

    /// `createWorldDimensions()` — `new WorldDimensions(this.dimensionsInOrder())`.
    ///
    /// The `WorldDimensions` compact constructor throws when the overworld stem
    /// is missing (`"Overworld settings missing"`); the port panics the same
    /// way. The ordered pairs from [`Self::dimensions_in_order`] are folded
    /// into a `HashMap` here, matching the settings unit's `WorldDimensions`
    /// storage, so the keysInOrder ordering Java's `ImmutableMap` preserves is
    /// dropped before the record is constructed. Java's ordering is observable
    /// to consumers (e.g. the `WorldDimensions.withOverworld` `putAll`
    /// iteration, and encoding/baking the dimension set), so re-establishing it
    /// when the settings unit adopts ordered storage is the shared
    /// `mc.world.level.levelgen.settings` ordered-storage/LevelStem registry
    /// seam (no owning issue yet).
    pub fn create_world_dimensions(&self) -> WorldDimensions {
        WorldDimensions::new(
            self.dimensions_in_order()
                .into_iter()
                .collect::<HashMap<_, _>>(),
        )
    }

    /// `overworld()` — `Optional.ofNullable(this.dimensions.get(LevelStem.OVERWORLD))`.
    pub fn overworld(&self) -> Option<&LevelStem> {
        self.dimensions.get(&*level_stem::OVERWORLD)
    }

    /// `requireOverworld(WorldPreset)` — the `DIRECT_CODEC` validator: an empty
    /// overworld errors `"Missing overworld dimension"`, otherwise the success
    /// carries `Lifecycle.stable()`.
    fn require_overworld(preset: &WorldPreset) -> DataResult<WorldPreset> {
        if preset.overworld().is_none() {
            DataResult::error("Missing overworld dimension")
        } else {
            DataResult::success_with_lifecycle(preset.clone(), Lifecycle::stable())
        }
    }
}

/// `WorldPreset.DIRECT_CODEC` — the ops-generic `direct_codec::<Ops>()`
/// factory: the single required `"dimensions"` field (`Codec.unboundedMap(
/// ResourceKey.codec(Registries.LEVEL_STEM), LevelStem.CODEC)`), applied to
/// [`WorldPreset::new`], then `.validate(WorldPreset::requireOverworld)`.
pub fn direct_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<WorldPreset, Ops>> {
    let dimensions_field = codec::field_of(
        codec::unbounded_map(
            resource_key_codec::<registries::LevelStem, Ops>(&registries::LEVEL_STEM),
            level_stem::level_stem_codec::<Ops>(),
        ),
        "dimensions".to_string(),
    );
    let base = record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|w: &WorldPreset| w.dimensions.clone()),
                dimensions_field,
            ))
            .apply(instance, Arc::new(WorldPreset::new))
    });
    codec::validate(base, Arc::new(WorldPreset::require_overworld))
}

/// `WorldPreset.CODEC` — `RegistryFileCodec.create(Registries.WORLD_PRESET,
/// DIRECT_CODEC)`, the `Holder<WorldPreset>` codec, as the ops-generic
/// `world_preset_codec::<Ops>()` factory.
pub fn world_preset_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Holder<WorldPreset>, Ops>> {
    Arc::new(RegistryFileCodec::create(
        &*crate::levelgen::presets::WORLD_PRESET,
        direct_codec::<Ops>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::settings::debug_level_source::DebugLevelSource;
    use rivet_registry::biome_id::BiomeId;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn debug_stem() -> LevelStem {
        let source = DebugLevelSource::new(Holder::direct(BiomeId::from_id(40)));
        LevelStem::new(Holder::direct(registries::DimensionType), Arc::new(source))
    }

    fn stem_map() -> HashMap<ResourceKey<registries::LevelStem>, LevelStem> {
        HashMap::from([
            ((*level_stem::OVERWORLD).clone(), debug_stem()),
            ((*level_stem::NETHER).clone(), debug_stem()),
            ((*level_stem::END).clone(), debug_stem()),
        ])
    }

    #[test]
    fn overworld_returns_the_stem_or_none() {
        let preset = WorldPreset::new(stem_map());
        assert!(preset.overworld().is_some());
        let no_overworld = WorldPreset::new(HashMap::from([(
            (*level_stem::NETHER).clone(),
            debug_stem(),
        )]));
        assert!(no_overworld.overworld().is_none());
    }

    #[test]
    fn create_world_dimensions_validates_the_overworld_present() {
        let preset = WorldPreset::new(stem_map());
        let dims = preset.create_world_dimensions();
        assert!(dims.get(&level_stem::OVERWORLD).is_some());
        assert!(dims.get(&level_stem::NETHER).is_some());
        assert!(dims.get(&level_stem::END).is_some());
    }

    #[test]
    fn direct_codec_missing_overworld_is_an_error_not_a_panic() {
        // The `validate` checker runs before the inner map codec on encode, so
        // a preset without the overworld stem errors with Java's exact message.
        let codec = direct_codec::<JsonOps>();
        let no_overworld = WorldPreset::new(HashMap::from([(
            (*level_stem::NETHER).clone(),
            debug_stem(),
        )]));
        let encoded = codec.encode_start(&JsonOps::INSTANCE, &no_overworld);
        let error = encoded
            .error_ref()
            .expect("a missing overworld must be a DataResult error");
        assert_eq!(error.message(), "Missing overworld dimension");
    }

    #[test]
    fn direct_codec_with_overworld_reaches_the_level_stem_seam() {
        // A preset WITH the overworld stem passes `requireOverworld` and the
        // encode reaches the `LevelStem.CODEC` poison leaf, which errors naming
        // the `mc.world.level.dimension`/`mc.world.level.chunk.generator`
        // deferrals — the structure is faithful, the round-trip unavailable
        // until the owning units land.
        let codec = direct_codec::<JsonOps>();
        let preset = WorldPreset::new(stem_map());
        let encoded = codec.encode_start(&JsonOps::INSTANCE, &preset);
        let error = encoded
            .error_ref()
            .expect("the dimensions codec must error through the LevelStem seam");
        let message = error.message();
        assert!(
            message.contains("LevelStem.CODEC") && message.contains("mc.world.level.dimension"),
            "the seam must name the LevelStem deferral, got: {message}"
        );
    }

    #[test]
    fn direct_codec_decode_structure_is_a_dimensions_field() {
        // The map shape is faithful: an empty `"dimensions"` map decodes and
        // the overworld check errors (no stems to hit the poison leaf).
        let codec = direct_codec::<JsonOps>();
        let decoded = codec.parse(&JsonOps::INSTANCE, &json!({ "dimensions": {} }));
        let error = decoded
            .error_ref()
            .expect("a missing overworld must be a DataResult error");
        assert_eq!(error.message(), "Missing overworld dimension");
    }
}
