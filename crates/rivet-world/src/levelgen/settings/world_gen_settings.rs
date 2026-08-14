//! Port of `net.minecraft.world.level.levelgen.WorldGenSettings` (26.2) — the
//! `mc.world.level.levelgen.settings` unit.
//!
//! The world-generation settings record: the `WorldOptions` (seed/structures/
//! bonus-chest) and the `WorldDimensions` level-stem map, plus the `CODEC`
//! (the two settings codecs grouped) and the `SavedDataType` `TYPE`.
//!
//! ### The `SavedDataType` seam
//!
//! `WorldGenSettings extends SavedData` and exposes `TYPE` — a
//! `SavedDataType<WorldGenSettings>` over `SavedData`/`DataFixTypes`. The
//! `mc.world.level.saveddata` unit is pending (RivetTodo #421), so `TYPE` is a
//! typed seam: [`world_gen_settings_type`] fails explicitly rather than
//! fabricate the saved-data binding. The `SavedDataTypeShell` marker types the
//! seam until the real type lands.
//!
//! ### The codec seam
//!
//! `CODEC` groups `WorldOptions.CODEC` with `WorldDimensions.CODEC`. The
//! dimensions leaf reads `LevelStem.CODEC` (the poison seam — see
//! `world_dimensions`/`level_stem`), so the settings round-trip is unavailable
//! until the `mc.world.level.dimension`/`mc.world.level.chunk.generator` units
//! land; the record structure is faithful and tested.
//!
//! Java's `hashCode`/`toString` are not ported: `Objects.hash(options,
//! dimensions)` combines Java identity/record hashes that the port's value
//! model does not reproduce, and no consumer observes them.

use crate::levelgen::settings::world_dimensions::{WorldDimensions, world_dimensions_map_codec};
use crate::levelgen::settings::world_options::{WorldOptions, world_options_map_codec};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `WorldGenSettings.TYPE`'s deferred type — the `SavedDataType<WorldGenSettings>`
/// stand-in.
///
/// The `mc.world.level.saveddata` unit owns the real `SavedDataType`/`SavedData`
/// base and `DataFixTypes` (RivetTodo #421); this marker only types the `TYPE`
/// seam until that unit lands.
#[derive(Debug, Clone, Copy)]
pub struct SavedDataTypeShell;

/// `net.minecraft.world.level.levelgen.WorldGenSettings`.
#[derive(Debug, Clone)]
pub struct WorldGenSettings {
    /// `options`.
    options: WorldOptions,
    /// `dimensions`.
    dimensions: WorldDimensions,
}

impl WorldGenSettings {
    /// The record constructor (the codec's `apply` function).
    pub fn new(options: WorldOptions, dimensions: WorldDimensions) -> Self {
        WorldGenSettings {
            options,
            dimensions,
        }
    }

    /// `of(WorldOptions, RegistryAccess)` — builds the dimensions from
    /// `registryAccess.lookupOrThrow(Registries.LEVEL_STEM)`.
    ///
    /// The level-stem registry element is the #213 placeholder
    /// (`WorldDimensions::from_registry`), so the factory is a typed seam: the
    /// world-creation flow that holds the registry access lands with the
    /// dimension/storage units (RivetTodo #388).
    pub fn of(
        _options: WorldOptions,
        _registry_access: &rivet_registry::access::RegistryAccess,
    ) -> Self {
        panic!(
            "WorldGenSettings.of is not implemented (RivetTodo #388): needs RegistryAccess.lookupOrThrow(LEVEL_STEM) over the #213 level-stem element placeholder"
        )
    }

    /// `options()`.
    pub fn options(&self) -> &WorldOptions {
        &self.options
    }

    /// `dimensions()`.
    pub fn dimensions(&self) -> &WorldDimensions {
        &self.dimensions
    }
}

/// `WorldGenSettings.CODEC` — the ops-generic `world_gen_settings_codec::<Ops>()`
/// factory.
///
/// `RecordCodecBuilder.create` over `WorldOptions.CODEC` (flattened into the
/// record) and `WorldDimensions.CODEC` (the `"dimensions"` field), `.stable()`.
/// The dimensions leaf errors through the `LevelStem.CODEC` seam (see the
/// module doc).
pub fn world_gen_settings_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<WorldGenSettings, Ops>>
{
    codec::stable(record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|w: &WorldGenSettings| w.options.clone()),
                world_options_map_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|w: &WorldGenSettings| w.dimensions.clone()),
                world_dimensions_map_codec::<Ops>(),
            ))
            .apply(instance, Arc::new(WorldGenSettings::new))
    }))
}

/// `WorldGenSettings.TYPE` — the `SavedDataType<WorldGenSettings>`.
///
/// Java: `new SavedDataType<>(Identifier.withDefaultNamespace(
/// "world_gen_settings"), () -> new WorldGenSettings(
/// WorldOptions.defaultWithRandomSeed(), new WorldDimensions(new HashMap<>())),
/// CODEC, DataFixTypes.SAVED_DATA_WORLD_GEN_SETTINGS)`. The `SavedData`/
/// `SavedDataType` base and `DataFixTypes` defer with the
/// `mc.world.level.saveddata` unit (RivetTodo #421); the seam fails explicitly
/// rather than fabricate the saved-data binding.
pub fn world_gen_settings_type() -> SavedDataTypeShell {
    panic!(
        "WorldGenSettings.TYPE is not implemented (RivetTodo #421): needs SavedDataType/SavedData/DataFixTypes from mc.world.level.saveddata"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::settings::debug_level_source::DebugLevelSource;
    use crate::levelgen::settings::level_stem::{self, LevelStem};
    use crate::levelgen::settings::world_dimensions::WorldDimensions;
    use rivet_registry::biome_id::BiomeId;
    use rivet_registry::holder::Holder;
    use rivet_registry::registries;
    use std::collections::HashMap;

    fn dimensions_with_overworld() -> WorldDimensions {
        let source = DebugLevelSource::new(Holder::direct(BiomeId::from_id(40)));
        let mut map = HashMap::new();
        map.insert(
            (*level_stem::OVERWORLD).clone(),
            LevelStem::new(Holder::direct(registries::DimensionType), Arc::new(source)),
        );
        WorldDimensions::new(map)
    }

    #[test]
    fn accessors_round_trip() {
        let options = WorldOptions::new(1234, true, false);
        let dimensions = dimensions_with_overworld();
        let settings = WorldGenSettings::new(options.clone(), dimensions.clone());
        assert_eq!(settings.options(), &options);
        // `LevelStem` is behavior (no `PartialEq`), so compare the map sizes.
        assert_eq!(
            settings.dimensions().dimensions().len(),
            dimensions.dimensions().len()
        );
    }

    #[test]
    fn codec_errors_through_the_dimensions_seam() {
        use rivet_serialization::json_ops::JsonOps;

        // The codec builds faithfully; encode reaches the LevelStem leaf and
        // fails with its deferral message.
        let settings = WorldGenSettings::new(
            WorldOptions::new(1, true, false),
            dimensions_with_overworld(),
        );
        let codec = world_gen_settings_codec::<JsonOps>();
        let encoded = codec.encode_start(&JsonOps::INSTANCE, &settings);
        let error = encoded
            .error_ref()
            .expect("the settings codec must error through the LevelStem seam");
        let message = error.message();
        assert!(
            message.contains("LevelStem.CODEC") && message.contains("RivetTodo #388"),
            "the seam must name the LevelStem deferral, got: {message}"
        );
    }
}
