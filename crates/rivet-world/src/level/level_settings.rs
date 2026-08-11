//! `net.minecraft.world.level.LevelSettings` — the world's level.dat settings
//! record (issue #486, the `WorldData` value-codec slice).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! LevelSettings.java`. A five-field record `(String levelName, GameType
//! gameType, LevelSettings.DifficultySettings difficultySettings, boolean
//! allowCommands, WorldDataConfiguration dataConfiguration)`.
//!
//! ## `parse(Dynamic, WorldDataConfiguration)`
//!
//! Java's `parse` is the read side of the level.dat `settings` fields, but it
//! is **not** the `PrimaryLevelData.parse` entry point (that lives in
//! `world.level.storage.PrimaryLevelData`, excluded from this slice). It is a
//! per-field `DynamicLike` read with these defaults (each `asX(default)`):
//!
//! - `GameType` — `GameType.byId(input.get("GameType").asInt(0))` (default id
//!   `0` → `SURVIVAL`; `by_id` falls back to `SURVIVAL` for any out-of-range
//!   id).
//! - `LevelName` — `asString("")`.
//! - `difficulty_settings` — `input.get("difficulty_settings").read(
//!   DifficultySettings.CODEC).result().orElse(DifficultySettings.DEFAULT)`:
//!   a present-but-malformed `difficulty_settings` falls back to `DEFAULT`
//!   (the `read().result().orElse(...)` swallows decode errors), and an absent
//!   field also yields `DEFAULT`.
//! - `allowCommands` — `asBoolean(gameType == GameType.CREATIVE)`: the default
//!   is `true` iff the parsed game type is CREATIVE.
//! - `dataConfiguration` — the `loadConfig` argument passed straight through.
//!
//! ## The `with*`/`copy` surface
//!
//! Every mutator rebuilds the record with one field replaced; `copy()` returns
//! a structurally-equal clone (Java's records copy on the copy constructor —
//! the `DifficultySettings` value is shared, and Rust's `Clone` mirrors that
//! sharing). Paper adds `withLevelName` and `withHardcore` beyond the vanilla
//! `with*` set.
//!
//! ## The nested `DifficultySettings` codec
//!
//! `DifficultySettings.CODEC` is a `RecordCodecBuilder.create` over three
//! **mandatory** `fieldOf` fields (`difficulty` via `Difficulty.CODEC`, then
//! `hardcore`, then `locked`). Unlike `WorldDataConfiguration`'s lenient
//! optional fields, a missing or malformed field FAILS the whole decode —
//! which is what makes `parse` fall back to `DifficultySettings.DEFAULT` on a
//! malformed `difficulty_settings` compound. The field declaration order
//! (`difficulty`, `hardcore`, `locked`) is the codec's encode order and is
//! preserved exactly.
//!
//! Placement: `rivet-world::level` (`mc.world.level` unit), next to
//! `WorldDataConfiguration` (#486). `Difficulty`/`GameType` live in
//! `rivet-registry::core` (per OWNERSHIP.md §Registries — pure value types).

use super::world_data_configuration::WorldDataConfiguration;
use rivet_registry::core::{Difficulty, GameType};
use rivet_serialization::dynamic::Dynamic;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `LevelSettings` — the `(levelName, gameType, difficultySettings,
/// allowCommands, dataConfiguration)` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelSettings {
    level_name: String,
    game_type: GameType,
    difficulty_settings: DifficultySettings,
    allow_commands: bool,
    data_configuration: WorldDataConfiguration,
}

impl LevelSettings {
    /// The canonical constructor.
    pub fn new(
        level_name: String,
        game_type: GameType,
        difficulty_settings: DifficultySettings,
        allow_commands: bool,
        data_configuration: WorldDataConfiguration,
    ) -> Self {
        LevelSettings {
            level_name,
            game_type,
            difficulty_settings,
            allow_commands,
            data_configuration,
        }
    }

