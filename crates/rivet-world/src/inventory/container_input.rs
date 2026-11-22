//! Paper `net.minecraft.world.inventory.ContainerInput`.

/// Paper `net.minecraft.world.inventory.ContainerInput`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ContainerInput {
    Pickup = 0,
    QuickMove = 1,
    Swap = 2,
    Clone = 3,
    Throw = 4,
    QuickCraft = 5,
    PickupAll = 6,
}

impl ContainerInput {
    /// Wire/protocol id matching Paper's `ContainerInput.id()`.
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// Decode a Paper `ContainerInput` id.
    pub const fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Pickup),
            1 => Some(Self::QuickMove),
            2 => Some(Self::Swap),
            3 => Some(Self::Clone),
            4 => Some(Self::Throw),
            5 => Some(Self::QuickCraft),
            6 => Some(Self::PickupAll),
            _ => None,
        }
    }
}
