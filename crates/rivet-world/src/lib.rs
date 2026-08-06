//! `net.minecraft.world.level` — world/chunk layer.
//!
//! #108 (M1.1) — chunk wire format. This crate currently ports the pure
//! value/wire structures for the superflat chunk section:
//!
//! - `chunk::paletted_container` — `PalettedContainer<T>` (+ `Data`, the
//!   `PackedData` NBT form, `unpack`/`pack` re-encode)
//! - `chunk::palette` — `Palette<T>` + `SingleValuePalette`/`LinearPalette`/
//!   `HashMapPalette`/`GlobalPalette`, the `GlobalIdMap` surface, `IdForResult`
//! - `chunk::strategy` — `Strategy<T>` (block-states/biomes ladders)
//! - `chunk::configuration` — `Configuration` (Global/Simple)
//!
//! Bit packing lives in `rivet-util` (`SimpleBitStorage`/`ZeroBitStorage`).
//! No world state, chunk packets, or Moonrise block-counting here (M2).

pub mod chunk;
