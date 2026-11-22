//! Bounded safe Rust capability facades over authoritative M3 state (#508).
//!
//! This module defines opaque ID-backed read snapshots and typed tick-thread
//! mutation commands for selected world, entity, player, item, inventory, and
//! menu capabilities.
//!
//! ## Ownership boundary (`OWNERSHIP.md` / #508)
//!
//! - No arena pointer, retained caller buffer, Rust reference into game state,
//!   or foreign exception crosses this boundary.
//! - Internal struct layouts are never frozen as a public ABI; plugins receive
//!   value-copy snapshots (`WorldSnapshot`, `EntitySnapshot`, `PlayerSnapshot`,
//!   `ItemStackSnapshot`, `InventorySnapshot`, `MenuSnapshot`) and opaque
//!   generational handles (`Handle<EntityMarker>`, `Handle<PlayerMarker>`,
//!   `Handle<InventoryMarker>`, `Handle<MenuMarker>`).
//! - All reads and mutations execute synchronously on the single tick thread;
//!   no `Arc<RwLock<_>>` game state is permitted.
//! - Missing M3 providers fail closed with [`FacadeError::ProviderUnverified`]
//!   rather than silently succeeding or falling back.

pub mod batch;
pub mod entity;
pub mod inventory;
pub mod player;
pub mod world;

use std::fmt;

use super::handle::HandleError;
use super::version::Capability;

pub use batch::{
    BatchMode, BatchReceipt, CommandOutcome, NativeFacadeView, SyntheticWorldState, TickCommand,
    TickCommandBatch,
};
pub use entity::{EntityCommand, EntityHandle, EntityMarker, EntitySnapshot};
pub use inventory::{
    InventoryCommand, InventoryHandle, InventoryKind, InventoryMarker, InventorySnapshot,
    ItemStackSnapshot, MenuCommand, MenuHandle, MenuMarker, MenuSnapshot,
};
pub use player::{GameMode, PlayerCommand, PlayerHandle, PlayerMarker, PlayerSnapshot};
pub use world::{BlockPos, BlockStateSnapshot, ChunkPos, WorldCommand, WorldId, WorldSnapshot};

/// Explicit failure taxonomy for capability facade reads and tick-thread commands.
///
/// Every stale, unloaded, dead, offline, unauthorized, off-thread, or
/// unverified-provider condition is surfaced as a typed error rather than
/// silently ignored or aliased.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum FacadeError {
    /// Calling plugin was not granted the required capability.
    CapabilityDenied(Capability),
    /// Operation was invoked from a thread other than the authoritative tick thread.
    NotOnTickThread,
    /// Target world does not exist or has been unloaded.
    WorldNotFound(WorldId),
    /// Target chunk is not currently loaded in the world.
    ChunkUnloaded { world: WorldId, chunk: ChunkPos },
    /// Target block Y coordinate is outside the world's `[min_y, min_y + height)` range.
    OutOfWorldBounds {
        world: WorldId,
        pos: BlockPos,
        min_y: i32,
        max_y_exclusive: i32,
    },
    /// Entity handle is invalid or refers to a revoked/reused slot.
    EntityStale(HandleError),
    /// Entity slot is known, but the entity is dead or discarded.
    EntityDead(EntityHandle),
    /// Player handle is invalid or refers to a revoked/reused slot.
    PlayerStale(HandleError),
    /// Player is no longer online.
    PlayerOffline(PlayerHandle),
    /// Inventory handle is invalid or refers to a closed/revoked container.
    InventoryStale(HandleError),
    /// Menu handle is invalid or refers to a revoked menu slot.
    MenuStale(HandleError),
    /// Menu is already closed.
    MenuClosed(MenuHandle),
    /// Requested inventory slot index is out of bounds for the container size.
    SlotOutOfBounds { slot: usize, size: usize },
    /// Item stack count is invalid (`< 0` or `> max_stack_size`).
    InvalidItemCount { count: i32, max_stack_size: i32 },
    /// Health or coordinate value is non-finite or outside valid bounds.
    InvalidValue { field: &'static str, detail: String },
    /// Live M3 provider seam is not wired or verified; fails closed as UNVERIFIED.
    ProviderUnverified {
        provider: &'static str,
        reason: String,
    },
}

impl fmt::Display for FacadeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapabilityDenied(cap) => {
                write!(f, "capability {cap:?} not granted to caller")
            }
            Self::NotOnTickThread => {
                write!(
                    f,
                    "facade operation must run on the authoritative tick thread"
                )
            }
            Self::WorldNotFound(id) => write!(f, "world {id} not found or unloaded"),
            Self::ChunkUnloaded { world, chunk } => {
                write!(
                    f,
                    "chunk ({}, {}) is not loaded in {world}",
                    chunk.x, chunk.z
                )
            }
            Self::OutOfWorldBounds {
                world,
                pos,
                min_y,
                max_y_exclusive,
            } => write!(
                f,
                "block position ({}, {}, {}) out of bounds for {world} [{min_y}, {max_y_exclusive})",
                pos.x, pos.y, pos.z
            ),
            Self::EntityStale(err) => write!(f, "stale entity handle: {err}"),
            Self::EntityDead(handle) => write!(f, "entity {handle:?} is dead or discarded"),
            Self::PlayerStale(err) => write!(f, "stale player handle: {err}"),
            Self::PlayerOffline(handle) => write!(f, "player {handle:?} is offline"),
            Self::InventoryStale(err) => write!(f, "stale inventory handle: {err}"),
            Self::MenuStale(err) => write!(f, "stale menu handle: {err}"),
            Self::MenuClosed(handle) => write!(f, "menu {handle:?} is closed"),
            Self::SlotOutOfBounds { slot, size } => {
                write!(f, "inventory slot {slot} out of bounds for size {size}")
            }
            Self::InvalidItemCount {
                count,
                max_stack_size,
            } => write!(
                f,
                "invalid item count {count} (expected 0..={max_stack_size})"
            ),
            Self::InvalidValue { field, detail } => {
                write!(f, "invalid value for {field}: {detail}")
            }
            Self::ProviderUnverified { provider, reason } => {
                write!(f, "UNVERIFIED M3 provider {provider}: {reason}")
            }
        }
    }
}

impl std::error::Error for FacadeError {}
