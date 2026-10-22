//! Root placer family from pinned Paper 26.2. The only registered vanilla
//! root placer is MangroveRootPlacer; no unavailable root payloads are erased.
pub mod above_root_placement;
pub mod mangrove_root_placement;
pub mod mangrove_root_placer;
pub mod root_placer;
pub use above_root_placement::AboveRootPlacement;
pub use mangrove_root_placement::MangroveRootPlacement;
pub use mangrove_root_placer::MangroveRootPlacer;
pub use root_placer::{RootPlacer, root_placer_codec};
