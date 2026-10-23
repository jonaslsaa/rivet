//! Moonrise's owning POI chunk and PoiManager's section consistency operation.
//! Section load/dirty notifications require the owning manager's authority;
//! this is not a substitute for village-distance tracking or FULL publication.
use rivet_nbt::{
    compound_tag::CompoundTag, nbt_ops::NbtOps, nbt_utils::CURRENT_DATA_VERSION, tag::Tag,
};
use rivet_registry::{
    Holder, Identifier, RegistryAccess, ResourceKey,
    block_state::BlockState,
    core::{BlockPos, ChunkPos, SectionPos},
    holder_lookup::HolderGetter,
    registry_ops::RegistryOps,
};
use rivet_world::{
    chunk::level_chunk_section::LevelChunkSection,
    poi::{POINT_OF_INTEREST_TYPE, PackedPoiSection, PoiSection, PoiType},
};
use std::sync::Arc;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PoiChunkError {
    #[error("invalid inclusive POI section bounds [{min}, {max}]")]
    InvalidBounds { min: i32, max: i32 },
    #[error("POI section {section} is outside [{min}, {max}]")]
    OutOfBounds { section: i32, min: i32, max: i32 },
    #[error("owning world has no point_of_interest_type registry")]
    MissingRegistry,
}

/// Owns each section once. Dirty callbacks deliberately go through the owning
/// manager, which locates the current chunk and updates distance tracking;
/// capturing this chunk's dirty flag would be wrong after owner replacement.
pub struct PoiChunk {
    pos: ChunkPos,
    min_section: i32,
    max_section: i32,
    sections: Vec<Option<PoiSection>>,
    access: RegistryAccess,
    on_dirty: Arc<dyn Fn(SectionPos) + Send + Sync>,
    dirty: bool,
    loaded: bool,
}

impl PoiChunk {
    pub fn new(
        pos: ChunkPos,
        min_section: i32,
        max_section: i32,
        access: RegistryAccess,
        on_dirty: Arc<dyn Fn(SectionPos) + Send + Sync>,
    ) -> Result<Self, PoiChunkError> {
        let count = max_section
            .checked_sub(min_section)
            .and_then(|n| n.checked_add(1))
            .filter(|n| *n > 0)
            .ok_or(PoiChunkError::InvalidBounds {
                min: min_section,
                max: max_section,
            })?;
        if access.lookup(&POINT_OF_INTEREST_TYPE).is_none() {
            return Err(PoiChunkError::MissingRegistry);
        }
        Ok(Self {
            pos,
            min_section,
            max_section,
            sections: (0..count).map(|_| None).collect(),
            access,
            on_dirty,
            dirty: false,
            loaded: false,
        })
    }

    /// Source's empty factory differs from the constructor: it is already loaded.
    pub fn empty(
        pos: ChunkPos,
        min_section: i32,
        max_section: i32,
        access: RegistryAccess,
        on_dirty: Arc<dyn Fn(SectionPos) + Send + Sync>,
    ) -> Result<Self, PoiChunkError> {
        let mut chunk = Self::new(pos, min_section, max_section, access, on_dirty)?;
        chunk.loaded = true;
        Ok(chunk)
    }

