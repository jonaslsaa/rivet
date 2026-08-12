//! Port of `net.minecraft.world.level.levelgen.OreVeinifier` (class, 26.2) —
//! the ore-vein `BlockStateFiller` the noise chunk registers when
//! `NoiseGeneratorSettings.oreVeinsEnabled()`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/levelgen/OreVeinifier.java`.
//!
//! The Java class is a stateless holder for the static `create(...)` factory;
//! Rust ports it as the free [`create`] function plus the [`VeinType`] enum.
//! Java's returned lambda is a `NoiseChunk.BlockStateFiller`; Rust has no way
//! for a closure to implement the custom [`BlockStateFiller`] trait, so the
//! closure body lives in the private [`OreVeinifierFiller`] struct.
//!
//! ## Float-promotion fidelity
//!
//! Every `float` literal (`0.4F`, `0.7F`, `0.1F`, `0.3F`, `0.6F`, `0.02F`,
//! `-0.3F`) is promoted to `double` where it meets a `double` operand (Java
//! binary numeric promotion). The port writes those literals as
//! `<literal>f32 as f64` so the promoted value is bit-identical
//! (`0.4f32 as f64 == (double)0.4F`). In particular:
//!
//! - `Mth.clampedMap(distanceFromEdge, 0.0, 20.0, -0.2, 0.0)` — the first arg
//!   is `(double)int`, the bounds are exact `double` literals.
//! - `Mth.clampedMap(veininessRidged, 0.4F, 0.6F, 0.1F, 0.3F)` — the four
//!   float bounds promote to their exact widened doubles.
//! - `positionalRandom.nextFloat() < richness` — `nextFloat()` returns `float`,
//!   `richness` is `double`; the float widens before the comparison.

use crate::block::blocks::Blocks;
use crate::levelgen::noise::density_function::{DensityFunction, FunctionContext};
use crate::levelgen::noisegen::noise_chunk::BlockStateFiller;
use rivet_registry::block_state::BlockState;
use rivet_util::mth;
use rivet_util::random::{PositionalRandomFactory, RandomSource};
use std::sync::Arc;

/// `OreVeinifier.VEININESS_THRESHOLD` — `0.4F`.
const VEININESS_THRESHOLD: f32 = 0.4;
/// `OreVeinifier.MAX_EDGE_ROUNDOFF` — `0.2` (a `double` constant).
const MAX_EDGE_ROUNDOFF: f64 = 0.2;
/// `OreVeinifier.VEIN_SOLIDNESS` — `0.7F`.
const VEIN_SOLIDNESS: f32 = 0.7;
/// `OreVeinifier.MIN_RICHNESS` — `0.1F`.
const MIN_RICHNESS: f32 = 0.1;
/// `OreVeinifier.MAX_RICHNESS` — `0.3F`.
const MAX_RICHNESS: f32 = 0.3;
/// `OreVeinifier.MAX_RICHNESS_THRESHOLD` — `0.6F`.
const MAX_RICHNESS_THRESHOLD: f32 = 0.6;
/// `OreVeinifier.CHANCE_OF_RAW_ORE_BLOCK` — `0.02F`.
const CHANCE_OF_RAW_ORE_BLOCK: f32 = 0.02;
/// `OreVeinifier.SKIP_ORE_IF_GAP_NOISE_IS_BELOW` — `-0.3F`.
const SKIP_ORE_IF_GAP_NOISE_IS_BELOW: f32 = -0.3;
/// `OreVeinifier.EDGE_ROUNDOFF_BEGIN` — `20`.
const EDGE_ROUNDOFF_BEGIN: i32 = 20;

/// `OreVeinifier.create(...)` — the ore-vein `BlockStateFiller`.
///
/// `defaultState = SharedConstants.DEBUG_ORE_VEINS ? Blocks.AIR.defaultBlockState()
/// : null`. The filler returns `defaultState` (a `None`) for every non-ore
/// decision, exactly Java's nullable-`defaultState` propagation.
pub fn create(
    vein_toggle: Arc<dyn DensityFunction>,
    vein_ridged: Arc<dyn DensityFunction>,
    vein_gap: Arc<dyn DensityFunction>,
    ore_veins_positional_random_factory: impl PositionalRandomFactory + Send + Sync + 'static,
) -> Arc<dyn BlockStateFiller> {
    let default_state = if rivet_core::shared_constants::DEBUG_ORE_VEINS {
        Some(Blocks::AIR.default_block_state())
    } else {
        None
    };
    Arc::new(OreVeinifierFiller {
        vein_toggle,
        vein_ridged,
        vein_gap,
        ore_veins_positional_random_factory,
        default_state,
    })
}

