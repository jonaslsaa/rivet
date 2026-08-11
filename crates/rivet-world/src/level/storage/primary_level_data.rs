//! `net.minecraft.world.level.storage.PrimaryLevelData` — the concrete
//! world-level data (issue #323).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/PrimaryLevelData.java`. The concrete `ServerLevelData` + `WorldData`
//! implementation that owns the world's persistent settings, spawn, game time,
//! and version bookkeeping. This slice ports the full **read** surface —
//! `parse`, the accessors/setters, the crash-report composition — on top of the
//! already-merged codec/value layer (`LevelData.RespawnData.CODEC` #382,
//! `LevelSettings` #486, `Level.RESOURCE_KEY_CODEC` #515, `UUIDUtil.CODEC`
//! #373).
//!
//! ## `parse` semantics (mirrors Paper exactly)
//!
//! Every field is read with Java's exact default:
//!
//! - `Time` — `asLong(0L)`; a missing or non-number `Time` yields `0`.
//! - `singleplayer_uuid` — `flatMap(UUIDUtil.CODEC::parse).result().orElse(
//!   null)`: absent or unparseable → `None`; a wrong-length int-array errors
//!   through `Util.fixedSize(…, 4)`.
//! - `WasModded` — `asBoolean(false)`.
//! - `spawn` — `read(RespawnData.CODEC).result().orElse(RespawnData.DEFAULT)`:
//!   absent or malformed → `DEFAULT` (the read swallows decode errors).
//! - `initialized` — `asBoolean(true)` (a missing field means an initialized
//!   world).
//! - `ServerBrands` / `removed_features` — `asStream().flatMap(b ->
//!   b.asString().result().stream())`: absent, non-list, or non-string elements
//!   all yield an empty/skipped collection, in iteration order.
//! - `version` (the level-data format version) — read through
//!   `LevelVersion.parse` (`levelDataVersion()`).
//! - Paper `paperSpawnDimension` — `read(Level.RESOURCE_KEY_CODEC).result()
//!   .orElse(data.respawnData.dimension())`: absent or unparseable → the
//!   respawn's dimension.
//!
//! The constructor arg order (`singlePlayerUUID, wasModded, respawnData,
//! gameTime, version, initialized, knownServerBrands, removedFeatureFlags,
//! settings, specialWorldProperty, worldGenSettingsLifecycle`) is preserved
//! exactly.
//!
//! ## Deferred (RivetTodo)
//!
//! - The write path (`createTag`/`setTagData` and the `writeVersionTag`/
//!   `writeLastPlayed` statics, `RivetTodo(#398)`) needs `CompoundTag` store
//!   plus the worldgen settings; the #323 read slice excludes it.
//! - `LevelDataAndDimensions` (same manifest unit) defers with `WorldGenSettings`
//!   / `WorldDimensions.Complete` (`RivetTodo`); the record wraps worldgen
//!   values.
//!
//! ## `removed_features` set type
//!
//! Java collects `removed_features` into a `HashSet` (unspecified iteration
//! order); the `WorldData::get_removed_feature_flags` surface is an `IndexSet`
//! (the merged `RivetTodo(#398)` divergence, see `world_data.rs`). The port
//! collects into `IndexSet` and keeps the ordering divergence marked there.

use indexmap::IndexSet;
use rivet_registry::ResourceKey;
use rivet_registry::core::uuid_codec;
use rivet_registry::core::{Difficulty, GameType};
use rivet_registry::registries::Level as LevelKey;
use rivet_serialization::Lifecycle;
use rivet_serialization::codec::decoder_of_codec;
use rivet_serialization::dynamic::Dynamic;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::optional_dynamic::OptionalDynamic;
use rivet_util::mth::Uuid;

use super::level_data::{LevelData, RespawnData, default_respawn_data, level_data_fill_default};
use super::level_version::LevelVersion;
use super::server_level_data::ServerLevelData;
use super::world_data::{ANVIL_VERSION_ID, WorldData};
use super::writable_level_data::WritableLevelData;
use crate::level::level::{overworld, resource_key_codec};
use crate::level::level_settings::LevelSettings;

/// `PrimaryLevelData.PAPER_RESPAWN_DIMENSION` — `"paperSpawnDimension"` (Paper).
pub const PAPER_RESPAWN_DIMENSION: &str = "paperSpawnDimension";

/// `PrimaryLevelData.SpecialWorldProperty` — the `@Deprecated` enum still
/// passed to `parse` and `new`. Java: `NONE, FLAT, DEBUG`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialWorldProperty {
    None,
    Flat,
    Debug,
}

