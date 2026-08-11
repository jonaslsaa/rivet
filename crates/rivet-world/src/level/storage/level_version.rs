//! Port of `net.minecraft.world.level.storage.LevelVersion` — the parsed
//! `version`/`LastPlayed`/`Version` block of `level.dat`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/LevelVersion.java`. `LevelVersion.parse(Dynamic)` reads the
//! level.dat header block: the top-level `"version"` (level-data format
//! version, `19133`), `"LastPlayed"` epoch millis, and the optional `"Version"`
//! sub-compound (the `WorldVersion` the world was written by). Inside a present
//! `Version` block, per-field defaults fall back to
//! `SharedConstants.getCurrentVersion()` (the pinned 26.2 build); an absent
//! `Version` block instead stores `""` / `0` / `"main"` / `false` for the
//! version fields (NOT the current-version fallbacks).
//!
//! The read side of this is exactly what `PrimaryLevelData.parse` needs first
//! (issue #54 codec cascade). The write side (`PrimaryLevelData.writeVersionTag`)
//! defers with the full `PrimaryLevelData` port.
//!
//! ## The version guard (#323)
//!
//! Paper rejects a world created by a different release series before
//! `PrimaryLevelData.parse` runs: `Main` checks `LevelSummary.isCompatible()`
//! (a `DataVersion.isCompatible` — series equality) and `getLevelDataAndDimensions`
//! throws when `DataFixers.getFileFixer().requiresFileFixing(NbtUtils.getDataVersion(..))`.
//! Rivet has no DFU (issue #323 explicitly rejects migrating old worlds), so
//! [`ensure_compatible`] is the honest no-migration boundary: it accepts only
//! the pinned 26.2 data version (the tracked `SharedConstants.WORLD_VERSION`)
//! and errors on anything else — old worlds Paper would have DFU-upgraded and
//! unknown-future versions both fail loudly instead of parsing under wrong
//! codec assumptions. An absent `Version` block (level-data format `version`
//! default `0`) is treated as incompatible rather than silently parsing an
//! unknown-version world. This guard is opt-in: the merged codec prereqs keep
//! parsing the header, and the boot composition (#516) decides when to call it.

use crate::level::storage::data_version::DataVersion;
use rivet_core::shared_constants::{SERIES, STABLE, VERSION_NAME, WORLD_VERSION};
use rivet_serialization::dynamic::Dynamic;
use rivet_serialization::dynamic_ops::DynamicOps;

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
    /// (default `0L`), then the optional `Version` compound. When the `Version`
    /// compound is absent, the Java `LevelVersion` stores `""`, `0`, `"main"`,
    /// `false` for the version fields (NOT the current-version fallbacks); the
    /// current-version fallbacks apply only per-field *inside* a present
    /// `Version` compound (see the module doc).
    pub fn parse<O>(input: &Dynamic<O>, ops: &impl DynamicOps<Output = O>) -> Self
    where
        O: Clone + std::fmt::Debug,
    {
        let level_data_version = input.get(ops, "version").as_int_or(ops, 0);
        let last_played = input.get(ops, "LastPlayed").as_long_or(ops, 0);
        let version = input.get(ops, "Version");
        if version.result().is_some() {
            let minecraft_version_name = version
                .get_field(ops, "Name")
                .as_string_or(ops, VERSION_NAME);
            let minecraft_version_id = version.get_field(ops, "Id").as_int_or(ops, WORLD_VERSION);
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

/// The no-migration data-version guard (see the module doc).
///
/// Returns `Ok(())` only when the world's `Version.Id` equals the pinned 26.2
/// `SharedConstants.WORLD_VERSION` **and** its series is the main series
/// (`DataVersion.isCompatible` against the current `WorldVersion`). `Err`
/// otherwise — including the absent-`Version`/`version`-`0` case, which Paper
/// would have file-fixed or rejected before parsing.
///
/// Paper's own file-fixing gate (`LevelStorageSource.getLevelDataAndDimensions`
/// / `LevelSummary`) keys off the *top-level* `DataVersion` tag via
/// `NbtUtils.getDataVersion`, whereas this guard keys off the `Version` block's
/// `Id`. The two agree on well-formed current-version worlds; this guard
/// deliberately rejects any world whose `Version` block is absent or not the
/// pinned main-series build, and the top-level `DataVersion` tag is not read
/// (Rivet has no DFU to upgrade through it).
pub fn ensure_compatible(level_version: &LevelVersion) -> Result<(), DataVersionMismatch> {
    let world_version = level_version.minecraft_version();
    if world_version.version == WORLD_VERSION && world_version.series == DataVersion::MAIN_SERIES {
        Ok(())
    } else {
        Err(DataVersionMismatch {
            found_version: world_version.version,
            found_series: world_version.series.clone(),
        })
    }
}

/// The incompatible-data-version error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "world data version {found_version} (series {found_series}) is incompatible: only the pinned 26.2 data version {WORLD_VERSION} is supported (no DFU migration)"
)]
pub struct DataVersionMismatch {
    /// The world's `Version.Id` (`0` when no `Version` block was present).
    pub found_version: i32,
    /// The world's `Version.Series`.
    pub found_series: String,
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

