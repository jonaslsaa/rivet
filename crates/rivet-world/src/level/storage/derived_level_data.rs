//! `net.minecraft.world.level.storage.DerivedLevelData` — the derived
//! (dimension-scoped) level data wrapper.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/DerivedLevelData.java`. Vanilla uses this for non-overworld
//! dimensions: the wrapped `ServerLevelData` (the overworld's data) carries
//! the runtime-per-dimension state (spawn, game time, initialized), while the
//! `WorldData` (the overworld's `PrimaryLevelData`) supplies the
//! world-level reads (level name, game type, hardcore, difficulty). The
//! *setters* that mutate world-wide settings (`setGameTime`, `setGameType`,
//! `setAllowCommands`, `setInitialized`) are **no-ops** — Java deliberately
//! drops them so a derived dimension cannot change the world's global data.
//! `setSpawn` is the one setter that *delegates* to the wrapped data.
//!
//! ## Ownership
//!
//! The Java constructor takes `(WorldData, ServerLevelData)` — vanilla calls
//! `new DerivedLevelData(worldData, worldData.overworldData())`, passing the
//! *same* object (Paper 26.2 has no remaining construction site — the
//! `MinecraftServer` import is stale). Rust cannot hold `&` and `&mut` to the
//! same object, so the port's `DerivedLevelData` holds two distinct
//! references and the caller supplies them. Read access reborrows the
//! `&mut` wrapped field through `&self` (the wrapped object's reads do not
//! mutate).

use rivet_registry::core::{Difficulty, GameType};

use super::super::height_accessor::LevelHeightAccessor;
use super::level_data::{LevelData, RespawnData};
use super::server_level_data::ServerLevelData;
use super::world_data::WorldData;

/// `DerivedLevelData` — a `ServerLevelData` that delegates the
/// per-dimension runtime state to a wrapped `ServerLevelData` and the
/// world-level reads to a `WorldData`.
pub struct DerivedLevelData<'a> {
    world_data: &'a dyn WorldData,
    wrapped: &'a mut dyn ServerLevelData,
}

impl<'a> DerivedLevelData<'a> {
    /// `new DerivedLevelData(WorldData, ServerLevelData)`.
    pub fn new(world_data: &'a dyn WorldData, wrapped: &'a mut dyn ServerLevelData) -> Self {
        DerivedLevelData {
            world_data,
            wrapped,
        }
    }

    /// The Java `fillCrashReportCategory` override body:
    /// `category.setDetail("Derived", true); this.wrapped.fillCrashReportCategory(...)`.
    ///
    /// Both the `LevelData` and `ServerLevelData` impls share this body so an
    /// upcast to either `&dyn LevelData` or `&dyn ServerLevelData` dispatches
    /// the same Java override. The wrapped call is `ServerLevelData::…`
    /// (virtual dispatch on the wrapped object's `ServerLevelData`
    /// implementation — matching Java's `this.wrapped.fillCrashReportCategory`).
    fn fill_crash_report_category_impl(
        &self,
        category: &mut rivet_core::CrashReportCategory,
        level_height_accessor: &dyn LevelHeightAccessor,
    ) {
        category.set_detail("Derived", true);
        ServerLevelData::fill_crash_report_category(self.wrapped, category, level_height_accessor);
    }
}

impl<'a> LevelData for DerivedLevelData<'a> {
    fn get_respawn_data(&self) -> &RespawnData {
        self.wrapped.get_respawn_data()
    }

    fn get_game_time(&self) -> i64 {
        self.wrapped.get_game_time()
    }

    fn is_hardcore(&self) -> bool {
        self.world_data.is_hardcore()
    }

    fn get_difficulty(&self) -> Difficulty {
        self.world_data.get_difficulty()
    }

    fn is_difficulty_locked(&self) -> bool {
        self.world_data.is_difficulty_locked()
    }

