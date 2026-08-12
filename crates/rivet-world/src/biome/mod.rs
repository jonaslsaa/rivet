//! `net.minecraft.world.level.biome` (issue #178, `mc.world.level.biome.core`
//! unit).
//!
//! The biome value core: [`climate`] ports the full `Climate` value layer
//! (parameters, parameter lists, the RTree index, the spawn finder, and the
//! quantization/wrapping/order semantics), [`biome_resolver`] ports the
//! `BiomeResolver` interface over `Holder<BiomeId>`, and the remaining five
//! classes (`Biome`, `BiomeGenerationSettings`, `BiomeManager`,
//! `BiomeSpecialEffects`, `MobSpawnSettings`) land as minimal compile-safe
//! identity shells — the value/codec/behavior surface of each defers to this
//! unit (sparse `RivetTodo(#178)` markers; no fake behavior or codecs).
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
