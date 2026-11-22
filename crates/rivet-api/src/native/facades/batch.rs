//! Tick-thread command batching, capability-checked facade views, and
//! deterministic single-threaded synthetic state (#508).
//!
//! Enforces:
//! - Tick-thread affinity (`ThreadId` check; off-thread access fails closed with
//!   [`FacadeError::NotOnTickThread`]).
//! - Capability checks on every read and write command.
//! - Explicit stale / unloaded / dead / offline semantics.
//! - Both [`BatchMode::Atomic`] (all-or-nothing rollback on any failure) and
//!   [`BatchMode::ContinueOnError`] (per-command receipts in deterministic order).

use std::collections::{BTreeMap, BTreeSet};
use std::thread::{self, ThreadId};

use super::super::handle::{HandleTable, PluginId};
use super::super::version::{Capability, CapabilitySet};
use super::FacadeError;
use super::entity::{EntityCommand, EntityHandle, EntityMarker, EntitySnapshot};
use super::inventory::{
    InventoryCommand, InventoryHandle, InventoryKind, InventoryMarker, InventorySnapshot,
    ItemStackSnapshot, MenuCommand, MenuHandle, MenuMarker, MenuSnapshot,
};
use super::player::{GameMode, PlayerCommand, PlayerHandle, PlayerMarker, PlayerSnapshot};
use super::world::{BlockPos, BlockStateSnapshot, ChunkPos, WorldCommand, WorldId, WorldSnapshot};

/// System owner ID used internally for host-owned authoritative state tables.
const HOST_OWNER: PluginId = PluginId::from_raw(0);

/// Unified typed tick-thread mutation command across all native capability facades.
#[derive(Clone, PartialEq, Debug)]
pub enum TickCommand {
    World(WorldCommand),
    Entity(EntityCommand),
    Player(PlayerCommand),
    Inventory(InventoryCommand),
    Menu(MenuCommand),
}

impl TickCommand {
    /// Returns the capability required to execute this command.
    #[must_use]
    pub const fn required_capability(&self) -> Capability {
        match self {
            Self::World(_) => Capability::WorldWrite,
            Self::Entity(_) => Capability::EntityWrite,
            Self::Player(_) => Capability::PlayerWrite,
            Self::Inventory(_) => Capability::InventoryWrite,
            Self::Menu(_) => Capability::MenuWrite,
        }
    }
}

/// Execution semantics for a [`TickCommandBatch`].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum BatchMode {
    /// All commands succeed or none take effect (rolled back on first error).
    Atomic,
    /// Commands execute sequentially in submission order; failures are recorded
    /// per index without aborting subsequent commands.
    ContinueOnError,
}

/// Ordered batch of typed tick-thread commands submitted by a plugin.
#[derive(Clone, PartialEq, Debug)]
pub struct TickCommandBatch {
    pub mode: BatchMode,
    pub commands: Vec<TickCommand>,
}

impl TickCommandBatch {
    #[must_use]
    pub const fn new(mode: BatchMode) -> Self {
        Self {
            mode,
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, command: TickCommand) -> &mut Self {
        self.commands.push(command);
        self
    }

    #[must_use]
    pub fn with(mut self, command: TickCommand) -> Self {
        self.commands.push(command);
        self
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }
}

/// Outcome of a single command in a [`TickCommandBatch`].
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum CommandOutcome {
    Applied,
    OpenedMenu(MenuHandle),
    Failed(FacadeError),
}

/// Deterministic execution receipt for a [`TickCommandBatch`].
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct BatchReceipt {
    pub mode: BatchMode,
    pub committed: bool,
    pub outcomes: Vec<CommandOutcome>,
}

impl BatchReceipt {
    #[must_use]
    pub fn succeeded_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o, CommandOutcome::Applied | CommandOutcome::OpenedMenu(_)))
            .count()
    }

    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o, CommandOutcome::Failed(_)))
            .count()
    }
}

#[derive(Clone)]
struct WorldRecord {
    id: WorldId,
    name: String,
    dimension_key: String,
    min_y: i32,
    height: i32,
    game_time: i64,
    day_time: i64,
    loaded_chunks: BTreeSet<ChunkPos>,
    blocks: BTreeMap<BlockPos, BlockStateSnapshot>,
}

impl WorldRecord {
    fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            id: self.id,
            name: self.name.clone(),
            dimension_key: self.dimension_key.clone(),
            loaded_chunks: self.loaded_chunks.len(),
            game_time: self.game_time,
            day_time: self.day_time,
            min_y: self.min_y,
            height: self.height,
        }
    }

    const fn max_y_exclusive(&self) -> i32 {
        self.min_y.wrapping_add(self.height)
    }

    fn validate_block_pos(&self, pos: BlockPos) -> Result<(), FacadeError> {
        let max_y_exclusive = self.max_y_exclusive();
        if pos.y < self.min_y || pos.y >= max_y_exclusive {
            return Err(FacadeError::OutOfWorldBounds {
                world: self.id,
                pos,
                min_y: self.min_y,
                max_y_exclusive,
            });
        }
        let chunk = pos.chunk_pos();
        if !self.loaded_chunks.contains(&chunk) {
            return Err(FacadeError::ChunkUnloaded {
                world: self.id,
                chunk,
            });
        }
        Ok(())
    }
}

