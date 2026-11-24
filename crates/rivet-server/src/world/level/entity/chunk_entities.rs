//! Port of `net.minecraft.world.level.entity.ChunkEntities` (Paper 26.2).
//!
//! Java source:
//! - `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/entity/ChunkEntities.java`

use rivet_registry::core::ChunkPos;

/// Port of `net.minecraft.world.level.entity.ChunkEntities<T>`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkEntities<T> {
    pos: ChunkPos,
    entities: Vec<T>,
}

impl<T> ChunkEntities<T> {
    /// Creates a new `ChunkEntities` container for `pos`.
    #[must_use]
    pub fn new(pos: ChunkPos, entities: Vec<T>) -> Self {
        Self { pos, entities }
    }

    /// Creates an empty `ChunkEntities` container for `pos` (`EntityStorage.emptyChunk`).
    #[must_use]
    pub fn empty(pos: ChunkPos) -> Self {
        Self {
            pos,
            entities: Vec::new(),
        }
    }

    /// Returns the chunk position (`ChunkEntities.getPos`).
    #[must_use]
    pub const fn pos(&self) -> ChunkPos {
        self.pos
    }

    /// Returns a slice of entities (`ChunkEntities.getEntities`).
    #[must_use]
    pub fn entities(&self) -> &[T] {
        &self.entities
    }

    /// Consumes the container and returns the underlying entity list.
    #[must_use]
    pub fn into_entities(self) -> Vec<T> {
        self.entities
    }

    /// Returns `true` if there are no entities in this chunk (`ChunkEntities.isEmpty`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Returns the number of entities in this chunk.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.len()
    }
}
