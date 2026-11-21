//! Ports of `net.minecraft.world.level.entity.ChunkEntities`,
//! `net.minecraft.world.level.entity.EntityPersistentStorage`, and
//! `net.minecraft.world.level.chunk.storage.EntityStorage` (Paper 26.2).
//!
//! Java sources:
//! - `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/entity/ChunkEntities.java`
//! - `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/chunk/storage/EntityStorage.java`

use crate::storage::entity_record::{
    EntitySaveLimits, EntityStorageError, NetworkIdAllocator, StoredEntityRecord,
    decode_chunk_entities_nbt, encode_chunk_entities_nbt,
};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::core::ChunkPos;
use std::collections::{HashMap, HashSet};

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

/// Region/chunk storage boundary for `EntityStorage`.
///
/// Connects `rivet-entity::storage` to M2 region/chunk storage (`SimpleRegionStorage` /
/// `entity_region_storage`) without introducing a circular crate dependency on
/// `rivet-server`.
pub trait EntityRegionBackend {
    /// Reads the raw entity chunk `CompoundTag` for `pos`, or `Ok(None)` if absent.
    fn read_chunk(&mut self, pos: ChunkPos) -> Result<Option<CompoundTag>, EntityStorageError>;

    /// Writes `tag` for `pos`, or deletes/clears the chunk entry when `tag` is `None`
    /// (`IOWorker.STORE_EMPTY`).
    fn write_chunk(
        &mut self,
        pos: ChunkPos,
        tag: Option<CompoundTag>,
    ) -> Result<(), EntityStorageError>;

    /// Flushes pending writes to durable storage (`EntityPersistentStorage.flush`).
    fn flush(&mut self, sync: bool) -> Result<(), EntityStorageError>;
}

/// In-memory `EntityRegionBackend` for deterministic tests and local harnesses.
#[derive(Clone, Debug, Default)]
pub struct InMemoryEntityRegionBackend {
    chunks: HashMap<i64, CompoundTag>,
    read_count: usize,
    write_count: usize,
    delete_count: usize,
    flush_count: usize,
    fail_next_write: bool,
}

impl InMemoryEntityRegionBackend {
    /// Creates an empty in-memory backend.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a raw chunk tag directly (useful for testing malformed or legacy NBT).
    pub fn insert_raw(&mut self, pos: ChunkPos, tag: CompoundTag) {
        self.chunks.insert(pos.pack(), tag);
    }

    /// Returns the raw stored `CompoundTag` for `pos`, if present.
    #[must_use]
    pub fn raw_chunk(&self, pos: ChunkPos) -> Option<&CompoundTag> {
        self.chunks.get(&pos.pack())
    }

    /// Returns the number of backend reads performed.
    #[must_use]
    pub const fn read_count(&self) -> usize {
        self.read_count
    }

    /// Returns the number of non-empty chunk writes performed.
    #[must_use]
    pub const fn write_count(&self) -> usize {
        self.write_count
    }

    /// Returns the number of empty-chunk deletions (`STORE_EMPTY`) performed.
    #[must_use]
    pub const fn delete_count(&self) -> usize {
        self.delete_count
    }

    /// Returns the number of flushes performed.
    #[must_use]
    pub const fn flush_count(&self) -> usize {
        self.flush_count
    }

    /// Configures the next `write_chunk` call to return a backend error.
    pub fn fail_next_write(&mut self) {
        self.fail_next_write = true;
    }
}

impl EntityRegionBackend for InMemoryEntityRegionBackend {
    fn read_chunk(&mut self, pos: ChunkPos) -> Result<Option<CompoundTag>, EntityStorageError> {
        self.read_count += 1;
        Ok(self.chunks.get(&pos.pack()).cloned())
    }

    fn write_chunk(
        &mut self,
        pos: ChunkPos,
        tag: Option<CompoundTag>,
    ) -> Result<(), EntityStorageError> {
        if self.fail_next_write {
            self.fail_next_write = false;
            return Err(EntityStorageError::Backend {
                message: "injected write failure".to_string(),
            });
        }
        match tag {
            Some(compound) => {
                self.write_count += 1;
                self.chunks.insert(pos.pack(), compound);
            }
            None => {
                self.delete_count += 1;
                self.chunks.remove(&pos.pack());
            }
        }
        Ok(())
    }

