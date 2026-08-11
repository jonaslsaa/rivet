//! Port of `net.minecraft.server.level.ChunkMap` (MC 26.2, Paper) — the minimal
//! M1 slice (issue #156).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! server/level/ChunkMap.java` (1438 lines).
//!
//! Owned by the `mc.server.level.pipeline.chunkmap` manifest unit (#185). This
//! slice ports only what issue #100/#156 need: the chunk storage (`HashMap` of
//! `ChunkPos → LevelChunk`, OWNERSHIP §Chunks & blocks "chunks owned by
//! ChunkMap by value"), the view-distance field (`serverViewDistance`, the
//! Paper `setServerViewDistance` clamp `Mth.clamp(view, 2,
//! MoonriseConstants.MAX_VIEW_DISTANCE)` = 32 by default) and the deterministic
//! loaded-chunk access. The Moonrise scheduler, the
//! ticket/level graph, the light engine, the `DistanceManager`, entity/POI
//! maps, and region IO are all deferred with the owning pipeline units.
//!
//! `MIN_VIEW_DISTANCE`/`MAX_VIEW_DISTANCE` are `ChunkMap` constants in Java;
//! `serverViewDistance` is the value the `server.properties` fixture pins to 4
//! (`view-distance=4`).
//!
//! RivetTodo(#185): the pipeline hub (DistanceManager, ticket storage, light
//! engine, TrackedEntity, worldgen context, region read/write) and the
//! `getChunk`/`getChunks` async scheduler surface are deferred to the owning
//! chunkmap unit. The M1 world holds exactly one loaded chunk — the superflat
//! spawn chunk — so `chunks` starts populated with it and never grows here.

use std::collections::HashMap;

use rivet_registry::core::ChunkPos;
use rivet_util::mth;

use super::level_chunk::LevelChunk;

/// `ChunkMap.MIN_VIEW_DISTANCE`.
pub const MIN_VIEW_DISTANCE: i32 = 2;
/// The clamp upper bound for `setServerViewDistance` — `MoonriseConstants.MAX_VIEW_DISTANCE`
/// (`Integer.getInteger(brand + ".MaxViewDistance", 32)`, default 32). Paper's
/// `ChunkMap.MAX_VIEW_DISTANCE` is also 32; the M1 hardcodes the default because
/// Rivet has no system-property override yet.
pub const MAX_VIEW_DISTANCE: i32 = 32;

/// `net.minecraft.server.level.ChunkMap` — the world's loaded-chunk storage.
pub struct ChunkMap {
    /// `ChunkMap.chunks` — the loaded chunks by `ChunkPos`, owned by value.
    chunks: HashMap<ChunkPos, LevelChunk>,
    /// `ChunkMap.serverViewDistance` — the clamped `view-distance`.
    server_view_distance: i32,
}

impl ChunkMap {
    /// `new ChunkMap(...)` — a world with one loaded superflat spawn chunk at
    /// `spawn_chunk` and the given `serverViewDistance`. The M1 world loads
    /// exactly the spawn chunk; `chunks` is seeded with it so the direct-send
    /// path (#100) can resolve it deterministically.
    pub fn new(spawn_chunk: ChunkPos, server_view_distance: i32) -> Self {
        let mut chunks = HashMap::with_capacity(1);
        chunks.insert(spawn_chunk, LevelChunk::new(spawn_chunk));
        ChunkMap {
            chunks,
            server_view_distance: Self::set_server_view_distance(server_view_distance),
        }
    }

    /// A chunk map with no seeded placeholder chunk and the given (clamped)
    /// server view distance. The #516 region-backed boot reconstructs every
    /// chunk of the view square from the read-only region and installs it
    /// explicitly — an empty map guarantees `RequireLoaded` fails on any
    /// position the boot did not install, instead of silently serving a
    /// superflat placeholder.
    pub fn empty(server_view_distance: i32) -> Self {
        ChunkMap {
            chunks: HashMap::new(),
            server_view_distance: Self::set_server_view_distance(server_view_distance),
        }
    }

    /// `ChunkMap.setServerViewDistance(int)` — `Mth.clamp(newViewDistance, 2,
    /// MoonriseConstants.MAX_VIEW_DISTANCE)` (default 32).
    pub fn set_server_view_distance(view_distance: i32) -> i32 {
        mth::clamp(view_distance, MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE)
    }

