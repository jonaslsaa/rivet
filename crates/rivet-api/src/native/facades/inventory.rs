//! Item, inventory, and container menu capability facade types and commands (#508).
//!
//! Mirrors Paper's `ItemStack`, `Inventory`, and `InventoryView` / `AbstractContainerMenu`
//! boundaries without exposing mutable slot arrays or internal component maps.

use std::collections::BTreeMap;

use super::super::handle::Handle;
use super::FacadeError;
use super::player::PlayerHandle;

/// Marker type for generational handles referencing inventories.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct InventoryMarker;

/// Marker type for generational handles referencing open container menus.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct MenuMarker;

/// Opaque generational handle to an authoritative inventory container.
pub type InventoryHandle = Handle<InventoryMarker>;

/// Opaque generational handle to an open player container menu view.
pub type MenuHandle = Handle<MenuMarker>;

/// Inventory container archetype.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub enum InventoryKind {
    Player,
    Chest,
    Furnace,
    Hopper,
    Custom,
}

/// Value-copy snapshot of an item stack.
///
/// Mirrors Paper/Minecraft's empty-stack rule: a stack with `item_id == "minecraft:air"`
/// or `count <= 0` is treated as empty.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ItemStackSnapshot {
    pub item_id: String,
    pub count: i32,
    pub max_stack_size: i32,
    pub custom_name: Option<String>,
    pub components: BTreeMap<String, String>,
}

impl ItemStackSnapshot {
    /// Vanilla default max stack size (`Item.DEFAULT_MAX_STACK_SIZE = 64`).
    pub const DEFAULT_MAX_STACK_SIZE: i32 = 64;

    /// Constructs a validated item stack snapshot.
    pub fn new(
        item_id: impl Into<String>,
        count: i32,
        max_stack_size: i32,
    ) -> Result<Self, FacadeError> {
        if max_stack_size <= 0 || max_stack_size > 99 {
            return Err(FacadeError::InvalidValue {
                field: "max_stack_size",
                detail: format!("must be in 1..=99, got {max_stack_size}"),
            });
        }
        if count < 0 || count > max_stack_size {
            return Err(FacadeError::InvalidItemCount {
                count,
                max_stack_size,
            });
        }
        Ok(Self {
            item_id: item_id.into(),
            count,
            max_stack_size,
            custom_name: None,
            components: BTreeMap::new(),
        })
    }

    /// Returns whether this stack is empty under Minecraft/Paper semantics.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item_id == "minecraft:air"
    }
}

/// Value-copy snapshot of an authoritative inventory.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct InventorySnapshot {
    pub handle: InventoryHandle,
    pub kind: InventoryKind,
    pub size: usize,
    pub slots: Vec<Option<ItemStackSnapshot>>,
}

/// Value-copy snapshot of an open container menu view.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct MenuSnapshot {
    pub handle: MenuHandle,
    pub viewer: PlayerHandle,
    pub inventory: InventoryHandle,
    pub title: String,
    pub container_id: i32,
    pub open: bool,
}

/// Typed tick-thread mutation commands targeting inventory state.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum InventoryCommand {
    /// Sets `slot` in `inventory` to `item` (or clears the slot when `item` is
    /// `None` or `item.is_empty()`).
    SetSlot {
        inventory: InventoryHandle,
        slot: usize,
        item: Option<ItemStackSnapshot>,
    },
    /// Clears a single slot in `inventory`.
    ClearSlot {
        inventory: InventoryHandle,
        slot: usize,
    },
    /// Clears every slot in `inventory`.
    ClearAll { inventory: InventoryHandle },
}

/// Typed tick-thread mutation commands targeting container menu state.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum MenuCommand {
    /// Opens `inventory` as a menu for an online `player`.
    /// If the player already has an open menu, it is closed first.
    OpenMenu {
        player: PlayerHandle,
        inventory: InventoryHandle,
        title: String,
    },
    /// Closes an open menu. Fails with `MenuClosed` if already closed or
    /// `MenuStale` if revoked.
    CloseMenu { menu: MenuHandle },
}
