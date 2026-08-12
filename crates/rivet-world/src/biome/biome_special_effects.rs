//! `net.minecraft.world.level.biome.BiomeSpecialEffects` — identity shell
//! (issue #178, `mc.world.level.biome.core` unit).
//!
//! Java's record carries the water/foliage/grass colors, the
//! `GrassColorModifier` enum, the `CODEC`, and the `Builder`. All of it defers
//! to the owning unit (the color codecs need `ExtraCodecs.STRING_RGB_COLOR`
//! and the `GrassColorModifier` `StringRepresentable` enum). This slice only
//! carries the type identity.
//!
//! RivetTodo(#178): full value/codec/behavior port.

/// `net.minecraft.world.level.biome.BiomeSpecialEffects` — the biome
/// special-effects identity. A unit shell (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BiomeSpecialEffects;
