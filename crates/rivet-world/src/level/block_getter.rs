//! `net.minecraft.world.level.BlockGetter` — the world's block-state accessor.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! BlockGetter.java`. The #232 value slice stakes the interface's place in the
//! `Level` hierarchy (its methods sit below `getGameTime` on the `Level` trait
//! chain) but defers the surface: the required methods return `BlockEntity`/
//! `BlockState`/`FluidState`, none of which are ported yet (the block-state
//! surface is active #228 in `rivet-registry`, and this unit stays disjoint
//! from it).

use super::LevelHeightAccessor;

/// `BlockGetter` — `getBlockState`/`getFluidState` access.
///
/// RivetTodo(#232): the `getBlockEntity`/`getBlockState`/`getFluidState` and
/// Paper `getBlockStateIfLoaded`/`getFluidIfLoaded` requirements plus the
/// default raytrace/collision surface (`traverseBlocks`, `clip`,
/// `forEachBlockIntersectedBetween`, `getBlockStates`, `getBlockFloorHeight`,
/// `clipWithInteractionOverride`) defer until the block-state/fluid/AABB
/// surfaces land (#228 / the chunk unit). The interface stays as the `Level`
/// trait-chain anchor below `LevelReader`.
pub trait BlockGetter: LevelHeightAccessor {}