    fn fill_crash_report_category(
        &self,
        category: &mut rivet_core::CrashReportCategory,
        level_height_accessor: &dyn LevelHeightAccessor,
    ) {
        self.fill_crash_report_category_impl(category, level_height_accessor);
    }
}

impl<'a> super::writable_level_data::WritableLevelData for DerivedLevelData<'a> {
    fn set_spawn(&mut self, respawn_data: RespawnData) {
        self.wrapped.set_spawn(respawn_data);
    }
}

impl<'a> ServerLevelData for DerivedLevelData<'a> {
    fn get_level_name(&self) -> &str {
        self.world_data.get_level_name()
    }

    fn get_game_type(&self) -> GameType {
        self.world_data.get_game_type()
    }

    fn is_initialized(&self) -> bool {
        self.wrapped.is_initialized()
    }

    fn set_initialized(&mut self, _initialized: bool) {
        // Java no-op: derived dimensions cannot mark the world initialized.
    }

    fn is_allow_commands(&self) -> bool {
        self.world_data.is_allow_commands()
    }

    fn set_allow_commands(&mut self, _allow_commands: bool) {
        // Java no-op.
    }

    fn set_game_type(&mut self, _game_type: GameType) {
        // Java no-op.
    }

    fn set_game_time(&mut self, _time: i64) {
        // Java no-op.
    }

