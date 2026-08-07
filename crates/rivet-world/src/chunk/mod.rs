//! `net.minecraft.world.level.chunk` — chunk wire-format structures (#108) and
//! the #100 `LevelChunkSection`/`DataLayer` content layer.
//!
//! This module ports the pure `PalettedContainer`/`Palette`/`Strategy`/
//! `Configuration` value layer plus the `LevelChunkSection` wire
//! write/size/recalc slice and the `DataLayer` light layer (issue #100).

pub mod configuration;
pub mod data_layer;
pub mod level_chunk_section;
pub mod palette;
pub mod paletted_container;
pub mod strategy;
