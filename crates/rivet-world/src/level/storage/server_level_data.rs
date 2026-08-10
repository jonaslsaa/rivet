//! `net.minecraft.world.level.storage.ServerLevelData` — the server-side
//! level data read/write surface.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/ServerLevelData.java`. Extends `WritableLevelData` with the level
//! name, game mode, initialized/allow-commands flags, and game time, plus the
//! `fillCrashReportCategory` default that composes the `LevelData` spawn
//! detail with the "Level name" and "Level game mode" details.
//!
//! ## Deviation: `fillCrashReportCategory` override dispatch
//!
//! The Java default calls `WritableLevelData.super.fillCrashReportCategory(...)`
//! — Java's `Interface.super` always runs the interface **default body**, never
//! a concrete override. The Rust port's `LevelData::fill_crash_report_category(
//! self, ...)` inside the default resolves through the vtable, so a concrete
//! type that *overrides* `LevelData::fill_crash_report_category` (none in-tree
//! today — `PrimaryLevelData.parse` is out of scope, #398) would run its
//! override instead of the default. Accepted: the only in-tree impls use the
//! default body, so behavior is identical; documented for the future
//! `PrimaryLevelData` port.
//!
//! The `"Level game mode"` detail is `String.format(Locale.ROOT,
//! "Game mode: %s (ID %d). Hardcore: %b. Commands: %b", getName(), getId(),
//! isHardcore(), isAllowCommands())`. `%b` on a boolean is `"true"`/`"false"`
//! — matching Rust's `{}` bool Display. `Locale.ROOT` means no digit grouping,
//! also matching Rust integer Display.

use rivet_registry::core::GameType;

use super::super::height_accessor::LevelHeightAccessor;
use super::level_data::LevelData;
use super::writable_level_data::WritableLevelData;

/// `ServerLevelData` — the read/write server level data surface.
pub trait ServerLevelData: WritableLevelData {
    /// `getLevelName()`.
    fn get_level_name(&self) -> &str;

    /// `getGameType()`.
    fn get_game_type(&self) -> GameType;

    /// `isInitialized()`.
    fn is_initialized(&self) -> bool;

    /// `setInitialized(boolean)`.
    fn set_initialized(&mut self, initialized: bool);

    /// `isAllowCommands()`.
    fn is_allow_commands(&self) -> bool;

    /// `setAllowCommands(boolean)`.
    fn set_allow_commands(&mut self, allow_commands: bool);

    /// `setGameType(GameType)`.
    fn set_game_type(&mut self, game_type: GameType);

    /// `setGameTime(long)`.
    fn set_game_time(&mut self, time: i64);