/// The private struct backing [`create`] — Java's returned lambda.
struct OreVeinifierFiller<F> {
    vein_toggle: Arc<dyn DensityFunction>,
    vein_ridged: Arc<dyn DensityFunction>,
    vein_gap: Arc<dyn DensityFunction>,
    ore_veins_positional_random_factory: F,
    default_state: Option<BlockState>,
}

impl<F: PositionalRandomFactory + Send + Sync> BlockStateFiller for OreVeinifierFiller<F> {
    fn calculate(&self, context: &dyn FunctionContext) -> Option<BlockState> {
        let ore_veininess_noise_value = self.vein_toggle.compute(context);
        let pos_y = context.block_y();
        let vein_type = if ore_veininess_noise_value > 0.0 {
            VeinType::Copper
        } else {
            VeinType::Iron
        };
        let veininess_ridged = ore_veininess_noise_value.abs();
        // Java `int` arithmetic — wraps.
        let distance_from_top = vein_type.max_y().wrapping_sub(pos_y);
        let distance_from_bottom = pos_y.wrapping_sub(vein_type.min_y());
        if distance_from_bottom >= 0 && distance_from_top >= 0 {
            let distance_from_edge = distance_from_top.min(distance_from_bottom);
            // `Mth.clampedMap(distanceFromEdge, 0.0, 20.0, -0.2, 0.0)` — the
            // `(double)` first arg promotes the `int`.
            let edge_roundoff = mth::clamped_map(
                distance_from_edge as f64,
                0.0,
                EDGE_ROUNDOFF_BEGIN as f64,
                -MAX_EDGE_ROUNDOFF,
                0.0,
            );
            if veininess_ridged + edge_roundoff < VEININESS_THRESHOLD as f64 {
                self.default_state
            } else {
                let mut positional_random = self.ore_veins_positional_random_factory.at(
                    context.block_x(),
                    pos_y,
                    context.block_z(),
                );
                #[allow(clippy::if_same_then_else)]
                // Java's exact chain: both `nextFloat() > 0.7F` and `veinRidged >= 0.0` return `defaultState` (OreVeinifier.java:44-47).
                if positional_random.next_float() as f64 > VEIN_SOLIDNESS as f64 {
                    self.default_state
                } else if self.vein_ridged.compute(context) >= 0.0 {
                    self.default_state
                } else {
                    // `Mth.clampedMap(veininessRidged, 0.4F, 0.6F, 0.1F, 0.3F)`
                    // — the float bounds promote to double; fromMin is
                    // `VEININESS_THRESHOLD (0.4F)`, fromMax `MAX_RICHNESS_THRESHOLD
                    // (0.6F)`, toMin `MIN_RICHNESS (0.1F)`, toMax `MAX_RICHNESS (0.3F)`.
                    let richness = mth::clamped_map(
                        veininess_ridged,
                        VEININESS_THRESHOLD as f64,
                        MAX_RICHNESS_THRESHOLD as f64,
                        MIN_RICHNESS as f64,
                        MAX_RICHNESS as f64,
                    );
                    if (positional_random.next_float() as f64) < richness
                        && self.vein_gap.compute(context) > (SKIP_ORE_IF_GAP_NOISE_IS_BELOW as f64)
                    {
                        if (positional_random.next_float() as f64) < CHANCE_OF_RAW_ORE_BLOCK as f64
                        {
                            Some(vein_type.raw_ore_block())
                        } else {
                            Some(vein_type.ore())
                        }
                    } else if rivet_core::shared_constants::DEBUG_ORE_VEINS {
                        Some(Blocks::OAK_BUTTON.default_block_state())
                    } else {
                        Some(vein_type.filler())
                    }
                }
            }
        } else {
            self.default_state
        }
    }
}