#[derive(Clone)]
struct EntityRecord {
    world: WorldId,
    entity_type: String,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    alive: bool,
    custom_name: Option<String>,
}

#[derive(Clone)]
struct PlayerRecord {
    entity: EntityHandle,
    uuid: [u8; 16],
    name: String,
    game_mode: GameMode,
    health: f32,
    max_health: f32,
    food_level: i32,
    online: bool,
    open_menu: Option<MenuHandle>,
    chat_log: Vec<String>,
}

#[derive(Clone)]
struct InventoryRecord {
    kind: InventoryKind,
    slots: Vec<Option<ItemStackSnapshot>>,
}

#[derive(Clone)]
struct MenuRecord {
    viewer: PlayerHandle,
    inventory: InventoryHandle,
    title: String,
    container_id: i32,
    open: bool,
}

/// Single-threaded, lock-free synthetic authoritative state for native facade
/// conformance testing (#508).
///
/// Mirrors the single tick-thread ownership model in `OWNERSHIP.md` without
/// `Arc<RwLock<_>>` or leaked arena references.
pub struct SyntheticWorldState {
    tick_thread: ThreadId,
    next_world_id: u32,
    next_container_id: i32,
    worlds: BTreeMap<WorldId, WorldRecord>,
    entities: HandleTable<EntityMarker, EntityRecord>,
    entity_handles: BTreeSet<EntityHandle>,
    players: HandleTable<PlayerMarker, PlayerRecord>,
    player_handles: BTreeSet<PlayerHandle>,
    inventories: HandleTable<InventoryMarker, InventoryRecord>,
    menus: HandleTable<MenuMarker, MenuRecord>,
    menu_handles: BTreeSet<MenuHandle>,
    live_m3_providers: BTreeSet<&'static str>,
}

