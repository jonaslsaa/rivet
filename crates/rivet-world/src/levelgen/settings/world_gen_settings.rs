//! Port of `net.minecraft.world.level.levelgen.WorldGenSettings` (26.2) — the
//! `mc.world.level.levelgen.settings` unit.
//!
//! The world-generation settings record: the `WorldOptions` (seed/structures/
//! bonus-chest) and the `WorldDimensions` level-stem map, plus the `CODEC`
//! (the two settings codecs grouped) and the `SavedDataType` `TYPE`.
//!
//! ### The `SavedDataType` handle
//!
//! `WorldGenSettings extends SavedData` and exposes `TYPE` — a
//! `SavedDataType<WorldGenSettings>` over the `SavedData`/`SavedDataType`
//! base and the `DataFixTypes` value-identity enum from the
//! `mc.world.level.saveddata` unit (deferral #421, closed by that unit). The
//! `TYPE` handle is a `LazyLock` static mirroring Java's `static final TYPE`.
//! Its constructor supplier builds the default
//! `WorldOptions` plus an empty `WorldDimensions` — exactly what Java's `TYPE`
//! passes — so invoking the constructor panics with "Overworld settings
//! missing", matching Java's `WorldDimensions` compact constructor
//! `IllegalStateException` for the same supplier.
//!
//! ### The inherited `SavedData` base
//!
//! Java's `WorldGenSettings` inherits `SavedData`'s `boolean dirty` +
//! `setDirty()`/`setDirty(boolean)`/`isDirty()`. The port does **not** embed
//! the base field: it is structurally and observably dead on this value.
//! `WorldGenSettings` is immutable (`private final` `options`/`dimensions`, no
//! setters), its `hashCode` is overridden as `Objects.hash(options, dimensions)`
//! (dirty excluded), it does not override `equals` (reference identity), and
//! its storage goes straight through `WorldGenSettings.CODEC` —
//! `LevelStorageSource.readExistingSavedData` (`savedDataType.codec().parse(...)`)
//! and `writeSavedData` (`codec.encodeStart(...)`) never read the flag, and no
//! consumer anywhere in Paper calls `isDirty`/`setDirty` on a `WorldGenSettings`.
//! Embedding the base would add an always-false, never-read `bool`. (Compare the
//! mutable saved-data payloads `WeatherData`/`WanderingTraderData`, which embed
//! the base because their dirty-marking setters and the ServerLevel storage
//! runtime make it observable.) Java's `hashCode`/`toString` are likewise not
//! ported for the same reason — no consumer observes them.
//!
//! ### The codec seam
//!
//! `CODEC` groups `WorldOptions.CODEC` with `WorldDimensions.CODEC`. The
//! dimensions leaf reads `LevelStem.CODEC` (the poison seam — see
//! `world_dimensions`/`level_stem`), so the settings round-trip is unavailable
//! until the `mc.world.level.dimension`/`mc.world.level.chunk.generator` units
//! land; the record structure is faithful and tested.

use crate::level::saveddata::saved_data_type::SavedDataType;
use crate::level::saveddata::stub_data_fix_types::DataFixTypes;
use crate::levelgen::settings::world_dimensions::{WorldDimensions, world_dimensions_map_codec};
use crate::levelgen::settings::world_options::{WorldOptions, world_options_map_codec};
use rivet_registry::Identifier;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

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

/// `WorldGenSettings.TYPE` — `new SavedDataType<>(
/// Identifier.withDefaultNamespace("world_gen_settings"), () -> new
/// WorldGenSettings(WorldOptions.defaultWithRandomSeed(), new
/// WorldDimensions(new HashMap<>())), CODEC,
/// DataFixTypes.SAVED_DATA_WORLD_GEN_SETTINGS)`. Java's `static final`
/// singleton is a `LazyLock` static in the port; the codec slot is the
/// NbtOps-pinned codec the disk runtime uses. Invoking the constructor panics
/// on the empty dimensions map — Java's `WorldDimensions` compact constructor
/// throws `IllegalStateException` for the same supplier, so the panic is
/// faithful.
pub static TYPE: LazyLock<SavedDataType<WorldGenSettings>> = LazyLock::new(|| {
    SavedDataType::new(
        Identifier::with_default_namespace("world_gen_settings"),
        Arc::new(|| {
            WorldGenSettings::new(
                WorldOptions::default_with_random_seed(),
                WorldDimensions::new(HashMap::new()),
            )
        }),
        world_gen_settings_codec::<rivet_nbt::nbt_ops::NbtOps>(),
        DataFixTypes::SavedDataWorldGenSettings,
    )
});

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
            message.contains("LevelStem.CODEC") && message.contains("mc.world.level.dimension"),
            "the seam must name the LevelStem deferral, got: {message}"
        );
    }

    #[test]
    fn type_has_expected_identity() {
        let t: &SavedDataType<WorldGenSettings> = &TYPE;
        assert_eq!(t.id().to_string(), "minecraft:world_gen_settings");
        assert_eq!(t.data_fix_type(), DataFixTypes::SavedDataWorldGenSettings);
        assert_eq!(t.to_string(), "SavedDataType[minecraft:world_gen_settings]");
    }

    #[test]
    #[should_panic(expected = "Overworld settings missing")]
    fn type_constructor_panics_on_empty_dimensions() {
        // Java's TYPE supplier passes `new WorldDimensions(new HashMap<>())`;
        // the compact constructor throws `IllegalStateException` — faithful.
        (TYPE.constructor())();
    }
}
