//! `net.minecraft.world.level.biome.MobSpawnSettings` — identity shell (issue
//! #178, `mc.world.level.biome.core` unit).
//!
//! Java's class carries the per-`MobCategory` `WeightedList<SpawnerData>`
//! spawners, the `MobSpawnCost` map, `creatureGenerationProbability`, the
//! `CODEC`, the `Builder`, and the `SpawnerData`/`MobSpawnCost` records. All
//! of it defers to the owning unit (it is entangled with `MobCategory`,
//! `EntityType`/`EntityTypes`, and `WeightedList` — the `mc.world.entity`
//! layer). This slice only carries the type identity.
//!
//! RivetTodo(#178): full value/codec/behavior port.

/// `net.minecraft.world.level.biome.MobSpawnSettings` — the mob-spawn-settings
/// identity. A unit shell (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MobSpawnSettings;

impl MobSpawnSettings {
    /// `MobSpawnSettings.EMPTY` — the identity shell's single value.
    pub const EMPTY: MobSpawnSettings = MobSpawnSettings;
}
