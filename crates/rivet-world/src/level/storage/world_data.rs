//! `net.minecraft.world.level.storage.WorldData` — the world's persistent
//! data facade.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/WorldData.java`. The root interface every level-data implementation
//! (concretely `PrimaryLevelData`) exposes: storage version, modded-info
//! bookkeeping, the crash-report default, and the value reads/writes the
//! level-data slice needs.
//!
//! ## Deferred members (out of scope)
//!
//! - `createTag(@Nullable UUID)` — the full `PrimaryLevelData` write path
//!   (`PrimaryLevelData.save` is out of scope, #323; needs `CompoundTag` store
//!   + worldgen settings). No declaration is emitted — a `RivetTodo(#398)`
//!     marker notes it.
//!
//! `getDataConfiguration`/`setDataConfiguration`/`getLevelSettings` and the
//! `enabledFeatures` default are present here (values from #486); the concrete
//! `PrimaryLevelData` (#323) backs them from its `LevelSettings`.

use rivet_registry::core::{Difficulty, GameType};
use rivet_serialization::Lifecycle;
use rivet_util::mth::Uuid;

use super::server_level_data::ServerLevelData;
use crate::level::world_data_configuration::WorldDataConfiguration;
use indexmap::IndexSet;

/// `WorldData.ANVIL_VERSION_ID` — `19133` (Java's interface constant
/// `WorldData.ANVIL_VERSION_ID`).
///
/// Module-level rather than a trait associated const: the trait is `dyn`
/// compatible (the concrete `PrimaryLevelData` doesn't exist yet, so
/// `DerivedLevelData` holds `&dyn WorldData`), and associated consts would
/// break that.
pub const ANVIL_VERSION_ID: i32 = 19133;

/// `WorldData.MCREGION_VERSION_ID` — `19132`.
pub const MCREGION_VERSION_ID: i32 = 19132;

/// `WorldData` — the world's persistent data facade.
pub trait WorldData {
    /// `wasModded()`.
    fn was_modded(&self) -> bool;

    /// `getKnownServerBrands()`.
    fn get_known_server_brands(&self) -> &IndexSet<String>;

    /// `getRemovedFeatureFlags()`.
    ///
    /// RivetTodo(#398): Java backs this with a `HashSet` (unspecified
    /// hash-probe iteration) vs the `IndexSet` insertion order — a future
    /// crash-report/serialized ordering divergence.
    fn get_removed_feature_flags(&self) -> &IndexSet<String>;

    /// `setModdedInfo(String serverBrand, boolean isModded)`.
    fn set_modded_info(&mut self, server_brand: &str, is_modded: bool);

    /// `WorldData.fillCrashReportCategory(CrashReportCategory)`.
    ///
    /// Java:
    /// ```java
    /// default void fillCrashReportCategory(final CrashReportCategory category) {
    ///     category.setDetail("Known server brands", () -> String.join(", ", this.getKnownServerBrands()));
    ///     category.setDetail("Removed feature flags", () -> String.join(", ", this.getRemovedFeatureFlags()));
    ///     category.setDetail("Level was modded", () -> Boolean.toString(this.wasModded()));
    ///     category.setDetail("Level storage version", () -> {
    ///         int version = this.getVersion();
    ///         return String.format(Locale.ROOT, "0x%05X - %s", version, this.getStorageVersionName(version));
    ///     });
    /// }
    /// ```
    ///
    /// `String.join(", ", set)` joins with `", "` in iteration order
    /// (`IndexSet` = the Java `LinkedHashSet` stand-in); `%05X`
    /// zero-pads the uppercase hex version to width 5.
    fn fill_crash_report_category(&self, category: &mut rivet_core::CrashReportCategory) {
        category.set_detail(
            "Known server brands",
            join_strings(self.get_known_server_brands()),
        );
        category.set_detail(
            "Removed feature flags",
            join_strings(self.get_removed_feature_flags()),
        );
        category.set_detail("Level was modded", self.was_modded());
        let version = self.get_version();
        category.set_detail(
            "Level storage version",
            format!(
                "0x{version:05X} - {}",
                self.get_storage_version_name(version)
            ),
        );
    }

