//! `net.minecraft.server.level` — the server world layer (issue #156, minimal).
//!
//! This module ports the M1 superflat world foundation: the deterministic
//! single-superflat-dimension `ServerLevel` owning a `ChunkMap` with exactly
//! the spawn chunk loaded, plus the `ChunkTrackingView` square the future
//! Moonrise direct-send (#100) and place-player (#101) use. Ownership follows
//! OWNERSHIP §Ownership tree: `Server` → `Vec<Level>`; the `ServerLevel` lives
//! on the tick thread and owns the `ChunkMap` by value (no `Arc<RwLock>`).
//!
//! The manifest split (#227) maps these classes to `rivet-server` units:
//! `ServerLevel`/`ServerPlayer` → `mc.server.level` residual, `ChunkMap` →
//! `mc.server.level.pipeline.chunkmap`, `ChunkTrackingView` →
//! `mc.server.level.pipeline.view`. The full pipeline units land on main later;
//! this slice is the smallest end-to-end world object #100/#101 build on.

pub mod chunk_map;
pub mod chunk_tracking_view;
pub mod entity_id_allocator;
pub mod level_chunk;
pub mod player_chunk_loader;
pub mod region_backed;
pub mod server_level;

pub use chunk_map::{ChunkMap, MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE};
pub use chunk_tracking_view::ChunkTrackingView;
pub use entity_id_allocator::EntityIdAllocator;
pub use level_chunk::{BiomeId, LevelChunk, StateId};
pub use player_chunk_loader::{
    PlayPacket, PlayerChunkLoader, encode_play_frame, get_client_view_distance,
    get_load_view_distance, get_send_view_distance, get_tick_distance,
};
pub use region_backed::{
    RegionBackedBootError, RegionChunkSource, RegionLevelPreparation, RegionWorldLayout,
};
pub use server_level::{ServerLevel, ServerLevelConfig, overworld_dimension};
