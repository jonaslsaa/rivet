//! `net.minecraft.world.level.chunk` — chunk wire-format structures (#108) and
//! the #100 `LevelChunkSection`/`DataLayer` content layer.
//!
//! This module ports the pure `PalettedContainer`/`Palette`/`Strategy`/
//! `Configuration` value layer, the `LevelChunkSection` wire write/size/recalc
//! slice, the `DataLayer` light layer, the `PalettedContainerRO` read view +
//! `PalettedContainerFactory` factory (#230), and the `chunk.support` leaf
//! types (`CarvingMask`/`BlockColumn`/`LightChunk`/`LightChunkGetter`/
//! `StructureAccess`).

pub mod block_column;
pub mod carving_mask;
pub mod configuration;
pub mod data_layer;
pub mod level_chunk_section;
pub mod light_chunk;
pub mod light_chunk_getter;
pub mod palette;
pub mod paletted_container;
pub mod paletted_container_factory;
pub mod strategy;
pub mod structure_access;
