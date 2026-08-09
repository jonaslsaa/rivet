//! `net.minecraft.world.level.levelgen` — worldgen module. Only the
//! client-heightmap slice (`Heightmap`, issue #100), the
//! `GenerationStep.Decoration` enum (proactively ported from the pending
//! `mc.world.level.levelgen.settings` unit — see `generation_step.rs`), the
//! `feature.configurations` core slice (the `mc.world.level.levelgen.feature.configurations.core`
//! unit), the `placement` core slice (the
//! `mc.world.level.levelgen.placement.core` unit), and the
//! `WorldGenerationContext` window are ported so far; the generators/feature
//! worldgen live under the owning manifest unit.

pub mod feature;
pub mod generation_step;
pub mod heightmap;
pub mod placement;
// The `mc.world.level.levelgen.noise` unit's `WorldGenerationContext` is
// ported here (the minY/height window placement derives from the generator);
// only the Paper `level()` accessor defers (RivetTodo #232, see the module).
pub mod world_generation_context;