/// `PrimaryLevelData` — the concrete world-level data.
///
/// Field order mirrors the Java fields; `settings` and `respawn_dimension` are
/// `public` in Java and stay exposed here through accessors.
///
/// No `PartialEq`/`Eq`: the `RespawnData` field is deliberately not
/// `PartialEq` (its Java record uses `Float.compare` equality, which Rust's
/// derived `f32` `PartialEq` does not match — see `level_data.rs`).
#[derive(Debug, Clone)]
pub struct PrimaryLevelData {
    settings: LevelSettings,
    special_world_property: SpecialWorldProperty,
    world_gen_settings_lifecycle: Lifecycle,
    respawn_data: RespawnData,
    respawn_dimension: ResourceKey<LevelKey>,
    game_time: i64,
    single_player_uuid: Option<Uuid>,
    version: i32,
    initialized: bool,
    known_server_brands: IndexSet<String>,
    was_modded: bool,
    removed_feature_flags: IndexSet<String>,
}

impl PrimaryLevelData {
    /// `new PrimaryLevelData(LevelSettings, SpecialWorldProperty, Lifecycle)`.
    ///
    /// Java delegates to the full private constructor with
    /// `singlePlayerUUID = null, wasModded = false, respawnData = DEFAULT,
    /// gameTime = 0, version = ANVIL_VERSION_ID, initialized = false, empty
    /// brand/flag sets, levelSettings.copy()`. `respawn_dimension` stays at its
    /// field initializer `Level.OVERWORLD`.
    pub fn new(
        level_settings: LevelSettings,
        special_world_property: SpecialWorldProperty,
        world_gen_settings_lifecycle: Lifecycle,
    ) -> Self {
        PrimaryLevelData::from_parts(
            None,
            false,
            default_respawn_data(),
            0,
            ANVIL_VERSION_ID,
            false,
            IndexSet::new(),
            IndexSet::new(),
            level_settings.copy(),
            special_world_property,
            world_gen_settings_lifecycle,
        )
    }

    /// The full private constructor — `PrimaryLevelData(singlePlayerUUID,
    /// wasModded, respawnData, gameTime, version, initialized,
    /// knownServerBrands, removedFeatureFlags, settings, specialWorldProperty,
    /// worldGenSettingsLifecycle)`.
    #[allow(clippy::too_many_arguments)] // mirrors the 11-arg Java constructor 1:1
    fn from_parts(
        single_player_uuid: Option<Uuid>,
        was_modded: bool,
        respawn_data: RespawnData,
        game_time: i64,
        version: i32,
        initialized: bool,
        known_server_brands: IndexSet<String>,
        removed_feature_flags: IndexSet<String>,
        settings: LevelSettings,
        special_world_property: SpecialWorldProperty,
        world_gen_settings_lifecycle: Lifecycle,
    ) -> Self {
        PrimaryLevelData {
            settings,
            special_world_property,
            world_gen_settings_lifecycle,
            respawn_data,
            respawn_dimension: overworld(),
            game_time,
            single_player_uuid,
            version,
            initialized,
            known_server_brands,
            was_modded,
            removed_feature_flags,
        }
    }

