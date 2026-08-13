//! Port of `net.minecraft.world.level.levelgen.WorldDimensions` (record, 26.2) —
//! the `mc.world.level.levelgen.settings` unit.
//!
//! The dimension map behind `WorldGenSettings`: a `Map<ResourceKey<LevelStem>,
//! LevelStem>` with the overworld-presence compact-constructor validation, the
//! `"dimensions"` map codec (`Codec.unboundedMap(ResourceKey.codec(LEVEL_STEM),
//! LevelStem.CODEC)`), and the ordering/overworld helpers. The `LevelStem`
//! value is the out-of-unit shell in [`level_stem`].
//!
//! ### The codec seam
//!
//! `WorldDimensions.CODEC` is structured faithfully over `LevelStem.CODEC`; the
//! leaf codec is a poison seam (it errors with a `DataResult::error` naming the
//! #388/#185 deferrals — see `level_stem::level_stem_codec`), so the dimensions
//! round-trip is unavailable until the `mc.world.level.dimension` and
//! `mc.world.level.chunk.generator` units land. The structure (the
//! `"dimensions"` field, the `ResourceKey` key codec) is ported and tested.
//!
//! ### The deferred seams
//!
//! - `isDebug()` needs `this.overworld() instanceof DebugLevelSource` — a
//!   concrete-type check on the `&dyn ChunkGenerator` the trait returns. The
//!   `ChunkGenerator` trait (RivetTodo #185) has no `&dyn Any` bridge, so the
//!   check fails explicitly (the `BiomeSource::as_any` precedent).
//! - `replaceOverworldGenerator`/`withOverworld(HolderLookup)` need
//!   `HolderLookup<DimensionType>` + `BuiltinDimensionTypes.OVERWORLD` (the
//!   pending `mc.world.level.dimension` unit, #388).
//! - `bake` needs the level-stem `Registry` construction (the `#213` registry
//!   element placeholder), the `MappedRegistry` lifecycle, and the
//!   `PrimaryLevelData.SpecialWorldProperty` computation — the world-creation
//!   flow (the `Complete` record it returns defers with it).

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::levelgen::settings::level_stem::{self, LevelStem};
use rivet_registry::ResourceKey;
use rivet_registry::holder::Holder;
use rivet_registry::registries;
use rivet_registry::resource_key::resource_key_codec;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// `WorldDimensions.BUILTIN_ORDER` — `ImmutableSet.of(LevelStem.OVERWORLD,
/// LevelStem.NETHER, LevelStem.END)`.
///
/// A function (not a `static`): the `LazyLock` deref that yields the
/// `&'static ResourceKey` is not a `const` operation.
pub fn builtin_order() -> [&'static ResourceKey<registries::LevelStem>; 3] {
    [
        &level_stem::OVERWORLD,
        &level_stem::NETHER,
        &level_stem::END,
    ]
}

/// `net.minecraft.world.level.levelgen.WorldDimensions`.
#[derive(Debug, Clone)]
pub struct WorldDimensions {
    /// `dimensions` — the level-stem map.
    dimensions: HashMap<ResourceKey<registries::LevelStem>, LevelStem>,
}

impl WorldDimensions {
    /// The record constructor — the compact-constructor validation: the
    /// overworld stem must be present, else Java throws
    /// `IllegalStateException("Overworld settings missing")` (the port panics).
    pub fn new(dimensions: HashMap<ResourceKey<registries::LevelStem>, LevelStem>) -> Self {
        if !dimensions.contains_key(&*level_stem::OVERWORLD) {
            panic!("Overworld settings missing");
        }
        WorldDimensions { dimensions }
    }

    /// `dimensions()`.
    pub fn dimensions(&self) -> &HashMap<ResourceKey<registries::LevelStem>, LevelStem> {
        &self.dimensions
    }