impl Default for SyntheticWorldState {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntheticWorldState {
    /// Creates a new synthetic authoritative state bound to the current thread
    /// as its tick thread.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tick_thread: thread::current().id(),
            next_world_id: 1,
            next_container_id: 1,
            worlds: BTreeMap::new(),
            entities: HandleTable::new(),
            entity_handles: BTreeSet::new(),
            players: HandleTable::new(),
            player_handles: BTreeSet::new(),
            inventories: HandleTable::new(),
            menus: HandleTable::new(),
            menu_handles: BTreeSet::new(),
            live_m3_providers: BTreeSet::new(),
        }
    }

    fn ensure_tick_thread(&self) -> Result<(), FacadeError> {
        if thread::current().id() != self.tick_thread {
            return Err(FacadeError::NotOnTickThread);
        }
        Ok(())
    }

    /// Marks an M3 provider seam as verified for live facade queries.
    pub fn register_verified_provider(&mut self, provider: &'static str) {
        self.live_m3_providers.insert(provider);
    }

    /// Checks whether a required M3 provider seam is verified; fails closed with
    /// [`FacadeError::ProviderUnverified`] when absent.
    pub fn require_provider(&self, provider: &'static str) -> Result<(), FacadeError> {
        self.ensure_tick_thread()?;
        if !self.live_m3_providers.contains(provider) {
            return Err(FacadeError::ProviderUnverified {
                provider,
                reason: "M3 provider seam is not registered or verified on this host".to_owned(),
            });
        }
        Ok(())
    }

    /// Creates a new world with the given vertical bounds (`min_y`, `height`).
    pub fn create_world(
        &mut self,
        name: impl Into<String>,
        dimension_key: impl Into<String>,
        min_y: i32,
        height: i32,
    ) -> WorldId {
        let id = WorldId::from_raw(self.next_world_id);
        self.next_world_id = self.next_world_id.saturating_add(1);
        self.worlds.insert(
            id,
            WorldRecord {
                id,
                name: name.into(),
                dimension_key: dimension_key.into(),
                min_y,
                height,
                game_time: 0,
                day_time: 0,
                loaded_chunks: BTreeSet::new(),
                blocks: BTreeMap::new(),
            },
        );
        id
    }

    /// Unloads a world, disconnecting any players and discarding all entities residing in it.
    pub fn unload_world(&mut self, world: WorldId) -> bool {
        let removed = self.worlds.remove(&world).is_some();
        if removed {
            let players_in_world: Vec<PlayerHandle> = self
                .player_handles
                .iter()
                .copied()
                .filter(|p| {
                    self.players.get(*p, None).is_ok_and(|rec| {
                        self.entities
                            .get(rec.entity, None)
                            .is_ok_and(|e| e.world == world)
                    })
                })
                .collect();
            for player in players_in_world {
                let _ = self.disconnect_player(player);
            }
            let to_discard: Vec<EntityHandle> = self
                .entity_handles
                .iter()
                .copied()
                .filter(|h| {
                    self.entities
                        .get(*h, None)
                        .is_ok_and(|rec| rec.world == world)
                })
                .collect();
            for handle in to_discard {
                let _ = self.discard_entity_internal(handle);
            }
        }
        removed
    }

    /// Marks a chunk as loaded in `world`.
    pub fn load_chunk(&mut self, world: WorldId, chunk: ChunkPos) -> Result<(), FacadeError> {
        let rec = self
            .worlds
            .get_mut(&world)
            .ok_or(FacadeError::WorldNotFound(world))?;
        rec.loaded_chunks.insert(chunk);
        Ok(())
    }

    /// Marks a chunk as unloaded in `world`.
    pub fn unload_chunk(&mut self, world: WorldId, chunk: ChunkPos) -> Result<bool, FacadeError> {
        let rec = self
            .worlds
            .get_mut(&world)
            .ok_or(FacadeError::WorldNotFound(world))?;
        Ok(rec.loaded_chunks.remove(&chunk))
    }

    /// Spawns a live entity in `world` and returns its generational handle.
    pub fn spawn_entity(
        &mut self,
        world: WorldId,
        entity_type: impl Into<String>,
        x: f64,
        y: f64,
        z: f64,
    ) -> Result<EntityHandle, FacadeError> {
        if !self.worlds.contains_key(&world) {
            return Err(FacadeError::WorldNotFound(world));
        }
        validate_finite_coords(x, y, z, 0.0, 0.0)?;
        let handle = self
            .entities
            .insert(
                HOST_OWNER,
                EntityRecord {
                    world,
                    entity_type: entity_type.into(),
                    x,
                    y,
                    z,
                    yaw: 0.0,
                    pitch: 0.0,
                    alive: true,
                    custom_name: None,
                },
            )
            .map_err(FacadeError::EntityStale)?;
        self.entity_handles.insert(handle);
        Ok(handle)
    }

    /// Marks an entity as dead (kept in table for `EntityDead` checks until removed).
    pub fn kill_entity(&mut self, handle: EntityHandle) -> Result<(), FacadeError> {
        let rec = self
            .entities
            .get_mut(handle, None)
            .map_err(FacadeError::EntityStale)?;
        rec.alive = false;
        Ok(())
    }

    /// Removes an entity from the table, bumping its slot generation so the
    /// handle becomes `EntityStale` immediately.
    pub fn remove_entity(&mut self, handle: EntityHandle) -> Result<(), FacadeError> {
        self.discard_entity_internal(handle)
    }

    fn discard_entity_internal(&mut self, handle: EntityHandle) -> Result<(), FacadeError> {
        self.entities
            .remove(handle, None)
            .map_err(FacadeError::EntityStale)?;
        self.entity_handles.remove(&handle);
        Ok(())
    }

    /// Connects a new player backed by a spawned `minecraft:player` entity.
    pub fn connect_player(
        &mut self,
        world: WorldId,
        uuid: [u8; 16],
        name: impl Into<String>,
        game_mode: GameMode,
    ) -> Result<PlayerHandle, FacadeError> {
        let entity = self.spawn_entity(world, "minecraft:player", 0.0, 64.0, 0.0)?;
        let handle = self
            .players
            .insert(
                HOST_OWNER,
                PlayerRecord {
                    entity,
                    uuid,
                    name: name.into(),
                    game_mode,
                    health: 20.0,
                    max_health: 20.0,
                    food_level: 20,
                    online: true,
                    open_menu: None,
                    chat_log: Vec::new(),
                },
            )
            .map_err(FacadeError::PlayerStale)?;
        self.player_handles.insert(handle);
        Ok(handle)
    }

    /// Disconnects a player: marks them offline, closes any open menu, and
    /// discards their backing entity.
    pub fn disconnect_player(&mut self, handle: PlayerHandle) -> Result<(), FacadeError> {
        let (entity, open_menu) = {
            let rec = self
                .players
                .get_mut(handle, None)
                .map_err(FacadeError::PlayerStale)?;
            rec.online = false;
            (rec.entity, rec.open_menu.take())
        };
        if let Some(menu) = open_menu
            && let Ok(menu_rec) = self.menus.get_mut(menu, None)
        {
            menu_rec.open = false;
        }
        let _ = self.discard_entity_internal(entity);
        Ok(())
    }

    /// Removes a disconnected player's handle slot, bumping its generation.
    pub fn purge_player(&mut self, handle: PlayerHandle) -> Result<(), FacadeError> {
        let _ = self.disconnect_player(handle);
        self.players
            .remove(handle, None)
            .map_err(FacadeError::PlayerStale)?;
        self.player_handles.remove(&handle);
        Ok(())
    }

    /// Returns the chat messages delivered to a player in deterministic order.
    pub fn player_chat_log(&self, handle: PlayerHandle) -> Result<&[String], FacadeError> {
        let rec = self
            .players
            .get(handle, None)
            .map_err(FacadeError::PlayerStale)?;
        Ok(&rec.chat_log)
    }

    /// Creates a new inventory container with `size` empty slots.
    pub fn create_inventory(
        &mut self,
        kind: InventoryKind,
        size: usize,
    ) -> Result<InventoryHandle, FacadeError> {
        self.inventories
            .insert(
                HOST_OWNER,
                InventoryRecord {
                    kind,
                    slots: vec![None; size],
                },
            )
            .map_err(FacadeError::InventoryStale)
    }

    /// Destroys an inventory container, closing any open menus viewing it.
    pub fn destroy_inventory(&mut self, handle: InventoryHandle) -> Result<(), FacadeError> {
        self.inventories
            .remove(handle, None)
            .map_err(FacadeError::InventoryStale)?;
        let affected_menus: Vec<MenuHandle> = self
            .menu_handles
            .iter()
            .copied()
            .filter(|m| {
                self.menus
                    .get(*m, None)
                    .is_ok_and(|rec| rec.inventory == handle && rec.open)
            })
            .collect();
        for menu in affected_menus {
            let _ = self.close_menu_internal(menu);
        }
        Ok(())
    }

    /// Removes all closed menu handles from the table, freeing their slots and
    /// bumping their generations so stale handles return `FacadeError::MenuStale`.
    pub fn purge_closed_menus(&mut self) -> usize {
        let closed: Vec<MenuHandle> = self
            .menu_handles
            .iter()
            .copied()
            .filter(|m| self.menus.get(*m, None).is_ok_and(|rec| !rec.open))
            .collect();
        let count = closed.len();
        for menu in closed {
            let _ = self.menus.remove(menu, None);
            self.menu_handles.remove(&menu);
        }
        count
    }

    fn close_menu_internal(&mut self, menu: MenuHandle) -> Result<(), FacadeError> {
        let viewer = {
            let rec = self
                .menus
                .get_mut(menu, None)
                .map_err(FacadeError::MenuStale)?;
            if !rec.open {
                return Err(FacadeError::MenuClosed(menu));
            }
            rec.open = false;
            rec.viewer
        };
        if let Ok(player) = self.players.get_mut(viewer, None)
            && player.open_menu == Some(menu)
        {
            player.open_menu = None;
        }
        Ok(())
    }

    /// Opens a capability-checked facade view for a plugin with `granted` capabilities.
    #[must_use]
    pub fn view<'a>(&'a mut self, granted: &'a CapabilitySet) -> NativeFacadeView<'a> {
        NativeFacadeView {
            state: self,
            granted,
        }
    }

    fn snapshot_for_rollback(&self) -> RollbackSnapshot {
        RollbackSnapshot {
            next_container_id: self.next_container_id,
            worlds: self.worlds.clone(),
            entities: self
                .entity_handles
                .iter()
                .filter_map(|&h| self.entities.get(h, None).ok().cloned().map(|r| (h, r)))
                .collect(),
            players: self
                .player_handles
                .iter()
                .filter_map(|&h| self.players.get(h, None).ok().cloned().map(|r| (h, r)))
                .collect(),
            inventories: self.snapshot_inventories(),
            menus: self
                .menu_handles
                .iter()
                .filter_map(|&h| {
                    self.menus
                        .get(h, None)
                        .ok()
                        .filter(|r| r.open)
                        .cloned()
                        .map(|r| (h, r))
                })
                .collect(),
        }
    }

    fn snapshot_inventories(&self) -> Vec<(InventoryHandle, InventoryRecord)> {
        // Track inventories referenced by handles via a bounded scan isn't needed
        // if we validate/restore per-command; we clone only touched inventories
        // in apply_atomic via a journal.
        Vec::new()
    }
}

