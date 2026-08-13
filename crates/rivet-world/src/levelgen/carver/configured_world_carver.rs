//! Port of `net.minecraft.world.level.levelgen.carver.ConfiguredWorldCarver`
//! (record, 26.2) — a world carver paired with its configuration.
//!
//! Java: `record ConfiguredWorldCarver<WC extends CarverConfiguration>
//! (WorldCarver<WC> worldCarver, WC config)`. The Rust shell keeps the config
//! generic and stores the carver's registry-held identity (`WorldCarverId`,
//! the `WorldCarver` half of the identity/behavior split — see `world_carver`).
//! `isStartChunk(RandomSource)` is `this.worldCarver.isStartChunk(this.config,
//! random)`, dispatched through `carver_is_start_chunk`; `carve(...)` is
//! `!debugVoidTerrain(chunk.getPos()) && this.worldCarver.carve(...)`,
//! dispatched through `carver_carve`.
//!
//! The `debugVoidTerrain` gate is `rivet_core::shared_constants::
//! debug_void_terrain(chunk.getPos().getMinBlockX(), ...getMinBlockZ())`.
//!
//! The codecs (`DIRECT_CODEC`/`CODEC`/`LIST_CODEC`, `WorldCarver.
//! configuredCodec`) defer with the `#126` by-name codec surface.

use crate::chunk::carving_mask::CarvingMask;
use crate::levelgen::carver::CarverConfiguration;
use crate::levelgen::carver::carving_context::CarvingContext;
use crate::levelgen::carver::world_carver::{
    CarveChunk, WorldCarverId, carver_carve, carver_is_start_chunk,
};
use crate::levelgen::noisegen::aquifer::Aquifer;
use rivet_registry::core::ChunkPos;
use rivet_util::RandomSource;
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.carver.ConfiguredWorldCarver<WC extends
/// CarverConfiguration>` — the record pairing a carver with its configuration.
///
/// Generic in Java over the configuration (`WC`); the Rust port keeps the
/// configuration generic and stores the carver's registry-held identity
/// (`WorldCarverId`). Start-chunk testing dispatches through
/// `carver_is_start_chunk`.
///
/// Value-semantic `PartialEq` mirrors the record's generated `equals`; `Eq` and
/// `Hash` are deliberately omitted — Java's record also has `hashCode`, but no
/// current consumer hashes configured carvers, so the port keeps the same
/// surface as `ConfiguredFeature` (the `#181` analogue).
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
    /// the carver hub.
    pub fn is_start_chunk<R: RandomSource>(&self, random: &mut R) -> bool {
        carver_is_start_chunk(self.world_carver.clone(), &self.config, random)
    }

    /// `ConfiguredWorldCarver.carve(CarvingContext, ChunkAccess, biomeGetter,
    /// RandomSource, Aquifer, ChunkPos, CarvingMask)` — `!debugVoidTerrain(
    /// chunk.getPos()) && this.worldCarver.carve(...)`. The `biomeGetter` is
    /// folded into the `CarvingContext.topMaterial` seam (see `world_carver`).
    pub fn carve<R: RandomSource>(
        &self,
        context: &CarvingContext,
        chunk: &mut dyn CarveChunk,
        random: &mut R,
        aquifer: &dyn Aquifer,
        source_chunk_pos: &ChunkPos,
        mask: &mut CarvingMask,
    ) -> bool {
        let pos = chunk.get_pos();
        if rivet_core::shared_constants::debug_void_terrain(
            pos.get_min_block_x(),
            pos.get_min_block_z(),
        ) {
            return false;
        }
        carver_carve(
            self.world_carver.clone(),
            &self.config,
            context,
            chunk,
            random,
            aquifer,
            source_chunk_pos,
            mask,
        )
    }

    /// Erase to the wildcard `ConfiguredWorldCarver<?>` — the form stored in
    /// `BiomeGenerationSettings.carvers` and the `LIST_CODEC` holder sets.
    pub fn into_erased(self) -> ConfiguredWorldCarverErased {
        ConfiguredWorldCarverErased {
            world_carver: self.world_carver,
            config: Arc::new(self.config),
        }
    }
}

