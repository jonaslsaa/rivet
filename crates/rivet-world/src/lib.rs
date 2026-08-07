//! `net.minecraft.world.level` — world/chunk layer.
//!
//! #108 (M1.1) — chunk wire format. This crate ports the pure value/wire
//! structures for the superflat chunk section plus the issue #100 content
//! construction:
//!
//! - `chunk::paletted_container` — `PalettedContainer<T>` (+ `Data`, the
//!   `PackedData` NBT form, `unpack`/`pack` re-encode)
//! - `chunk::palette` — `Palette<T>` + `SingleValuePalette`/`LinearPalette`/
//!   `HashMapPalette`/`GlobalPalette`, the `GlobalIdMap` surface, `IdForResult`
//! - `chunk::strategy` — `Strategy<T>` (block-states/biomes ladders)
//! - `chunk::configuration` — `Configuration` (Global/Simple)
//! - `chunk::level_chunk_section` — `LevelChunkSection` wire write/size/recalc
//! - `chunk::data_layer` — `DataLayer` (16³ light layer)
//! - `levelgen::heightmap` — `Heightmap`/`primeHeightmaps`
//! - `lighting::light_update_data` — the light payload producer
//! - `superflat` — the deterministic single-stone superflat content builder
//!   (issue #100) that feeds the #94 `ClientboundLevelChunkWithLightPacket`.
//!
//! Bit packing lives in `rivet-util` (`SimpleBitStorage`/`ZeroBitStorage`).
//! No world state or chunk-send plumbing here yet (#100 keeps the content
//! construction pure; `PlayerChunkSender` is deferred).

pub mod chunk;
pub mod levelgen;
pub mod lighting;
pub mod superflat;