struct RollbackSnapshot {
    next_container_id: i32,
    worlds: BTreeMap<WorldId, WorldRecord>,
    entities: Vec<(EntityHandle, EntityRecord)>,
    players: Vec<(PlayerHandle, PlayerRecord)>,
    inventories: Vec<(InventoryHandle, InventoryRecord)>,
    menus: Vec<(MenuHandle, MenuRecord)>,
}

/// Capability-checked, tick-thread-verified safe Rust facade view over
/// authoritative world, entity, player, item, inventory, and menu state.
pub struct NativeFacadeView<'a> {
    state: &'a mut SyntheticWorldState,
    granted: &'a CapabilitySet,
}

impl<'a> NativeFacadeView<'a> {
    fn check(&self, cap: Capability) -> Result<(), FacadeError> {
        self.state.ensure_tick_thread()?;
        if !self.granted.contains(cap) {
            return Err(FacadeError::CapabilityDenied(cap));
        }
        Ok(())
    }

    /// Enforces that a named M3 provider seam is verified before live calls.
    pub fn require_provider(&self, provider: &'static str) -> Result<(), FacadeError> {
        self.state.require_provider(provider)
    }

    // ---- World reads ----

    pub fn world_info(&self, world: WorldId) -> Result<WorldSnapshot, FacadeError> {
        self.check(Capability::WorldRead)?;
        let rec = self
            .state
            .worlds
            .get(&world)
            .ok_or(FacadeError::WorldNotFound(world))?;
        Ok(rec.snapshot())
    }

