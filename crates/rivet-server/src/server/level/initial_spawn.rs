//! Native initial GLOBAL spawn search from MinecraftServer.setInitialSpawn.
//! This is not PlayerSpawnFinder.findSpawn's randomized player-respawn search:
//! the global search checks a supporting collision UP face, not a player AABB.
//! Plugin fixed-spawn, debug worlds and bonus-chest placement remain separate
//! outer bootstrap branches; this module implements the normal native search.

use super::{chunk_map::ChunkMap, generation_environment::GenerationEnvironment};
use rivet_registry::{
    ResourceKey,
    block_state::BlockState,
    core::{BlockPos, ChunkPos, Direction},
    registries::Level,
};
use rivet_world::{
    biome::climate::Sampler,
    chunk::chunk_generator::ChunkGenerator,
    level::{
        WorldGenLevel,
        block_shape_context::{BlockShapeContext, ShapeQuery, ShapeQueryError, position_shape},
        height_accessor::LevelHeightAccessor,
        storage::level_data::RespawnData,
    },
    levelgen::heightmap::Types,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InitialSpawnError {
    #[error("initial spawn requires actual FULL chunk {0:?}")]
    MissingFullChunk(ChunkPos),
    #[error("initial spawn chunk {0:?} does not match the actual dimension height")]
    DimensionMismatch(ChunkPos),
    #[error("initial spawn collision query at {position:?} is unavailable: {reason}")]
    Shape {
        position: BlockPos,
        reason: ShapeQueryError,
    },
    #[error("source initial-spawn loop wraps indefinitely at the integer coordinate boundary")]
    CoordinateWrap,
    #[error("actual generation environment is unavailable for initial spawn")]
    EnvironmentUnavailable,
}

/// The kernel's real world operations. getChunkHeight is ChunkAccess.getHeight
/// (highest occupied Y), NOT Level.getHeight's first-free Y. The concrete
/// loaded-chunk adapter below executes these operations on canonical chunks.
pub trait InitialSpawnTerrain: WorldGenLevel {
    fn require_full_chunk(&mut self, chunk: ChunkPos) -> Result<(), InitialSpawnError>;
    fn chunk_height(&mut self, kind: Types, x: i32, z: i32) -> Result<i32, InitialSpawnError>;
    fn has_ceiling(&self) -> bool;
    fn generator_spawn_height(&self) -> i32;
    fn store_global_spawn(&mut self, position: BlockPos);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialSpawnOutcome {
    pub climate_position: BlockPos,
    pub climate_chunk: ChunkPos,
    pub position: BlockPos,
    pub searched_chunks: usize,
    pub used_fallback: bool,
}

/// The native normal-world branch. Store the real climate-centered fallback
/// BEFORE requesting search chunks, exactly as source; a later unavailable
/// chunk/shape does not fabricate a successful fallback or roll back this write.
pub fn set_initial_spawn(
    world: &mut impl InitialSpawnTerrain,
    sampler: &Sampler,
) -> Result<InitialSpawnOutcome, InitialSpawnError> {
    let climate_position = sampler.find_spawn_position();
    let climate_chunk = ChunkPos::containing(&climate_position);
    let x = climate_chunk.get_min_block_x().wrapping_add(8);
    let z = climate_chunk.get_min_block_z().wrapping_add(8);
    let mut height = world.generator_spawn_height();
    if height < world.get_min_y() {
        height = world
            .chunk_height(Types::WorldSurface, x, z)?
            .wrapping_add(1);
    }
    let fallback = BlockPos::new(x, height, z);
    world.store_global_spawn(fallback);
    let mut outcome = InitialSpawnOutcome {
        climate_position,
        climate_chunk,
        position: fallback,
        searched_chunks: 0,
        used_fallback: true,
    };
    let (mut dx, mut dz, mut step_x, mut step_z) = (0_i32, 0_i32, 0_i32, -1_i32);
    for _ in 0..121 {
        if (-5..=5).contains(&dx) && (-5..=5).contains(&dz) {
            outcome.searched_chunks += 1;
            let chunk = ChunkPos::new(
                climate_chunk.x().wrapping_add(dx),
                climate_chunk.z().wrapping_add(dz),
            );
            if let Some(position) = get_spawn_pos_in_chunk(world, chunk)? {
                world.store_global_spawn(position);
                outcome.position = position;
                outcome.used_fallback = false;
                return Ok(outcome);
            }
        }
        if dx == dz || dx < 0 && dx == -dz || dx > 0 && dx == 1 - dz {
            (step_x, step_z) = (-step_z, step_x);
        }
        dx = dx.wrapping_add(step_x);
        dz = dz.wrapping_add(step_z);
    }
    Ok(outcome)
}

/// PlayerSpawnFinder.getSpawnPosInChunk: X outer, Z inner, first supporting
/// column wins. The pinned release's debugVoidTerrain flag is false.
pub fn get_spawn_pos_in_chunk(
    world: &mut impl InitialSpawnTerrain,
    chunk: ChunkPos,
) -> Result<Option<BlockPos>, InitialSpawnError> {
    for x in chunk.get_min_block_x()..=chunk.get_max_block_x() {
        for z in chunk.get_min_block_z()..=chunk.get_max_block_z() {
            if let Some(position) = get_level_respawn_pos(world, x, z)? {
                return Ok(Some(position));
            }
            if z == i32::MAX {
                return Err(InitialSpawnError::CoordinateWrap);
            }
        }
        if x == i32::MAX {
            return Err(InitialSpawnError::CoordinateWrap);
        }
    }
    Ok(None)
}

/// PlayerSpawnFinder.getLevelRespawnPos, including short-circuit heightmap
/// reads, fluid abort and collision (NOT support-sturdiness) face selection.
pub fn get_level_respawn_pos(
    world: &mut impl InitialSpawnTerrain,
    x: i32,
    z: i32,
) -> Result<Option<BlockPos>, InitialSpawnError> {
    world.require_full_chunk(ChunkPos::new(x >> 4, z >> 4))?;
    let top = if world.has_ceiling() {
        world.generator_spawn_height()
    } else {
        world.chunk_height(Types::MotionBlocking, x, z)?
    };
    if top < world.get_min_y() {
        return Ok(None);
    }
    let surface = world.chunk_height(Types::WorldSurface, x, z)?;
    if surface <= top && surface > world.chunk_height(Types::OceanFloor, x, z)? {
        return Ok(None);
    }
    let mut y = top.wrapping_add(1);
    while y >= world.get_min_y() {
        let position = BlockPos::new(x, y, z);
        let state = world.get_block_state(&position);
        if !state.fluid_empty() {
            break;
        }
        let shape = world
            .shape_query(&position, &state)
            .map_err(|reason| InitialSpawnError::Shape { position, reason })?;
        if shape.is_collision_face_full(Direction::Up) {
            return Ok(Some(position.above()));
        }
        if y == i32::MIN {
            return Err(InitialSpawnError::CoordinateWrap);
        }
        y -= 1;
    }
    Ok(None)
}

/// Concrete canonical LevelChunk adapter. Missing FULL chunks are explicit:
/// this never creates terrain, borrows a detached approximation, or substitutes
/// an origin spawn. The generated owner can provide another terrain adapter
/// once it can genuinely fulfill FULL requests.
pub struct LoadedSpawnTerrain<'a, 'context> {
    chunks: &'a mut ChunkMap,
    spawn: &'a mut RespawnData,
    dimension: ResourceKey<Level>,
    seed: i64,
    min_y: i32,
    height: i32,
    ceiling: bool,
    generator: &'a dyn ChunkGenerator,
    shape_context: Option<&'a mut (dyn BlockShapeContext + Send + 'context)>,
}
impl<'a, 'context> LoadedSpawnTerrain<'a, 'context> {
    pub fn new(
        chunks: &'a mut ChunkMap,
        spawn: &'a mut RespawnData,
        dimension: ResourceKey<Level>,
        seed: i64,
        environment: &GenerationEnvironment,
        generator: &'a dyn ChunkGenerator,
        shape_context: Option<&'a mut (dyn BlockShapeContext + Send + 'context)>,
    ) -> Self {
        let (min_y, height) = environment.height_range();
        Self {
            chunks,
            spawn,
            dimension,
            seed,
            min_y,
            height,
            ceiling: environment.has_ceiling(),
            generator,
            shape_context,
        }
    }
}
impl LevelHeightAccessor for LoadedSpawnTerrain<'_, '_> {
    fn get_min_y(&self) -> i32 {
        self.min_y
    }
    fn get_height(&self) -> i32 {
        self.height
    }
}
impl WorldGenLevel for LoadedSpawnTerrain<'_, '_> {
    fn get_seed(&self) -> i64 {
        self.seed
    }
    fn get_block_state(&self, position: &BlockPos) -> BlockState {
        if position.get_y() < self.min_y || position.get_y() >= self.min_y + self.height {
            return BlockState::of(
                rivet_registry::generated::blocks::BlockId::from_name("minecraft:void_air")
                    .unwrap(),
            );
        }
        let chunk = self
            .chunks
            .get_chunk(ChunkPos::containing(position))
            .expect("initial spawn requires the actual column's FULL chunk first");
        BlockState::new(chunk.get_block_state(position.get_x(), position.get_y(), position.get_z()))
    }
    fn shape_context(&self) -> Option<&dyn BlockShapeContext> {
        self.shape_context
            .as_ref()
            .map(|context| &**context as &dyn BlockShapeContext)
    }
    fn shape_query(
        &self,
        position: &BlockPos,
        state: &BlockState,
    ) -> Result<ShapeQuery, ShapeQueryError> {
        if !state.has_dynamic_shape() {
            return Ok(ShapeQuery::from_static_state(*state));
        }
        if let Some(shape) = position_shape(state, position) {
            return Ok(shape);
        }
        self.shape_context()
            .ok_or(ShapeQueryError::DynamicShapeContextUnavailable)?
            .shape_query(state, position)
    }
}
impl InitialSpawnTerrain for LoadedSpawnTerrain<'_, '_> {
    fn require_full_chunk(&mut self, pos: ChunkPos) -> Result<(), InitialSpawnError> {
        let chunk = self
            .chunks
            .get_chunk(pos)
            .ok_or(InitialSpawnError::MissingFullChunk(pos))?;
        if chunk.get_min_y() != self.min_y || chunk.get_height() != self.height {
            return Err(InitialSpawnError::DimensionMismatch(pos));
        }
        Ok(())
    }
    fn chunk_height(&mut self, kind: Types, x: i32, z: i32) -> Result<i32, InitialSpawnError> {
        let pos = ChunkPos::new(x >> 4, z >> 4);
        self.require_full_chunk(pos)?;
        Ok(self
            .chunks
            .get_chunk_mut(pos)
            .unwrap()
            .get_height_at(kind, x & 15, z & 15))
    }
    fn has_ceiling(&self) -> bool {
        self.ceiling
    }
    fn generator_spawn_height(&self) -> i32 {
        self.generator.get_spawn_height(self)
    }
    fn store_global_spawn(&mut self, position: BlockPos) {
        *self.spawn = RespawnData::of(self.dimension.clone(), position, 0.0, 0.0);
    }
}

#[cfg(test)]
#[path = "initial_spawn_fixture.rs"]
mod fixture;