    fn fill_crash_report_category(
        &self,
        category: &mut rivet_core::CrashReportCategory,
        level_height_accessor: &dyn LevelHeightAccessor,
    ) {
        self.fill_crash_report_category_impl(category, level_height_accessor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::storage::level_data::{LevelData, RespawnData, default_respawn_data};
    use crate::level::storage::server_level_data::ServerLevelData;
    use crate::level::storage::world_data::WorldData;
    use crate::level::storage::writable_level_data::WritableLevelData;
    use rivet_registry::core::BlockPos;
    use rivet_serialization::Lifecycle;

    /// A fake `WorldData` for the derived-delegation tests.
    struct FakeWorld {
        level_name: &'static str,
        game_type: GameType,
        hardcore: bool,
        allow_commands: bool,
        difficulty: Difficulty,
        locked: bool,
    }

    impl WorldData for FakeWorld {
        fn was_modded(&self) -> bool {
            false
        }
        fn get_known_server_brands(&self) -> &indexmap::IndexSet<String> {
            panic!("known server brands are not used by derived delegation tests")
        }
        fn get_removed_feature_flags(&self) -> &indexmap::IndexSet<String> {
            panic!("removed feature flags are not used by derived delegation tests")
        }
        fn set_modded_info(&mut self, _server_brand: &str, _is_modded: bool) {}
        fn overworld_data(&self) -> &dyn ServerLevelData {
            panic!("overworld data is not used by derived delegation tests")
        }
        fn is_hardcore(&self) -> bool {
            self.hardcore
        }
        fn get_version(&self) -> i32 {
            crate::level::storage::world_data::ANVIL_VERSION_ID
        }
        fn get_level_name(&self) -> &str {
            self.level_name
        }
        fn get_game_type(&self) -> GameType {
            self.game_type
        }
        fn set_game_type(&mut self, _game_type: GameType) {}
        fn is_allow_commands(&self) -> bool {
            self.allow_commands
        }
        fn set_allow_commands(&mut self, _allow_commands: bool) {}
        fn get_difficulty(&self) -> Difficulty {
            self.difficulty
        }
        fn set_difficulty(&mut self, _difficulty: Difficulty) {}
        fn is_difficulty_locked(&self) -> bool {
            self.locked
        }
        fn set_difficulty_locked(&mut self, _difficulty_locked: bool) {}
        fn get_single_player_uuid(&self) -> Option<rivet_util::mth::Uuid> {
            None
        }
        fn is_flat_world(&self) -> bool {
            false
        }
        fn is_debug_world(&self) -> bool {
            false
        }
        fn world_gen_settings_lifecycle(&self) -> Lifecycle {
            Lifecycle::Stable
        }
        fn get_data_configuration(
            &self,
        ) -> &crate::level::world_data_configuration::WorldDataConfiguration {
            panic!("data configuration is not used by derived delegation tests")
        }
        fn set_data_configuration(
            &mut self,
            _data_configuration: crate::level::world_data_configuration::WorldDataConfiguration,
        ) {
        }
        fn get_level_settings(&self) -> crate::level::level_settings::LevelSettings {
            panic!("level settings are not used by derived delegation tests")
        }
    }

    impl FakeWorld {
        fn standard() -> Self {
            FakeWorld {
                level_name: "world",
                game_type: GameType::Survival,
                hardcore: false,
                allow_commands: false,
                difficulty: Difficulty::Normal,
                locked: true,
            }
        }
    }

    /// A fake `ServerLevelData` — the wrapped per-dimension data.
    struct FakeServer {
        respawn: RespawnData,
        game_time: i64,
        initialized: bool,
    }

    impl LevelData for FakeServer {
        fn get_respawn_data(&self) -> &RespawnData {
            &self.respawn
        }
        fn get_game_time(&self) -> i64 {
            self.game_time
        }
        fn is_hardcore(&self) -> bool {
            false
        }
        fn get_difficulty(&self) -> Difficulty {
            Difficulty::Peaceful
        }
        fn is_difficulty_locked(&self) -> bool {
            false
        }
    }

    impl WritableLevelData for FakeServer {
        fn set_spawn(&mut self, respawn_data: RespawnData) {
            self.respawn = respawn_data;
        }
    }

    impl ServerLevelData for FakeServer {
        fn get_level_name(&self) -> &str {
            "wrapped-level"
        }
        fn get_game_type(&self) -> GameType {
            GameType::Adventure
        }
        fn is_initialized(&self) -> bool {
            self.initialized
        }
        fn set_initialized(&mut self, initialized: bool) {
            self.initialized = initialized;
        }
        fn is_allow_commands(&self) -> bool {
            true
        }
        fn set_allow_commands(&mut self, _allow_commands: bool) {}
        fn set_game_type(&mut self, _game_type: GameType) {}
        fn set_game_time(&mut self, _time: i64) {}
    }

    impl FakeServer {
        fn standard() -> Self {
            FakeServer {
                respawn: default_respawn_data(),
                game_time: 1234,
                initialized: false,
            }
        }
    }

    fn overworld_height() -> crate::level::height_accessor::SimpleLevelHeightAccessor {
        crate::level::height_accessor::create(-64, 384)
    }

    /// The world-level reads come from the `WorldData`; the runtime reads from
    /// the wrapped `ServerLevelData`.
    #[test]
    fn delegates_reads_to_world_data_and_wrapped() {
        let world = FakeWorld::standard();
        let mut wrapped = FakeServer::standard();
        let derived = DerivedLevelData::new(&world, &mut wrapped);
        assert_eq!(derived.get_level_name(), "world");
        assert_eq!(derived.get_game_type(), GameType::Survival);
        assert!(!derived.is_hardcore());
        assert!(!derived.is_allow_commands());
        assert_eq!(derived.get_difficulty(), Difficulty::Normal);
        assert!(derived.is_difficulty_locked());
        assert_eq!(derived.get_respawn_data().pos(), BlockPos::ZERO);
        assert_eq!(derived.get_game_time(), 1234);
        assert!(!derived.is_initialized());
    }

    /// `setSpawn` delegates to the wrapped data (Java's one delegating setter).
    #[test]
    fn set_spawn_delegates_to_wrapped() {
        let world = FakeWorld::standard();
        let mut wrapped = FakeServer::standard();
        let mut derived = DerivedLevelData::new(&world, &mut wrapped);
        let new_spawn = RespawnData::new(
            rivet_registry::core::GlobalPos::of(
                crate::level::level::overworld(),
                BlockPos::new(1, 2, 3),
            ),
            0.0,
            0.0,
        );
        derived.set_spawn(new_spawn);
        assert_eq!(wrapped.respawn.pos(), BlockPos::new(1, 2, 3));
    }

    /// The four setters Java makes no-ops must NOT touch the wrapped data.
    #[test]
    fn no_op_setters_do_not_mutate_wrapped() {
        let world = FakeWorld::standard();
        let mut wrapped = FakeServer::standard();
        {
            let mut derived = DerivedLevelData::new(&world, &mut wrapped);
            derived.set_game_time(9999);
            derived.set_game_type(GameType::Spectator);
            derived.set_initialized(true);
            derived.set_allow_commands(true);
            // The world reads are unchanged too (DerivedLevelData reads them
            // from the WorldData, which it never mutates).
            assert_eq!(derived.get_game_time(), 1234);
            assert_eq!(derived.get_game_type(), GameType::Survival);
            assert!(!derived.is_initialized());
            assert!(!derived.is_allow_commands());
        }
        // The wrapped values are unchanged.
        assert_eq!(wrapped.game_time, 1234);
        assert!(!wrapped.initialized);
    }

    /// `fillCrashReportCategory` records the `"Derived"` detail then delegates
    /// to the wrapped `ServerLevelData`'s default (the "Level spawn location"
    /// from the wrapped data, not the WorldData).
    #[test]
    fn fill_crash_report_category_prepends_derived_and_delegates() {
        let world = FakeWorld::standard();
        let mut wrapped = FakeServer::standard();
        // Give the wrapped a non-default spawn so the delegation is visible.
        wrapped.respawn = RespawnData::new(
            rivet_registry::core::GlobalPos::of(
                crate::level::level::overworld(),
                BlockPos::new(10, 20, 30),
            ),
            0.0,
            0.0,
        );
        let derived = DerivedLevelData::new(&world, &mut wrapped);
        let mut category = rivet_core::CrashReportCategory::new("test");
        ServerLevelData::fill_crash_report_category(&derived, &mut category, &overworld_height());
        assert_eq!(
            category.entries(),
            &[
                ("Derived".to_string(), "true".to_string()),
                (
                    "Level spawn location".to_string(),
                    "World: (10,20,30), Section: (at 10,4,14 in 0,1,1; chunk contains blocks 0,-64,16 to 15,319,31), Region: (0,0; contains chunks 0,0 to 31,31, blocks 0,-64,0 to 511,319,511)".to_string()
                ),
                ("Level name".to_string(), "wrapped-level".to_string()),
                (
                    "Level game mode".to_string(),
                    "Game mode: adventure (ID 2). Hardcore: false. Commands: true".to_string()
                ),
            ]
        );
    }

    /// The derived wrapper also implements `LevelData` (upcastable to
    /// `&dyn LevelData`), whose crash-report default is the same override
    /// body.
    #[test]
    fn level_data_upcast_dispatches_same_crash_report_body() {
        let world = FakeWorld::standard();
        let mut wrapped = FakeServer::standard();
        let derived = DerivedLevelData::new(&world, &mut wrapped);
        let mut category = rivet_core::CrashReportCategory::new("test");
        LevelData::fill_crash_report_category(&derived, &mut category, &overworld_height());
        assert_eq!(
            category.entries(),
            &[
                ("Derived".to_string(), "true".to_string()),
                (
                    "Level spawn location".to_string(),
                    "World: (0,0,0), Section: (at 0,0,0 in 0,0,0; chunk contains blocks 0,-64,0 to 15,319,15), Region: (0,0; contains chunks 0,0 to 31,31, blocks 0,-64,0 to 511,319,511)".to_string()
                ),
                ("Level name".to_string(), "wrapped-level".to_string()),
                (
                    "Level game mode".to_string(),
                    "Game mode: adventure (ID 2). Hardcore: false. Commands: true".to_string()
                ),
            ]
        );
    }
}