    /// `LevelSettings.parse(Dynamic, WorldDataConfiguration)`.
    ///
    /// Field-by-field `DynamicLike` read with Java's exact defaults (see the
    /// module doc). `game_type` is read first (the `allowCommands` default
    /// depends on it).
    pub fn parse<O, Ops: DynamicOps<Output = O> + 'static>(
        input: &Dynamic<O>,
        ops: &Ops,
        load_config: WorldDataConfiguration,
    ) -> Self
    where
        O: Clone + std::fmt::Debug,
    {
        let game_type = GameType::by_id(input.get(ops, "GameType").as_int_or(ops, 0));
        // `input.get("difficulty_settings").read(DifficultySettings.CODEC)
        // .result().orElse(DifficultySettings.DEFAULT)` — a present-but-malformed
        // compound or an absent field both yield DEFAULT (read().result()
        // swallows decode errors). The codec is ops-generic in the port, so the
        // concrete `Ops` is the one passed in.
        let difficulty_decoder =
            rivet_serialization::codec::decoder_of_codec(DifficultySettings::codec::<Ops>());
        let difficulty_settings = input
            .get(ops, "difficulty_settings")
            .decode(ops, &*difficulty_decoder)
            .result()
            .map(|(ds, _)| ds.clone())
            .unwrap_or_else(DifficultySettings::default_settings);
        LevelSettings {
            level_name: input.get(ops, "LevelName").as_string_or(ops, ""),
            game_type,
            allow_commands: input
                .get(ops, "allowCommands")
                .as_boolean_or(ops, game_type == GameType::Creative),
            difficulty_settings,
            data_configuration: load_config,
        }
    }

    /// `LevelSettings.withGameType(GameType)`.
    pub fn with_game_type(&self, game_type: GameType) -> Self {
        LevelSettings::new(
            self.level_name.clone(),
            game_type,
            self.difficulty_settings.clone(),
            self.allow_commands,
            self.data_configuration.clone(),
        )
    }

    /// `LevelSettings.withAllowCommands(boolean)`.
    pub fn with_allow_commands(&self, allow_commands: bool) -> Self {
        LevelSettings::new(
            self.level_name.clone(),
            self.game_type,
            self.difficulty_settings.clone(),
            allow_commands,
            self.data_configuration.clone(),
        )
    }

    /// `LevelSettings.withDifficulty(Difficulty)` — rebuilds the
    /// `DifficultySettings` preserving `hardcore`/`locked`.
    pub fn with_difficulty(&self, difficulty: Difficulty) -> Self {
        LevelSettings::new(
            self.level_name.clone(),
            self.game_type,
            DifficultySettings::new(
                difficulty,
                self.difficulty_settings.hardcore(),
                self.difficulty_settings.locked(),
            ),
            self.allow_commands,
            self.data_configuration.clone(),
        )
    }

    /// `LevelSettings.withDifficultyLock(boolean)` — rebuilds the
    /// `DifficultySettings` preserving `difficulty`/`hardcore`.
    pub fn with_difficulty_lock(&self, locked: bool) -> Self {
        LevelSettings::new(
            self.level_name.clone(),
            self.game_type,
            DifficultySettings::new(
                self.difficulty_settings.difficulty(),
                self.difficulty_settings.hardcore(),
                locked,
            ),
            self.allow_commands,
            self.data_configuration.clone(),
        )
    }

    /// `LevelSettings.withDataConfiguration(WorldDataConfiguration)`.
    pub fn with_data_configuration(&self, data_configuration: WorldDataConfiguration) -> Self {
        LevelSettings::new(
            self.level_name.clone(),
            self.game_type,
            self.difficulty_settings.clone(),
            self.allow_commands,
            data_configuration,
        )
    }

    /// `LevelSettings.copy()`.
    pub fn copy(&self) -> Self {
        self.clone()
    }

    // --- Paper additions (Paper diff) ---

    /// `LevelSettings.withLevelName(String)` (Paper).
    pub fn with_level_name(&self, name: String) -> Self {
        LevelSettings::new(
            name,
            self.game_type,
            self.difficulty_settings.clone(),
            self.allow_commands,
            self.data_configuration.clone(),
        )
    }

    /// `LevelSettings.withHardcore(boolean)` (Paper) — rebuilds the
    /// `DifficultySettings` preserving `difficulty`/`locked`.
    pub fn with_hardcore(&self, hardcore: bool) -> Self {
        LevelSettings::new(
            self.level_name.clone(),
            self.game_type,
            DifficultySettings::new(
                self.difficulty_settings.difficulty(),
                hardcore,
                self.difficulty_settings.locked(),
            ),
            self.allow_commands,
            self.data_configuration.clone(),
        )
    }

    // --- record accessors ---

    /// `LevelSettings.levelName()`.
    pub fn level_name(&self) -> &str {
        &self.level_name
    }

    /// `LevelSettings.gameType()`.
    pub fn game_type(&self) -> GameType {
        self.game_type
    }

    /// `LevelSettings.difficultySettings()`.
    pub fn difficulty_settings(&self) -> &DifficultySettings {
        &self.difficulty_settings
    }

    /// `LevelSettings.allowCommands()`.
    pub fn allow_commands(&self) -> bool {
        self.allow_commands
    }

    /// `LevelSettings.dataConfiguration()`.
    pub fn data_configuration(&self) -> &WorldDataConfiguration {
        &self.data_configuration
    }
}