    pub fn pos(&self) -> ChunkPos {
        self.pos
    }
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }
    pub fn is_empty(&self) -> bool {
        self.sections.iter().flatten().all(PoiSection::is_empty)
    }
    fn index(&self, y: i32) -> Option<usize> {
        (self.min_section..=self.max_section)
            .contains(&y)
            .then(|| (y - self.min_section) as usize)
    }
    fn dirty_callback(&self, y: i32) -> Arc<dyn Fn() + Send + Sync> {
        let dirty = Arc::clone(&self.on_dirty);
        let pos = SectionPos::of_chunk_pos(&self.pos, y);
        Arc::new(move || dirty(pos))
    }
    pub fn section(&self, y: i32) -> Option<&PoiSection> {
        self.sections.get(self.index(y)?)?.as_ref()
    }
    pub fn section_mut(&mut self, y: i32) -> Option<&mut PoiSection> {
        let index = self.index(y)?;
        self.sections[index].as_mut()
    }
    pub fn get_or_create_section(&mut self, y: i32) -> Result<&mut PoiSection, PoiChunkError> {
        let index = self.index(y).ok_or(PoiChunkError::OutOfBounds {
            section: y,
            min: self.min_section,
            max: self.max_section,
        })?;
        if self.sections[index].is_none() {
            self.sections[index] = Some(PoiSection::new(self.dirty_callback(y)));
        }
        Ok(self.sections[index].as_mut().unwrap())
    }

    /// Sets loaded before the first manager callback, and never repeats load.
    /// The callback is actual onSectionLoad/distance-tracking authority, not
    /// an optional notification silently replaced by a successful no-op.
    pub fn load(&mut self, mut on_section_load: impl FnMut(SectionPos, &Self)) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        for y in self.min_section..=self.max_section {
            if self.section(y).is_some_and(|s| !s.is_empty()) {
                on_section_load(SectionPos::of_chunk_pos(&self.pos, y), self);
            }
        }
    }

    /// Current-version data only; the storage data-fixer remains the caller's
    /// responsibility. Codec partial values are retained exactly like Paper.
    #[allow(clippy::too_many_arguments)]
    pub fn parse(
        pos: ChunkPos,
        min_section: i32,
        max_section: i32,
        access: RegistryAccess,
        on_dirty: Arc<dyn Fn(SectionPos) + Send + Sync>,
        data: &CompoundTag,
        mut diagnostic: impl FnMut(i32, &str),
    ) -> Result<Self, PoiChunkError> {
        let mut chunk = Self::empty(pos, min_section, max_section, access, on_dirty)?;
        let ops = RegistryOps::create_from_access(&NbtOps, chunk.access.clone());
        let registry = chunk.access.lookup(&POINT_OF_INTEREST_TYPE).unwrap();
        let sections = data.get_compound_or_empty("Sections");
        for y in min_section..=max_section {
            let key = y.to_string();
            if sections.get(&key).is_none() {
                continue;
            }
            let tag = Tag::Compound(sections.get_compound_or_empty(&key));
            let packed = PackedPoiSection::codec()
                .parse(&ops, &tag)
                .result_or_partial(|error| diagnostic(y, error));
            if let Some(packed) = packed {
                let section = packed.unpack(registry, chunk.dirty_callback(y));
                if !section.is_empty() {
                    chunk.sections[(y - min_section) as usize] = Some(section);
                    chunk.loaded = false;
                }
            }
        }
        Ok(chunk)
    }

    /// None means no serializable nonempty sections, not an empty chunk tag.
    /// Packing neither resets the chunk dirty flag nor marks it loaded.
    pub fn save(&self, mut diagnostic: impl FnMut(i32, &str)) -> Option<CompoundTag> {
        let ops = RegistryOps::create_from_access(&NbtOps, self.access.clone());
        let mut sections = CompoundTag::new();
        for y in self.min_section..=self.max_section {
            let Some(section) = self.section(y).filter(|s| !s.is_empty()) else {
                continue;
            };
            if let Some(tag) = PackedPoiSection::codec()
                .encode_start(&ops, &section.pack())
                .result_or_partial(|error| diagnostic(y, error))
            {
                sections.put(y.to_string(), tag);
            }
        }
        if sections.is_empty() {
            return None;
        }
        let mut tag = CompoundTag::new();
        tag.put("Sections".into(), Tag::Compound(sections));
        tag.put_int("DataVersion", CURRENT_DATA_VERSION);
        Some(tag)
    }

    /// PoiManager.checkConsistencyWithBlocks on this canonical chunk's section.
    /// Both proto BlockState and runtime StateId palettes use their real lookup.
    pub fn check_consistency_with_blocks<T, B>(
        &mut self,
        y: i32,
        blocks: &LevelChunkSection<T, B>,
        state: impl Fn(&T) -> BlockState,
    ) -> Result<(), PoiChunkError>
    where
        T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
        B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    {
        let index = self.index(y).ok_or(PoiChunkError::OutOfBounds {
            section: y,
            min: self.min_section,
            max: self.max_section,
        })?;
        // Valid stored sections are deliberately not rescanned, even if the
        // current blocks differ. Source refresh only repairs invalid sections.
        if self.sections[index]
            .as_ref()
            .is_some_and(PoiSection::is_valid)
        {
            return Ok(());
        }
        let may_have = blocks
            .states()
            .maybe_has(|value| PoiType::for_state(state(value)).is_some());
        let access = self.access.clone();
        let registry = access.lookup(&POINT_OF_INTEREST_TYPE).unwrap();
        let section_pos = SectionPos::of_chunk_pos(&self.pos, y);
        let scan = |output: &mut dyn FnMut(BlockPos, Holder<PoiType>)| {
            if may_have {
                // SectionPos.blocksInside: X fastest, then Y, then Z.
                for pos in section_pos.blocks_inside() {
                    if let Some(kind) = PoiType::for_state(state(&blocks.get_block_state(
                        pos.get_x() & 15,
                        pos.get_y() & 15,
                        pos.get_z() & 15,
                    ))) {
                        let holder = registry
                            .get(&ResourceKey::create(
                                &POINT_OF_INTEREST_TYPE,
                                Identifier::parse(kind.name()),
                            ))
                            .expect("owning POI registry contains the builtin state type");
                        output(pos, holder);
                    }
                }
            }
        };
        if let Some(section) = &mut self.sections[index] {
            section.refresh(registry, scan);
        } else if may_have {
            let section = self.get_or_create_section(y)?;
            scan(&mut |pos, kind| {
                section.add(pos, kind, registry);
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::tag_parser::parse_compound_fully;
    use rivet_registry::generated::block_properties::BlockPropertyId;
    use rivet_world::{block::Block, poi::bootstrap_poi_types};
    use serde_json::{Value, json};
    use std::sync::Mutex;

    fn access() -> RegistryAccess {
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(POINT_OF_INTEREST_TYPE.identifier().clone()),
            Box::new(bootstrap_poi_types()),
        )])
    }
    fn compare_snapshot(chunk: &PoiChunk, expected: &Value) {
        assert_eq!(chunk.is_loaded(), expected["loaded"].as_bool().unwrap());
        assert_eq!(chunk.is_dirty(), expected["dirty"].as_bool().unwrap());
        assert_eq!(chunk.is_empty(), expected["empty"].as_bool().unwrap());
        assert_eq!(
            chunk.save(|_, error| panic!("{error}")),
            expected
                .get("saved")
                .map(|s| parse_compound_fully(s.as_str().unwrap()).unwrap())
        );
    }

    #[test]
    fn actual_paper_poi_chunk_parse_load_save_and_block_consistency() {
        let air_chunk = super::super::LevelChunk::new(ChunkPos::ZERO);
        let air = air_chunk.get_sections()[4].copy();
        let mut mixed = air.copy();
        let bed = Block::from_name("minecraft:red_bed")
            .unwrap()
            .default_block_state();
        let head_index = BlockPropertyId::Part
            .values()
            .iter()
            .position(|v| *v == "head")
            .unwrap();
        let head = bed
            .set_property(BlockPropertyId::Part, head_index as u16)
            .unwrap();
        for (x, y, z, state) in [
            (1, 2, 3, head),
            (
                2,
                1,
                3,
                Block::from_name("minecraft:bell")
                    .unwrap()
                    .default_block_state(),
            ),
            (
                2,
                2,
                1,
                Block::from_name("minecraft:composter")
                    .unwrap()
                    .default_block_state(),
            ),
            (1, 1, 2, bed),
        ] {
            // These exact default dry states have no random/fluid ticks and
            // no Moonrise special collision. Only palette contents are read
            // by the actual POI consistency operation.
            mixed.set_block_state(
                x,
                y,
                z,
                state.id(),
                &|s| BlockState::new(*s).is_air(),
                &|_| false,
                &|_| true,
                &|_| false,
                &|_| false,
            );
        }
        let mut count = 0;
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/poi-chunk.jsonl").lines()
        {
            let row: Value = serde_json::from_str(line).unwrap();
            let pending = Arc::new(Mutex::new(Vec::new()));
            let queue = Arc::clone(&pending);
            let mut chunk = PoiChunk::parse(
                ChunkPos::new(-2, 3),
                -1,
                1,
                access(),
                Arc::new(move |pos| queue.lock().unwrap().push(pos)),
                &parse_compound_fully(row["input"].as_str().unwrap()).unwrap(),
                |_, _| {},
            )
            .unwrap();
            compare_snapshot(&chunk, &row["before"]);
            let mut events = Vec::new();
            for _ in 0..2 {
                chunk.load(|pos, current| {
                    events.push(json!(["load", pos.get_y(), current.is_loaded()]))
                });
            }
            compare_snapshot(&chunk, &row["loaded"]);
            for y in -1..=1 {
                let blocks = if y == 0 && row["pattern"] == "mixed" {
                    &mixed
                } else {
                    &air
                };
                chunk
                    .check_consistency_with_blocks(y, blocks, |id| BlockState::new(*id))
                    .unwrap();
                // The probe's manager notification sink marks its owning
                // chunk dirty. Distance propagation is outside this fixture.
                for pos in pending.lock().unwrap().drain(..) {
                    chunk.set_dirty(true);
                    events.push(json!(["dirty", pos.get_y(), chunk.is_loaded()]));
                }
            }
            compare_snapshot(&chunk, &row["after"]);
            assert_eq!(json!(events), row["events"], "{}", row["name"]);
            count += 1;
        }
        assert_eq!(count, 11);
    }
}