    /// `WorldDimensions(Registry<LevelStem>)` — builds the map from
    /// `registry.listElements()`.
    ///
    /// The level-stem `Registry` element is the `#213` placeholder (`LevelStem`
    /// cannot be registered yet), so the constructor is a typed seam: the
    /// world-creation flow that bakes a level-stem registry and passes it here
    /// lands with the dimension/storage units (RivetTodo #388).
    pub fn from_registry(
        _registry: &rivet_registry::registry::Registry<registries::LevelStem>,
    ) -> Self {
        panic!(
            "WorldDimensions(Registry<LevelStem>) is not implemented (RivetTodo #388): the level-stem registry element is the #213 placeholder"
        )
    }

    /// `keysInOrder(Set<ResourceKey<LevelStem>>)` — the builtin keys first (in
    /// `BUILTIN_ORDER`), then the remaining known keys.
    pub fn keys_in_order(
        known_keys: &HashSet<ResourceKey<registries::LevelStem>>,
    ) -> Vec<ResourceKey<registries::LevelStem>> {
        let builtin = builtin_order();
        let mut result = Vec::with_capacity(known_keys.len());
        for key in builtin {
            if known_keys.contains(key) {
                result.push((*key).clone());
            }
        }
        for key in known_keys {
            if !builtin.contains(&key) {
                result.push(key.clone());
            }
        }
        result
    }

    /// `overworld()` — `this.dimensions.get(LevelStem.OVERWORLD).generator()`,
    /// with the same `"Overworld settings missing"` guard.
    pub fn overworld(&self) -> Arc<dyn ChunkGenerator> {
        let stem = self
            .dimensions
            .get(&*level_stem::OVERWORLD)
            .unwrap_or_else(|| panic!("Overworld settings missing"));
        stem.generator.clone()
    }

    /// `get(ResourceKey<LevelStem>)` — `Optional.ofNullable(this.dimensions.get(key))`.
    pub fn get(&self, key: &ResourceKey<registries::LevelStem>) -> Option<&LevelStem> {
        self.dimensions.get(key)
    }

    /// `levels()` — `this.dimensions().keySet().stream().map(Registries::
    /// levelStemToLevel).collect(ImmutableSet.toImmutableSet())`.
    pub fn levels(&self) -> HashSet<ResourceKey<registries::Level>> {
        self.dimensions
            .keys()
            .map(registries::level_stem_to_level)
            .collect()
    }

    /// `isDebug()` — `this.overworld() instanceof DebugLevelSource`.
    ///
    /// The concrete-type check needs a `&dyn Any` bridge on the `ChunkGenerator`
    /// trait; the `#185` owning realization provides it (the
    /// `BiomeSource::as_any` precedent). Fails explicitly rather than fabricate
    /// a result.
    pub fn is_debug(&self) -> bool {
        panic!(
            "WorldDimensions.isDebug is not implemented (RivetTodo #185): needs a ChunkGenerator type-downcast (as_any)"
        )
    }

    /// `replaceOverworldGenerator(HolderLookup.Provider, ChunkGenerator)` — the
    /// `registries.lookupOrThrow(DIMENSION_TYPE)` holder lookup defers with the
    /// `mc.world.level.dimension` unit (#388).
    pub fn replace_overworld_generator(
        &self,
        _generator: Arc<dyn ChunkGenerator>,
    ) -> WorldDimensions {
        panic!(
            "WorldDimensions.replaceOverworldGenerator is not implemented (RivetTodo #388): needs HolderLookup.Provider/DimensionType"
        )
    }

    /// `withOverworld(HolderLookup<DimensionType>, Map, ChunkGenerator)` — the
    /// holder-lookup overload. The 3-arg form without the lookup is ported
    /// ([`Self::with_overworld`]).
    pub fn with_overworld_lookup(
        _dimension_types: &dyn rivet_registry::holder_lookup::HolderLookup<registries::DimensionType>,
        _dimensions: &HashMap<ResourceKey<registries::LevelStem>, LevelStem>,
        _generator: Arc<dyn ChunkGenerator>,
    ) -> HashMap<ResourceKey<registries::LevelStem>, LevelStem> {
        panic!(
            "WorldDimensions.withOverworld(HolderLookup) is not implemented (RivetTodo #388): needs HolderLookup<DimensionType>/BuiltinDimensionTypes"
        )
    }

