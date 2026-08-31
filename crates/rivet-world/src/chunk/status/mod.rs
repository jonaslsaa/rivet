//! The `mc.world.level.chunk.status` value layer (issue #185, A1 slice): the
//! 12-rung `ChunkStatus` ladder (`chunk_status`), the `ChunkDependencies` /
//! `ChunkStep` / `ChunkPyramid` dependency-DAG values (with the access-radius
//! tables), the `ChunkStatusTask` identities, the pass-through task bodies, and
//! the `WorldGenContext` executor seam that runs the DAG through LIGHT.
//!
//! The generation/loading DAGs are built as deterministic pure functions of the
//! builder calls (`ChunkPyramid::GENERATION_PYRAMID` / `LOADING_PYRAMID`), and
//! the access-radius tables are the `getAccessRadius0` recursion ported from
//! the deferred scheduler (`chunk_pyramid::access_radius`). The executor seam
//! enforces the `BIOMES`-before-`NOISE` ordering (§3.2 of
//! `docs/chunk-pipeline-spec.md`): a chunk is only labeled `NOISE` after the
//! `BIOMES` task ran (see `world_gen_context`).

pub mod chunk_dependencies;
pub mod chunk_pyramid;
pub mod chunk_status;
pub mod chunk_status_task;
pub mod chunk_status_tasks;
pub mod chunk_step;
pub mod world_gen_context;

pub use chunk_dependencies::ChunkDependencies;
pub use chunk_pyramid::{ChunkPyramid, GENERATION_PYRAMID, LOADING_PYRAMID};
pub use chunk_status::{ChunkStatus, ChunkType};
pub use chunk_status_task::ChunkStatusTask;
pub use chunk_step::ChunkStep;
pub use world_gen_context::{GenError, GeneratedLightTask, WorldGenContext};
