//! Port of `net.minecraft.server.level.ServerLevel` (MC 26.2, Paper) — the
//! minimal M1 slice (issue #156).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! server/level/ServerLevel.java` (2997 lines).
//!
//! Owned by the `mc.server.level` residual manifest unit (#227). This slice
//! ports only the M1 world object behind the single superflat spawn chunk
//! (issue #156): the dimension key, the respawn data (`LevelData.RespawnData`),
//! the height access (minY/height), the sea level, and the `ChunkMap` owner.
//! The tick method, entity storage, weather/time, game rules, and the full
//! `Level` base surface are deferred with the owning units.
//!
//! Per OWNERSHIP §Ownership tree the world is tick-thread-owned (the "one
//! owner: the tick thread" rule): the `ServerLevel` lives on the tick thread
//! and is never `Arc<RwLock>`-shared. Cross-thread consumers (network, worker
//! pools) resolve chunk/level data through tick-thread confinement or the
//! immutable config snapshot.
//!
//! `LevelData.RespawnData` is the Java record `(GlobalPos, yaw, pitch)`, ported
//! once in `rivet_world::level` (issue #232) and imported here; the superflat
//! spawn is `BlockPos(0, -63, 0)` — `FlatLevelSource.getSpawnHeight` returns
//! `minY + min(height, layers.size()) = -64 + min(384, 1) = -63`, and the spawn
//! chunk is the chunk containing it, `(0,0)`.
//!
//! RivetTodo(#227): the `Level` base surface (tick, `getBlockState`,
//! entities, time, weather, game rules), the `ServerLevel.tick` phases, and the
//! `ServerLevelData` storage half are deferred with the owning residual unit.
//! The `dimensionType()` holder is not wired yet (`Holder<DimensionType>` needs
//! a runtime `RegistryAccess`; issue #126).

use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_registry::registries::{self, Level};
use rivet_world::level::RespawnData;
use rivet_world::superflat::{SUPERFLAT_HEIGHT, SUPERFLAT_MIN_Y};

use super::chunk_map::ChunkMap;
use super::chunk_tracking_view::ChunkTrackingView;

/// How the player send path handles a position absent from `ChunkMap`.
/// Repeating spawn content is confined to the legacy no-level fixture;
/// region-backed worlds require an actually loaded coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingChunkPolicy {
    RepeatSpawnFixture,
    RequireLoaded,
}

/// `Level.OVERWORLD` — `ResourceKey.create(Registries.DIMENSION,
/// Identifier.withDefaultNamespace("overworld"))`.
pub fn overworld_dimension() -> ResourceKey<Level> {
    ResourceKey::create(
        &*registries::DIMENSION,
        Identifier::with_default_namespace("overworld"),
    )
}

/// The config that builds the M1 superflat world (issue #156). Immutable after
/// construction — the world's constants and spawn geometry are fixed so the
/// M1 `chunk_count` and spawn chunk are deterministic across boots.
#[derive(Clone, Debug)]
pub struct ServerLevelConfig {
    /// The dimension key (`Level.OVERWORLD` for the M1 world).
    pub dimension: ResourceKey<Level>,
    /// The world seed (the `level-seed=42` oracle fixture).
    pub seed: i64,
    /// The superflat world's min Y (`min_y=-64`).
    pub min_y: i32,
    /// The superflat world's height (`height=384`).
    pub height: i32,
    /// The superflat sea level (`FlatLevelSource.getSeaLevel()` returns -63).
    pub sea_level: i32,
    /// The spawn chunk (`(0,0)`, the chunk containing the `(0, -63, 0)` spawn).
    pub spawn_chunk: ChunkPos,
    /// The respawn data (spawn position + angles).
    pub respawn_data: RespawnData,
    /// The view distance (the `view-distance=4` fixture).
    pub view_distance: i32,
    /// The simulation distance (the `simulation-distance=4` fixture) — the
    /// tick-thread driver of `ClientboundSetSimulationDistancePacket` and the
    /// Moonrise `tickViewDistance` (issue #100).
    pub simulation_distance: i32,
    /// Policy for absent view chunks. The legacy no-level fixture repeats
    /// spawn content; region-backed composition requires loaded coordinates.
    pub missing_chunk_policy: MissingChunkPolicy,
}

impl Default for ServerLevelConfig {
    fn default() -> Self {
        let dimension = overworld_dimension();
        let spawn_pos = BlockPos::new(0, SUPERFLAT_MIN_Y + 1, 0); // y = -63
        ServerLevelConfig {
            dimension: dimension.clone(),
            seed: 42,
            min_y: SUPERFLAT_MIN_Y,
            height: SUPERFLAT_HEIGHT,
            sea_level: -63,
            spawn_chunk: ChunkPos::containing(&spawn_pos),
            respawn_data: RespawnData::of(dimension.clone(), spawn_pos, 0.0, 0.0),
            view_distance: 4,
            simulation_distance: 4,
            missing_chunk_policy: MissingChunkPolicy::RepeatSpawnFixture,
        }
    }
}