/// `OreVeinifier.VeinType` — the two ore-vein archetypes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VeinType {
    /// `COPPER(Blocks.COPPER_ORE, Blocks.RAW_COPPER_BLOCK, Blocks.GRANITE, 0, 50)`.
    Copper,
    /// `IRON(Blocks.DEEPSLATE_IRON_ORE, Blocks.RAW_IRON_BLOCK, Blocks.TUFF, -60, -8)`.
    Iron,
}

impl VeinType {
    /// `ore` (record accessor).
    pub fn ore(self) -> BlockState {
        match self {
            VeinType::Copper => Blocks::COPPER_ORE.default_block_state(),
            VeinType::Iron => Blocks::DEEPSLATE_IRON_ORE.default_block_state(),
        }
    }

    /// `rawOreBlock` (record accessor).
    pub fn raw_ore_block(self) -> BlockState {
        match self {
            VeinType::Copper => Blocks::RAW_COPPER_BLOCK.default_block_state(),
            VeinType::Iron => Blocks::RAW_IRON_BLOCK.default_block_state(),
        }
    }

    /// `filler` (record accessor).
    pub fn filler(self) -> BlockState {
        match self {
            VeinType::Copper => Blocks::GRANITE.default_block_state(),
            VeinType::Iron => Blocks::TUFF.default_block_state(),
        }
    }

    /// `minY` (record accessor).
    pub fn min_y(self) -> i32 {
        match self {
            VeinType::Copper => 0,
            VeinType::Iron => -60,
        }
    }

    /// `maxY` (record accessor).
    pub fn max_y(self) -> i32 {
        match self {
            VeinType::Copper => 50,
            VeinType::Iron => -8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::noise::density_function::SinglePointContext;
    use crate::levelgen::noise::density_functions;
    use rivet_util::random::XoroshiroPositionalRandomFactory;

    #[test]
    fn vein_type_archetypes_match_java() {
        assert_eq!(VeinType::Copper.min_y(), 0);
        assert_eq!(VeinType::Copper.max_y(), 50);
        assert_eq!(VeinType::Iron.min_y(), -60);
        assert_eq!(VeinType::Iron.max_y(), -8);
        assert_eq!(
            VeinType::Copper.ore(),
            Blocks::COPPER_ORE.default_block_state()
        );
        assert_eq!(
            VeinType::Copper.raw_ore_block(),
            Blocks::RAW_COPPER_BLOCK.default_block_state()
        );
        assert_eq!(
            VeinType::Copper.filler(),
            Blocks::GRANITE.default_block_state()
        );
        assert_eq!(
            VeinType::Iron.ore(),
            Blocks::DEEPSLATE_IRON_ORE.default_block_state()
        );
        assert_eq!(
            VeinType::Iron.raw_ore_block(),
            Blocks::RAW_IRON_BLOCK.default_block_state()
        );
        assert_eq!(VeinType::Iron.filler(), Blocks::TUFF.default_block_state());
    }

    #[test]
    fn out_of_vertical_range_returns_default() {
        // A constant toggling vein with `blockY` far below IRON's range
        // (`minY -60`) — `distanceFromBottom` negative → `defaultState`.
        let toggle = density_functions::constant(1.0); // > 0.0 → COPPER (0..50)
        let ridged = density_functions::constant(-100.0);
        let gap = density_functions::constant(0.0);
        let filler = create(
            toggle,
            ridged,
            gap,
            XoroshiroPositionalRandomFactory::new(0, 0),
        );
        let context = SinglePointContext::new(0, -100, 0);
        // Debug off → defaultState is `None`.
        assert_eq!(filler.calculate(&context), None);
    }

    #[test]
    fn within_range_but_not_veiny_returns_default() {
        // `veininessRidged + edgeRoundoff < 0.4F` at a deep in-range y where
        // `distanceFromEdge` is large (edgeRoundoff ~ 0) and `veininess` tiny.
        let toggle = density_functions::constant(0.1); // > 0 → COPPER, |0.1| small
        let ridged = density_functions::constant(100.0);
        let gap = density_functions::constant(0.0);
        let filler = create(
            toggle,
            ridged,
            gap,
            XoroshiroPositionalRandomFactory::new(0, 0),
        );
        let context = SinglePointContext::new(0, 25, 0);
        assert_eq!(filler.calculate(&context), None);
    }
}