    pub fn is_chunk_loaded(&self, world: WorldId, chunk: ChunkPos) -> Result<bool, FacadeError> {
        self.check(Capability::WorldRead)?;
        let rec = self
            .state
            .worlds
            .get(&world)
            .ok_or(FacadeError::WorldNotFound(world))?;
        Ok(rec.loaded_chunks.contains(&chunk))
    }

    pub fn get_block(
        &self,
        world: WorldId,
        pos: BlockPos,
    ) -> Result<BlockStateSnapshot, FacadeError> {
        self.check(Capability::WorldRead)?;
        let rec = self
            .state
            .worlds
            .get(&world)
            .ok_or(FacadeError::WorldNotFound(world))?;
        rec.validate_block_pos(pos)?;
        Ok(rec
            .blocks
            .get(&pos)
            .cloned()
            .unwrap_or_else(BlockStateSnapshot::air))
    }

    // ---- Entity reads ----

    pub fn get_entity(&self, handle: EntityHandle) -> Result<EntitySnapshot, FacadeError> {
        self.check(Capability::EntityRead)?;
        let rec = self
            .state
            .entities
            .get(handle, None)
            .map_err(FacadeError::EntityStale)?;
        Ok(EntitySnapshot {
            handle,
            world: rec.world,
            entity_type: rec.entity_type.clone(),
            x: rec.x,
            y: rec.y,
            z: rec.z,
            yaw: rec.yaw,
            pitch: rec.pitch,
            alive: rec.alive,
            custom_name: rec.custom_name.clone(),
        })
    }

    pub fn entities_in_world(&self, world: WorldId) -> Result<Vec<EntityHandle>, FacadeError> {
        self.check(Capability::EntityRead)?;
        if !self.state.worlds.contains_key(&world) {
            return Err(FacadeError::WorldNotFound(world));
        }
        Ok(self
            .state
            .entity_handles
            .iter()
            .copied()
            .filter(|&h| {
                self.state
                    .entities
                    .get(h, None)
                    .is_ok_and(|rec| rec.world == world && rec.alive)
            })
            .collect())
    }

    // ---- Player reads ----

    pub fn get_player(&self, handle: PlayerHandle) -> Result<PlayerSnapshot, FacadeError> {
        self.check(Capability::PlayerRead)?;
        let rec = self
            .state
            .players
            .get(handle, None)
            .map_err(FacadeError::PlayerStale)?;
        let world = self
            .state
            .entities
            .get(rec.entity, None)
            .map(|e| e.world)
            .unwrap_or(WorldId::from_raw(0));
        Ok(PlayerSnapshot {
            handle,
            entity: rec.entity,
            uuid: rec.uuid,
            name: rec.name.clone(),
            world,
            game_mode: rec.game_mode,
            health: rec.health,
            max_health: rec.max_health,
            food_level: rec.food_level,
            online: rec.online,
            open_menu: rec.open_menu,
        })
    }

    pub fn online_players(&self) -> Result<Vec<PlayerHandle>, FacadeError> {
        self.check(Capability::PlayerRead)?;
        Ok(self
            .state
            .player_handles
            .iter()
            .copied()
            .filter(|&h| self.state.players.get(h, None).is_ok_and(|rec| rec.online))
            .collect())
    }

    // ---- Inventory & menu reads ----

    pub fn get_inventory(&self, handle: InventoryHandle) -> Result<InventorySnapshot, FacadeError> {
        self.check(Capability::InventoryRead)?;
        let rec = self
            .state
            .inventories
            .get(handle, None)
            .map_err(FacadeError::InventoryStale)?;
        Ok(InventorySnapshot {
            handle,
            kind: rec.kind,
            size: rec.slots.len(),
            slots: rec.slots.clone(),
        })
    }

