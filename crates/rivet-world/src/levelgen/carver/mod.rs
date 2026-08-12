//! `net.minecraft.world.level.levelgen.carver` — the world carver type shell.
//!
//! Owned by the `mc.world.level.levelgen.carver` manifest unit (26.2):
//! `WorldCarver.java`, `ConfiguredWorldCarver.java`, `CarverConfiguration.java`,
//! `CarvingContext.java`, `CaveWorldCarver.java`, `CaveCarverConfiguration.java`,
//! `NetherWorldCarver.java`, `CanyonWorldCarver.java`,
//! `CanyonCarverConfiguration.java`, `CarverDebugSettings.java`,
//! `package-info.java`.
//!
//! This is the #306 `ConfiguredWorldCarver` *type shell* — the smallest
//! faithful slice that unblocks #180 (the carver algorithm). It ports the
//! record's type skeleton and the identity/behavior split:
//!
//! - `CarverConfiguration` — the `WC extends CarverConfiguration` bound as a
//!   marker trait (Java's class carries only field + codec surface, all
//!   deferred).
//! - `WorldCarverId` — the `BuiltInRegistries.CARVER` element identity.
//! - `WorldCarverBehavior<C>` — the abstract base's overridable behavior
//!   (`isStartChunk`, `getRange` defaulting to 4).
//! - `ConfiguredWorldCarver<C>` — the record (`WorldCarver<WC> worldCarver, WC
//!   config`), with `isStartChunk` dispatching through `carver_is_start_chunk`.
//!
//! The full #180 algorithm is explicitly NOT here:
//! RivetTodo(#180): `ConfiguredWorldCarver.carve` and `WorldCarver.carve` (the
//! abstract behavior plus the protected `carveEllipsoid`/`carveBlock`/
//! `getCarveState`/`canReplaceBlock`/`canReach` helpers and the
//! `CarveSkipChecker` interface) are deferred — the signatures need
//! `CarvingContext` (a `WorldGenerationContext` subclass holding
//! `RegistryAccess`/`NoiseChunk`/`RandomState`/`SurfaceRules` from the
//! noisegen/surface units), `Aquifer`, `Function<BlockPos, Holder<Biome>>` and
//! `ChunkAccess`'s block surface. The concrete carvers (`CaveWorldCarver`/
//! `NetherWorldCarver`/`CanyonWorldCarver`), their configurations
//! (`CaveCarverConfiguration`/`CanyonCarverConfiguration`), `CarverDebugSettings`,
//! the `CAVE`/`NETHER_CAVE`/`CANYON` `BuiltInRegistries.CARVER` registrations,
//! and the `SharedConstants.debugVoidTerrain` gate land with the algorithm.
//! RivetTodo(#126): the dispatch codecs (`DIRECT_CODEC`/`CODEC`/`LIST_CODEC`,
//! `WorldCarver.configuredCodec`/`configured`) defer with the by-name codec
//! surface.
//! RivetTodo(#228): `canReplaceBlock` reads `configuration.replaceable`
//! (`HolderSet<Block>`) and `carveBlock` writes `BlockState`s — the block
//! slice owns those types.
//!
//! The erased wildcard `ConfiguredWorldCarver<?>` (stored by
//! `BiomeGenerationSettings.carvers` and the codecs) is NOT in this shell: the
//! `BiomeGenerationSettings` holder and the codecs that consume it are not
//! ported, so there is no consumer yet — it lands with the biome unit / #180.

pub mod carver_configuration;
pub mod configured_world_carver;
pub mod world_carver;

pub use carver_configuration::CarverConfiguration;
pub use configured_world_carver::{ConfiguredWorldCarver, ConfiguredWorldCarverErased};
pub use world_carver::{WorldCarverBehavior, WorldCarverId, carver_is_start_chunk};