    /// `withOverworld(Map, Holder<DimensionType>, ChunkGenerator)` (static) —
    /// `ImmutableMap.builder().putAll(dimensions).put(OVERWORLD, new
    /// LevelStem(type, generator)).buildKeepingLast()`.
    pub fn with_overworld(
        dimensions: &HashMap<ResourceKey<registries::LevelStem>, LevelStem>,
        ty: Holder<registries::DimensionType>,
        generator: Arc<dyn ChunkGenerator>,
    ) -> HashMap<ResourceKey<registries::LevelStem>, LevelStem> {
        let mut new_dimensions = dimensions.clone();
        new_dimensions.insert(
            (*level_stem::OVERWORLD).clone(),
            LevelStem::new(ty, generator),
        );
        new_dimensions
    }

    /// `bake(Registry<LevelStem>)` — the frozen level-stem registry + the
    /// `SpecialWorldProperty` computation.
    ///
    /// The `MappedRegistry` construction over the level-stem element (the #213
    /// placeholder), the per-stem stability checks, and the
    /// `PrimaryLevelData.SpecialWorldProperty` derivation all defer with the
    /// world-creation flow (the `Complete` record it returns is part of that
    /// seam; RivetTodo #388/#213).
    pub fn bake(
        &self,
        _base_dimensions: &rivet_registry::registry::Registry<registries::LevelStem>,
    ) -> Complete {
        panic!(
            "WorldDimensions.bake is not implemented (RivetTodo #388): needs the level-stem Registry construction (the #213 element placeholder) and the SpecialWorldProperty computation"
        )
    }
}

/// The `WorldDimensions.Complete` record `bake` returns. Its construction (the
/// frozen level-stem registry + the special-world property) defers with the
/// world-creation flow; the record is declared for the class shape.
pub struct Complete {
    /// `dimensions` — the frozen `Registry<LevelStem>` (the #213 element
    /// placeholder until the dimension unit lands).
    pub dimensions: rivet_registry::registry::Registry<registries::LevelStem>,
    /// `specialWorldProperty`.
    pub special_world_property: crate::level::storage::SpecialWorldProperty,
}

/// `WorldDimensions.CODEC` — the ops-generic
/// `world_dimensions_map_codec::<Ops>()` factory.
///
/// The single `"dimensions"` field: `Codec.unboundedMap(ResourceKey.codec(
/// Registries.LEVEL_STEM), LevelStem.CODEC).fieldOf("dimensions")`. The
/// `LevelStem.CODEC` leaf is a poison seam (see the module doc), so the map
/// round-trip errors until that codec lands; the structure is faithful.
pub fn world_dimensions_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<WorldDimensions, Ops>> {
    let dimensions_field = codec::field_of(
        codec::unbounded_map(
            resource_key_codec::<registries::LevelStem, Ops>(&registries::LEVEL_STEM),
            level_stem::level_stem_codec::<Ops>(),
        ),
        "dimensions".to_string(),
    );
    map_codec::stable(record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|w: &WorldDimensions| w.dimensions.clone()),
                dimensions_field,
            ))
            .apply(instance, Arc::new(WorldDimensions::new))
    }))
}

