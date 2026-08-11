//! `net.minecraft.world.level.levelgen` — worldgen module. Only the
//! client-heightmap slice (`Heightmap`, issue #100), the
//! `GenerationStep.Decoration` enum (proactively ported from the pending
//! `mc.world.level.levelgen.settings` unit — see `generation_step.rs`), the
//! `feature` core slice (the `mc.world.level.levelgen.feature.core` unit —
//! `feature_place`'s `#181` codegen dispatch stays a STUB) and its
//! `configurations` slice (the
//! `mc.world.level.levelgen.feature.configurations.core` unit), the `placement`
//! core slice (the
//! `mc.world.level.levelgen.placement.core` unit), the `carver` type shell
//! (the `mc.world.level.levelgen.carver` unit's `ConfiguredWorldCarver`
//! record/identity skeleton — the `#180` algorithm stays a STUB), the
//! `blockpredicates` slice (issue #399 — the block-predicate value/codec
//! framework), the `synth` primitive-noise classes (the
//! `mc.world.level.levelgen.synth` unit — issue #177), and the
//! `WorldGenerationContext` window are ported so far; the generators/feature
//! worldgen live under the owning manifest unit.

pub mod blockpredicates;
pub mod carver;
pub mod feature;
pub mod generation_step;
pub mod heightmap;
pub mod placement;
// The `mc.world.level.levelgen.noise` unit's `WorldGenerationContext` is
// ported here (the minY/height window placement derives from the generator);
// only the Paper `level()` accessor defers (RivetTodo #232, see the module).
pub mod world_generation_context;
// The `mc.world.level.levelgen.synth` unit's seven primitive-noise classes
// (issue #177). `DensityFunction`/registry dispatch seams defer as
// `RivetTodo(#177)`; see `synth::mod`.
pub mod synth;