    /// `WorldData.getStorageVersionName(int)` — the storage-version name
    /// lookup (a Java default *instance* method).
    fn get_storage_version_name(&self, version: i32) -> &'static str {
        match version {
            19132 => "McRegion",
            19133 => "Anvil",
            _ => "Unknown?",
        }
    }

    /// `overworldData()`.
    fn overworld_data(&self) -> &dyn ServerLevelData;

    // `createTag(@Nullable UUID)` — the `PrimaryLevelData` write path — defers
    // with `CompoundTag` + worldgen settings (RivetTodo(#398), no declaration
    // emitted).

    /// `isHardcore()`.
    fn is_hardcore(&self) -> bool;

    /// `getVersion()`.
    fn get_version(&self) -> i32;

    /// `getLevelName()`.
    fn get_level_name(&self) -> &str;

    /// `getGameType()`.
    fn get_game_type(&self) -> GameType;

    /// `setGameType(GameType)`.
    fn set_game_type(&mut self, game_type: GameType);

    /// `isAllowCommands()`.
    fn is_allow_commands(&self) -> bool;

    /// `setAllowCommands(boolean)`.
    fn set_allow_commands(&mut self, allow_commands: bool);

    /// `getDifficulty()`.
    fn get_difficulty(&self) -> Difficulty;

    /// `setDifficulty(Difficulty)`.
    fn set_difficulty(&mut self, difficulty: Difficulty);

    /// `isDifficultyLocked()`.
    fn is_difficulty_locked(&self) -> bool;

    /// `setDifficultyLocked(boolean)`.
    fn set_difficulty_locked(&mut self, difficulty_locked: bool);

    /// `getSinglePlayerUUID()` — `@Nullable UUID`.
    fn get_single_player_uuid(&self) -> Option<Uuid>;

    /// `isFlatWorld()`.
    fn is_flat_world(&self) -> bool;

    /// `isDebugWorld()`.
    fn is_debug_world(&self) -> bool;

    /// `worldGenSettingsLifecycle()`.
    fn world_gen_settings_lifecycle(&self) -> Lifecycle;

    /// `getDataConfiguration()`.
    fn get_data_configuration(&self) -> &WorldDataConfiguration;

    /// `setDataConfiguration(WorldDataConfiguration)`.
    fn set_data_configuration(&mut self, data_configuration: WorldDataConfiguration);

    /// `getLevelSettings()`.
    fn get_level_settings(&self) -> crate::level::level_settings::LevelSettings;

    /// `enabledFeatures()` — `getDataConfiguration().enabledFeatures()`.
    fn enabled_features(&self) -> &crate::flag::FeatureFlagSet {
        self.get_data_configuration().enabled_features()
    }
}