    pub fn get_slot(
        &self,
        handle: InventoryHandle,
        slot: usize,
    ) -> Result<Option<ItemStackSnapshot>, FacadeError> {
        self.check(Capability::InventoryRead)?;
        let rec = self
            .state
            .inventories
            .get(handle, None)
            .map_err(FacadeError::InventoryStale)?;
        let size = rec.slots.len();
        rec.slots
            .get(slot)
            .cloned()
            .ok_or(FacadeError::SlotOutOfBounds { slot, size })
    }

    pub fn get_menu(&self, handle: MenuHandle) -> Result<MenuSnapshot, FacadeError> {
        self.check(Capability::MenuRead)?;
        let rec = self
            .state
            .menus
            .get(handle, None)
            .map_err(FacadeError::MenuStale)?;
        Ok(MenuSnapshot {
            handle,
            viewer: rec.viewer,
            inventory: rec.inventory,
            title: rec.title.clone(),
            container_id: rec.container_id,
            open: rec.open,
        })
    }

    // ---- Single command & batch mutation ----

    /// Executes a single typed tick-thread command.
    pub fn execute(&mut self, command: TickCommand) -> Result<CommandOutcome, FacadeError> {
        self.check(command.required_capability())?;
        self.apply_command(command)
    }

    /// Executes a batch of typed tick-thread commands under `batch.mode`.
    ///
    /// - Under [`BatchMode::Atomic`], if any command fails (including capability
    ///   denial, stale/dead/offline/unloaded targets, or invalid values), all
    ///   prior mutations in the batch are rolled back cleanly so authoritative
    ///   state is unchanged and `receipt.committed == false`.
    /// - Under [`BatchMode::ContinueOnError`], each command runs in submission
    ///   order and records its own [`CommandOutcome`].
    pub fn execute_batch(&mut self, batch: TickCommandBatch) -> Result<BatchReceipt, FacadeError> {
        self.state.ensure_tick_thread()?;
        match batch.mode {
            BatchMode::ContinueOnError => {
                let mut outcomes = Vec::with_capacity(batch.commands.len());
                for cmd in batch.commands {
                    let outcome = match self.execute(cmd) {
                        Ok(ok) => ok,
                        Err(err) => CommandOutcome::Failed(err),
                    };
                    outcomes.push(outcome);
                }
                Ok(BatchReceipt {
                    mode: BatchMode::ContinueOnError,
                    committed: true,
                    outcomes,
                })
            }
            BatchMode::Atomic => self.execute_batch_atomic(batch.commands),
        }
    }

    fn execute_batch_atomic(
        &mut self,
        commands: Vec<TickCommand>,
    ) -> Result<BatchReceipt, FacadeError> {
        // Save rollback state for worlds, entities, players, menus, and any
        // inventories touched by the batch.
        let mut rollback = self.state.snapshot_for_rollback();
        let mut seen_inventories = BTreeSet::new();
        for cmd in &commands {
            let inv_handle = match cmd {
                TickCommand::Inventory(
                    InventoryCommand::SetSlot { inventory, .. }
                    | InventoryCommand::ClearSlot { inventory, .. }
                    | InventoryCommand::ClearAll { inventory },
                ) => Some(*inventory),
                _ => None,
            };
            if let Some(h) = inv_handle
                && seen_inventories.insert(h)
                && let Ok(rec) = self.state.inventories.get(h, None)
            {
                rollback.inventories.push((h, rec.clone()));
            }
        }

        let mut outcomes = Vec::with_capacity(commands.len());
        let mut opened_menus = Vec::new();
        let mut discarded_entities = Vec::new();
        let mut failed = false;

        for cmd in commands {
            let cap = cmd.required_capability();
            if !self.granted.contains(cap) {
                outcomes.push(CommandOutcome::Failed(FacadeError::CapabilityDenied(cap)));
                failed = true;
                break;
            }
            let maybe_discard = match &cmd {
                TickCommand::Entity(EntityCommand::Discard { handle }) => Some(*handle),
                _ => None,
            };
            match self.apply_command(cmd) {
                Ok(CommandOutcome::OpenedMenu(menu)) => {
                    opened_menus.push(menu);
                    outcomes.push(CommandOutcome::OpenedMenu(menu));
                }
                Ok(ok) => {
                    if let Some(h) = maybe_discard {
                        discarded_entities.push(h);
                    }
                    outcomes.push(ok);
                }
                Err(err) => {
                    outcomes.push(CommandOutcome::Failed(err));
                    failed = true;
                    break;
                }
            }
        }

        if failed {
            self.restore_rollback(rollback, &opened_menus, &discarded_entities);
            return Ok(BatchReceipt {
                mode: BatchMode::Atomic,
                committed: false,
                outcomes,
            });
        }

        Ok(BatchReceipt {
            mode: BatchMode::Atomic,
            committed: true,
            outcomes,
        })
    }

