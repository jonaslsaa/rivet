//! `net.minecraft.world.level.chunk` — chunk wire-format structures (#108),
//! the #100 `LevelChunkSection`/`DataLayer` content layer, and the `#183`
//! `ChunkAccess` read spine.
//!
//! This module ports the pure `PalettedContainer`/`Palette`/`Strategy`/
//! `Configuration` value layer, the `LevelChunkSection` wire write/size/recalc
//! slice, the `DataLayer` light layer, the `PalettedContainerRO` read view +
//! `PalettedContainerFactory` factory (#230), the `chunk.support` leaf types
//! (`CarvingMask`/`BlockColumn`/`LightChunk`/`LightChunkGetter`/
//! `StructureAccess`), and the `#183` chunk-access read spine: the generic
//! `ChunkAccess` base, the concrete chunk values (`LevelChunk`/`ProtoChunk`/
//! `EmptyLevelChunk`/`ImposterProtoChunk`), the `UpgradeData` carrier, and the
//! `ChunkSource` provider seam. `storage` holds the `world.level.chunk.storage`
//! region-file foundation (issue #231) and the disjoint heightmap/light read
//! carriers from `SerializableChunkData` (issue #337).

pub mod block_column;
pub mod carving_mask;
pub mod chunk_access;
// STUB(mc.world.level.chunk.generator) — `ChunkGenerator`, the opaque
// parameter behind every feature placement; owned by the worldgen unit.
pub mod chunk_generator;
pub use chunk_generator::ChunkGenerator;
pub mod chunk_source;
pub mod configuration;
pub mod data_layer;
pub mod empty_level_chunk;
pub mod imposter_proto_chunk;
pub mod level_chunk;
pub mod level_chunk_section;
pub mod light_chunk;
pub mod light_chunk_getter;
pub mod moonrise_short_list;
pub mod palette;
pub mod paletted_container;
pub mod paletted_container_factory;
pub mod proto_chunk;
pub mod status;
pub mod storage;
pub mod strategy;
pub mod structure_access;
pub mod upgrade_data;