    fn flush(&mut self, _sync: bool) -> Result<(), EntityStorageError> {
        self.flush_count += 1;
        Ok(())
    }
}

/// Port of `net.minecraft.world.level.chunk.storage.EntityStorage`.
///
/// Tracks `empty_chunks` identically to Paper so repeated loads or empty stores
/// for already-empty chunks avoid redundant region I/O.
#[derive(Debug)]
pub struct EntityStorage<B: EntityRegionBackend> {
    backend: B,
    empty_chunks: HashSet<i64>,
    save_limits: EntitySaveLimits,
    id_allocator: NetworkIdAllocator,
}

impl<B: EntityRegionBackend> EntityStorage<B> {
    /// Creates a new `EntityStorage` wrapping `backend`.
    #[must_use]
    pub fn new(backend: B, save_limits: EntitySaveLimits) -> Self {
        Self {
            backend,
            empty_chunks: HashSet::new(),
            save_limits,
            id_allocator: NetworkIdAllocator::new(),
        }
    }

    /// Returns a reference to the underlying region backend.
    #[must_use]
    pub const fn backend(&self) -> &B {
        &self.backend
    }

    /// Returns a mutable reference to the underlying region backend.
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Returns the configured per-chunk entity save/load limits.
    #[must_use]
    pub const fn save_limits(&self) -> &EntitySaveLimits {
        &self.save_limits
    }

    /// Allocates the next network entity ID from this storage's allocator.
    pub fn allocate_network_id(&mut self) -> i32 {
        self.id_allocator.next_id()
    }

    /// Returns `true` if `pos` is currently cached in `empty_chunks`.
    #[must_use]
    pub fn is_known_empty(&self, pos: ChunkPos) -> bool {
        self.empty_chunks.contains(&pos.pack())
    }

    /// Port of `EntityStorage.loadEntities(ChunkPos)`.
    ///
    /// Returns immediately with an empty `ChunkEntities` if `pos` is cached in
    /// `empty_chunks`. Otherwise reads from the backend, caches absence in
    /// `empty_chunks`, or decodes current-format NBT with hard validation.
    pub fn load_entities(
        &mut self,
        pos: ChunkPos,
    ) -> Result<ChunkEntities<StoredEntityRecord>, EntityStorageError> {
        let key = pos.pack();
        if self.empty_chunks.contains(&key) {
            return Ok(ChunkEntities::empty(pos));
        }

        let Some(tag) = self.backend.read_chunk(pos)? else {
            self.empty_chunks.insert(key);
            return Ok(ChunkEntities::empty(pos));
        };

        let entities =
            decode_chunk_entities_nbt(pos, &tag, &mut self.id_allocator, &self.save_limits)?;
        Ok(ChunkEntities::new(pos, entities))
    }

    /// Port of `EntityStorage.storeEntities(ChunkEntities<Entity>)`.
    ///
    /// Empty chunks write `None` (`IOWorker.STORE_EMPTY`) only when newly transitioning
    /// into `empty_chunks`. Non-empty chunks write current-format NBT and remove the
    /// chunk from `empty_chunks`.
    pub fn store_entities(
        &mut self,
        chunk: &ChunkEntities<StoredEntityRecord>,
    ) -> Result<(), EntityStorageError> {
        let pos = chunk.pos();
        let key = pos.pack();
        if chunk.is_empty() {
            if self.empty_chunks.insert(key)
                && let Err(err) = self.backend.write_chunk(pos, None)
            {
                self.empty_chunks.remove(&key);
                return Err(err);
            }
            return Ok(());
        }

        let chunk_tag = encode_chunk_entities_nbt(pos, chunk.entities(), &self.save_limits);
        self.backend.write_chunk(pos, Some(chunk_tag))?;
        self.empty_chunks.remove(&key);
        Ok(())
    }

    /// Flushes the underlying region backend (`EntityStorage.flush`).
    pub fn flush(&mut self, sync: bool) -> Result<(), EntityStorageError> {
        self.backend.flush(sync)
    }
}