/// `LevelSettings.DifficultySettings` — the `(Difficulty difficulty, boolean
/// hardcore, boolean locked)` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DifficultySettings {
    difficulty: Difficulty,
    hardcore: bool,
    locked: bool,
}

impl DifficultySettings {
    /// `LevelSettings.DifficultySettings.DEFAULT` — `new DifficultySettings(
    /// Difficulty.NORMAL, false, false)`.
    pub fn default_settings() -> Self {
        DifficultySettings::new(Difficulty::Normal, false, false)
    }

    /// The canonical constructor.
    pub fn new(difficulty: Difficulty, hardcore: bool, locked: bool) -> Self {
        DifficultySettings {
            difficulty,
            hardcore,
            locked,
        }
    }

    /// `DifficultySettings.difficulty()`.
    pub fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    /// `DifficultySettings.hardcore()`.
    pub fn hardcore(&self) -> bool {
        self.hardcore
    }

    /// `DifficultySettings.locked()`.
    pub fn locked(&self) -> bool {
        self.locked
    }

    /// `LevelSettings.DifficultySettings.CODEC` — `RecordCodecBuilder.create`
    /// over the three mandatory `fieldOf` fields in declaration order
    /// (`difficulty`, `hardcore`, `locked`). A missing or malformed field fails
    /// the whole decode (see the module doc — this is what makes `parse` fall
    /// back to `DEFAULT`).
    pub fn codec<Ops: DynamicOps + 'static>()
    -> Arc<dyn rivet_serialization::Codec<DifficultySettings, Ops>>
    where
        DifficultySettings: 'static,
    {
        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of_named(
                    Arc::new(|d: &DifficultySettings| d.difficulty),
                    "difficulty".to_string(),
                    Arc::new(rivet_registry::core::difficulty_codec::<Ops>()),
                ))
                .and(RecordCodecBuilder::of_named(
                    Arc::new(|d: &DifficultySettings| d.hardcore),
                    "hardcore".to_string(),
                    rivet_serialization::codec::bool_codec::<Ops>(),
                ))
                .and(RecordCodecBuilder::of_named(
                    Arc::new(|d: &DifficultySettings| d.locked),
                    "locked".to_string(),
                    rivet_serialization::codec::bool_codec::<Ops>(),
                ))
                .apply(instance, Arc::new(DifficultySettings::new))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::pair::Pair;

    fn default_settings() -> LevelSettings {
        LevelSettings::new(
            "world".to_string(),
            GameType::Survival,
            DifficultySettings::default_settings(),
            false,
            WorldDataConfiguration::default_config(),
        )
    }

    #[test]
    fn difficulty_settings_default_is_normal_hardcore_false_locked_false() {
        let d = DifficultySettings::default_settings();
        assert_eq!(d.difficulty(), Difficulty::Normal);
        assert!(!d.hardcore());
        assert!(!d.locked());
    }