    /// `ChunkMap.getServerViewDistance()`.
    pub fn server_view_distance(&self) -> i32 {
        self.server_view_distance
    }

    /// `ChunkMap.getChunk(ChunkPos)` — the loaded chunk at `pos`, `None` when
    /// not loaded. The M1 world resolves the spawn chunk; a missing chunk means
    /// "not loaded" (no generation yet — issue #185).
    pub fn get_chunk(&self, pos: ChunkPos) -> Option<&LevelChunk> {
        self.chunks.get(&pos)
    }

    /// Install an owned reconstructed chunk at `pos`, replacing any previously
    /// loaded chunk there. The #516 region-backed boot composes the read-only
    /// world into an empty map by installing every chunk of the view square
    /// the caller already validated (tick-thread-owned by value, never
    /// `Arc<RwLock>`).
    pub fn install(&mut self, pos: ChunkPos, chunk: LevelChunk) {
        self.chunks.insert(pos, chunk);
    }

    /// The number of loaded chunks. Deterministic: 1 for the M1 superflat
    /// world.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether the map holds no chunks.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_chunk_is_loaded() {
        let map = ChunkMap::new(ChunkPos::ZERO, 4);
        assert_eq!(map.len(), 1);
        let chunk = map.get_chunk(ChunkPos::ZERO).expect("spawn chunk loaded");
        assert_eq!(chunk.pos(), ChunkPos::ZERO);
        assert_eq!(chunk.get_x(), 0);
        assert_eq!(chunk.get_z(), 0);
    }

    #[test]
    fn unloaded_chunk_is_none() {
        let map = ChunkMap::new(ChunkPos::ZERO, 4);
        assert!(map.get_chunk(ChunkPos::new(1, 0)).is_none());
        assert!(map.get_chunk(ChunkPos::new(0, 1)).is_none());
        assert!(map.get_chunk(ChunkPos::new(-1, -1)).is_none());
    }

    #[test]
    fn view_distance_clamps_to_chunk_map_bounds() {
        // `setServerViewDistance` clamps to [2, 32].
        assert_eq!(ChunkMap::set_server_view_distance(4), 4);
        assert_eq!(ChunkMap::set_server_view_distance(1), MIN_VIEW_DISTANCE);
        assert_eq!(ChunkMap::set_server_view_distance(100), MAX_VIEW_DISTANCE);
        assert_eq!(ChunkMap::set_server_view_distance(-5), MIN_VIEW_DISTANCE);
    }

    #[test]
    fn spawn_chunk_content_is_the_deterministic_superflat() {
        let map = ChunkMap::new(ChunkPos::ZERO, 4);
        let chunk = map.get_chunk(ChunkPos::ZERO).unwrap();
        // The 24-section single-stone content: section 0 (Y=-4) holds the stone
        // layer (superflat minY -64, height 384).
        assert_eq!(chunk.get_sections().len(), 24);
        // The three `Usage.CLIENT` heightmaps (WORLD_SURFACE, MOTION_BLOCKING,
        // MOTION_BLOCKING_NO_LEAVES) in enum id order — issue #156's DoD
        // heightmap set. All stored offsets 1 (stone at y=-64).
        use rivet_protocol::protocol::game::heightmap_types::HeightmapType;
        let types: Vec<HeightmapType> = chunk
            .client_heightmaps()
            .iter()
            .map(|(ty, _)| *ty)
            .collect();
        assert_eq!(
            types,
            vec![
                HeightmapType::WorldSurface,
                HeightmapType::MotionBlocking,
                HeightmapType::MotionBlockingNoLeaves,
            ]
        );
        for (_, raw) in chunk.client_heightmaps() {
            assert_eq!(raw.len(), 37, "9-bit heightmap storage longs");
        }
    }

    #[test]
    fn superflat_content_is_byte_deterministic() {
        // Two independently built worlds produce identical content bytes.
        let a = ChunkMap::new(ChunkPos::ZERO, 4);
        let b = ChunkMap::new(ChunkPos::ZERO, 4);
        let ca = a.get_chunk(ChunkPos::ZERO).unwrap();
        let cb = b.get_chunk(ChunkPos::ZERO).unwrap();
        assert_eq!(ca.sections_buffer(), cb.sections_buffer());
        assert_eq!(
            ca.chunk_packet_data().buffer(),
            cb.chunk_packet_data().buffer()
        );
    }
}
