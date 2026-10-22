//! `net.minecraft.world.level.levelgen.structure` — structure generation.
//!
//! Executable buried-treasure pieces, native mineshaft start graphs, ordered
//! persistence and integer bounds coexist with template rule tests. These types do not
//! claim complete world structure selection/references or plugin transformer
//! authority; unsupported subtype decoding and placement capabilities refuse
//! explicitly until their owning runtime is installed.

pub mod bounding_box;
pub mod generation;
pub mod mineshaft;
pub mod pieces_container;
pub mod placement;
pub mod registry_keys;
pub mod selection;
pub mod state;
pub mod structure_body;
pub mod structure_piece;
pub mod structure_registry;
pub mod structure_settings;
pub mod structure_start;
pub mod templatesystem;

pub use bounding_box::BoundingBox;
pub use pieces_container::PiecesContainer;
pub use structure_piece::{StructurePiece, StructurePieceData, StructurePlacementAuthority};
pub use structure_start::{StructureDefinition, StructureError, StructureStart, TerrainAdjustment};