    #[test]
    fn difficulty_settings_codec_round_trips() {
        let ops = JsonOps::INSTANCE;
        let codec = DifficultySettings::codec::<JsonOps>();
        let value = DifficultySettings::new(Difficulty::Hard, true, true);
        let encoded = codec
            .encode_start(&ops, &value)
            .get_or_throw("encode")
            .clone();
        let obj = encoded.as_object().expect("object");
        assert_eq!(
            obj.keys().collect::<Vec<_>>(),
            vec!["difficulty", "hardcore", "locked"]
        );
        let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
        assert_eq!(decoded.0, value);
    }

    #[test]
    fn difficulty_settings_codec_requires_all_fields() {
        let ops = JsonOps::INSTANCE;
        let codec = DifficultySettings::codec::<JsonOps>();
        // Missing "locked" fails the whole decode (mandatory field).
        let missing_locked = ops.create_map(vec![
            Pair::of(
                ops.create_string("difficulty".to_string()),
                ops.create_string("normal".to_string()),
            ),
            Pair::of(
                ops.create_string("hardcore".to_string()),
                ops.create_boolean(false),
            ),
        ]);
        assert!(codec.decode(&ops, &missing_locked).result().is_none());
        // Unknown difficulty name fails the decode.
        let unknown = ops.create_map(vec![
            Pair::of(
                ops.create_string("difficulty".to_string()),
                ops.create_string("bogus".to_string()),
            ),
            Pair::of(
                ops.create_string("hardcore".to_string()),
                ops.create_boolean(false),
            ),
            Pair::of(
                ops.create_string("locked".to_string()),
                ops.create_boolean(false),
            ),
        ]);
        assert!(codec.decode(&ops, &unknown).result().is_none());
    }

    #[test]
    fn parse_defaults_from_empty_dynamic() {
        // Empty input → GameType SURVIVAL, name "", difficulty NORMAL,
        // allowCommands false (SURVIVAL != CREATIVE), config passed through.
        let ops = JsonOps::INSTANCE;
        let d = Dynamic::new(&ops, ops.empty_map());
        let parsed = LevelSettings::parse(&d, &ops, WorldDataConfiguration::default_config());
        assert_eq!(parsed.game_type(), GameType::Survival);
        assert_eq!(parsed.level_name(), "");
        assert_eq!(
            parsed.difficulty_settings(),
            &DifficultySettings::default_settings()
        );
        assert!(!parsed.allow_commands());
        assert_eq!(
            parsed.data_configuration(),
            &WorldDataConfiguration::default_config()
        );
    }

    #[test]
    fn parse_reads_present_fields() {
        let ops = JsonOps::INSTANCE;
        let d = Dynamic::new(
            &ops,
            ops.create_map(vec![
                Pair::of(ops.create_string("GameType".to_string()), ops.create_int(1)),
                Pair::of(
                    ops.create_string("LevelName".to_string()),
                    ops.create_string("MyWorld".to_string()),
                ),
                Pair::of(
                    ops.create_string("allowCommands".to_string()),
                    ops.create_boolean(true),
                ),
                Pair::of(
                    ops.create_string("difficulty_settings".to_string()),
                    ops.create_map(vec![
                        Pair::of(
                            ops.create_string("difficulty".to_string()),
                            ops.create_string("hard".to_string()),
                        ),
                        Pair::of(
                            ops.create_string("hardcore".to_string()),
                            ops.create_boolean(true),
                        ),
                        Pair::of(
                            ops.create_string("locked".to_string()),
                            ops.create_boolean(false),
                        ),
                    ]),
                ),
            ]),
        );
        let parsed = LevelSettings::parse(&d, &ops, WorldDataConfiguration::default_config());
        assert_eq!(parsed.game_type(), GameType::Creative);
        assert_eq!(parsed.level_name(), "MyWorld");
        assert!(parsed.allow_commands());
        assert_eq!(
            parsed.difficulty_settings(),
            &DifficultySettings::new(Difficulty::Hard, true, false)
        );
    }