    fn restore_rollback(
        &mut self,
        rollback: RollbackSnapshot,
        opened_menus: &[MenuHandle],
        discarded_entities: &[EntityHandle],
    ) {
        self.state.next_container_id = rollback.next_container_id;
        self.state.worlds = rollback.worlds;

        // Revoke any menus opened during the aborted batch.
        for &menu in opened_menus {
            let _ = self.state.menus.remove(menu, None);
            self.state.menu_handles.remove(&menu);
        }
        // Restore previously existing menus.
        for (handle, rec) in rollback.menus {
            if let Ok(slot) = self.state.menus.get_mut(handle, None) {
                *slot = rec;
            }
        }
        // Restore entities (including any that were marked dead during an aborted
        // atomic batch). Note: Discard in atomic mode marks alive=false before
        // commit so handles are preserved cleanly on rollback.
        let _ = discarded_entities;
        for (handle, rec) in rollback.entities {
            if let Ok(slot) = self.state.entities.get_mut(handle, None) {
                *slot = rec;
            }
        }
        // Restore players.
        for (handle, rec) in rollback.players {
            if let Ok(slot) = self.state.players.get_mut(handle, None) {
                *slot = rec;
            }
        }
        // Restore touched inventories.
        for (handle, rec) in rollback.inventories {
            if let Ok(slot) = self.state.inventories.get_mut(handle, None) {
                *slot = rec;
            }
        }
    }

