//! Player capability facade types and tick-thread commands (#508).
//!
//! Mirrors Paper's `org.bukkit.entity.Player` / `ServerPlayer` boundary using
//! generational [`PlayerHandle`] values with explicit offline/stale semantics.

use super::super::handle::Handle;
use super::entity::EntityHandle;
use super::inventory::MenuHandle;
use super::world::WorldId;

/// Marker type for generational handles referencing players.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct PlayerMarker;

/// Opaque generational handle to a connected player.
pub type PlayerHandle = Handle<PlayerMarker>;

/// Minecraft/Paper game mode matching `net.minecraft.world.level.GameType` IDs.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
#[repr(u8)]
pub enum GameMode {
    Survival = 0,
    Creative = 1,
    Adventure = 2,
    Spectator = 3,
}

impl GameMode {
    #[must_use]
    pub const fn id(self) -> i32 {
        self as i32
    }

    #[must_use]
    pub const fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Survival),
            1 => Some(Self::Creative),
            2 => Some(Self::Adventure),
            3 => Some(Self::Spectator),
            _ => None,
        }
    }
}

/// Value-copy snapshot of a player's observable state.
#[derive(Clone, PartialEq, Debug)]
pub struct PlayerSnapshot {
    pub handle: PlayerHandle,
    pub entity: EntityHandle,
    pub uuid: [u8; 16],
    pub name: String,
    pub world: WorldId,
    pub game_mode: GameMode,
    pub health: f32,
    pub max_health: f32,
    pub food_level: i32,
    pub online: bool,
    pub open_menu: Option<MenuHandle>,
}

/// Typed tick-thread mutation commands targeting player state.
#[derive(Clone, PartialEq, Debug)]
pub enum PlayerCommand {
    /// Changes an online player's game mode.
    SetGameMode {
        handle: PlayerHandle,
        mode: GameMode,
    },
    /// Sets an online player's health (`0.0..=max_health`, must be finite).
    SetHealth { handle: PlayerHandle, health: f32 },
    /// Sets an online player's food level (`0..=20`, matching vanilla `FoodData`).
    SetFoodLevel {
        handle: PlayerHandle,
        food_level: i32,
    },
    /// Queues a system chat message to an online player.
    SendMessage {
        handle: PlayerHandle,
        message: String,
    },
    /// Closes the player's currently open menu, if any.
    CloseOpenMenu { handle: PlayerHandle },
}