    #[test]
    fn parse_allow_commands_defaults_to_creative() {
        // allowCommands absent + GameType CREATIVE → true.
        let ops = JsonOps::INSTANCE;
        let d = Dynamic::new(
            &ops,
            ops.create_map(vec![Pair::of(
                ops.create_string("GameType".to_string()),
                ops.create_int(1),
            )]),
        );
        let parsed = LevelSettings::parse(&d, &ops, WorldDataConfiguration::default_config());
        assert!(parsed.allow_commands());
        // allowCommands absent + GameType SURVIVAL → false.
        let d2 = Dynamic::new(
            &ops,
            ops.create_map(vec![Pair::of(
                ops.create_string("GameType".to_string()),
                ops.create_int(0),
            )]),
        );
        let parsed2 = LevelSettings::parse(&d2, &ops, WorldDataConfiguration::default_config());
        assert!(!parsed2.allow_commands());
    }

    #[test]
    fn parse_malformed_difficulty_settings_falls_back_to_default() {
        // A present but malformed difficulty_settings (missing locked) →
        // DEFAULT, not an error.
        let ops = JsonOps::INSTANCE;
        let d = Dynamic::new(
            &ops,
            ops.create_map(vec![
                Pair::of(ops.create_string("GameType".to_string()), ops.create_int(0)),
                Pair::of(
                    ops.create_string("difficulty_settings".to_string()),
                    ops.create_map(vec![
                        Pair::of(
                            ops.create_string("difficulty".to_string()),
                            ops.create_string("normal".to_string()),
                        ),
                        Pair::of(
                            ops.create_string("hardcore".to_string()),
                            ops.create_boolean(false),
                        ),
                    ]),
                ),
            ]),
        );
        let parsed = LevelSettings::parse(&d, &ops, WorldDataConfiguration::default_config());
        assert_eq!(
            parsed.difficulty_settings(),
            &DifficultySettings::default_settings()
        );
    }

    #[test]
    fn with_methods_preserve_other_fields() {
        let s = default_settings();
        let g = s.with_game_type(GameType::Adventure);
        assert_eq!(g.game_type(), GameType::Adventure);
        assert_eq!(g.level_name(), "world");
        assert_eq!(
            g.difficulty_settings(),
            &DifficultySettings::default_settings()
        );
        assert!(!g.allow_commands());

        let a = s.with_allow_commands(true);
        assert!(a.allow_commands());
        assert_eq!(a.game_type(), GameType::Survival);

        let d = s.with_difficulty(Difficulty::Easy);
        assert_eq!(d.difficulty_settings().difficulty(), Difficulty::Easy);
        assert!(!d.difficulty_settings().hardcore());
        assert!(!d.difficulty_settings().locked());

        let l = s.with_difficulty_lock(true);
        assert!(l.difficulty_settings().locked());
        assert_eq!(l.difficulty_settings().difficulty(), Difficulty::Normal);

        let c = s.copy();
        assert_eq!(c, s);
    }

    #[test]
    fn with_difficulty_preserves_hardcore_and_locked() {
        let s = LevelSettings::new(
            "w".to_string(),
            GameType::Survival,
            DifficultySettings::new(Difficulty::Normal, true, true),
            false,
            WorldDataConfiguration::default_config(),
        );
        let d = s.with_difficulty(Difficulty::Peaceful);
        assert_eq!(d.difficulty_settings().difficulty(), Difficulty::Peaceful);
        assert!(d.difficulty_settings().hardcore());
        assert!(d.difficulty_settings().locked());
    }

    #[test]
    fn paper_with_level_name_and_hardcore() {
        let s = default_settings();
        let n = s.with_level_name("renamed".to_string());
        assert_eq!(n.level_name(), "renamed");
        assert_eq!(n.game_type(), GameType::Survival);
        let h = s.with_hardcore(true);
        assert!(h.difficulty_settings().hardcore());
        assert_eq!(h.difficulty_settings().difficulty(), Difficulty::Normal);
    }
}
