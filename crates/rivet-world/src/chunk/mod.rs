//! `net.minecraft.world.level.chunk` — chunk wire-format structures (#108).
//!
//! `LevelChunkSection` itself is deferred to #100 (the superflat chunk
//! pipeline); this module ports the pure `PalettedContainer`/`Palette`/
//! `Strategy`/`Configuration` value layer it serializes through.
//!
//! RivetTodo(#100): `LevelChunkSection` (the container consumer) is not ported
//! — deferred with the superflat chunk pipeline.

pub mod configuration;
pub mod palette;
pub mod paletted_container;
pub mod strategy;