/// `String.join(", ", Collection)` over the set's iteration order.
fn join_strings(set: &IndexSet<String>) -> String {
    set.iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::storage::server_level_data::ServerLevelData;
    use rivet_serialization::Lifecycle;

    struct FakeWorldData {
        modded: bool,
        known_server_brands: IndexSet<String>,
        removed_feature_flags: IndexSet<String>,
        version: i32,
        level_name: &'static str,
        game_type: GameType,
        allow_commands: bool,
        difficulty: Difficulty,
        difficulty_locked: bool,
        single_player_uuid: Option<Uuid>,
        flat: bool,
        debug: bool,
        lifecycle: Lifecycle,
    }

    impl FakeWorldData {
        fn standard() -> Self {
            let mut known = IndexSet::new();
            known.insert("vanilla".to_string());
            let mut removed = IndexSet::new();
            removed.insert("minecraft:experimental".to_string());
            FakeWorldData {
                modded: false,
                known_server_brands: known,
                removed_feature_flags: removed,
                version: 19133,
                level_name: "world",
                game_type: GameType::Survival,
                allow_commands: false,
                difficulty: Difficulty::Normal,
                difficulty_locked: true,
                single_player_uuid: None,
                flat: false,
                debug: false,
                lifecycle: Lifecycle::Stable,
            }
        }
    }

    impl WorldData for FakeWorldData {
        fn was_modded(&self) -> bool {
            self.modded
        }

        fn get_known_server_brands(&self) -> &IndexSet<String> {
            &self.known_server_brands
        }

        fn get_removed_feature_flags(&self) -> &IndexSet<String> {
            &self.removed_feature_flags
        }

        fn set_modded_info(&mut self, server_brand: &str, is_modded: bool) {
            self.known_server_brands.insert(server_brand.to_string());
            self.modded |= is_modded;
        }

        fn overworld_data(&self) -> &dyn ServerLevelData {
            panic!("overworld data is not used by WorldData-only tests")
        }

        fn is_hardcore(&self) -> bool {
            false
        }

        fn get_version(&self) -> i32 {
            self.version
        }

        fn get_level_name(&self) -> &str {
            self.level_name
        }

        fn get_game_type(&self) -> GameType {
            self.game_type
        }

        fn set_game_type(&mut self, game_type: GameType) {
            self.game_type = game_type;
        }

        fn is_allow_commands(&self) -> bool {
            self.allow_commands
        }

        fn set_allow_commands(&mut self, allow_commands: bool) {
            self.allow_commands = allow_commands;
        }

        fn get_difficulty(&self) -> Difficulty {
            self.difficulty
        }

        fn set_difficulty(&mut self, difficulty: Difficulty) {
            self.difficulty = difficulty;
        }

        fn is_difficulty_locked(&self) -> bool {
            self.difficulty_locked
        }

        fn set_difficulty_locked(&mut self, difficulty_locked: bool) {
            self.difficulty_locked = difficulty_locked;
        }

        fn get_single_player_uuid(&self) -> Option<Uuid> {
            self.single_player_uuid
        }

        fn is_flat_world(&self) -> bool {
            self.flat
        }

        fn is_debug_world(&self) -> bool {
            self.debug
        }

        fn world_gen_settings_lifecycle(&self) -> Lifecycle {
            self.lifecycle
        }

        fn get_data_configuration(&self) -> &WorldDataConfiguration {
            panic!("data configuration is not used by WorldData-only tests")
        }

        fn set_data_configuration(&mut self, _data_configuration: WorldDataConfiguration) {}

        fn get_level_settings(&self) -> crate::level::level_settings::LevelSettings {
            panic!("level settings are not used by WorldData-only tests")
        }
    }

    /// The version id constants.
    #[test]
    fn version_ids() {
        assert_eq!(ANVIL_VERSION_ID, 19133);
        assert_eq!(MCREGION_VERSION_ID, 19132);
    }

    /// `getStorageVersionName` maps the two known ids and "Unknown?" otherwise.
    #[test]
    fn storage_version_name_lookup() {
        let data = FakeWorldData::standard();
        assert_eq!(data.get_storage_version_name(19132), "McRegion");
        assert_eq!(data.get_storage_version_name(19133), "Anvil");
        assert_eq!(data.get_storage_version_name(19131), "Unknown?");
        assert_eq!(data.get_storage_version_name(0), "Unknown?");
    }

    /// `fillCrashReportCategory` records the four details in Java order, with
    /// the `0x%05X` hex-storage-version formatting.
    #[test]
    fn fill_crash_report_category_records_details() {
        let mut data = FakeWorldData::standard();
        data.version = 19132;
        data.modded = true;
        data.removed_feature_flags
            .insert("minecraft:update_1_21".to_string());
        let mut category = rivet_core::CrashReportCategory::new("test");
        WorldData::fill_crash_report_category(&data, &mut category);
        assert_eq!(
            category.entries(),
            &[
                ("Known server brands".to_string(), "vanilla".to_string()),
                (
                    "Removed feature flags".to_string(),
                    "minecraft:experimental, minecraft:update_1_21".to_string()
                ),
                ("Level was modded".to_string(), "true".to_string()),
                (
                    "Level storage version".to_string(),
                    "0x04ABC - McRegion".to_string()
                ),
            ]
        );
    }

    /// The storage-version detail for the Anvil id (19133 → 0x4ABD).
    #[test]
    fn storage_version_detail_is_zero_padded_hex() {
        let data = FakeWorldData::standard();
        let mut category = rivet_core::CrashReportCategory::new("test");
        WorldData::fill_crash_report_category(&data, &mut category);
        let last = &category.entries()[3];
        assert_eq!(last.0, "Level storage version");
        assert_eq!(last.1, "0x04ABD - Anvil");
    }

    /// `setModdedInfo` inserts the brand and ORs the modded flag.
    #[test]
    fn set_modded_info_inserts_brand_and_ors_modded() {
        let mut data = FakeWorldData::standard();
        assert!(!data.modded);
        data.set_modded_info("paper", false);
        assert!(data.known_server_brands.contains("paper"));
        assert!(!data.modded);
        data.set_modded_info("fabric", true);
        assert!(data.modded);
        // OR semantics: a second false after true stays true.
        data.set_modded_info("quilt", false);
        assert!(data.modded);
    }

    /// The remaining value setters mutate their fields.
    #[test]
    fn value_setters_mutate() {
        let mut data = FakeWorldData::standard();
        data.set_game_type(GameType::Creative);
        data.set_allow_commands(true);
        data.set_difficulty(Difficulty::Hard);
        data.set_difficulty_locked(false);
        assert_eq!(data.game_type, GameType::Creative);
        assert!(data.allow_commands);
        assert_eq!(data.difficulty, Difficulty::Hard);
        assert!(!data.difficulty_locked);
    }
}
