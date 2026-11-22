//! Paper `net.minecraft.world.inventory.ClickAction`.

/// Paper `net.minecraft.world.inventory.ClickAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClickAction {
    Primary,
    Secondary,
}
