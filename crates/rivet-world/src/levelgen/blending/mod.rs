//! `net.minecraft.world.level.levelgen.blending` — the old-world blending
//! value slice (issue #177).
//!
//! The `mc.world.level.levelgen.blending` unit owns the three Java files
//! (`Blender`, `BlendingData`, `package-info`). This slice ports only the
//! independently compilable empty-`Blender` value prerequisite that noisegen
//! shares (see `blender`):
//!
//! - [`blender::BlendingOutput`] — the `blendOffsetAndFactor` result record.
//! - [`blender::Blender`] — the empty singleton (`empty()`/`isEmpty()`), the
//!   identity `blendDensity`, and the empty `blendOffsetAndFactor` constant
//!   `(1.0, 0.0)`.
//!
//! `BlendingData` (the per-chunk height/biome/density grid) is NOT ported in
//! this slice: `Blender.of(WorldGenRegion)` and the non-empty weighted
//! height/density blends that read it defer as `RivetTodo(#177)` owned by the
//! blending unit.

pub mod blender;
