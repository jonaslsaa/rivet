//! `net.minecraft.world.level.levelgen.blending` — the old-world blending
//! value slice (issue #177).
//!
//! The `mc.world.level.levelgen.blending` unit owns the three Java files
//! (`Blender`, `BlendingData`, `package-info`). This module ports the full
//! value surface:
//!
//! - [`blending_data`] — `BlendingData`, the per-old-chunk height/biome/density
//!   grid, plus the `Packed` record and its `CODEC` (round-trips the
//!   `blending_data` compound `serializable_chunk_data` carries; both agree on
//!   `CELL_COLUMN_COUNT == 16`). The chunk-reading half (`calculateData`,
//!   `getOrUpdateBlendingData`, …) defers as `RivetTodo(#177)`.
//! - [`blender`] — `Blender`, the non-empty weighted height/density blends
//!   backed by `BlendingData` (`blendOffsetAndFactor`/`blendDensity`), the
//!   empty singleton, and `BlendingOutput`. `of(WorldGenRegion)` and the
//!   chunk-border surfaces (`generateBorderTicks`,
//!   `addAroundOldChunksCarvingMaskFilter`, …) defer as `RivetTodo(#177)`.

pub mod blender;
pub mod blending_data;
