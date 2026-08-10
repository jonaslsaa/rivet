//! Port of `net.minecraft.SharedConstants` (MC 26.2) — the pinned compile-time
//! version constants. Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/SharedConstants.java`
//! and the `WorldVersion` it serves (`DetectedVersion` from
//! `paper-server/src/minecraft/resources/version.json`).
//!
//! Only the constants the codec-cascade prerequisites read are ported:
//! `WORLD_VERSION` (the data version, read by `NbtUtils.addCurrentDataVersion`
//! — see rivet-nbt's rewire, issue #202) and `getCurrentVersion()` (the
//! `WorldVersion` defaults `LevelVersion.parse` falls back to). The rest of
//! `SharedConstants` (network protocol version, pack formats, debug flags)
//! stays pinned in the crates that own those surfaces (e.g.
//! `PROTOCOL_VERSION` in rivet-server).
//!
//! The values are a manual port of the pinned 26.2 `version.json` /
//! `SharedConstants` — exactly the pattern `rivet-server` uses for
//! `PROTOCOL_VERSION = 776` and `rivet-util` for the `minecraft:core:26.2` pack
//! id. `getCurrentVersion()` returns a `&'static` value (Java's
//! `SharedConstants.CURRENT_VERSION` is a lazily-initialized singleton).

/// `SharedConstants.WORLD_VERSION` — the current data version (4903 in 26.2,
/// `DetectedVersion` `new DataVersion(4903, "main")`).
pub const WORLD_VERSION: i32 = 4903;

/// `SharedConstants.SERIES` — `"main"` (the default `DataVersion.series`).
pub const SERIES: &str = "main";

/// `SharedConstants.getCurrentVersion().name()` — 26.2's `WorldVersion.name`.
pub const VERSION_NAME: &str = "26.2";

/// `SharedConstants.getCurrentVersion().stable()` — 26.2 is a stable release.
pub const STABLE: bool = true;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_26_2_values_match_version_json() {
        // `version.json`: id 26.2, world_version 4903, series_id main, stable true.
        assert_eq!(VERSION_NAME, "26.2");
        assert_eq!(WORLD_VERSION, 4903);
        assert_eq!(SERIES, "main");
        assert!(STABLE);
    }
}
