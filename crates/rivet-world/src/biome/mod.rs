//! `net.minecraft.world.level.biome` (issue #178, `mc.world.level.biome.core`
//! unit).
//!
//! The biome value core: [`climate`] ports the full `Climate` value layer
//! (parameters, parameter lists, the RTree index, the spawn finder, and the
//! quantization/wrapping/order semantics); [`biome_resolver`] ports the
//! `BiomeResolver` interface over `Holder<BiomeId>`; and the five value types
//! port their 26.2 surfaces:
//!
//! - [`biome`] — `Biome` (the `ClimateSettings` record, the
//!   `Precipitation`/`TemperatureModifier` enums, the `DIRECT_CODEC`/
//!   `NETWORK_CODEC`/`CODEC`/`LIST_CODEC`, the temperature/color behavior, and
//!   the `BiomeBuilder`) plus the `EnvironmentAttributeMap` STUB.
//! - [`biome_special_effects`] — the `BiomeSpecialEffects` record, its
//!   `CODEC`, `Builder`, and the `GrassColorModifier` enum.
//! - [`biome_generation_settings`] — the carvers/features settings, `CODEC`,
//!   `Builder`/`PlainBuilder`, and `EMPTY` (the `CONFIGURED_CARVER`/
//!   `PLACED_FEATURE` registry keys and the carver holder-set codec land here).
//! - [`mob_spawn_settings`] — the per-category spawner lists, spawn costs,
//!   `CODEC`, `Builder` (with the Paper perf subclasses), and the
//!   `SpawnerData`/`MobSpawnCost` records.
//! - [`biome_manager`] — the fiddled-distance corner interpolation over the
//!   `NoiseBiomeSource`, `obfuscateSeed`, and `CHUNK_CENTER_QUART`.
//!
//! The `mc.world.level.biome.data`/`.source` units build on top of this core.

// The `Biome` class module mirrors the Java file name; the containing module
// is the `net.minecraft.world.level.biome` package, so the classic
// package/class name collision trips `module_inception`.
#[allow(clippy::module_inception)]
pub mod biome;
pub mod biome_generation_settings;
pub mod biome_manager;
pub mod biome_resolver;
pub mod biome_special_effects;
pub mod climate;
pub mod mob_spawn_settings;

pub use biome::Biome;
pub use biome_generation_settings::BiomeGenerationSettings;
pub use biome_manager::BiomeManager;
pub use biome_resolver::BiomeResolver;
pub use biome_special_effects::BiomeSpecialEffects;
pub use climate::{Climate, Parameter, ParameterList, ParameterPoint, Sampler, TargetPoint};
pub use mob_spawn_settings::MobSpawnSettings;
