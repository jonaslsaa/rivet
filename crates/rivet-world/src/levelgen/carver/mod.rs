//! `net.minecraft.world.level.levelgen.carver` — the world carver unit
//! (26.2).
//!
//! Owned by the `mc.world.level.levelgen.carver` manifest unit (26.2):
//! `WorldCarver.java`, `ConfiguredWorldCarver.java`, `CarverConfiguration.java`,
//! `CarvingContext.java`, `CaveWorldCarver.java`, `CaveCarverConfiguration.java`,
//! `NetherWorldCarver.java`, `CanyonWorldCarver.java`,
//! `CanyonCarverConfiguration.java`, `CarverDebugSettings.java`,
//! `package-info.java`.
//!
//! This is the #180 carver-algorithm port, built on the #306
//! `ConfiguredWorldCarver` type shell:
//!
//! - `CarverConfiguration` — the `WC extends CarverConfiguration` bound as a
//!   trait (the `CarverConfigurationBase` value + the 7 accessors; Java's
//!   class field surface, OWNERSHIP.md — no inheritance: the concrete
//!   sub-configurations embed the base and implement the trait by delegation).
//! - `WorldCarverId` — the `BuiltInRegistries.CARVER` element identity, with
//!   the `CAVE`/`NETHER_CAVE`/`CANYON` constants (ids 0/1/2, registration
//!   order).
//! - `WorldCarverBehavior<C>` — the abstract base's overridable behavior:
//!   `isStartChunk`, `getRange` (default 4), the `carve` abstract plus the
//!   protected `carveEllipsoid`/`carveBlock`/`canReplaceBlock` helpers and the
//!   `CarveSkipChecker` interface.
//! - `CarvingContext` — the `WorldGenerationContext` subclass carrying the
//!   `RandomState` and the `topMaterial` surface seam.
//! - `ConfiguredWorldCarver<C>` — the record (`WorldCarver<WC> worldCarver, WC
//!   config`), with `isStartChunk` and `carve` dispatching through the id
//!   hubs.
//! - The concrete carvers (`CaveWorldCarver`/`NetherWorldCarver`/
//!   `CanyonWorldCarver`) and their configurations (`CaveCarverConfiguration`/
//!   `CanyonCarverConfiguration`), plus `CarverDebugSettings`.
//!
//! Seams (marked `RivetTodo`):
//! - RivetTodo(#399): the `CarveChunk` block surface (`getBlockState`/
//!   `setBlockState`/`isUpgrading`/`markPosForPostProcessing`/`getPos`) and
//!   the `CarvingContext.topMaterial` surface-system call are the smallest
//!   typed seams the #399 block surface / surface unit bind; no state is
//!   fabricated in this unit.
//! - RivetTodo(#126): the dispatch codecs (`DIRECT_CODEC`/`CODEC`/`LIST_CODEC`,
//!   `WorldCarver.configuredCodec`/`configured`, `CarvingContext.registryAccess`)
//!   defer with the by-name codec surface.
//! - `SharedConstants.debugVoidTerrain`/`DEBUG_CARVERS` are the
//!   `rivet_core::shared_constants` pins.

pub mod canyon_carver_configuration;
pub mod canyon_world_carver;
pub mod carver_configuration;
pub mod carver_debug_settings;
pub mod carving_context;
pub mod cave_carver_configuration;
pub mod cave_world_carver;
pub mod configured_world_carver;
pub mod world_carver;

pub use canyon_carver_configuration::{
    CanyonCarverConfiguration, CanyonShapeConfiguration, canyon_carver_configuration_codec,
};
pub use carver_configuration::{CarverConfiguration, CarverConfigurationBase};
pub use carver_debug_settings::{CarverDebugSettings, carver_debug_settings_codec};
pub use carving_context::CarvingContext;
pub use cave_carver_configuration::{CaveCarverConfiguration, cave_carver_configuration_codec};
pub use cave_world_carver::{CaveCarverHooks, CaveWorldCarver, NetherWorldCarver};
pub use configured_world_carver::{ConfiguredWorldCarver, ConfiguredWorldCarverErased};
pub use world_carver::{
    CarveChunk, CarveSkipChecker, ClosureSkipChecker, WorldCarverBehavior, WorldCarverId,
    can_reach, carver_carve, carver_is_start_chunk,
};