    fn apply_command(&mut self, command: TickCommand) -> Result<CommandOutcome, FacadeError> {
        match command {
            TickCommand::World(WorldCommand::SetBlock { world, pos, state }) => {
                let rec = self
                    .state
                    .worlds
                    .get_mut(&world)
                    .ok_or(FacadeError::WorldNotFound(world))?;
                rec.validate_block_pos(pos)?;
                if state.block_id == "minecraft:air" && state.properties.is_empty() {
                    rec.blocks.remove(&pos);
                } else {
                    rec.blocks.insert(pos, state);
                }
                Ok(CommandOutcome::Applied)
            }
            TickCommand::World(WorldCommand::SetDayTime { world, day_time }) => {
                let rec = self
                    .state
                    .worlds
                    .get_mut(&world)
                    .ok_or(FacadeError::WorldNotFound(world))?;
                rec.day_time = day_time;
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Entity(EntityCommand::Teleport {
                handle,
                world,
                x,
                y,
                z,
                yaw,
                pitch,
            }) => {
                if !self.state.worlds.contains_key(&world) {
                    return Err(FacadeError::WorldNotFound(world));
                }
                validate_finite_coords(x, y, z, yaw, pitch)?;
                let rec = self
                    .state
                    .entities
                    .get_mut(handle, None)
                    .map_err(FacadeError::EntityStale)?;
                if !rec.alive {
                    return Err(FacadeError::EntityDead(handle));
                }
                rec.world = world;
                rec.x = x;
                rec.y = y;
                rec.z = z;
                rec.yaw = yaw;
                rec.pitch = pitch;
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Entity(EntityCommand::SetCustomName { handle, name }) => {
                let rec = self
                    .state
                    .entities
                    .get_mut(handle, None)
                    .map_err(FacadeError::EntityStale)?;
                if !rec.alive {
                    return Err(FacadeError::EntityDead(handle));
                }
                rec.custom_name = name;
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Entity(EntityCommand::Discard { handle }) => {
                let rec = self
                    .state
                    .entities
                    .get_mut(handle, None)
                    .map_err(FacadeError::EntityStale)?;
                if !rec.alive {
                    return Err(FacadeError::EntityDead(handle));
                }
                rec.alive = false;
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Player(PlayerCommand::SetGameMode { handle, mode }) => {
                let rec = self
                    .state
                    .players
                    .get_mut(handle, None)
                    .map_err(FacadeError::PlayerStale)?;
                if !rec.online {
                    return Err(FacadeError::PlayerOffline(handle));
                }
                rec.game_mode = mode;
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Player(PlayerCommand::SetHealth { handle, health }) => {
                let rec = self
                    .state
                    .players
                    .get_mut(handle, None)
                    .map_err(FacadeError::PlayerStale)?;
                if !rec.online {
                    return Err(FacadeError::PlayerOffline(handle));
                }
                if !health.is_finite() || health < 0.0 || health > rec.max_health {
                    return Err(FacadeError::InvalidValue {
                        field: "health",
                        detail: format!("must be finite in 0.0..={}, got {health}", rec.max_health),
                    });
                }
                rec.health = health;
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Player(PlayerCommand::SetFoodLevel { handle, food_level }) => {
                let rec = self
                    .state
                    .players
                    .get_mut(handle, None)
                    .map_err(FacadeError::PlayerStale)?;
                if !rec.online {
                    return Err(FacadeError::PlayerOffline(handle));
                }
                if !(0..=20).contains(&food_level) {
                    return Err(FacadeError::InvalidValue {
                        field: "food_level",
                        detail: format!("must be in 0..=20, got {food_level}"),
                    });
                }
                rec.food_level = food_level;
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Player(PlayerCommand::SendMessage { handle, message }) => {
                let rec = self
                    .state
                    .players
                    .get_mut(handle, None)
                    .map_err(FacadeError::PlayerStale)?;
                if !rec.online {
                    return Err(FacadeError::PlayerOffline(handle));
                }
                rec.chat_log.push(message);
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Player(PlayerCommand::CloseOpenMenu { handle }) => {
                let open_menu = {
                    let rec = self
                        .state
                        .players
                        .get(handle, None)
                        .map_err(FacadeError::PlayerStale)?;
                    if !rec.online {
                        return Err(FacadeError::PlayerOffline(handle));
                    }
                    rec.open_menu
                };
                if let Some(menu) = open_menu {
                    self.state.close_menu_internal(menu)?;
                }
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Inventory(InventoryCommand::SetSlot {
                inventory,
                slot,
                item,
            }) => {
                if let Some(ref stack) = item
                    && (stack.max_stack_size <= 0
                        || stack.max_stack_size > 99
                        || stack.count < 0
                        || stack.count > stack.max_stack_size)
                {
                    return Err(FacadeError::InvalidItemCount {
                        count: stack.count,
                        max_stack_size: stack.max_stack_size,
                    });
                }
                let rec = self
                    .state
                    .inventories
                    .get_mut(inventory, None)
                    .map_err(FacadeError::InventoryStale)?;
                let size = rec.slots.len();
                let cell = rec
                    .slots
                    .get_mut(slot)
                    .ok_or(FacadeError::SlotOutOfBounds { slot, size })?;
                *cell = item.filter(|stack| !stack.is_empty());
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Inventory(InventoryCommand::ClearSlot { inventory, slot }) => {
                let rec = self
                    .state
                    .inventories
                    .get_mut(inventory, None)
                    .map_err(FacadeError::InventoryStale)?;
                let size = rec.slots.len();
                let cell = rec
                    .slots
                    .get_mut(slot)
                    .ok_or(FacadeError::SlotOutOfBounds { slot, size })?;
                *cell = None;
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Inventory(InventoryCommand::ClearAll { inventory }) => {
                let rec = self
                    .state
                    .inventories
                    .get_mut(inventory, None)
                    .map_err(FacadeError::InventoryStale)?;
                for slot in &mut rec.slots {
                    *slot = None;
                }
                Ok(CommandOutcome::Applied)
            }
            TickCommand::Menu(MenuCommand::OpenMenu {
                player,
                inventory,
                title,
            }) => {
                // Validate player online and inventory live before mutating.
                let previous_menu = {
                    let rec = self
                        .state
                        .players
                        .get(player, None)
                        .map_err(FacadeError::PlayerStale)?;
                    if !rec.online {
                        return Err(FacadeError::PlayerOffline(player));
                    }
                    rec.open_menu
                };
                self.state
                    .inventories
                    .get(inventory, None)
                    .map_err(FacadeError::InventoryStale)?;

                if let Some(prev) = previous_menu {
                    let _ = self.state.close_menu_internal(prev);
                }

                // Vanilla ServerPlayer.nextContainerCounter wraps 1..=100.
                let container_id = self.state.next_container_id;
                self.state.next_container_id = (container_id % 100) + 1;

                let menu = self
                    .state
                    .menus
                    .insert(
                        HOST_OWNER,
                        MenuRecord {
                            viewer: player,
                            inventory,
                            title,
                            container_id,
                            open: true,
                        },
                    )
                    .map_err(FacadeError::MenuStale)?;
                self.state.menu_handles.insert(menu);
                if let Ok(rec) = self.state.players.get_mut(player, None) {
                    rec.open_menu = Some(menu);
                }
                Ok(CommandOutcome::OpenedMenu(menu))
            }
            TickCommand::Menu(MenuCommand::CloseMenu { menu }) => {
                self.state.close_menu_internal(menu)?;
                Ok(CommandOutcome::Applied)
            }
        }
    }
}

fn validate_finite_coords(x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> Result<(), FacadeError> {
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return Err(FacadeError::InvalidValue {
            field: "position",
            detail: format!("coordinates must be finite, got ({x}, {y}, {z})"),
        });
    }
    if !yaw.is_finite() || !pitch.is_finite() {
        return Err(FacadeError::InvalidValue {
            field: "rotation",
            detail: format!("yaw/pitch must be finite, got ({yaw}, {pitch})"),
        });
    }
    Ok(())
}
