//! `net.minecraft.world.level.levelgen` — worldgen module. Only the
//! client-heightmap slice (`Heightmap`, issue #100), the
//! `GenerationStep.Decoration` enum (proactively ported from the pending
//! `mc.world.level.levelgen.settings` unit — see `generation_step.rs`), the
//! `feature` core slice (the `mc.world.level.levelgen.feature.core` unit —
//! `feature_place`'s `#181` codegen dispatch stays a STUB) and its
//! `configurations` slice (the
//! `mc.world.level.levelgen.feature.configurations.core` unit), the `placement`
//! core slice (the
//! `mc.world.level.levelgen.placement.core` unit), the full `carver` unit (the
//! `mc.world.level.levelgen.carver` — `CarverConfiguration` + base codec,
//! `CarverDebugSettings`, `CarvingContext`, the concrete carvers, and
//! `ConfiguredWorldCarver` with `carve`/`isStartChunk`; the `#399` `CarveChunk`
//! block-surface trait and the `#126` dispatch codecs stay unbound), the
//! `blockpredicates` slice (issue #399 — the block-predicate value/codec
//! framework), the `synth` primitive-noise classes (the
//! `mc.world.level.levelgen.synth` unit — issue #177), the
//! `WorldGenerationContext` window, and the `PositionalRandomFactory`
//! BlockPos/Identifier default overloads (`random`, issue #208) are ported so
//! far; the generators/feature worldgen live under the owning manifest unit.

pub mod blockpredicates;
// The `mc.world.level.levelgen.blending` unit's shared Blender value
// prerequisite (issue #177): the empty singleton (`empty()`/`isEmpty()`, the
// identity `blendDensity`, the `(1.0, 0.0)` empty
// `blendOffsetAndFactor`/`BlendingOutput`, and the generic identity
// `getBiomeResolver` override) — the non-empty `of`/`BlendingData` surface
// defers (RivetTodo #177, see `blending::blender`).
pub mod blending;
pub mod carver;
pub mod feature;
pub mod generation_step;
pub mod heightmap;
// The `mc.world.level.levelgen.heightproviders` unit (issue #181 leaf): the
// `HeightProvider` value/codec layer, unblocked by the merged VerticalAnchor
// #388 and weighted-random #353.
pub mod heightproviders;
// The `mc.world.level.levelgen.noise` unit's density-function/noise-router
// value slice (issue #177).
pub mod noise;
// The `mc.world.level.levelgen.noisegen` unit (issue #183): the
// noise-based-chunk-generator class-level SCC (`NoiseGeneratorSettings`,
// `NoiseRouterData`, `RandomState`, `Aquifer`, `NoiseChunk`,
// `NoiseBasedChunkGenerator`, `OreVeinifier`).
pub mod noisegen;
pub mod placement;
// The `mc.world.level.levelgen.surface` unit's `SurfaceRules` value shell:
// the `RuleSource`/`SurfaceRule`/`Context` type identities + the erased
// `ArcRuleSource` carrier `NoiseGeneratorSettings` stores, the
// `SurfaceRuleData` builder stand-ins, and the `SurfaceSystem` type identity
// (RivetTodo to the surface unit — see the module).
pub mod surface_rules;
// The `mc.world.level.levelgen.structure.templatesystem.rules` unit (issue
// #182) — the `RuleTest`/`PosRuleTest` template-system rule tests.
pub mod structure;
// The `mc.world.level.levelgen.random` unit's registry-aware overloads (issue
// #208) live here because `BlockPos`/`Identifier` come from `rivet-registry`,
// which `rivet-util` cannot depend on without a Cargo cycle. The registry-free
// base `PositionalRandomFactory` trait stays in `rivet-util::random`.
pub mod random;
// The `mc.world.level.levelgen.noise` unit's `VerticalAnchor` is ported here
// (issue #388 leaf: the value/codec layer unblocking height providers); the
// noise wave must not re-port it.
pub mod vertical_anchor;
// The `mc.world.level.levelgen.noise` unit's `WorldGenerationContext` is
// ported here (the minY/height window placement derives from the generator);
// only the Paper `level()` accessor defers (RivetTodo #232, see the module).
pub mod world_generation_context;
// The `mc.world.level.levelgen.synth` unit's seven primitive-noise classes
// (issue #177). `DensityFunction`/registry dispatch seams defer as
// `RivetTodo(#177)`; see `synth::mod`.
pub mod synth;
