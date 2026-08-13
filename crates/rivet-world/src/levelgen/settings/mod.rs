//! `net.minecraft.world.level.levelgen` — the `mc.world.level.levelgen.settings`
//! manifest unit (issue #179): the world-level-gen *settings* sources.
//!
//! Ported here, all under `levelgen::settings` (the Java package's settings
//! classes):
//!
//! - [`world_options`] — `WorldOptions` (seed / structures / bonus-chest /
//!   legacy-custom-options + the `parseSeed`/`randomSeed` factories).
//! - [`world_dimensions`] — `WorldDimensions` + the out-of-unit `LevelStem`
//!   value shell the record stores (the real `LevelStem` lives with the
//!   pending `mc.world.level.dimension` unit).
//! - [`world_gen_settings`] — `WorldGenSettings` (options + dimensions + the
//!   `SavedDataType` shell; the `SavedData` base defers with
//!   `mc.world.level.saveddata`, #421).
//! - [`geode_block_settings`] / [`geode_crack_settings`] / [`geode_layer_settings`]
//!   — the geode feature's three settings records.
//! - [`below_zero_retrogen`] — `BelowZeroRetrogen` (the below-zero retrogen
//!   mask/codec + the `java.util.BitSet` subset it serializes).
//! - [`flat_level_source`] / [`debug_level_source`] — the two concrete
//!   `ChunkGenerator` realizations owned by this unit, implementing the
//!   `ChunkGenerator` trait seam (rivet-world::chunk::chunk_generator).
//!
//! `GenerationStep` is *not* re-ported here: it was proactively ported as
//! `levelgen::generation_step` by the #306 feature-shell wave (see the
//! `generation_step` module doc — the settings wave must not re-port it).
//!
//! ### Ownership seams
//!
//! The unit's `java_paths` include `FlatLevelGeneratorSettings` (via
//! `FlatLevelSource.settings`), `LevelStem` (via `WorldDimensions`), and the
//! `SavedData` base — all pending units at the time of this wave. Where the
//! dependency type is genuinely unavailable the port keeps the class shape and
//! the faithful *portable* surface, and exposes the smallest typed seam for the
//! rest (the `RivetTodo` on each module names the owning unit/issue). The two
//! `ChunkGenerator` realizations follow the noisegen value-shell pattern: they
//! implement the `ChunkGenerator` trait for the nameable surface and carry the
//! world-touching lifecycle bodies as inherent methods whose full signatures
//! the owning `mc.world.level.chunk.generator` realization reconciles
//! (RivetTodo #185).

pub mod below_zero_retrogen;
pub mod debug_level_source;
pub mod flat_level_source;
pub mod geode_block_settings;
pub mod geode_crack_settings;
pub mod geode_layer_settings;
pub mod level_stem;
pub mod world_dimensions;
pub mod world_gen_settings;
pub mod world_options;