/// `net.minecraft.server.level.ServerLevel` — the world object behind the one
/// superflat spawn chunk.
///
/// `PhantomData<Cell<()>>` (in `_confinement`) makes the world `Send` but not
/// `Sync`: it can be moved onto the tick thread, but a shared `&ServerLevel`
/// cannot cross threads — the OWNERSHIP "one owner: the tick thread" rule,
/// encoded at the type level. `ChunkMap` is owned by value (no `Arc<RwLock>`).
pub struct ServerLevel {
    dimension: ResourceKey<Level>,
    seed: i64,
    min_y: i32,
    height: i32,
    sea_level: i32,
    respawn_data: RespawnData,
    chunk_map: ChunkMap,
    /// The world's view — the view-distance-4 square centered on the spawn
    /// chunk (the 117-chunk `Event::ReceiveChunk` shape, issue #100).
    view: ChunkTrackingView,
    /// The simulation distance (the Moonrise world `tickViewDistance` driver;
    /// the M1 world pins it to the `simulation-distance=4` fixture).
    simulation_distance: i32,
    missing_chunk_policy: MissingChunkPolicy,
    /// Tick-thread confinement marker (OWNERSHIP §Ownership tree): `Cell` is
    /// `Send + !Sync`, so a `&ServerLevel` is rejected at compile time when it
    /// would cross threads.
    _confinement: std::marker::PhantomData<std::cell::Cell<()>>,
}

impl ServerLevel {
    /// `new ServerLevel(...)` — a deterministic single-superflat-dimension
    /// world owning a `ChunkMap` with the spawn chunk loaded.
    pub fn new(config: ServerLevelConfig) -> Self {
        let spawn_chunk = config.spawn_chunk;
        let chunk_map = ChunkMap::new(spawn_chunk, config.view_distance);
        // The view radius comes from the *clamped* server view distance so the
        // send square never exceeds it (Java invariant: the send radius is
        // bounded by `serverViewDistance`; `setLoadDistance(serverViewDistance + 1)`
        // derives from the same clamped value).
        let view = ChunkTrackingView::of(spawn_chunk, chunk_map.server_view_distance());
        ServerLevel {
            dimension: config.dimension,
            seed: config.seed,
            min_y: config.min_y,
            height: config.height,
            sea_level: config.sea_level,
            respawn_data: config.respawn_data,
            chunk_map,
            view,
            simulation_distance: config.simulation_distance,
            missing_chunk_policy: config.missing_chunk_policy,
            _confinement: std::marker::PhantomData,
        }
    }

    /// `ServerLevel.dimension()`.
    pub fn dimension(&self) -> &ResourceKey<Level> {
        &self.dimension
    }

    /// `ServerLevel.getSeed()`.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// `LevelHeightAccessor.getMinY()`.
    pub fn get_min_y(&self) -> i32 {
        self.min_y
    }

    /// `LevelHeightAccessor.getHeight()`.
    pub fn get_height(&self) -> i32 {
        self.height
    }

    /// `LevelHeightAccessor.getMaxY()` — `getMinY() + getHeight() - 1`.
    pub fn get_max_y(&self) -> i32 {
        self.min_y + self.height - 1
    }

    /// `ServerLevel.getSeaLevel()` — `chunkSource.getGenerator().getSeaLevel()`,
    /// which for `FlatLevelSource` returns the literal `-63` (the superflat sea
    /// level).
    pub fn get_sea_level(&self) -> i32 {
        self.sea_level
    }

    /// `ServerLevel.getRespawnData()`.
    pub fn get_respawn_data(&self) -> &RespawnData {
        &self.respawn_data
    }

    /// `ServerLevel.getChunkSource().getChunk` — the `ChunkMap`.
    pub fn chunk_map(&self) -> &ChunkMap {
        &self.chunk_map
    }

    /// `ServerLevel.getChunkMap()` — the mutable owner (tick-thread).
    pub fn chunk_map_mut(&mut self) -> &mut ChunkMap {
        &mut self.chunk_map
    }

    /// The world's view-distance square (the M1 117-chunk shape).
    pub fn view(&self) -> &ChunkTrackingView {
        &self.view
    }

    /// `Level.getSimulationDistance()` — the world's simulation distance (the
    /// Moonrise world `tickViewDistance` driver; the M1 world pins 4). Java's
    /// `DistanceManager.updateSimulationDistance` clamps to
    /// `[0, MoonriseConstants.MAX_VIEW_DISTANCE]` before `setTickDistance`; the
    /// M1 fixture value 4 is in range, so the clamp is deferred to the config
    /// wiring.
    pub fn get_simulation_distance(&self) -> i32 {
        self.simulation_distance
    }

    /// `ChunkMap.setServerViewDistance` → Moonrise `setLoadDistance(view+1)` —
    /// the world's Moonrise load view distance (api view distance + 1).
    pub fn load_view_distance(&self) -> i32 {
        self.view.view_distance() + 1
    }

