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

/// `net.minecraft.world.level.biome.Biome` — the biome element identity. A
/// unit shell: no fields, no codec, no behavior (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Biome;
