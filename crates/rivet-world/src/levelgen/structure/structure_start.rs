//! Native executable structure-start authority. This is NOT a proof that a
//! world's structure registry, start generation, or reference graph is complete.

use super::{BoundingBox, PiecesContainer, StructurePiece, StructurePlacementAuthority};
use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_util::RandomSource;
use std::sync::OnceLock;

#[cfg(test)]
#[path = "structure_start_test.rs"]
mod tests;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum StructureError {
    #[error("structure piece BB must contain exactly six integers")]
    MalformedBoundingBox,
    #[error("structure child is not a compound")]
    MalformedPiece,
    #[error("unsupported structure piece decoder: {0}")]
    UnsupportedPiece(String),
    #[error("structure registry has no executable authority for {0}")]
    UnsupportedStructure(String),
    #[error("structure start registry/decoder authority is unavailable")]
    RegistryUnavailable,
    #[error("cannot calculate bounds of an empty structure")]
    EmptyPieces,
    #[error("structure placement authority is unavailable: {0}")]
    PlacementUnavailable(String),
}

/// The first concrete native structure. Its generation stub creates one real
/// buried-treasure piece; its afterPlace is the actual inherited no-op.
/// Other structures must gain a typed variant and real afterPlace before load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructureDefinition {
    BuriedTreasure {
        registry_id: String,
        terrain_adjustment: TerrainAdjustment,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainAdjustment {
    None,
    Bury,
    BeardThin,
    BeardBox,
    Encapsulate,
}

impl StructureDefinition {
    pub fn registry_id(&self) -> &str {
        match self {
            Self::BuriedTreasure { registry_id, .. } => registry_id,
        }
    }
    fn adjust_bounding_box(&self, bounds: BoundingBox) -> BoundingBox {
        match self {
            Self::BuriedTreasure {
                terrain_adjustment, ..
            } => {
                if *terrain_adjustment == TerrainAdjustment::None {
                    bounds
                } else {
                    bounds.inflated_by(12)
                }
            }
        }
    }
    /// The builder consumer from BuriedTreasureStructure.generatePieces.
    /// Height/biome eligibility and structure-set selection remain the
    /// generator's responsibility, not an implicit always-success generation.
    pub fn buried_treasure_pieces(chunk: ChunkPos) -> PiecesContainer {
        PiecesContainer::new(vec![StructurePiece::buried_treasure(BlockPos::new(
            chunk.get_min_block_x().wrapping_add(9),
            90,
            chunk.get_min_block_z().wrapping_add(9),
        ))])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructureStart {
    structure: Option<StructureDefinition>,
    piece_container: PiecesContainer,
    chunk_pos: ChunkPos,
    references: i32,
    cached_bounding_box: OnceLock<BoundingBox>,
    pub persistent_data: CompoundTag,
}

impl StructureStart {
    pub fn new(
        structure: StructureDefinition,
        chunk_pos: ChunkPos,
        references: i32,
        pieces: PiecesContainer,
    ) -> Self {
        Self {
            structure: Some(structure),
            chunk_pos,
            references,
            piece_container: pieces,
            cached_bounding_box: OnceLock::new(),
            persistent_data: CompoundTag::new(),
        }
    }
    pub fn invalid() -> Self {
        Self {
            structure: None,
            chunk_pos: ChunkPos::ZERO,
            references: 0,
            piece_container: PiecesContainer::default(),
            cached_bounding_box: OnceLock::new(),
            persistent_data: CompoundTag::new(),
        }
    }
    pub fn is_valid(&self) -> bool {
        !self.piece_container.is_empty()
    }
    pub fn chunk_pos(&self) -> ChunkPos {
        self.chunk_pos
    }
    pub fn references(&self) -> i32 {
        self.references
    }
    pub fn can_be_referenced(&self) -> bool {
        self.references < 1
    }
    pub fn add_reference(&mut self) {
        self.references = self.references.wrapping_add(1);
    }
    pub fn structure(&self) -> Option<&StructureDefinition> {
        self.structure.as_ref()
    }
    pub fn pieces(&self) -> &PiecesContainer {
        &self.piece_container
    }
    pub fn bounding_box(&self) -> Result<BoundingBox, StructureError> {
        if let Some(bounds) = self.cached_bounding_box.get() {
            return Ok(*bounds);
        }
        let bounds = self.piece_container.calculate_bounding_box()?;
        let bounds = self
            .structure
            .as_ref()
            .ok_or(StructureError::EmptyPieces)?
            .adjust_bounding_box(bounds);
        let _ = self.cached_bounding_box.set(bounds);
        Ok(*self.cached_bounding_box.get().expect("bounds initialized"))
    }

    pub fn load(
        tag: &CompoundTag,
        resolve: impl FnOnce(&str) -> Option<StructureDefinition>,
    ) -> Result<Self, StructureError> {
        let id = tag.get_string_or("id", "");
        if id == "INVALID" {
            return Ok(Self::invalid());
        }
        let structure =
            resolve(id).ok_or_else(|| StructureError::UnsupportedStructure(id.to_string()))?;
        let pieces = PiecesContainer::load(&tag.get_list_or_empty("Children"))?;
        // Paper loadStaticStart itself does not restore StructureBukkitValues.
        Ok(Self::new(
            structure,
            ChunkPos::new(tag.get_int_or("ChunkX", 0), tag.get_int_or("ChunkZ", 0)),
            tag.get_int_or("references", 0),
            pieces,
        ))
    }

    /// Uses the supplied save chunk, not the start's original chunk (Paper).
    pub fn create_tag(&self, chunk: ChunkPos) -> CompoundTag {
        let mut tag = CompoundTag::new();
        if !self.persistent_data.is_empty() {
            tag.put(
                "StructureBukkitValues".into(),
                Tag::Compound(self.persistent_data.clone()),
            );
        }
        if self.is_valid() {
            tag.put_string(
                "id",
                self.structure
                    .as_ref()
                    .expect("valid start has structure")
                    .registry_id(),
            );
            tag.put_int("ChunkX", chunk.x());
            tag.put_int("ChunkZ", chunk.z());
            tag.put_int("references", self.references);
            tag.put("Children".into(), Tag::List(self.piece_container.save()));
        } else {
            tag.put_string("id", "INVALID");
        }
        tag
    }

    pub fn place_in_chunk<R: RandomSource>(
        &mut self,
        authority: &mut impl StructurePlacementAuthority,
        random: &mut R,
        chunk_bb: BoundingBox,
        chunk: ChunkPos,
    ) -> Result<(), StructureError> {
        if self.piece_container.is_empty() {
            return Ok(());
        }
        // Compute the entire intersecting list before any piece mutates its
        // bounds, as Paper's stream().filter(...).toList() does.
        let intersecting: Vec<_> = self
            .piece_container
            .pieces()
            .iter()
            .enumerate()
            .filter_map(|(i, piece)| piece.data().bounding_box.intersects(chunk_bb).then_some(i))
            .collect();
        if !intersecting.is_empty() {
            authority.begin_structure(
                self.structure.as_ref().expect("nonempty structure"),
                chunk_bb,
                chunk,
            )?;
            for index in intersecting {
                self.piece_container.pieces_mut()[index]
                    .post_process(authority, random, chunk_bb)?;
            }
            authority.finish_structure()?;
        }
        // BuriedTreasureStructure inherits the real no-op Structure.afterPlace,
        // including when no piece intersects. Future variants must dispatch it.
        Ok(())
    }
}