    /// `ServerLevelData.fillCrashReportCategory` — the `LevelData` spawn
    /// detail plus "Level name" and "Level game mode".
    ///
    /// Java composes `WritableLevelData.super.fillCrashReportCategory(category,
    /// levelHeightAccessor)` with the two level details. See the module docs
    /// for the override-dispatch deviation.
    fn fill_crash_report_category(
        &self,
        category: &mut rivet_core::CrashReportCategory,
        level_height_accessor: &dyn LevelHeightAccessor,
    ) {
        // WritableLevelData.super.fillCrashReportCategory(...)
        LevelData::fill_crash_report_category(self, category, level_height_accessor);
        category.set_detail("Level name", self.get_level_name());
        category.set_detail(
            "Level game mode",
            format!(
                "Game mode: {} (ID {}). Hardcore: {}. Commands: {}",
                self.get_game_type().get_name(),
                self.get_game_type().get_id(),
                self.is_hardcore(),
                self.is_allow_commands(),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::storage::level_data::{LevelData, RespawnData, default_respawn_data};
    use crate::level::storage::writable_level_data::WritableLevelData;

    /// A fake `ServerLevelData` for the default-behavior tests.
    struct FakeServerLevelData {
        respawn: RespawnData,
        game_time: i64,
        level_name: &'static str,
        game_type: GameType,
        initialized: bool,
        allow_commands: bool,
        hardcore: bool,
    }

    impl LevelData for FakeServerLevelData {
        fn get_respawn_data(&self) -> &RespawnData {
            &self.respawn
        }

        fn get_game_time(&self) -> i64 {
            self.game_time
        }

        fn is_hardcore(&self) -> bool {
            self.hardcore
        }

        fn get_difficulty(&self) -> rivet_registry::core::Difficulty {
            rivet_registry::core::Difficulty::Normal
        }

        fn is_difficulty_locked(&self) -> bool {
            false
        }
    }

    impl WritableLevelData for FakeServerLevelData {
        fn set_spawn(&mut self, respawn_data: RespawnData) {
            self.respawn = respawn_data;
        }
    }

    impl ServerLevelData for FakeServerLevelData {
        fn get_level_name(&self) -> &str {
            self.level_name
        }

        fn get_game_type(&self) -> GameType {
            self.game_type
        }

        fn is_initialized(&self) -> bool {
            self.initialized
        }

        fn set_initialized(&mut self, initialized: bool) {
            self.initialized = initialized;
        }

        fn is_allow_commands(&self) -> bool {
            self.allow_commands
        }

        fn set_allow_commands(&mut self, allow_commands: bool) {
            self.allow_commands = allow_commands;
        }

        fn set_game_type(&mut self, game_type: GameType) {
            self.game_type = game_type;
        }

        fn set_game_time(&mut self, time: i64) {
            self.game_time = time;
        }
    }

    fn fake_level_data() -> FakeServerLevelData {
        FakeServerLevelData {
            respawn: default_respawn_data(),
            game_time: 1234,
            level_name: "world",
            game_type: GameType::Creative,
            initialized: false,
            allow_commands: true,
            hardcore: true,
        }
    }

    fn overworld_height() -> crate::level::height_accessor::SimpleLevelHeightAccessor {
        crate::level::height_accessor::create(-64, 384)
    }

    /// `ServerLevelData.fillCrashReportCategory` composes the `LevelData`
    /// spawn detail with "Level name" and "Level game mode" (Java order).
    #[test]
    fn fill_crash_report_category_composes_level_data_and_server_details() {
        let data = fake_level_data();
        let mut category = rivet_core::CrashReportCategory::new("test");
        ServerLevelData::fill_crash_report_category(&data, &mut category, &overworld_height());
        assert_eq!(
            category.entries(),
            &[
                (
                    "Level spawn location".to_string(),
                    "World: (0,0,0), Section: (at 0,0,0 in 0,0,0; chunk contains blocks 0,-64,0 to 15,319,15), Region: (0,0; contains chunks 0,0 to 31,31, blocks 0,-64,0 to 511,319,511)".to_string()
                ),
                ("Level name".to_string(), "world".to_string()),
                (
                    "Level game mode".to_string(),
                    "Game mode: creative (ID 1). Hardcore: true. Commands: true".to_string()
                ),
            ]
        );
    }

    /// The Java `%b` booleans format as `true`/`false` — the same as Rust's
    /// bool Display.
    #[test]
    fn game_mode_detail_matches_java_boolean_format() {
        let mut data = fake_level_data();
        data.allow_commands = false;
        data.hardcore = false;
        let mut category = rivet_core::CrashReportCategory::new("test");
        ServerLevelData::fill_crash_report_category(&data, &mut category, &overworld_height());
        let game_mode = &category.entries()[2];
        assert_eq!(game_mode.0, "Level game mode");
        assert_eq!(
            game_mode.1,
            "Game mode: creative (ID 1). Hardcore: false. Commands: false"
        );
    }

    /// The setters mutate the concrete state (the `DerivedLevelData` no-ops
    /// are a separate concern).
    #[test]
    fn setters_mutate_state() {
        let mut data = fake_level_data();
        data.set_game_time(9999);
        data.set_game_type(GameType::Spectator);
        data.set_initialized(true);
        data.set_allow_commands(false);
        assert_eq!(data.game_time, 9999);
        assert_eq!(data.game_type, GameType::Spectator);
        assert!(data.initialized);
        assert!(!data.allow_commands);
    }

    /// `setSpawn` replaces the respawn data.
    #[test]
    fn set_spawn_replaces_respawn() {
        let mut data = fake_level_data();
        let new_spawn = RespawnData::new(
            rivet_registry::core::GlobalPos::of(
                crate::level::level::overworld(),
                rivet_registry::core::BlockPos::new(1, 2, 3),
            ),
            0.0,
            0.0,
        );
        data.set_spawn(new_spawn);
        assert_eq!(
            data.respawn.pos(),
            rivet_registry::core::BlockPos::new(1, 2, 3)
        );
    }
}
