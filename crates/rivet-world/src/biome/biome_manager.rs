//! `net.minecraft.world.level.biome.BiomeManager` — identity shell (issue
//! #178, `mc.world.level.biome.core` unit).
//!
//! Java's class carries the `NoiseBiomeSource` + `biomeZoomSeed`, the
//! `getBiome` fiddled-distance corner interpolation, `getNoiseBiomeAtPosition`
//! / `getNoiseBiomeAtQuart`, the `NoiseBiomeSource` interface, and
//! `CHUNK_CENTER_QUART`/`obfuscateSeed`. All of it defers to the owning unit
//! (the fiddled-distance interpolation needs the biome zoom seed plumbing from
//! the world/level layer). This slice only carries the type identity.
//!
//! RivetTodo(#178): full value/behavior port.

/// `net.minecraft.world.level.biome.BiomeManager` — the biome-manager
/// identity. A unit shell (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BiomeManager;
