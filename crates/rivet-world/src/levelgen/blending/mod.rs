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
//!   `getOrUpdateBlendingData`, …) defers (RivetTodo #177, see `blending_data`).
//! - [`blender`] — `Blender`, the non-empty weighted height/density blends
//!   backed by `BlendingData` (`blendOffsetAndFactor`/`blendDensity`), the
//!   empty singleton, and `BlendingOutput`. `of(WorldGenRegion)` and the
//!   chunk-border surfaces (`generateBorderTicks`,
//!   `addAroundOldChunksCarvingMaskFilter`, …) defer (RivetTodo #177, see
//!   `blender`).
//!
//! RivetTodo(#177): the chunk/region-reading half of `BlendingData`
//! (`getOrUpdateBlendingData`/`sideByGenerationAge`/`calculateData` and the
//! per-column readers) needs the `ChunkAccess`/`WorldGenRegion` surfaces; see
//! `blending_data`'s module doc.
//! RivetTodo(#177): `Blender.of(WorldGenRegion)` and the chunk-border surfaces
//! (`generateBorderTicks`/`addAroundOldChunksCarvingMaskFilter`/the distance
//! getters) need `WorldGenRegion`/`ProtoChunk`/`WorldGenLevel`; see `blender`'s
//! module doc.

pub mod blender;
pub mod blending_data;