    /// `PrimaryLevelData.parse(Dynamic, LevelSettings, SpecialWorldProperty,
    /// Lifecycle)`.
    ///
    /// Field-by-field `DynamicLike` read mirroring Paper exactly (see the
    /// module doc): `Time` first, then `LevelVersion.parse`, then the
    /// `PrimaryLevelData` constructor args in order. `singleplayer_uuid`,
    /// `spawn`, and the Paper `paperSpawnDimension` codec reads all swallow
    /// errors with `read().result().orElse(...)` — an absent or malformed
    /// field falls back, never fails the parse.
    pub fn parse<O, Ops: DynamicOps<Output = O> + 'static>(
        input: &Dynamic<O>,
        ops: &Ops,
        settings: LevelSettings,
        special_world_property: SpecialWorldProperty,
        world_gen_settings_lifecycle: Lifecycle,
    ) -> Self
    where
        O: Clone + std::fmt::Debug,
    {
        let game_time = input.get(ops, "Time").as_long_or(ops, 0);
        let level_version = LevelVersion::parse(input, ops);
        let mut data = PrimaryLevelData::from_parts(
            // `input.get("singleplayer_uuid").flatMap(UUIDUtil.CODEC::parse)
            // .result().orElse(null)` — the 1-arg `Decoder.parse(Dynamic)`
            // overload resolves the ops from the Dynamic; the port passes the
            // ops explicitly.
            input
                .get(ops, "singleplayer_uuid")
                .flat_map(|d| uuid_codec::<Ops>().parse(ops, d.get_value()))
                .result()
                .cloned(),
            input.get(ops, "WasModded").as_boolean_or(ops, false),
            // `input.get("spawn").read(RespawnData.CODEC).result()
            // .orElse(RespawnData.DEFAULT)`.
            input
                .get(ops, "spawn")
                .decode(
                    ops,
                    &*decoder_of_codec(
                        crate::level::storage::level_data::respawn_data_codec::<Ops>(),
                    ),
                )
                .result()
                .map(|(respawn, _)| respawn.clone())
                .unwrap_or_else(default_respawn_data),
            game_time,
            level_version.level_data_version(),
            input.get(ops, "initialized").as_boolean_or(ops, true),
            // `input.get("ServerBrands").asStream().flatMap(b ->
            // b.asString().result().stream())` → LinkedHashSet.
            string_collection(input.get(ops, "ServerBrands"), ops),
            // `…collect(Collectors.toSet())` (HashSet; see module doc for the
            // `IndexSet` ordering divergence).
            string_collection(input.get(ops, "removed_features"), ops),
            settings,
            special_world_property,
            world_gen_settings_lifecycle,
        );
        // Paper start
        data.respawn_dimension = input
            .get(ops, PAPER_RESPAWN_DIMENSION)
            .decode(ops, &*decoder_of_codec(resource_key_codec::<Ops>()))
            .result()
            .map(|(key, _)| key.clone())
            .unwrap_or_else(|| data.respawn_data.dimension().clone());
        // Paper end
        data
    }

    /// `settings()` — the `public` `settings` field.
    pub fn settings(&self) -> &LevelSettings {
        &self.settings
    }

    /// `respawnDimension()` — the Paper `public` `respawnDimension` field.
    pub fn respawn_dimension(&self) -> &ResourceKey<LevelKey> {
        &self.respawn_dimension
    }

    /// The Java `fillCrashReportCategory` override body, shared by the
    /// `LevelData` and `ServerLevelData` impls (Java's single class method):
    ///
    /// ```java
    /// ServerLevelData.super.fillCrashReportCategory(category, levelHeightAccessor);
    /// WorldData.super.fillCrashReportCategory(category);
    /// ```
    ///
    /// `ServerLevelData.super` = the `LevelData` default + "Level name" +
    /// "Level game mode", composed via [`level_data_fill_default`]; `WorldData.
    /// super` is the `WorldData` default (not overridden here). Recorded detail
    /// order is therefore: spawn location, name, game mode, then the four
    /// `WorldData` details.
    fn fill_crash_report_category_impl(
        &self,
        category: &mut rivet_core::CrashReportCategory,
        level_height_accessor: &dyn crate::level::height_accessor::LevelHeightAccessor,
    ) {
        level_data_fill_default(self, category, level_height_accessor);
        category.set_detail("Level name", ServerLevelData::get_level_name(self));
        category.set_detail(
            "Level game mode",
            format!(
                "Game mode: {} (ID {}). Hardcore: {}. Commands: {}",
                ServerLevelData::get_game_type(self).get_name(),
                ServerLevelData::get_game_type(self).get_id(),
                LevelData::is_hardcore(self),
                ServerLevelData::is_allow_commands(self),
            ),
        );
        WorldData::fill_crash_report_category(self, category);
    }
}

/// `input.get(key).asStream().flatMap(b -> b.asString().result().stream())`
/// collected into an insertion-ordered set — the `ServerBrands` /
/// `removed_features` reads. A missing key or non-list value yields the empty
/// set (Java's `asStreamOpt().result().orElseGet(Stream::empty)`); a list
/// element that fails `asString()` is skipped.
fn string_collection<O>(
    field: OptionalDynamic<O>,
    ops: &impl DynamicOps<Output = O>,
) -> IndexSet<String>
where
    O: Clone,
{
    field
        .flat_map(|d| d.as_stream_opt(ops))
        .result()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|element| element.as_string(ops).result().cloned())
        .collect()
}

impl LevelData for PrimaryLevelData {
    fn get_respawn_data(&self) -> &RespawnData {
        &self.respawn_data
    }

    fn get_game_time(&self) -> i64 {
        self.game_time
    }

    fn is_hardcore(&self) -> bool {
        self.settings.difficulty_settings().hardcore()
    }

    fn get_difficulty(&self) -> Difficulty {
        self.settings.difficulty_settings().difficulty()
    }

    fn is_difficulty_locked(&self) -> bool {
        self.settings.difficulty_settings().locked()
    }

    fn fill_crash_report_category(
        &self,
        category: &mut rivet_core::CrashReportCategory,
        level_height_accessor: &dyn crate::level::height_accessor::LevelHeightAccessor,
    ) {
        self.fill_crash_report_category_impl(category, level_height_accessor);
    }
}

