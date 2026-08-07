//! The tick-owned player/session value skeleton (Slice A of #101) — the
//! minimal `ServerPlayer` spawn info and the `PlayerList` `playersByUUID`
//! indices, both owned by the tick thread (OWNERSHIP.md "one owner: the tick
//! thread"; no `Arc<RwLock>` game state).
//!
//! Java source of truth: `working/Paper/.../server/players/PlayerList.java` and
//! `.../server/level/ServerPlayer.java`. This slice ports only the join-burst
//! surface: the spawn geometry the `placeNewPlayer` burst carries (position,
//! yaw/pitch, game type, offline profile), the login `playerId`, the owning
//! [`ConnectionId`], and the UUID↔ConnectionId lookup indices. The entity
//! surface (`ServerPlayer extends Player extends LivingEntity`, syncer, data
//! slots) is deferred with the entity unit — RivetTodo(#222) tracks entity
//! pairing; the `EntityId` type does not exist yet, so `player_id` is the
//! capture's raw login `playerId` int.

pub mod join;
pub mod play_sender;

use std::collections::HashMap;

use rivet_registry::core::{GameProfile, GameType, Vec3};
use rivet_util::mth::Uuid;

use crate::server::network::connection_id::ConnectionId;

/// `ServerPlayer` — the minimal join-burst value skeleton, owned by the tick
/// thread. Holds the spawn info the login/teleport/player-info burst encodes
/// plus the connection that owns the session.
#[derive(Debug, Clone)]
pub struct ServerPlayer {
    /// The owning connection (`PlayerList` keeps the reverse
    /// `playersByUUID`/`playersByConnection` indices; `ConnectionId` is the
    /// tick-side key).
    connection_id: ConnectionId,
    /// The offline `GameProfile` (name + uuid + properties) the login and
    /// `player_info_update` entries carry.
    profile: GameProfile,
    /// `Entity.getId()` — the login `playerId`. No `EntityId` value type exists
    /// yet (RivetTodo(#222)); the capture's raw int.
    player_id: i32,
    /// `position` — the spawn position (`PositionMoveRotation.position`).
    position: Vec3,
    /// `yRot`.
    yaw: f32,
    /// `xRot`.
    pitch: f32,
    /// `gameMode` — the game type the spawn-info and player-info update carry.
    game_type: GameType,
}

impl ServerPlayer {
    /// The record's canonical constructor.
    pub fn new(
        connection_id: ConnectionId,
        profile: GameProfile,
        player_id: i32,
        position: Vec3,
        yaw: f32,
        pitch: f32,
        game_type: GameType,
    ) -> Self {
        ServerPlayer {
            connection_id,
            profile,
            player_id,
            position,
            yaw,
            pitch,
            game_type,
        }
    }

    /// `ServerPlayer.connection` — the owning connection id.
    pub fn connection_id(&self) -> ConnectionId {
        self.connection_id
    }

    /// `ServerPlayer.getGameProfile()`.
    pub fn profile(&self) -> &GameProfile {
        &self.profile
    }

    /// `GameProfile.getId()`.
    pub fn uuid(&self) -> Uuid {
        self.profile.id()
    }

    /// `GameProfile.getName()`.
    pub fn name(&self) -> &str {
        self.profile.name()
    }

    /// `Entity.getId()` — the login `playerId`.
    pub fn player_id(&self) -> i32 {
        self.player_id
    }

    /// `position()` — the spawn position.
    pub fn position(&self) -> Vec3 {
        self.position
    }

    /// `getYRot()`.
    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    /// `getXRot()`.
    pub fn pitch(&self) -> f32 {
        self.pitch
    }

    /// `gameMode()`.
    pub fn game_type(&self) -> GameType {
        self.game_type
    }
}

/// `PlayerList.playersByUUID` (Spigot also keeps `playersByName`) — the
/// `ConnectionId`↔UUID lookup indices, owned by the tick thread. The one
/// direction maps the player's UUID to the owning connection so the network
/// path can target a session; the reverse resolves a connection's player.
#[derive(Debug, Default)]
pub struct PlayerIndices {
    by_uuid: HashMap<Uuid, ConnectionId>,
    by_connection: HashMap<ConnectionId, Uuid>,
}

impl PlayerIndices {
    /// `playersByUUID.put(player.getUUID(), player)` — register a session. The
    /// two maps stay a bijection: a UUID re-registered under a different
    /// connection drops its stale reverse entry, and a connection re-used by a
    /// different UUID drops the stale forward entry.
    pub fn insert(&mut self, uuid: Uuid, connection_id: ConnectionId) {
        if let Some(&old) = self.by_uuid.get(&uuid) {
            self.by_connection.remove(&old);
        }
        if let Some(&old) = self.by_connection.get(&connection_id) {
            self.by_uuid.remove(&old);
        }
        self.by_uuid.insert(uuid, connection_id);
        self.by_connection.insert(connection_id, uuid);
    }

    /// `playersByUUID.remove(player.getUUID())`.
    pub fn remove(&mut self, uuid: &Uuid) {
        if let Some(connection_id) = self.by_uuid.remove(uuid) {
            self.by_connection.remove(&connection_id);
        }
    }

    /// Resolve a player UUID to its owning connection.
    pub fn connection_for(&self, uuid: &Uuid) -> Option<ConnectionId> {
        self.by_uuid.get(uuid).copied()
    }

    /// Resolve a connection to its player UUID.
    pub fn uuid_for(&self, connection_id: ConnectionId) -> Option<Uuid> {
        self.by_connection.get(&connection_id).copied()
    }

    /// The number of registered sessions.
    pub fn len(&self) -> usize {
        self.by_uuid.len()
    }

    /// Whether any session is registered.
    pub fn is_empty(&self) -> bool {
        self.by_uuid.is_empty()
    }
}
