//! LevelDebugSynchronizers' sleeping lifecycle and native block value sources.
//! The hive synchronizer is executable; other source kinds are not silently
//! substituted with hive data. Global wake-up enumeration remains the owning
//! server's job, and must precede polling after `begin_tick` returns Wake.
use super::{chunk_map::ChunkMap, level_block_entity_runtime::BlockEntityTickerError};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_registry::{
    block_state::BlockState,
    block_state_properties::BlockStateProperties,
    block_state_property::PropertyValue,
    core::java_block_pos_set::JavaBlockPosSet,
    core::{BlockPos, ChunkPos},
    generated::{block_behaviors::static_collision_shape_of, tags::BLOCK_TAG_BY_NAME},
};
use rivet_util::mth::Uuid;
use rivet_world::block_entity::{NativeBlockEntity, NativeBlockEntityData};
use std::{
    collections::{HashMap, HashSet},
    sync::Weak,
};

type Result<T> = std::result::Result<T, BlockEntityTickerError>;

/// Actual built-in debug registry order (Paper 0a99345). This is an identity
/// table, not a declaration that all sixteen debug producers are implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DebugSubscription {
    DedicatedServerTickTime,
    Bees,
    Brains,
    Breezes,
    GoalSelectors,
    EntityPaths,
    EntityBlockIntersections,
    BeeHives,
    Pois,
    RedstoneWireOrientations,
    VillageSections,
    Raids,
    Structures,
    GameEventListeners,
    NeighborUpdates,
    GameEvents,
}
impl DebugSubscription {
    pub const NAMES: [&'static str; 16] = [
        "minecraft:dedicated_server_tick_time",
        "minecraft:bees",
        "minecraft:brains",
        "minecraft:breezes",
        "minecraft:goal_selectors",
        "minecraft:entity_paths",
        "minecraft:entity_block_intersections",
        "minecraft:bee_hives",
        "minecraft:pois",
        "minecraft:redstone_wire_orientations",
        "minecraft:village_sections",
        "minecraft:raids",
        "minecraft:structures",
        "minecraft:game_event_listeners",
        "minecraft:neighbor_updates",
        "minecraft:game_events",
    ];
}