impl WritableLevelData for PrimaryLevelData {
    fn set_spawn(&mut self, respawn_data: RespawnData) {
        self.respawn_data = respawn_data;
    }
}

impl ServerLevelData for PrimaryLevelData {
    fn get_level_name(&self) -> &str {
        self.settings.level_name()
    }

    fn get_game_type(&self) -> GameType {
        self.settings.game_type()
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn set_initialized(&mut self, initialized: bool) {
        self.initialized = initialized;
    }

    fn is_allow_commands(&self) -> bool {
        self.settings.allow_commands()
    }

    fn set_allow_commands(&mut self, allow_commands: bool) {
        self.settings = self.settings.with_allow_commands(allow_commands);
    }

    fn set_game_type(&mut self, game_type: GameType) {
        self.settings = self.settings.with_game_type(game_type);
    }

    fn set_game_time(&mut self, time: i64) {
        self.game_time = time;
    }

    fn fill_crash_report_category(
        &self,
        category: &mut rivet_core::CrashReportCategory,
        level_height_accessor: &dyn crate::level::height_accessor::LevelHeightAccessor,
    ) {
        self.fill_crash_report_category_impl(category, level_height_accessor);
    }
}

impl WorldData for PrimaryLevelData {
    fn get_data_configuration(
        &self,
    ) -> &crate::level::world_data_configuration::WorldDataConfiguration {
        self.settings.data_configuration()
    }

    fn set_data_configuration(
        &mut self,
        data_configuration: crate::level::world_data_configuration::WorldDataConfiguration,
    ) {
        self.settings = self.settings.with_data_configuration(data_configuration);
    }

    fn was_modded(&self) -> bool {
        self.was_modded
    }

    fn get_known_server_brands(&self) -> &IndexSet<String> {
        &self.known_server_brands
    }

    fn get_removed_feature_flags(&self) -> &IndexSet<String> {
        &self.removed_feature_flags
    }

    fn set_modded_info(&mut self, server_brand: &str, is_modded: bool) {
        self.known_server_brands.insert(server_brand.to_string());
        self.was_modded |= is_modded;
    }

    fn overworld_data(&self) -> &dyn ServerLevelData {
        self
    }

    fn get_level_settings(&self) -> LevelSettings {
        self.settings.copy()
    }

    fn is_hardcore(&self) -> bool {
        self.settings.difficulty_settings().hardcore()
    }

    fn get_version(&self) -> i32 {
        self.version
    }

    fn get_level_name(&self) -> &str {
        self.settings.level_name()
    }

    fn get_game_type(&self) -> GameType {
        self.settings.game_type()
    }

    fn set_game_type(&mut self, game_type: GameType) {
        self.settings = self.settings.with_game_type(game_type);
    }

    fn is_allow_commands(&self) -> bool {
        self.settings.allow_commands()
    }

    fn set_allow_commands(&mut self, allow_commands: bool) {
        self.settings = self.settings.with_allow_commands(allow_commands);
    }

    fn get_difficulty(&self) -> Difficulty {
        self.settings.difficulty_settings().difficulty()
    }

    fn set_difficulty(&mut self, difficulty: Difficulty) {
        self.settings = self.settings.with_difficulty(difficulty);
    }

    fn is_difficulty_locked(&self) -> bool {
        self.settings.difficulty_settings().locked()
    }

    fn set_difficulty_locked(&mut self, difficulty_locked: bool) {
        self.settings = self.settings.with_difficulty_lock(difficulty_locked);
    }

    fn get_single_player_uuid(&self) -> Option<Uuid> {
        self.single_player_uuid
    }

    fn is_flat_world(&self) -> bool {
        self.special_world_property == SpecialWorldProperty::Flat
    }

    fn is_debug_world(&self) -> bool {
        self.special_world_property == SpecialWorldProperty::Debug
    }

