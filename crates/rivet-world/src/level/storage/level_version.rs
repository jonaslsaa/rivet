//! Port of `net.minecraft.world.level.storage.LevelVersion` — the parsed
//! `version`/`LastPlayed`/`Version` block of `level.dat`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/LevelVersion.java`. `LevelVersion.parse(Dynamic)` reads the
//! level.dat header block: the top-level `"version"` (level-data format
//! version, `19133`), `"LastPlayed"` epoch millis, and the optional `"Version"`
//! sub-compound (the `WorldVersion` the world was written by). The Java
//! defaults fall back to `SharedConstants.getCurrentVersion()` (the pinned
//! 26.2 build) when the `Version` block is absent.
//!
//! The read side of this is exactly what `PrimaryLevelData.parse` needs first
//! (issue #54 codec cascade). The write side (`PrimaryLevelData.writeVersionTag`)
//! defers with the full `PrimaryLevelData` port.

use crate::level::storage::data_version::DataVersion;
use rivet_core::shared_constants::{SERIES, STABLE, VERSION_NAME, WORLD_VERSION};
use rivet_serialization::dynamic::Dynamic;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::number::Number;

/// `net.minecraft.world.level.storage.LevelVersion` — the parsed level-data
/// version block.
#[derive(Clone, Debug)]
pub struct LevelVersion {
    level_data_version: i32,
    last_played: i64,
    minecraft_version_name: String,
    minecraft_version: DataVersion,
    snapshot: bool,
}

impl LevelVersion {
    /// `LevelVersion.parse(Dynamic)`.
    ///
    /// Mirrors the Java field-by-field: `version` (default `0`), `LastPlayed`
    /// (default `0L`), then the optional `Version` compound (defaults to the
    /// pinned current version when absent). When the `Version` compound is
    /// absent, the Java `LevelVersion` stores `""`, `0`, `"main"`, `false`
    /// for the version fields (NOT the current-version fallbacks — the
    /// fallbacks are only applied *inside* the present `Version` compound).
    pub fn parse<O>(input: &Dynamic<O>, ops: &impl DynamicOps<Output = O>) -> Self
    where
        O: Clone + std::fmt::Debug,
    {
        let level_data_version = input.get(ops, "version").as_int_or(ops, Number::Int(0));
        let last_played = input
            .get(ops, "LastPlayed")
            .as_long_or(ops, Number::Long(0));
        let version = input.get(ops, "Version");
        if version.result().is_some() {
            let minecraft_version_name = version
                .get_field(ops, "Name")
                .as_string_or(ops, VERSION_NAME);
            let minecraft_version_id = version
                .get_field(ops, "Id")
                .as_int_or(ops, Number::Int(WORLD_VERSION));
            let series = version.get_field(ops, "Series").as_string_or(ops, SERIES);
            let snapshot = version
                .get_field(ops, "Snapshot")
                .as_boolean_or(ops, !STABLE);
            LevelVersion {
                level_data_version,
                last_played,
                minecraft_version_name,
                minecraft_version: DataVersion::new(minecraft_version_id, series),
                snapshot,
            }
        } else {
            LevelVersion {
                level_data_version,
                last_played,
                minecraft_version_name: String::new(),
                minecraft_version: DataVersion::new(0, SERIES.to_string()),
                snapshot: false,
            }
        }
    }

    /// `LevelVersion.levelDataVersion()`.
    pub fn level_data_version(&self) -> i32 {
        self.level_data_version
    }

    /// `LevelVersion.lastPlayed()`.
    pub fn last_played(&self) -> i64 {
        self.last_played
    }

    /// `LevelVersion.minecraftVersionName()`.
    pub fn minecraft_version_name(&self) -> &str {
        &self.minecraft_version_name
    }

    /// `LevelVersion.minecraftVersion()`.
    pub fn minecraft_version(&self) -> &DataVersion {
        &self.minecraft_version
    }

