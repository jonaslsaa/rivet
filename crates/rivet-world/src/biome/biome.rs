//! `net.minecraft.world.level.biome.Biome` — identity shell (issue #178,
//! `mc.world.level.biome.core` unit).
//!
//! Java's `Biome` is a behaviour-carrying registry element: the
//! `ClimateSettings` record, `Precipitation`/`TemperatureModifier` enums, the
//! `DIRECT_CODEC`/`NETWORK_CODEC`/`CODEC`/`LIST_CODEC`, the
//! `EnvironmentAttributeMap`, the temperature/freeze/snow behavior, and the
//! `BiomeBuilder` all defer to the owning unit. This slice only needs the
//! *type identity* so the biome registry can carry `Holder<Biome>` references
//! and downstream `mc.world.level.biome.source` types can name the element.
//!
//! RivetTodo(#178): full value/codec/behavior port.

use crate::levelgen::synth::perlin_simplex_noise::PerlinSimplexNoise;
use rivet_util::random::LegacyRandomSource;
use rivet_util::worldgen_random::WorldgenRandom;
use std::sync::LazyLock;

/// `Biome.BIOME_INFO_NOISE` — `new PerlinSimplexNoise(new WorldgenRandom(new
/// LegacyRandomSource(2345L)), ImmutableList.of(0))`, marked
/// `@Deprecated(forRemoval = true)` in Java.
///
/// STUB(mc.world.level.biome.core) — the `Biome` value core (issue #178) has
/// not ported its static noise fields (`TEMPERATURE_NOISE`,
/// `FROZEN_TEMPERATURE_NOISE`, `BIOME_INFO_NOISE`). The placement modifiers
/// (`mc.world.level.levelgen.placement.repeating`) sample `BIOME_INFO_NOISE`
/// in their `count` hooks, so it is declared here as a functional out-of-unit
/// stub built on the already-ported `synth::PerlinSimplexNoise` — the exact
/// seed/RNG construction from `Biome.java`'s static initializer. When
/// `biome.core` lands its statics, this declaration should be replaced by the
/// owning unit's (the placement unit reads it through the same path).
pub static BIOME_INFO_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(2345));
    PerlinSimplexNoise::new(&mut random, &[0])
});

/// `net.minecraft.world.level.biome.Biome` — the biome element identity. A
/// unit shell: no fields, no codec, no behavior (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Biome;
