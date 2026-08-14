//! `net.minecraft.world.level.border` — the world-border package (the
//! `mc.world.level.border` manifest unit, wave 3).
//!
//! Ports the package's four Java files:
//!
//! - [`WorldBorder`] (`world_border.rs`) — the border itself: the `Settings`
//!   record (nine-field DFU codec), the `StaticBorderExtent`/`MovingBorderExtent`
//!   sum type, the bounds/distance/collision surface, and the `SavedData`/
//!   `SavedDataType` (`TYPE`) supertype handled by the `mc.world.level.saveddata`
//!   unit.
//! - [`BorderStatus`] (`border_status.rs`) — the movement status enum with its
//!   debug color.
//! - [`BorderChangeListener`] (`border_change_listener.rs`) — the listener
//!   trait notified on every mutation.
//! - `package-info.java` (`@NullMarked`) — no code; the port is fully-typed.
//!
//! Java source root:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! border/`.
//!
//! `WorldBorder extends SavedData`; the `SavedData`/`SavedDataType`/
//! `DataFixTypes` supertype surface comes from the merged
//! `mc.world.level.saveddata` unit (`crate::level::saveddata`), with
//! `WorldBorder.TYPE` a `LazyLock` static pinned to `NbtOps`.
//!
//! Cross-unit STUB seams (pending units; `// STUB(unit-id)`):
//! [`shapes`] (`VoxelShape`/`Shapes`/`BooleanOp` — `mc.world.phys.shapes`),
//! [`entity_stub`] (`Entity` — `mc.world.entity`). Paper's plugin-event and
//! server-tick additions defer (`RivetTodo(#417)` in `world_border`).

pub mod border_change_listener;
pub mod border_status;
pub mod entity_stub;
pub mod shapes;
pub mod world_border;

pub use border_change_listener::BorderChangeListener;
pub use border_status::BorderStatus;
pub use world_border::{Settings, WorldBorder};