/// `toString()` — the record default
/// `"ConfiguredWorldCarver[worldCarver=..., config=...]"` (Java does not
/// override `toString` on this record, unlike `ConfiguredFeature`). The
/// `worldCarver` component renders the carver's registry identity; the config
/// renders its `Debug` form, the closest value-string stand-in (the same
/// convention the `ConfiguredFeature` port uses).
impl<C: CarverConfiguration> fmt::Display for ConfiguredWorldCarver<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ConfiguredWorldCarver[worldCarver={:?}, config={:?}]",
            self.world_carver, self.config
        )
    }
}

/// Java's `ConfiguredWorldCarver<?>` wildcard, erased. Java erases both the
/// configuration and the carver to their bounds; the Rust port erases the
/// carver to its `WorldCarverId` identity and the configuration to a
/// `dyn CarverConfiguration`. The concrete configuration type is recovered by
/// the `#180` dispatch, which downcasts before calling the concrete carver's
/// behavior. Like `ConfiguredFeatureErased`, the erased form keeps the
/// behavior surface the wildcard inherits (`isStartChunk`, mirroring Java's
/// `ConfiguredWorldCarver<?>.isStartChunk`; `carve` is only on the generic
/// form — no consumer calls the wildcard's `carve` through the erased
/// surface).
///
/// The wildcard's record `equals`/`hashCode` are not ported here: a
/// `dyn CarverConfiguration` is not object-safely comparable, so the erased
/// form is equality-less (`Debug`/`Clone` only), a deliberate erasure cost
/// mirroring `ConfiguredFeatureErased`. The generic form keeps value-semantic
/// `PartialEq` (Java's generated record `equals`); port the erased equality
/// when a consumer needs it (no consumer compares erased carver holder sets).
#[derive(Debug, Clone)]
pub struct ConfiguredWorldCarverErased {
    /// `ConfiguredWorldCarver.worldCarver` — the carver's registry-held
    /// identity.
    pub world_carver: WorldCarverId,
    /// `ConfiguredWorldCarver.config`, erased to the `CarverConfiguration`
    /// surface.
    pub config: Arc<dyn CarverConfiguration>,
}

impl ConfiguredWorldCarverErased {
    /// `new ConfiguredWorldCarver(WorldCarver, config)` from the erased halves.
    pub fn new(world_carver: WorldCarverId, config: Arc<dyn CarverConfiguration>) -> Self {
        ConfiguredWorldCarverErased {
            world_carver,
            config,
        }
    }

    /// `ConfiguredWorldCarver<?>.isStartChunk(RandomSource)` — the wildcard
    /// inherits the record's start-chunk test from the erased halves
    /// (`this.worldCarver.isStartChunk(this.config, random)`), dispatched
    /// through `carver_is_start_chunk` (mirroring
    /// `ConfiguredFeatureErased::place`).
    pub fn is_start_chunk<R: RandomSource>(&self, random: &mut R) -> bool {
        carver_is_start_chunk(self.world_carver.clone(), self.config.as_ref(), random)
    }
}

/// `ConfiguredWorldCarver<?>.toString()` — the same record default `toString`
/// as the generic form (Java's record `toString` is type-agnostic after
/// erasure, so the erased wildcard prints the same shape).
impl fmt::Display for ConfiguredWorldCarverErased {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ConfiguredWorldCarver[worldCarver={:?}, config={:?}]",
            self.world_carver, self.config
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::carver::carver_configuration::CarverConfigurationBase;
    use crate::levelgen::carver::carver_debug_settings::CarverDebugSettings;
    use crate::levelgen::carver::cave_carver_configuration::CaveCarverConfiguration;
    use crate::levelgen::heightproviders::constant_height::ConstantHeight;
    use crate::levelgen::heightproviders::height_provider::HeightProvider;
    use crate::levelgen::vertical_anchor::VerticalAnchor;
    use rivet_registry::holder_set::HolderSet;
    use rivet_util::valueproviders::constant_float::ConstantFloat;
    use rivet_util::valueproviders::float_provider::FloatProvider;