    /// The world's Moonrise `sendViewDistance` holder value. Paper leaves it
    /// `-1` (unset) until a `World#setSendViewDistance` Bukkit call (the
    /// `ViewDistances` record default `(-1, -1, -1)`); each player's send
    /// distance is auto-configured per-player (`auto-config-send-distance`), so
    /// the world value stays `-1` on the M1 boot path.
    ///
    /// RivetTodo(#236): the Bukkit `World#setSendViewDistance` world-config
    /// wiring that sets this holder; until then it is hardcoded to the Paper
    /// unset default.
    pub fn send_view_distance(&self) -> i32 {
        -1
    }

    /// The explicit absent-chunk policy consumed by the player send path.
    pub fn missing_chunk_policy(&self) -> MissingChunkPolicy {
        self.missing_chunk_policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overworld() -> ServerLevel {
        ServerLevel::new(ServerLevelConfig::default())
    }

    #[test]
    fn overworld_geometry_is_superflat() {
        let world = overworld();
        assert_eq!(world.get_min_y(), -64);
        assert_eq!(world.get_height(), 384);
        assert_eq!(world.get_max_y(), 319);
        assert_eq!(world.get_sea_level(), -63);
    }

    #[test]
    fn overworld_dimension_key() {
        let world = overworld();
        assert_eq!(world.dimension(), &overworld_dimension());
        // The spawn chunk is the chunk containing the spawn pos (0,-63,0) = (0,0).
        assert_eq!(
            world.get_respawn_data().global_pos().dimension(),
            &overworld_dimension()
        );
        let spawn = world.get_respawn_data().pos();
        assert_eq!(spawn, BlockPos::new(0, -63, 0));
    }

    #[test]
    fn spawn_chunk_is_zero_and_loaded() {
        let world = overworld();
        let chunk = world
            .chunk_map()
            .get_chunk(ChunkPos::ZERO)
            .expect("spawn chunk loaded");
        assert_eq!(chunk.pos(), ChunkPos::ZERO);
        assert_eq!(world.chunk_map().len(), 1);
    }

    #[test]
    fn respawn_data_of_clamps_pitch_and_wraps_yaw() {
        // `LevelData.RespawnData.of` normalizes: yaw wraps, pitch clamps.
        let pos = BlockPos::new(1, 2, 3);
        let r = RespawnData::of(overworld_dimension(), pos, 370.0, 100.0);
        assert_eq!(r.yaw(), 10.0); // wrapDegrees(370) = 10
        assert_eq!(r.pitch(), 90.0); // clamp(100, -90, 90)
        assert_eq!(r.pos(), pos);
    }

    #[test]
    fn moonrise_view_distances_from_the_m1_fixture() {
        // `view-distance=4` → the Moonrise world distances the chunk loader
        // derives from: load = 5 (api view + 1), send = -1 (the unset world
        // holder — Paper auto-configures each player), simulation = 4.
        let world = overworld();
        assert_eq!(world.get_simulation_distance(), 4);
        assert_eq!(world.load_view_distance(), 5);
        assert_eq!(world.send_view_distance(), -1);
    }

    #[test]
    fn view_distance_4_shape_has_117_chunks() {
        let world = overworld();
        let mut count = 0;
        world.view().for_each(|_| count += 1);
        assert_eq!(count, 117);
        // Center is the spawn chunk (0,0).
        assert_eq!(world.view().center(), ChunkPos::ZERO);
        assert_eq!(world.view().view_distance(), 4);
    }

    #[test]
    fn world_owns_chunk_map_by_value_no_arc_rwlock() {
        // OWNERSHIP §Chunks & blocks: `ServerLevel` owns its `ChunkMap` by
        // value — there is no `Arc`/`RwLock` in the world. The spawn chunk is
        // reached through the owned map, not a shared handle.
        let mut world = overworld();
        assert_eq!(world.chunk_map().len(), 1);
        assert_eq!(world.chunk_map().server_view_distance(), 4);
        // `chunk_map_mut` is the tick-thread mutator (`&mut self`).
        let chunk_map = world.chunk_map_mut();
        assert_eq!(chunk_map.get_chunk(ChunkPos::ZERO).unwrap().get_x(), 0);
    }

    #[test]
    fn world_is_send_but_not_sync_tick_thread_confinement() {
        // OWNERSHIP "one owner: the tick thread": the world may move onto the
        // tick thread (Send) but a shared `&ServerLevel` must never cross
        // threads (!Sync), so `ServerLevel` can never be placed behind
        // `Arc<RwLock>`-style shared state. Both assertions live in `mod tests`
        // (`#[cfg(test)]`), so they are enforced in test builds (the merge gate
        // runs tests) rather than in `cargo build`.
        fn assert_send<T: Send>() {}
        assert_send::<ServerLevel>();
        // The !Sync assertion below is a compile-time check (ambiguity trick
        // from `static_assertions::assert_not_impl_any`): it fails to compile if
        // `ServerLevel` ever becomes Sync.
        const _: fn() = || {
            trait AmbiguousIfImpl<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            struct Invalid;
            impl<T: ?Sized + Sync> AmbiguousIfImpl<Invalid> for T {}
            // Resolves only if `ServerLevel` does NOT implement Sync.
            let _ = <ServerLevel as AmbiguousIfImpl<_>>::some_item;
        };
    }
}
