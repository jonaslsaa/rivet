//! Tick-thread POI chunk ownership and real village-distance source updates.
//! Storage supplies parsed data or confirmed absent data explicitly; missing
//! chunks are never silently treated as empty. This does not publish FULL.
use super::poi_chunk::{PoiChunk, PoiChunkError};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::{
    Holder, HolderSet, Identifier, RegistryAccess, TagKey,
    block_state::BlockState,
    core::{BlockPos, ChunkPos, SectionPos},
    holder_lookup::HolderGetter,
};
use rivet_world::{
    chunk::level_chunk_section::LevelChunkSection,
    poi::{
        Delayed26WayDistancePropagator3D, POINT_OF_INTEREST_TYPE, PoiOccupancy, PoiSectionError,
        PoiType,
    },
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Debug, thiserror::Error)]
pub enum PoiManagerError {
    #[error(transparent)]
    Chunk(#[from] PoiChunkError),
    #[error(transparent)]
    Section(#[from] PoiSectionError),
    #[error("POI chunk {0} has not been loaded from storage")]
    MissingChunk(ChunkPos),
    #[error("POI chunk {0} already has a canonical owner")]
    DuplicateChunk(ChunkPos),
    #[error("owning POI registry has no bound minecraft:village tag")]
    MissingVillageTag,
    #[error("POI holder does not belong to the owning registry")]
    ForeignHolder,
}

pub struct PoiManager {
    chunks: HashMap<ChunkPos, PoiChunk>,
    access: RegistryAccess,
    village: HolderSet<PoiType>,
    min_section: i32,
    max_section: i32,
    dirty_sections: Arc<Mutex<Vec<SectionPos>>>,
    distance: Delayed26WayDistancePropagator3D,
}

impl PoiManager {
    pub fn has_chunk(&self, pos: ChunkPos) -> bool {
        self.chunks.contains_key(&pos)
    }
    /// Requires the owning world's actual bound #minecraft:village tag. It
    /// cannot default to a hand-picked subset or mix holders from other worlds.
    pub fn new(
        access: RegistryAccess,
        min_section: i32,
        max_section: i32,
    ) -> Result<Self, PoiManagerError> {
        let registry = access
            .lookup(&POINT_OF_INTEREST_TYPE)
            .ok_or(PoiChunkError::MissingRegistry)?;
        let village = HolderGetter::get_tag(
            registry,
            &TagKey::create(
                &POINT_OF_INTEREST_TYPE,
                Identifier::with_default_namespace("village"),
            ),
        )
        .ok_or(PoiManagerError::MissingVillageTag)?;
        if max_section < min_section {
            return Err(PoiChunkError::InvalidBounds {
                min: min_section,
                max: max_section,
            }
            .into());
        }
        Ok(Self {
            chunks: HashMap::new(),
            access,
            village,
            min_section,
            max_section,
            dirty_sections: Arc::new(Mutex::new(Vec::new())),
            distance: Delayed26WayDistancePropagator3D::new(),
        })
    }

    fn callback(&self) -> Arc<dyn Fn(SectionPos) + Send + Sync> {
        let queue = Arc::clone(&self.dirty_sections);
        Arc::new(move |pos| queue.lock().unwrap().push(pos))
    }

    fn update_distance(&mut self, pos: SectionPos) {
        let chunk_pos = ChunkPos::new(pos.get_x(), pos.get_z());
        let occupied = self
            .chunks
            .get(&chunk_pos)
            .filter(|chunk| chunk.is_loaded())
            .and_then(|chunk| chunk.section(pos.get_y()))
            .is_some_and(|section| {
                section.any_record(|kind| self.village.contains(kind), PoiOccupancy::IsOccupied)
            });
        if occupied {
            self.distance
                .set_source(pos.as_long_self(), 7)
                .expect("constant POI level is valid");
        } else {
            self.distance.remove_source(pos.as_long_self());
        }
    }

    /// Source callbacks occur inside the borrowed section. Drain before every
    /// public observation and after each mutation; no chunk lock is held while
    /// a record invokes its dirty callback. Release's duplicate callbacks are
    /// retained. Source propagation itself stays delayed until tick/query.
    fn flush_dirty(&mut self) {
        let pending = std::mem::take(&mut *self.dirty_sections.lock().unwrap());
        for pos in pending {
            if let Some(chunk) = self
                .chunks
                .get_mut(&ChunkPos::new(pos.get_x(), pos.get_z()))
            {
                chunk.set_dirty(true);
            }
            self.update_distance(pos);
        }
    }

    pub fn install_from_storage(
        &mut self,
        pos: ChunkPos,
        data: Option<&CompoundTag>,
        diagnostic: impl FnMut(i32, &str),
    ) -> Result<(), PoiManagerError> {
        self.flush_dirty();
        if self.chunks.contains_key(&pos) {
            return Err(PoiManagerError::DuplicateChunk(pos));
        }
        let chunk = match data {
            Some(data) => PoiChunk::parse(
                pos,
                self.min_section,
                self.max_section,
                self.access.clone(),
                self.callback(),
                data,
                diagnostic,
            )?,
            None => PoiChunk::empty(
                pos,
                self.min_section,
                self.max_section,
                self.access.clone(),
                self.callback(),
            )?,
        };
        self.chunks.insert(pos, chunk);
        let mut loaded_sections = Vec::new();
        self.chunks
            .get_mut(&pos)
            .unwrap()
            .load(|section, _| loaded_sections.push(section));
        for section in loaded_sections {
            self.update_distance(section);
        }
        Ok(())
    }

    /// Call only after the storage owner has completed its save/eviction
    /// protocol. Removal precedes source updates, matching onUnload lookups.
    pub fn take_after_save(&mut self, pos: ChunkPos) -> Option<PoiChunk> {
        self.flush_dirty();
        let chunk = self.chunks.remove(&pos)?;
        for y in self.min_section..=self.max_section {
            self.update_distance(SectionPos::of_chunk_pos(&pos, y));
        }
        Some(chunk)
    }

    pub fn is_dirty(&mut self, pos: ChunkPos) -> Result<bool, PoiManagerError> {
        self.flush_dirty();
        Ok(self
            .chunks
            .get(&pos)
            .ok_or(PoiManagerError::MissingChunk(pos))?
            .is_dirty())
    }
    pub fn save(
        &mut self,
        pos: ChunkPos,
        diagnostic: impl FnMut(i32, &str),
    ) -> Result<Option<CompoundTag>, PoiManagerError> {
        self.flush_dirty();
        Ok(self
            .chunks
            .get(&pos)
            .ok_or(PoiManagerError::MissingChunk(pos))?
            .save(diagnostic))
    }
    pub fn mark_saved(&mut self, pos: ChunkPos) -> Result<(), PoiManagerError> {
        self.flush_dirty();
        self.chunks
            .get_mut(&pos)
            .ok_or(PoiManagerError::MissingChunk(pos))?
            .set_dirty(false);
        Ok(())
    }

    pub fn add(&mut self, pos: BlockPos, kind: Holder<PoiType>) -> Result<bool, PoiManagerError> {
        self.flush_dirty();
        let registry = self.access.lookup(&POINT_OF_INTEREST_TYPE).unwrap();
        if kind.registry_id() != Some(registry.registry_id()) {
            return Err(PoiManagerError::ForeignHolder);
        }
        let chunk_pos = ChunkPos::containing(&pos);
        let chunk = self
            .chunks
            .get_mut(&chunk_pos)
            .ok_or(PoiManagerError::MissingChunk(chunk_pos))?;
        let added = chunk
            .get_or_create_section(pos.get_y() >> 4)?
            .add(pos, kind, registry)
            .is_some();
        self.flush_dirty();
        Ok(added)
    }
    pub fn remove(&mut self, pos: BlockPos) -> Result<bool, PoiManagerError> {
        self.flush_dirty();
        let chunk_pos = ChunkPos::containing(&pos);
        let removed = self
            .chunks
            .get_mut(&chunk_pos)
            .ok_or(PoiManagerError::MissingChunk(chunk_pos))?
            .section_mut(pos.get_y() >> 4)
            .is_some_and(|section| section.remove(&pos));
        self.flush_dirty();
        Ok(removed)
    }
    /// Acquires the current position-indexed record. This is not PoiManager.take:
    /// that search may select a retained old record in a by-type bucket after
    /// a mismatch, and must acquire that exact object rather than relookup here.
    pub fn acquire_current_record(&mut self, pos: BlockPos) -> Result<bool, PoiManagerError> {
        self.flush_dirty();
        let chunk_pos = ChunkPos::containing(&pos);
        let acquired = self
            .chunks
            .get(&chunk_pos)
            .ok_or(PoiManagerError::MissingChunk(chunk_pos))?
            .section(pos.get_y() >> 4)
            .and_then(|section| section.record(&pos))
            .is_some_and(|record| record.acquire_ticket());
        self.flush_dirty();
        Ok(acquired)
    }
    pub fn release(&mut self, pos: BlockPos) -> Result<bool, PoiManagerError> {
        self.flush_dirty();
        let chunk_pos = ChunkPos::containing(&pos);
        let released = self
            .chunks
            .get(&chunk_pos)
            .ok_or(PoiManagerError::MissingChunk(chunk_pos))?
            .section(pos.get_y() >> 4)
            .ok_or(PoiSectionError::MissingRecord(pos))?
            .release(&pos)?;
        self.flush_dirty();
        Ok(released)
    }
    pub fn check_consistency_with_blocks<T, B>(
        &mut self,
        pos: SectionPos,
        blocks: &LevelChunkSection<T, B>,
        state: impl Fn(&T) -> BlockState,
    ) -> Result<(), PoiManagerError>
    where
        T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
        B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    {
        self.flush_dirty();
        let chunk_pos = ChunkPos::new(pos.get_x(), pos.get_z());
        self.chunks
            .get_mut(&chunk_pos)
            .ok_or(PoiManagerError::MissingChunk(chunk_pos))?
            .check_consistency_with_blocks(pos.get_y(), blocks, state)?;
        self.flush_dirty();
        Ok(())
    }
    pub fn tick(&mut self) {
        self.flush_dirty();
        self.distance.propagate_updates();
    }
    pub fn sections_to_village(&mut self, pos: SectionPos) -> i32 {
        self.tick();
        7 - self.distance.get_level(pos.as_long_self())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::tag_parser::parse_compound_fully;
    use rivet_registry::{RegistrationInfo, RegistryBuilder, ResourceKey, holder::HolderId};
    use serde_json::{Value, json};

    #[test]
    fn actual_paper_manager_load_ticket_changes_and_unload_update_village_distance() {
        let rows: Vec<Value> =
            include_str!("../../../../../tools/rivet-oracle/fixtures/poi-manager.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let mut builder = RegistryBuilder::new(&POINT_OF_INTEREST_TYPE);
        let mut village_ids = Vec::new();
        for (id, kind) in PoiType::all().enumerate() {
            builder.register(
                &ResourceKey::create(&POINT_OF_INTEREST_TYPE, Identifier::parse(kind.name())),
                Arc::new(kind),
                RegistrationInfo::BUILT_IN,
            );
            if rows[0]["types"]
                .as_array()
                .unwrap()
                .iter()
                .any(|name| name == kind.name())
            {
                village_ids.push(HolderId(id as u32));
            }
        }
        builder.bind_tags(vec![(
            TagKey::create(
                &POINT_OF_INTEREST_TYPE,
                Identifier::with_default_namespace("village"),
            ),
            village_ids,
        )]);
        let access = RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(POINT_OF_INTEREST_TYPE.identifier().clone()),
            Box::new(builder.freeze()),
        )]);
        let registry = access.lookup(&POINT_OF_INTEREST_TYPE).unwrap();
        let holder = |name| {
            registry
                .get(&ResourceKey::create(
                    &POINT_OF_INTEREST_TYPE,
                    Identifier::with_default_namespace(name),
                ))
                .unwrap()
        };
        let mut manager = PoiManager::new(access.clone(), -1, 1).unwrap();
        let chunk_pos = ChunkPos::new(-2, 3);
        let home = BlockPos::new(-31, 2, 51);
        let meeting = BlockPos::new(-30, 2, 51);
        for row in rows.iter().skip(1) {
            match row["phase"].as_str().unwrap() {
                "loaded_occupied" => manager.install_from_storage(chunk_pos,Some(&parse_compound_fully(
                    "{Sections:{'0':{Valid:1b,Records:[{pos:[I;-31,2,51],type:'home'}]}}}").unwrap()),|_,error|panic!("{error}")).unwrap(),
                "released" => { assert!(manager.release(home).unwrap()); },
                "reacquired" => { assert!(manager.acquire_current_record(home).unwrap()); },
                "meeting_remains" => {
                    assert!(manager.add(meeting,holder("meeting")).unwrap());
                    assert!(manager.acquire_current_record(meeting).unwrap());
                    assert!(manager.remove(home).unwrap());
                },
                "all_removed" => { assert!(manager.remove(meeting).unwrap()); },
                "occupied_again" => {
                    assert!(manager.add(home,holder("home")).unwrap());
                    assert!(manager.acquire_current_record(home).unwrap());
                },
                "unloaded" => { assert!(manager.take_after_save(chunk_pos).is_some()); },
                other => panic!("unexpected phase {other}"),
            }
            let distances: Vec<_> = [
                [0, 0, 0],
                [1, 0, 0],
                [1, 1, 1],
                [6, 0, 0],
                [7, 0, 0],
                [0, -6, 0],
            ]
            .map(|[x, y, z]| manager.sections_to_village(SectionPos::of(-2 + x, y, 3 + z)))
            .into_iter()
            .collect();
            assert_eq!(json!(distances), row["distances"], "{}", row["phase"]);
            if let Some(expected) = row.get("chunk") {
                let chunk = manager.chunks.get(&chunk_pos).unwrap();
                assert_eq!(chunk.is_loaded(), expected["loaded"].as_bool().unwrap());
                assert_eq!(chunk.is_dirty(), expected["dirty"].as_bool().unwrap());
                assert_eq!(chunk.is_empty(), expected["empty"].as_bool().unwrap());
                assert_eq!(
                    chunk.save(|_, error| panic!("{error}")),
                    expected
                        .get("saved")
                        .map(|s| parse_compound_fully(s.as_str().unwrap()).unwrap())
                );
            } else {
                assert!(!manager.chunks.contains_key(&chunk_pos));
            }
        }
        assert_eq!(rows.len(), 8);
    }
}
