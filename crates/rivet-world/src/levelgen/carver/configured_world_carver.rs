//! Port of `net.minecraft.world.level.levelgen.carver.ConfiguredWorldCarver`
//! (record, 26.2) — a world carver paired with its configuration.
//!
//! Java: `record ConfiguredWorldCarver<WC extends CarverConfiguration>
//! (WorldCarver<WC> worldCarver, WC config)`. The Rust shell keeps the config
//! generic and stores the carver's registry-held identity (`WorldCarverId`,
//! the `WorldCarver` half of the identity/behavior split — see `world_carver`).
//! `isStartChunk(RandomSource)` is `this.worldCarver.isStartChunk(this.config,
//! random)`, dispatched through `carver_is_start_chunk`.
//!
//! `ConfiguredWorldCarver.carve(...)` is NOT part of this shell: its signature
//! needs `CarvingContext`, `Aquifer`, `Function<BlockPos, Holder<Biome>>` and
//! `ChunkAccess`'s block surface, none of which are ported yet (see the module
//! doc and RivetTodo(#180)). The codecs (`DIRECT_CODEC`/`CODEC`/`LIST_CODEC`)
//! defer with the `#126` by-name codec surface.

use crate::levelgen::carver::CarverConfiguration;
use crate::levelgen::carver::world_carver::{WorldCarverId, carver_is_start_chunk};
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.carver.ConfiguredWorldCarver<WC extends
/// CarverConfiguration>` — the record pairing a carver with its configuration.
///
/// Generic in Java over the configuration (`WC`); the Rust port keeps the
/// configuration generic and stores the carver's registry-held identity
/// (`WorldCarverId`). Start-chunk testing dispatches through
/// `carver_is_start_chunk`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfiguredWorldCarver<C: CarverConfiguration> {
    /// `ConfiguredWorldCarver.worldCarver` — the carver's registry-held
    /// identity.
    pub world_carver: WorldCarverId,
    /// `ConfiguredWorldCarver.config` — the carver configuration.
    pub config: C,
}

impl<C: CarverConfiguration> ConfiguredWorldCarver<C> {
    /// `new ConfiguredWorldCarver(WorldCarver<WC> worldCarver, WC config)` —
    /// the record constructor.
    pub fn new(world_carver: WorldCarverId, config: C) -> Self {
        ConfiguredWorldCarver {
            world_carver,
            config,
        }
    }

    /// `ConfiguredWorldCarver.isStartChunk(RandomSource)` —
    /// `this.worldCarver.isStartChunk(this.config, random)`, dispatched through
    /// the `#180` carver hub.
    pub fn is_start_chunk<R: RandomSource>(&self, random: &mut R) -> bool {
        carver_is_start_chunk(self.world_carver.clone(), &self.config, random)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::carver::CarverConfiguration;

    /// A minimal configuration exercising the `WC extends CarverConfiguration`
    /// bound.
    #[derive(Debug)]
    struct TestCarverConfiguration;

    impl CarverConfiguration for TestCarverConfiguration {}

    #[test]
    fn record_pairs_carver_and_config() {
        // `new ConfiguredWorldCarver(WorldCarver, config)` — the record
        // constructor stores both accessor components verbatim.
        let carver = WorldCarverId::new(0, "minecraft:cave");
        let configured = ConfiguredWorldCarver::new(carver.clone(), TestCarverConfiguration);
        assert_eq!(configured.world_carver, carver);
        // The config is generic and preserved as-is (value-semantic `PartialEq`).
        assert!(matches!(configured.config, TestCarverConfiguration));
    }

    #[test]
    #[should_panic(expected = "Trying to check start chunk for world carver 'minecraft:cave'")]
    fn is_start_chunk_dispatches_to_the_hub() {
        // `ConfiguredWorldCarver.isStartChunk(random)` = `this.worldCarver.
        // isStartChunk(this.config, random)`; the hub is a pre-wire STUB until
        // the `#180` concrete carver bindings land.
        let configured = ConfiguredWorldCarver::new(
            WorldCarverId::new(0, "minecraft:cave"),
            TestCarverConfiguration,
        );
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let _ = configured.is_start_chunk(&mut random);
    }
}