    fn world_gen_settings_lifecycle(&self) -> Lifecycle {
        self.world_gen_settings_lifecycle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::level::{nether, overworld};
    use crate::level::level_settings::{DifficultySettings, LevelSettings};
    use crate::level::world_data_configuration::WorldDataConfiguration;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::float_tag::FloatTag;
    use rivet_nbt::int_array_tag::IntArrayTag;
    use rivet_nbt::list_tag::ListTag;
    use rivet_nbt::nbt_io;
    use rivet_nbt::nbt_ops::NbtOps;
    use rivet_nbt::string_tag::StringTag;
    use rivet_nbt::tag::Tag;
    use rivet_registry::core::BlockPos;
    use rivet_registry::core::uuid_from_int_array;

    fn settings() -> LevelSettings {
        LevelSettings::new(
            "world".to_string(),
            GameType::Survival,
            DifficultySettings::default_settings(),
            false,
            WorldDataConfiguration::default_config(),
        )
    }

    fn dynamic(tag: CompoundTag) -> Dynamic<rivet_nbt::tag::Tag> {
        let ops = NbtOps::instance();
        Dynamic::new(&ops, rivet_nbt::tag::Tag::Compound(tag))
    }

    /// The `spawn` compound `RespawnData.CODEC` reads: `pos` (IntArray 3),
    /// `dimension`, then `yaw`/`pitch`.
    fn spawn_compound(pos: [i32; 3], dimension: &str, yaw: f32, pitch: f32) -> CompoundTag {
        let mut spawn = CompoundTag::new();
        spawn.put(
            "pos".to_string(),
            Tag::IntArray(IntArrayTag::new(pos.to_vec())),
        );
        spawn.put_string("dimension", dimension);
        spawn.put("yaw".to_string(), Tag::Float(FloatTag::new(yaw)));
        spawn.put("pitch".to_string(), Tag::Float(FloatTag::new(pitch)));
        spawn
    }

    fn string_list(values: &[&str]) -> Tag {
        let mut list = ListTag::new();
        for v in values {
            list.add(Tag::String(StringTag::value_of(v.to_string())));
        }
        Tag::List(list)
    }

    /// A full current-version `Data` compound exercising every `parse` field.
    fn full_data() -> CompoundTag {
        let mut data = CompoundTag::new();
        data.put_long("Time", 123456);
        data.put(
            "singleplayer_uuid".to_string(),
            Tag::IntArray(IntArrayTag::new(vec![1, 2, 3, 4])),
        );
        data.put_boolean("WasModded", true);
        data.put(
            "spawn".to_string(),
            Tag::Compound(spawn_compound(
                [0, -60, 0],
                "minecraft:overworld",
                10.0,
                20.0,
            )),
        );
        data.put_boolean("initialized", false);
        data.put(
            "ServerBrands".to_string(),
            string_list(&["vanilla", "Paper"]),
        );
        data.put(
            "removed_features".to_string(),
            string_list(&["minecraft:experimental"]),
        );
        data.put_string("paperSpawnDimension", "minecraft:the_nether");
        data.put_int("version", 19133);
        let mut version = CompoundTag::new();
        version.put_string("Name", "26.2");
        version.put_int("Id", 4903);
        version.put_string("Series", "main");
        version.put_boolean("Snapshot", false);
        data.put("Version".to_string(), Tag::Compound(version));
        data
    }

    /// Every field of the full synthetic compound parses with the exact Java
    /// semantics.
    #[test]
    fn parse_full_synthetic_data_compound() {
        let ops = NbtOps::instance();
        let d = dynamic(full_data());
        let parsed = PrimaryLevelData::parse(
            &d,
            &ops,
            settings(),
            SpecialWorldProperty::Flat,
            Lifecycle::Experimental,
        );

        assert_eq!(parsed.get_game_time(), 123456);
        assert_eq!(
            parsed.get_single_player_uuid(),
            Some(uuid_from_int_array(&[1, 2, 3, 4]))
        );
        assert!(parsed.was_modded());
        // `spawn` decode: dimension overworld, pos (0,-60,0), yaw 10, pitch 20.
        assert_eq!(parsed.get_respawn_data().pos(), BlockPos::new(0, -60, 0));
        assert_eq!(parsed.get_respawn_data().dimension(), &overworld());
        assert_eq!(parsed.get_respawn_data().yaw(), 10.0);
        assert_eq!(parsed.get_respawn_data().pitch(), 20.0);
        assert!(!parsed.is_initialized());
        assert_eq!(
            parsed.get_known_server_brands().iter().collect::<Vec<_>>(),
            vec!["vanilla", "Paper"]
        );
        assert_eq!(
            parsed
                .get_removed_feature_flags()
                .iter()
                .collect::<Vec<_>>(),
            vec!["minecraft:experimental"]
        );
        // Paper `paperSpawnDimension` → the_nether.
        assert_eq!(parsed.respawn_dimension(), &nether());
        // `LevelVersion.parse` reads the level-data format `version` (19133).
        assert_eq!(parsed.get_version(), 19133);
        assert!(parsed.is_flat_world());
        assert!(!parsed.is_debug_world());
        assert_eq!(
            parsed.world_gen_settings_lifecycle(),
            Lifecycle::Experimental
        );
        // Settings are the passed-through value.
        assert_eq!(parsed.settings(), &settings());
        assert_eq!(ServerLevelData::get_level_name(&parsed), "world");
        assert_eq!(ServerLevelData::get_game_type(&parsed), GameType::Survival);
    }

    /// The committed Paper 26.2 `level.dat` fixture parses deterministically.
    ///
    /// The fixture is a real launcher-written `level.dat`: `Time=31`,
    /// `WasModded=1`, a well-formed `spawn` (pos (0,-60,0), overworld), a
    /// `ServerBrands=["Paper"]` list, `initialized=1`, the `version`/`Version`
    /// header block, and `paperSpawnDimension=minecraft:overworld`. No
    /// `singleplayer_uuid` / `removed_features` keys, so those take their
    /// absent defaults (`None` / empty). Every assertion mirrors what Paper's
    /// `parse` produces for exactly these bytes.
    #[test]
    fn parse_real_committed_fixture() {
        let path = workspace_root().join("tools/rivet-oracle/fixtures/level.dat");
        assert!(
            path.is_file(),
            "fixture {path:?} is missing — the committed 26.2 level.dat is git-tracked, so a missing fixture means this end-to-end parse test silently stopped exercising the parser"
        );
        let bytes = std::fs::read(&path).expect("level.dat readable");
        let tag = nbt_io::read_compressed(
            &bytes[..],
            &mut rivet_nbt::nbt_accounter::NbtAccounter::unlimited_heap(),
        )
        .expect("read_compressed must read Paper's gzip level.dat");
        let data = tag
            .get_compound("Data")
            .expect("level.dat must carry a Data compound")
            .clone();
        let ops = NbtOps::instance();
        let d = Dynamic::new(&ops, Tag::Compound(data));
        let parsed = PrimaryLevelData::parse(
            &d,
            &ops,
            settings(),
            SpecialWorldProperty::None,
            Lifecycle::Stable,
        );

        assert_eq!(parsed.get_game_time(), 31); // the fixture's `Time`
        assert_eq!(parsed.get_single_player_uuid(), None); // absent key
        assert!(parsed.was_modded()); // `WasModded=1` (numeric → truthy)
        // Well-formed `spawn`: pos (0,-60,0), overworld, yaw/pitch 0.0.
        assert_eq!(parsed.get_respawn_data().pos(), BlockPos::new(0, -60, 0));
        assert_eq!(parsed.get_respawn_data().dimension(), &overworld());
        assert_eq!(parsed.get_respawn_data().yaw(), 0.0);
        assert_eq!(parsed.get_respawn_data().pitch(), 0.0);
        assert!(parsed.is_initialized()); // `initialized=1`
        assert_eq!(
            parsed.get_known_server_brands().iter().collect::<Vec<_>>(),
            vec!["Paper"]
        );
        assert!(parsed.get_removed_feature_flags().is_empty()); // absent key
        // `version=19133` → the level-data format version.
        assert_eq!(parsed.get_version(), 19133);
        // `paperSpawnDimension=minecraft:overworld` → overworld.
        assert_eq!(parsed.respawn_dimension(), &overworld());
    }

    /// An empty `Data` compound uses every Java default.
    #[test]
    fn parse_empty_data_uses_java_defaults() {
        let ops = NbtOps::instance();
        let d = dynamic(CompoundTag::new());
        let parsed = PrimaryLevelData::parse(
            &d,
            &ops,
            settings(),
            SpecialWorldProperty::None,
            Lifecycle::Stable,
        );
        assert_eq!(parsed.get_game_time(), 0);
        assert_eq!(parsed.get_single_player_uuid(), None);
        assert!(!parsed.was_modded());
        assert_eq!(parsed.get_respawn_data().pos(), BlockPos::ZERO);
        assert_eq!(parsed.get_respawn_data().dimension(), &overworld());
        assert!(parsed.is_initialized()); // default true
        assert!(parsed.get_known_server_brands().is_empty());
        assert!(parsed.get_removed_feature_flags().is_empty());
        assert_eq!(parsed.get_version(), 0);
        assert_eq!(parsed.respawn_dimension(), &overworld());
        assert_eq!(parsed.settings(), &settings());
    }

    /// A wrong-length `singleplayer_uuid` int-array errors through
    /// `UUIDUtil.CODEC` → `None` (Java's `flatMap(...).result().orElse(null)`).
    #[test]
    fn malformed_singleplayer_uuid_falls_back_to_none() {
        let mut data = full_data();
        data.put(
            "singleplayer_uuid".to_string(),
            Tag::IntArray(IntArrayTag::new(vec![1, 2, 3])),
        );
        let ops = NbtOps::instance();
        let d = dynamic(data);
        let parsed = PrimaryLevelData::parse(
            &d,
            &ops,
            settings(),
            SpecialWorldProperty::None,
            Lifecycle::Stable,
        );
        assert_eq!(parsed.get_single_player_uuid(), None);
        // A non-int-list (a string) also errors → None.
        let mut data2 = full_data();
        data2.put_string("singleplayer_uuid", "not-a-uuid");
        let ops = NbtOps::instance();
        let parsed2 = PrimaryLevelData::parse(
            &dynamic(data2),
            &ops,
            settings(),
            SpecialWorldProperty::None,
            Lifecycle::Stable,
        );
        assert_eq!(parsed2.get_single_player_uuid(), None);
    }

    /// A present-but-malformed `spawn` falls back to `DEFAULT`, not an error.
    #[test]
    fn malformed_spawn_falls_back_to_default() {
        let mut data = full_data();
        // `spawn` missing the mandatory `dimension` field.
        data.put("spawn".to_string(), Tag::Compound(CompoundTag::new()));
        let ops = NbtOps::instance();
        let parsed = PrimaryLevelData::parse(
            &dynamic(data),
            &ops,
            settings(),
            SpecialWorldProperty::None,
            Lifecycle::Stable,
        );
        assert_eq!(parsed.get_respawn_data().pos(), BlockPos::ZERO);
        assert_eq!(parsed.get_respawn_data().dimension(), &overworld());
        assert_eq!(parsed.get_respawn_data().yaw(), 0.0);
    }

    /// A malformed `paperSpawnDimension` falls back to the respawn's
    /// dimension (Java's `read(...).result().orElse(respawnData.dimension())`).
    #[test]
    fn malformed_paper_spawn_dimension_falls_back_to_respawn_dimension() {
        let mut data = full_data();
        data.put_string("paperSpawnDimension", "a b:c"); // invalid resource location
        let ops = NbtOps::instance();
        let parsed = PrimaryLevelData::parse(
            &dynamic(data),
            &ops,
            settings(),
            SpecialWorldProperty::None,
            Lifecycle::Stable,
        );
        // `spawn` decodes to overworld, so the fallback is overworld — NOT the
        // malformed paperSpawnDimension.
        assert_eq!(parsed.respawn_dimension(), &overworld());
        assert_eq!(parsed.get_respawn_data().dimension(), &overworld());
    }

    /// A non-list `ServerBrands` / `removed_features` yields the empty set
    /// (Java's `asStreamOpt().result().orElseGet(Stream::empty)`).
    #[test]
    fn non_list_brands_and_features_yield_empty_sets() {
        let mut data = full_data();
        data.put_int("ServerBrands", 42);
        data.put(
            "removed_features".to_string(),
            Tag::Compound(CompoundTag::new()),
        );
        let ops = NbtOps::instance();
        let parsed = PrimaryLevelData::parse(
            &dynamic(data),
            &ops,
            settings(),
            SpecialWorldProperty::None,
            Lifecycle::Stable,
        );
        assert!(parsed.get_known_server_brands().is_empty());
        assert!(parsed.get_removed_feature_flags().is_empty());
    }

    /// List elements that fail `asString()` are skipped, not fatal.
    #[test]
    fn non_string_brand_elements_are_skipped() {
        let mut brands = ListTag::new();
        brands.add(Tag::String(StringTag::value_of("vanilla".to_string())));
        brands.add(Tag::Int(rivet_nbt::int_tag::IntTag::new(7)));
        brands.add(Tag::String(StringTag::value_of("Paper".to_string())));
        let mut data = full_data();
        data.put("ServerBrands".to_string(), Tag::List(brands));
        let ops = NbtOps::instance();
        let parsed = PrimaryLevelData::parse(
            &dynamic(data),
            &ops,
            settings(),
            SpecialWorldProperty::None,
            Lifecycle::Stable,
        );
        assert_eq!(
            parsed.get_known_server_brands().iter().collect::<Vec<_>>(),
            vec!["vanilla", "Paper"]
        );
    }

    /// A wrong-typed `Time` / `initialized` / `WasModded` uses the default.
    ///
    /// `NbtOps` booleans are *numeric* (`getBooleanValue` = `number != 0.0`),
    /// so a non-numeric `String` is the genuinely wrong-typed case — an `Int`
    /// would be a valid boolean. `asLong` on a string errors → `0`;
    /// `asBoolean` on a string errors → the default (`true` / `false`).
    #[test]
    fn wrong_typed_scalars_use_defaults() {
        let mut data = full_data();
        data.put_string("Time", "not-a-number");
        data.put_string("initialized", "x");
        data.put_string("WasModded", "false");
        let ops = NbtOps::instance();
        let parsed = PrimaryLevelData::parse(
            &dynamic(data),
            &ops,
            settings(),
            SpecialWorldProperty::None,
            Lifecycle::Stable,
        );
        assert_eq!(parsed.get_game_time(), 0); // asLong(0)
        assert!(parsed.is_initialized()); // asBoolean(true)
        assert!(!parsed.was_modded()); // asBoolean(false)
    }

    /// `new` uses the Java constructor defaults (spawn DEFAULT, gameTime 0,
    /// ANVIL version, uninitialized, empty sets, respawn dimension overworld).
    #[test]
    fn new_constructor_defaults() {
        let data = PrimaryLevelData::new(settings(), SpecialWorldProperty::None, Lifecycle::Stable);
        assert_eq!(data.get_respawn_data().pos(), BlockPos::ZERO);
        assert_eq!(data.get_respawn_data().dimension(), &overworld());
        assert_eq!(data.get_game_time(), 0);
        assert_eq!(data.get_version(), 19133);
        assert!(!data.is_initialized());
        assert!(data.get_known_server_brands().is_empty());
        assert_eq!(data.get_single_player_uuid(), None);
        assert_eq!(data.respawn_dimension(), &overworld());
        assert_eq!(data.settings(), &settings());
    }

    /// `fillCrashReportCategory` composes the `LevelData` spawn detail, the
    /// `ServerLevelData` name/game-mode details, then the `WorldData` details —
    /// in Java's exact recorded order.
    #[test]
    fn crash_report_composition_order() {
        let height = crate::level::height_accessor::create(-64, 384);
        let mut data =
            PrimaryLevelData::new(settings(), SpecialWorldProperty::None, Lifecycle::Stable);
        data.set_spawn(RespawnData::new(
            rivet_registry::core::GlobalPos::of(overworld(), BlockPos::new(0, 64, 0)),
            0.0,
            0.0,
        ));
        data.set_modded_info("vanilla", false);
        let mut category = rivet_core::CrashReportCategory::new("test");
        ServerLevelData::fill_crash_report_category(&data, &mut category, &height);
        assert_eq!(
            category.entries(),
            &[
                (
                    "Level spawn location".to_string(),
                    "World: (0,64,0), Section: (at 0,0,0 in 0,4,0; chunk contains blocks 0,-64,0 to 15,319,15), Region: (0,0; contains chunks 0,0 to 31,31, blocks 0,-64,0 to 511,319,511)".to_string()
                ),
                ("Level name".to_string(), "world".to_string()),
                (
                    "Level game mode".to_string(),
                    "Game mode: survival (ID 0). Hardcore: false. Commands: false".to_string()
                ),
                ("Known server brands".to_string(), "vanilla".to_string()),
                ("Removed feature flags".to_string(), String::new()),
                ("Level was modded".to_string(), "false".to_string()),
                (
                    "Level storage version".to_string(),
                    "0x04ABD - Anvil".to_string()
                ),
            ]
        );
    }

    /// Upcasting to `&dyn LevelData` dispatches the same full override body.
    #[test]
    fn level_data_upcast_dispatches_full_override() {
        let height = crate::level::height_accessor::create(-64, 384);
        let data = PrimaryLevelData::new(settings(), SpecialWorldProperty::None, Lifecycle::Stable);
        let mut category = rivet_core::CrashReportCategory::new("test");
        LevelData::fill_crash_report_category(&data, &mut category, &height);
        assert_eq!(category.entries().len(), 7);
    }

    /// The setters mutate the concrete state (not the `DerivedLevelData`
    /// no-ops).
    #[test]
    fn setters_mutate_state() {
        let mut data =
            PrimaryLevelData::new(settings(), SpecialWorldProperty::Debug, Lifecycle::Stable);
        data.set_game_time(9999);
        ServerLevelData::set_game_type(&mut data, GameType::Creative);
        data.set_initialized(true);
        ServerLevelData::set_allow_commands(&mut data, true);
        WorldData::set_difficulty(&mut data, Difficulty::Hard);
        WorldData::set_difficulty_locked(&mut data, true);
        assert_eq!(data.get_game_time(), 9999);
        assert_eq!(ServerLevelData::get_game_type(&data), GameType::Creative);
        assert!(data.is_initialized());
        assert!(ServerLevelData::is_allow_commands(&data));
        assert_eq!(LevelData::get_difficulty(&data), Difficulty::Hard);
        assert!(LevelData::is_difficulty_locked(&data));
        assert!(data.is_debug_world());
        assert!(!data.is_flat_world());
        // `get_data_configuration`/`get_level_settings` back onto the mutated
        // settings (game_type Creative, difficulty Hard+locked, commands on).
        assert_eq!(
            data.get_data_configuration(),
            &WorldDataConfiguration::default_config()
        );
        let expected = LevelSettings::new(
            "world".to_string(),
            GameType::Creative,
            DifficultySettings::new(Difficulty::Hard, false, true),
            true,
            WorldDataConfiguration::default_config(),
        );
        assert_eq!(data.get_level_settings(), expected);
    }

    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    }
}