/// ServerDebugSubscribers rebuilds membership from the actual player list each
/// tick, without applying permissions again (the request handler owns that).
#[derive(Debug, Default)]
pub struct ServerDebugSubscribers {
    subscribers: HashMap<DebugSubscription, Vec<Uuid>>,
}
impl ServerDebugSubscribers {
    pub fn tick<'a>(
        &mut self,
        players: impl IntoIterator<Item = (Uuid, &'a HashSet<DebugSubscription>)>,
    ) {
        for players in self.subscribers.values_mut() {
            players.clear();
        }
        for (player, subscriptions) in players {
            for subscription in subscriptions {
                self.subscribers
                    .entry(*subscription)
                    .or_default()
                    .push(player);
            }
        }
        self.subscribers.retain(|_, players| !players.is_empty());
    }
    pub fn enabled_subscriptions(&self) -> HashSet<DebugSubscription> {
        self.subscribers.keys().copied().collect()
    }
    pub fn subscribers(&self, subscription: DebugSubscription) -> &[Uuid] {
        self.subscribers
            .get(&subscription)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugTickTransition {
    Sleeping,
    Wake,
    Awake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugHiveInfo {
    pub block_id: u16,
    pub occupant_count: i32,
    pub honey_level: i32,
    pub sedated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiveDebugUpdate {
    pub position: BlockPos,
    pub value: Option<DebugHiveInfo>,
}
impl HiveDebugUpdate {
    /// ClientboundDebugBlockValuePacket payload, excluding its packet ID.
    pub fn encode_body(self) -> Vec<u8> {
        let mut buf = FriendlyByteBuf::new(Default::default());
        buf.write_long(self.position.as_long());
        buf.write_var_int(DebugSubscription::BeeHives as i32);
        buf.write_boolean(self.value.is_some());
        if let Some(value) = self.value {
            buf.write_var_int(i32::from(value.block_id));
            buf.write_var_int(value.occupant_count);
            buf.write_var_int(value.honey_level);
            buf.write_boolean(value.sedated);
        }
        buf.into_inner().to_vec()
    }
}

/// Real transport and the original fastutil UUID-set iteration authority.
/// Supplying player-list order as a substitute for that order is not valid.
pub trait HiveDebugTransport {
    fn subscriber_order(&self, members: &HashSet<Uuid>) -> Result<Vec<Uuid>>;
    fn is_chunk_tracked(&self, player: Uuid, chunk: ChunkPos) -> bool;
    fn send(&mut self, player: Uuid, update: HiveDebugUpdate) -> Result<()>;
}

#[derive(Debug)]
struct HiveSource {
    identity: Weak<()>,
    last_value: Option<DebugHiveInfo>,
}

#[derive(Debug, Default)]
pub struct LevelDebugSynchronizers {
    awake: bool,
    enabled: HashSet<DebugSubscription>,
    hives: HashMap<BlockPos, HiveSource>,
    hive_order: JavaBlockPosSet,
    hive_subscribers: HashSet<Uuid>,
}
impl LevelDebugSynchronizers {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_sleeping(&self) -> bool {
        !self.awake
    }
    pub fn has_any_subscriber_for(&self, subscription: DebugSubscription) -> bool {
        self.enabled.contains(&subscription)
    }
    /// Source updates the snapshot and sleeping flag BEFORE wake-up callbacks.
    /// Wake requires actual ready-to-send chunks/tracked entities/POI producers;
    /// this method is deliberately not called a complete level debug tick.
    pub fn begin_tick(&mut self, subscribers: &ServerDebugSubscribers) -> DebugTickTransition {
        self.enabled = subscribers.enabled_subscriptions();
        let awake = !self.enabled.is_empty();
        let was_awake = std::mem::replace(&mut self.awake, awake);
        if !awake {
            if was_awake {
                self.hives.clear();
                self.hive_order = JavaBlockPosSet::new();
            }
            DebugTickTransition::Sleeping
        } else if was_awake {
            DebugTickTransition::Awake
        } else {
            DebugTickTransition::Wake
        }
    }
    /// ServerLevel.onBlockEntityAdded's base Level body is genuinely empty.
    /// Register regardless of BEE_HIVES membership whenever the level is awake.
    pub fn register_block_entity(&mut self, entity: &NativeBlockEntity) -> Result<()> {
        if !self.awake {
            return Ok(());
        }
        if matches!(entity.data(), NativeBlockEntityData::Beehive(_)) {
            self.hive_order
                .try_insert(entity.position())
                .map_err(|error| BlockEntityTickerError::Unavailable(error.to_string()))?;
            self.hives.insert(
                entity.position(),
                HiveSource {
                    identity: entity.ticker_identity(),
                    last_value: None,
                },
            );
        }
        // Chest/Spawner/CreakingHeart inherit BlockEntity.registerDebugValues.
        Ok(())
    }
    pub fn registered_hive_count(&self) -> usize {
        self.hives.len()
    }
    pub fn clear_chunk(&mut self, chunk: ChunkPos) {
        if !self.awake {
            return;
        }
        let positions: Vec<_> = self
            .hive_order
            .iter()
            .copied()
            .filter(|pos| ChunkPos::containing(pos) == chunk)
            .collect();
        for pos in positions {
            self.hives.remove(&pos);
            self.hive_order.remove(&pos);
        }
    }
    pub fn start_tracking_chunk(
        &self,
        player: Uuid,
        chunk: ChunkPos,
        transport: &mut impl HiveDebugTransport,
    ) -> Result<()> {
        if !self.awake || !self.hive_subscribers.contains(&player) {
            return Ok(());
        }
        for pos in self.hive_order.iter() {
            if ChunkPos::containing(pos) == chunk
                && let Some(value) = self.hives[pos].last_value
            {
                transport.send(
                    player,
                    HiveDebugUpdate {
                        position: *pos,
                        value: Some(value),
                    },
                )?;
            }
        }
        Ok(())
    }
    /// Poll the hive SourceSynchronizer only after actual subscriber refresh.
    /// The caller supplies this dimension's actual player list and each
    /// player's subscriptions, not merely the global enabled union.
    pub fn tick_hives<'a>(
        &mut self,
        chunks: &ChunkMap,
        players: impl IntoIterator<Item = (Uuid, &'a HashSet<DebugSubscription>, Vec<ChunkPos>)>,
        transport: &mut impl HiveDebugTransport,
    ) -> Result<()> {
        if !self.awake {
            return Ok(());
        }
        let mut present = HashSet::new();
        for (player, subscriptions, tracking_view_without_pending) in players {
            present.insert(player);
            if subscriptions.contains(&DebugSubscription::BeeHives) {
                if self.hive_subscribers.insert(player) {
                    // Source traverses the full tracking view in its native
                    // order and skips only PlayerChunkSender.isPending. Do
                    // not replace this with isChunkTracked/ready-chunk tests.
                    for chunk in tracking_view_without_pending {
                        self.start_tracking_chunk(player, chunk, transport)?;
                    }
                }
            } else {
                self.hive_subscribers.remove(&player);
            }
        }
        self.hive_subscribers
            .retain(|player| present.contains(player));
        if self.hive_subscribers.is_empty() {
            return Ok(());
        }
        for pos in self.hive_order.iter().copied() {
            let source = self.hives.get_mut(&pos).expect("source order");
            let entity = chunks
                .get_chunk(ChunkPos::containing(&pos))
                .and_then(|chunk| chunk.generation_base().get_block_entity(&pos))
                .filter(|entity| Weak::ptr_eq(&source.identity, &entity.ticker_identity()))
                .ok_or_else(|| {
                    BlockEntityTickerError::Unavailable(format!(
                        "registered hive source no longer canonical at {pos:?}"
                    ))
                })?;
            let value = pack_hive(entity, chunks)?;
            if source.last_value != Some(value) {
                // ValueSource commits the cache before transport callbacks.
                source.last_value = Some(value);
                send_to_tracking(
                    &self.hive_subscribers,
                    HiveDebugUpdate {
                        position: pos,
                        value: Some(value),
                    },
                    transport,
                )?;
            }
        }
        Ok(())
    }
    pub fn drop_block_entity(
        &mut self,
        pos: BlockPos,
        transport: &mut (impl HiveDebugTransport + ?Sized),
    ) -> Result<()> {
        self.drop_block_entity_with(pos, |members, update| {
            send_to_tracking(members, update, transport)
        })
    }
    pub(super) fn drop_block_entity_with(
        &mut self,
        pos: BlockPos,
        send: impl FnOnce(&HashSet<Uuid>, HiveDebugUpdate) -> Result<()>,
    ) -> Result<()> {
        if self.awake && self.hives.remove(&pos).is_some() {
            self.hive_order.remove(&pos);
            if !self.hive_subscribers.is_empty() {
                send(
                    &self.hive_subscribers,
                    HiveDebugUpdate {
                        position: pos,
                        value: None,
                    },
                )?;
            }
        }
        Ok(())
    }
}

pub(super) fn send_to_tracking(
    members: &HashSet<Uuid>,
    update: HiveDebugUpdate,
    transport: &mut (impl HiveDebugTransport + ?Sized),
) -> Result<()> {
    if members.is_empty() {
        return Ok(());
    }
    let order = transport.subscriber_order(members)?;
    if order.len() != members.len() || order.iter().copied().collect::<HashSet<_>>() != *members {
        return Err(BlockEntityTickerError::Unavailable(
            "debug subscriber iteration authority returned a foreign or duplicate player".into(),
        ));
    }
    for player in order {
        if transport.is_chunk_tracked(player, ChunkPos::containing(&update.position)) {
            transport.send(player, update)?;
        }
    }
    Ok(())
}

fn state(chunks: &ChunkMap, pos: BlockPos) -> Result<BlockState> {
    let chunk = chunks
        .get_chunk(ChunkPos::containing(&pos))
        .ok_or_else(|| {
            BlockEntityTickerError::Unavailable(format!(
                "debug hive smoke query requires loaded chunk at {pos:?}"
            ))
        })?;
    Ok(BlockState::new(chunk.get_block_state(
        pos.get_x(),
        pos.get_y(),
        pos.get_z(),
    )))
}
fn lit_campfire(state: BlockState) -> bool {
    BLOCK_TAG_BY_NAME["minecraft:campfires"].contains(&state.block().name())
        && state.get_value(BlockStateProperties::LIT) == Some(PropertyValue::Bool(true))
}
pub fn pack_hive(entity: &NativeBlockEntity, chunks: &ChunkMap) -> Result<DebugHiveInfo> {
    let NativeBlockEntityData::Beehive(data) = entity.data() else {
        return Err(BlockEntityTickerError::Unavailable(
            "DebugHiveInfo requires an actual native hive".into(),
        ));
    };
    let pos = entity.position();
    let mut sedated = false;
    for distance in 1..=5 {
        let below = pos.offset(0, -distance, 0);
        let block = state(chunks, below)?;
        if lit_campfire(block) {
            sedated = true;
            break;
        }
        // CampfireBlock deliberately passes the HIVE position to the queried
        // state's collision shape, not posToCheck. Dynamic shapes must have
        // their actual original-position world context; don't approximate.
        let boxes = static_collision_shape_of(block.id()).ok_or_else(|| {
            BlockEntityTickerError::Unavailable(
                "hive smoke needs genuine contextual collision geometry".into(),
            )
        })?;
        if boxes.iter().any(|shape| {
            shape.min_x < 20
                && shape.max_x > 12
                && shape.min_y < 32
                && shape.max_y > 0
                && shape.min_z < 20
                && shape.max_z > 12
        }) {
            sedated = lit_campfire(state(chunks, below.offset(0, -1, 0))?);
            break;
        }
    }
    let honey_level = match entity.state().get_value(BlockStateProperties::LEVEL_HONEY) {
        Some(PropertyValue::Int(value)) => value,
        _ => 0,
    };
    Ok(DebugHiveInfo {
        block_id: entity.state().block().0,
        occupant_count: data.occupant_count(),
        honey_level,
        sedated,
    })
}

impl super::ServerLevel {
    pub fn debug_synchronizers(&self) -> &LevelDebugSynchronizers {
        &self.debug_synchronizers
    }

    /// Begins the actual retained owner's subscription phase. A Wake result
    /// is a required request for the full source wake traversal, not evidence
    /// that the other chunk/entity/POI synchronizers have executed.
    pub fn begin_debug_tick(
        &mut self,
        subscribers: &ServerDebugSubscribers,
    ) -> DebugTickTransition {
        self.debug_synchronizers.begin_tick(subscribers)
    }
}

#[cfg(test)]
#[path = "level_debug_synchronizers_test.rs"]
mod tests;