    fn config() -> CarverConfigurationBase {
        CarverConfigurationBase::new(
            1.0,
            HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::absolute(0))),
            FloatProvider::Constant(ConstantFloat::of(1.0)),
            VerticalAnchor::absolute(0),
            CarverDebugSettings::default(),
            HolderSet::Direct(Vec::new()),
        )
    }

    /// A `CaveCarverConfiguration` for the real-dispatch tests (the `CAVE`
    /// hub downcasts the erased config to it).
    fn cave_config() -> CaveCarverConfiguration {
        CaveCarverConfiguration::new(
            1.0,
            HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::absolute(0))),
            FloatProvider::Constant(ConstantFloat::of(1.0)),
            VerticalAnchor::absolute(0),
            CarverDebugSettings::default(),
            HolderSet::Direct(Vec::new()),
            FloatProvider::Constant(ConstantFloat::of(1.0)),
            FloatProvider::Constant(ConstantFloat::of(1.0)),
            FloatProvider::Constant(ConstantFloat::of(0.0)),
        )
    }

    #[test]
    fn record_pairs_carver_and_config() {
        // `new ConfiguredWorldCarver(WorldCarver, config)` — the record
        // constructor stores both accessor components verbatim.
        let carver = WorldCarverId::new(0, "minecraft:cave");
        let cfg = config();
        let configured = ConfiguredWorldCarver::new(carver.clone(), cfg.clone());
        assert_eq!(configured.world_carver, carver);
        assert_eq!(configured.config, cfg);
    }

    #[test]
    fn record_to_string_mirrors_record_default() {
        // Java's record default toString:
        // `ConfiguredWorldCarver[worldCarver=..., config=...]`. The carver
        // renders its registry identity (Debug) and the config its Debug form.
        let configured =
            ConfiguredWorldCarver::new(WorldCarverId::new(0, "minecraft:cave"), config());
        let s = configured.to_string();
        assert!(
            s.starts_with(
                "ConfiguredWorldCarver[worldCarver=WorldCarverId { id: 0, location: \"minecraft:cave\" }, config=CarverConfigurationBase {"
            ),
            "record default opens with the worldCarver + config components: {s}"
        );
        assert!(
            s.ends_with("}]"),
            "record default closes with the config: {s}"
        );
    }

    #[test]
    fn erased_to_string_mirrors_record_default() {
        // The erased wildcard `ConfiguredWorldCarver<?>` inherits the same
        // record default toString shape as the generic form.
        let configured =
            ConfiguredWorldCarver::new(WorldCarverId::new(0, "minecraft:cave"), config())
                .into_erased();
        let s = configured.to_string();
        assert!(
            s.starts_with(
                "ConfiguredWorldCarver[worldCarver=WorldCarverId { id: 0, location: \"minecraft:cave\" }, config=CarverConfigurationBase {"
            ),
            "erased record default opens with the worldCarver + config components: {s}"
        );
        assert!(
            s.ends_with("}]"),
            "erased record default closes with the config: {s}"
        );
    }

    #[test]
    fn erased_form_keeps_is_start_chunk_on_the_surface() {
        // Java's wildcard `ConfiguredWorldCarver<?>` inherits `isStartChunk`
        // (`this.worldCarver.isStartChunk(this.config, random)`); the erased
        // form dispatches through the same `carver_is_start_chunk` hub
        // (mirroring `ConfiguredFeatureErased::place`).
        let configured =
            ConfiguredWorldCarver::new(WorldCarverId::CAVE, cave_config()).into_erased();
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        // `isStartChunk` = `random.nextFloat() <= probability`; probability 1.0
        // is always reached.
        assert!(configured.is_start_chunk(&mut random));
    }
}
