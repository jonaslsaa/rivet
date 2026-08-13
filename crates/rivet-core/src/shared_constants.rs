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

// ---------------------------------------------------------------------------
// Debug flags (worldgen) — the `SharedConstants.DEBUG_*` booleans the
// `mc.world.level.levelgen.noisegen` unit reads. Every one is
// `debugFlag("...")`, i.e. `System.getProperty("debug." + key) != null`. A
// system property is absent in normal runs, so every flag is `false`; the
// value is a faithful port of the *pinned default* (the system-property probe
// itself — reading `std::env::var("debug.ORE_VEINS")` — is not observable in
// a server process and is not ported; the compile-time `false` matches the
// non-debug default exactly).
// ---------------------------------------------------------------------------

/// `SharedConstants.DEBUG_ORE_VEINS` — `debugFlag("ORE_VEINS")`.
pub const DEBUG_ORE_VEINS: bool = false;
/// `SharedConstants.DEBUG_AQUIFERS` — `debugFlag("AQUIFERS")`.
pub const DEBUG_AQUIFERS: bool = false;
/// `SharedConstants.DEBUG_DISABLE_FLUID_GENERATION` —
/// `debugFlag("DISABLE_FLUID_GENERATION")`.
pub const DEBUG_DISABLE_FLUID_GENERATION: bool = false;
/// `SharedConstants.DEBUG_DISABLE_AQUIFERS` — `debugFlag("DISABLE_AQUIFERS")`.
pub const DEBUG_DISABLE_AQUIFERS: bool = false;
/// `SharedConstants.DEBUG_DISABLE_SURFACE` — `debugFlag("DISABLE_SURFACE")`.
pub const DEBUG_DISABLE_SURFACE: bool = false;
/// `SharedConstants.DEBUG_DISABLE_CARVERS` — `debugFlag("DISABLE_CARVERS")`.
pub const DEBUG_DISABLE_CARVERS: bool = false;
/// `SharedConstants.DEBUG_CARVERS` — `debugFlag("CARVERS")`; the carver unit
/// (`mc.world.level.levelgen.carver`) reads it in `WorldCarver.isDebugEnabled`.
pub const DEBUG_CARVERS: bool = false;
/// `SharedConstants.DEBUG_DISABLE_ORE_VEINS` — `debugFlag("DISABLE_ORE_VEINS")`.
pub const DEBUG_DISABLE_ORE_VEINS: bool = false;
/// `SharedConstants.DEBUG_ONLY_GENERATE_HALF_THE_WORLD` —
/// `debugFlag("ONLY_GENERATE_HALF_THE_WORLD")`.
pub const DEBUG_ONLY_GENERATE_HALF_THE_WORLD: bool = false;
/// `SharedConstants.debugGenerateSquareTerrainWithoutNoise` —
/// `debugFlag("GENERATE_SQUARE_TERRAIN_WITHOUT_NOISE")`.
pub const DEBUG_GENERATE_SQUARE_TERRAIN_WITHOUT_NOISE: bool = false;

/// `SharedConstants.debugVoidTerrain(ChunkPos)` — the debug void-terrain gate
/// `NoiseBasedChunkGenerator.buildSurface`/`fillFromNoise` consult.
///
/// ```java
/// int posX = pos.getMinBlockX();
/// int posZ = pos.getMinBlockZ();
/// return DEBUG_ONLY_GENERATE_HALF_THE_WORLD
///     ? posZ < 0
///     : debugGenerateSquareTerrainWithoutNoise
///         && (posX > 8192 || posX < 0 || posZ > 1024 || posZ < 0);
/// ```
///
/// With both flags `false` (the pinned defaults) the function is a constant
/// `false`; it is ported in full so the debug-flags build keeps Java's exact
/// geometry gate. `rivet-core` cannot name `ChunkPos` (a `rivet-registry`
/// type — that crate depends on `rivet-core`), so the caller passes the two
/// min-block coordinates `ChunkPos.getMinBlockX()`/`getMinBlockZ()`.
pub fn debug_void_terrain(pos_x: i32, pos_z: i32) -> bool {
    if DEBUG_ONLY_GENERATE_HALF_THE_WORLD {
        pos_z < 0
    } else {
        DEBUG_GENERATE_SQUARE_TERRAIN_WITHOUT_NOISE
            && (!(0..=8192).contains(&pos_x) || !(0..=1024).contains(&pos_z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_26_2_values_match_version_json() {
        // `version.json`: id 26.2, world_version 4903, series_id main, stable true.
        assert_eq!(VERSION_NAME, "26.2");
        assert_eq!(WORLD_VERSION, 4903);
        assert_eq!(SERIES, "main");
        // `STABLE` is compile-time known, so assert it in a const block: the pin
        // fails the build (not just the test) if the constant ever drifts.
        const { assert!(STABLE) };
    }
}