/// `WorldDimensions.CODEC` lifted to a full `Codec` — `map_codec::codec_of`.
pub fn world_dimensions_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<WorldDimensions, Ops>> {
    map_codec::codec_of(world_dimensions_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::settings::debug_level_source::DebugLevelSource;
    use rivet_registry::biome_id::BiomeId;
    use rivet_registry::holder::Holder;
    use rivet_serialization::json_ops::JsonOps;
    use std::collections::HashMap;

    fn debug_stem() -> LevelStem {
        let source = DebugLevelSource::new(Holder::direct(BiomeId::from_id(40)));
        LevelStem::new(Holder::direct(registries::DimensionType), Arc::new(source))
    }

    fn stem_map() -> HashMap<ResourceKey<registries::LevelStem>, LevelStem> {
        let mut map = HashMap::new();
        map.insert((*level_stem::OVERWORLD).clone(), debug_stem());
        map.insert((*level_stem::NETHER).clone(), debug_stem());
        map
    }

    #[test]
    fn constructor_validates_overworld_present() {
        let dims = WorldDimensions::new(stem_map());
        assert_eq!(dims.dimensions().len(), 2);
        // Missing overworld -> the compact-constructor IllegalStateException.
        let mut no_overworld = HashMap::new();
        no_overworld.insert((*level_stem::END).clone(), debug_stem());
        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            WorldDimensions::new(no_overworld)
        }));
        let message = panic_message(panic_result);
        assert_eq!(message, "Overworld settings missing");
    }

    #[test]
    fn overworld_and_get_round_trip() {
        let dims = WorldDimensions::new(stem_map());
        // `overworld()` resolves the stem's generator without panicking.
        let _ = dims.overworld();
        assert!(dims.get(&level_stem::OVERWORLD).is_some());
        assert!(dims.get(&level_stem::NETHER).is_some());
        assert!(dims.get(&level_stem::END).is_none());
    }

    #[test]
    fn keys_in_order_puts_builtins_first() {
        let known: HashSet<ResourceKey<registries::LevelStem>> = [
            (*level_stem::NETHER).clone(),
            (*level_stem::END).clone(),
            (*level_stem::OVERWORLD).clone(),
            ResourceKey::create(
                &registries::LEVEL_STEM,
                rivet_registry::Identifier::with_default_namespace("custom"),
            ),
        ]
        .into_iter()
        .collect();
        let order = WorldDimensions::keys_in_order(&known);
        assert_eq!(order[0], *level_stem::OVERWORLD);
        assert_eq!(order[1], *level_stem::NETHER);
        assert_eq!(order[2], *level_stem::END);
        assert_eq!(order[3].identifier().path(), "custom");
    }

    #[test]
    fn with_overworld_replaces_the_stem() {
        let dims = WorldDimensions::new(stem_map());
        let replacement = DebugLevelSource::new(Holder::direct(BiomeId::from_id(40)));
        let new_map = WorldDimensions::with_overworld(
            dims.dimensions(),
            Holder::direct(registries::DimensionType),
            Arc::new(replacement),
        );
        let replaced = WorldDimensions::new(new_map);
        assert!(replaced.get(&level_stem::OVERWORLD).is_some());
        assert!(replaced.get(&level_stem::NETHER).is_some());
    }

    #[test]
    fn levels_maps_to_level_keys() {
        let dims = WorldDimensions::new(stem_map());
        let levels = dims.levels();
        assert_eq!(levels.len(), 2);
        assert!(levels.contains(&crate::level::overworld()));
        assert!(levels.contains(&crate::level::nether()));
    }

    #[test]
    fn codec_structure_errors_through_the_level_stem_seam() {
        // The codec builds faithfully; encode reaches the LevelStem leaf and
        // fails with its deferral message.
        let codec = map_codec::codec_of(world_dimensions_map_codec::<JsonOps>());
        let dims = WorldDimensions::new(stem_map());
        let encoded = codec.encode_start(&JsonOps::INSTANCE, &dims);
        let error = encoded
            .error_ref()
            .expect("the dimensions codec must error through the LevelStem seam");
        let message = error.message();
        assert!(
            message.contains("LevelStem.CODEC") && message.contains("RivetTodo #388"),
            "the seam must name the LevelStem deferral, got: {message}"
        );
    }

    fn panic_message<T>(result: std::thread::Result<T>) -> String {
        match result {
            Ok(_) => panic!("expected a panic, got Ok"),
            Err(payload) => payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("<non-str panic payload>")
                .to_string(),
        }
    }
}