    /// `ensure_compatible` accepts only the pinned 26.2 data version in the
    /// main series.
    #[test]
    fn ensure_compatible_accepts_pinned_26_2() {
        let d = dynamic(nbt_compound());
        let ops = NbtOps::instance();
        let lv = LevelVersion::parse(&d, &ops);
        assert_eq!(ensure_compatible(&lv), Ok(()));
    }

    /// An old data version (Paper would have DFU-upgraded it) is rejected —
    /// Rivet has no migration, so it must fail loudly.
    #[test]
    fn ensure_compatible_rejects_old_version() {
        let mut tag = CompoundTag::new();
        tag.put_int("version", 19133);
        let mut version = CompoundTag::new();
        version.put_int("Id", 3700); // a pre-26.2 data version
        version.put_string("Series", "main");
        tag.put(
            "Version".to_string(),
            rivet_nbt::tag::Tag::Compound(version),
        );
        let ops = NbtOps::instance();
        let lv = LevelVersion::parse(&dynamic(tag), &ops);
        assert_eq!(
            ensure_compatible(&lv),
            Err(DataVersionMismatch {
                found_version: 3700,
                found_series: "main".to_string(),
            })
        );
    }

    /// A future / unknown data version is rejected too (parsing it under the
    /// current codec assumptions would be wrong).
    #[test]
    fn ensure_compatible_rejects_future_version() {
        let mut tag = CompoundTag::new();
        tag.put_int("version", 19133);
        let mut version = CompoundTag::new();
        version.put_int("Id", 5000);
        version.put_string("Series", "main");
        tag.put(
            "Version".to_string(),
            rivet_nbt::tag::Tag::Compound(version),
        );
        let ops = NbtOps::instance();
        let lv = LevelVersion::parse(&dynamic(tag), &ops);
        assert_eq!(
            ensure_compatible(&lv),
            Err(DataVersionMismatch {
                found_version: 5000,
                found_series: "main".to_string(),
            })
        );
    }

    /// A side-series version (same id, different series) is incompatible —
    /// Paper's `DataVersion.isCompatible` is series equality.
    #[test]
    fn ensure_compatible_rejects_side_series() {
        let mut tag = CompoundTag::new();
        tag.put_int("version", 19133);
        let mut version = CompoundTag::new();
        version.put_int("Id", 4903); // the 26.2 data version...
        version.put_string("Series", "snapshot"); // ...but a side series
        tag.put(
            "Version".to_string(),
            rivet_nbt::tag::Tag::Compound(version),
        );
        let ops = NbtOps::instance();
        let lv = LevelVersion::parse(&dynamic(tag), &ops);
        assert_eq!(
            ensure_compatible(&lv),
            Err(DataVersionMismatch {
                found_version: 4903,
                found_series: "snapshot".to_string(),
            })
        );
    }

    /// An absent `Version` block (format `version` default 0) is treated as
    /// incompatible rather than silently parsing an unknown-version world.
    #[test]
    fn ensure_compatible_rejects_absent_version_block() {
        let tag = CompoundTag::new();
        let ops = NbtOps::instance();
        let lv = LevelVersion::parse(&dynamic(tag), &ops);
        assert_eq!(
            ensure_compatible(&lv),
            Err(DataVersionMismatch {
                found_version: 0,
                found_series: "main".to_string(),
            })
        );
    }
}
