//! Paper 26.2 `net.minecraft.world.InteractionHand` port.

use super::packets::InteractionPacketError;

/// `net.minecraft.world.InteractionHand` — declaration order matches Java ordinals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum InteractionHand {
    MainHand = 0,
    OffHand = 1,
}

impl InteractionHand {
    /// Converts a wire VarInt ordinal to `InteractionHand`.
    pub fn from_ordinal(ordinal: i32) -> Result<Self, InteractionPacketError> {
        match ordinal {
            0 => Ok(Self::MainHand),
            1 => Ok(Self::OffHand),
            other => Err(InteractionPacketError::InvalidHandOrdinal(other)),
        }
    }

    /// Java `Enum.ordinal()`.
    pub fn ordinal(self) -> i32 {
        self as i32
    }
}
