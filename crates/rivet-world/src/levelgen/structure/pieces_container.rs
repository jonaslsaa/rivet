//! Paper's immutable ordered piece list; placement may mutate each piece's box.

use super::{BoundingBox, StructureError, StructurePiece};
use rivet_nbt::{list_tag::ListTag, tag::Tag};
use rivet_registry::core::BlockPos;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PiecesContainer {
    pieces: Vec<StructurePiece>,
}

impl PiecesContainer {
    pub fn new(pieces: Vec<StructurePiece>) -> Self {
        Self { pieces }
    }
    pub fn pieces(&self) -> &[StructurePiece] {
        &self.pieces
    }
    pub(super) fn pieces_mut(&mut self) -> &mut [StructurePiece] {
        &mut self.pieces
    }
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }
    pub fn is_inside_piece(&self, pos: &BlockPos) -> bool {
        self.pieces
            .iter()
            .any(|piece| piece.data().bounding_box.is_inside(pos))
    }
    pub fn calculate_bounding_box(&self) -> Result<BoundingBox, StructureError> {
        let mut pieces = self.pieces.iter();
        let mut bounds = pieces
            .next()
            .ok_or(StructureError::EmptyPieces)?
            .data()
            .bounding_box;
        for piece in pieces {
            bounds.encapsulate(piece.data().bounding_box);
        }
        Ok(bounds)
    }
    pub fn save(&self) -> ListTag {
        ListTag::with_list(
            self.pieces
                .iter()
                .map(|piece| Tag::Compound(piece.create_tag()))
                .collect(),
        )
    }
    /// Unlike Paper's log-and-skip recovery, unsupported native decoders are
    /// surfaced explicitly. Silently dropping an unported piece would fabricate
    /// an executable but incomplete structure.
    pub fn load(children: &ListTag) -> Result<Self, StructureError> {
        let pieces = children
            .list
            .iter()
            .map(|tag| {
                let Tag::Compound(tag) = tag else {
                    return Err(StructureError::MalformedPiece);
                };
                StructurePiece::load(tag)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::new(pieces))
    }
}