    /// `LevelVersion.snapshot()`.
    pub fn snapshot(&self) -> bool {
        self.snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::nbt_ops::NbtOps;
    use rivet_serialization::Dynamic;

    fn dynamic(tag: CompoundTag) -> Dynamic<rivet_nbt::tag::Tag> {
        let ops = NbtOps::instance();
        Dynamic::new(&ops, rivet_nbt::tag::Tag::Compound(tag))
    }

    fn nbt_compound() -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.put_int("version", 19133);
        tag.put_long("LastPlayed", 1786152964225);
        let mut version = CompoundTag::new();
        version.put_string("Name", "26.2");
        version.put_int("Id", 4903);
        version.put_string("Series", "main");
        version.put_byte("Snapshot", 0);
        tag.put(
            "Version".to_string(),
            rivet_nbt::tag::Tag::Compound(version),
        );
        tag
    }

    #[test]
    fn parses_present_version_block() {
        let d = dynamic(nbt_compound());
        let ops = NbtOps::instance();
        let lv = LevelVersion::parse(&d, &ops);
        assert_eq!(lv.level_data_version(), 19133);
        assert_eq!(lv.last_played(), 1786152964225);
        assert_eq!(lv.minecraft_version_name(), "26.2");
        assert_eq!(lv.minecraft_version().version, 4903);
        assert_eq!(lv.minecraft_version().series, "main");
        assert!(!lv.snapshot());
    }

    #[test]
    fn absent_version_block_uses_absent_defaults() {
        let mut tag = CompoundTag::new();
        tag.put_int("version", 0);
        tag.put_long("LastPlayed", 0);
        let d = dynamic(tag);
        let ops = NbtOps::instance();
        let lv = LevelVersion::parse(&d, &ops);
        // Java: no Version → `"", 0, "main", false` (NOT current-version).
        assert_eq!(lv.level_data_version(), 0);
        assert_eq!(lv.last_played(), 0);
        assert_eq!(lv.minecraft_version_name(), "");
        assert_eq!(lv.minecraft_version().version, 0);
        assert_eq!(lv.minecraft_version().series, "main");
        assert!(!lv.snapshot());
    }

    #[test]
    fn present_version_block_falls_back_to_current_version_per_field() {
        // Java: a *present* Version block with missing fields falls back to
        // the current version per-field (`version.get("Name").asString(
        // SharedConstants.getCurrentVersion().getName())`). This differs from
        // the absent-block branch (`""`/`0`).
        let mut tag = CompoundTag::new();
        tag.put_int("version", 19133);
        let mut version = CompoundTag::new();
        version.put_int("Id", 4903); // present
        // Name / Series / Snapshot absent.
        tag.put(
            "Version".to_string(),
            rivet_nbt::tag::Tag::Compound(version),
        );
        let d = dynamic(tag);
        let ops = NbtOps::instance();
        let lv = LevelVersion::parse(&d, &ops);
        assert_eq!(lv.minecraft_version_name(), VERSION_NAME);
        assert_eq!(lv.minecraft_version().version, 4903);
        assert_eq!(lv.minecraft_version().series, SERIES);
        assert!(!lv.snapshot()); // `!stable` (26.2 is stable)
    }

    #[test]
    fn empty_input_uses_all_defaults() {
        let tag = CompoundTag::new();
        // No version / LastPlayed / Version keys at all.
        let d = dynamic(tag);
        let ops = NbtOps::instance();
        let lv = LevelVersion::parse(&d, &ops);
        // version defaults 0, LastPlayed defaults 0, Version absent → absent
        // branch: "" / 0 / "main" / false.
        assert_eq!(lv.level_data_version(), 0);
        assert_eq!(lv.last_played(), 0);
        assert_eq!(lv.minecraft_version_name(), "");
        assert_eq!(lv.minecraft_version().version, 0);
        assert_eq!(lv.minecraft_version().series, "main");
        assert!(!lv.snapshot());
    }
}
