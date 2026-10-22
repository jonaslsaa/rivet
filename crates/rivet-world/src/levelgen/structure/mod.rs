//! `net.minecraft.world.level.levelgen.structure` — structure generation.
//!
//! Executable starts, ordered pieces, integer bounds and the first concrete
//! buried-treasure piece coexist with template rule tests. These types do not
//! claim complete world structure selection/references or plugin transformer
//! authority; unsupported subtype decoding and placement capabilities refuse
//! explicitly until their owning runtime is installed.

pub mod bounding_box;
pub mod pieces_container;
pub mod structure_piece;
pub mod structure_start;
pub mod templatesystem;

pub use bounding_box::BoundingBox;
pub use pieces_container::PiecesContainer;
pub use structure_piece::{StructurePiece, StructurePieceData, StructurePlacementAuthority};
pub use structure_start::{StructureDefinition, StructureError, StructureStart, TerrainAdjustment};
