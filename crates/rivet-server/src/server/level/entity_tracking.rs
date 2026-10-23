//! Native ChunkMap.TrackedEntity + ServerEntity tracking for the currently
//! executable entity families. Packet pairing/update state is owned here;
//! actual players, visibility policy, transport and events remain required
//! world authorities, not default-success callbacks. This does not tick AI.
use super::entity_lookup::{EntityLifecycle, EntityLifecycleEvent, EntityLookupError};
use bytes::BytesMut;
use rivet_protocol::{
    codec::StreamEncoder,
    friendly_byte_buf::FriendlyByteBuf,
    game::clientbound_add_entity_packet::{
        ClientboundAddEntityPacket, add_entity_codec, write_lp_vec3,
    },
    registry_friendly_byte_buf::RegistryFriendlyByteBuf,
};
use rivet_registry::{RegistryAccess, core::Vec3};
use rivet_util::mth::Uuid;
use rivet_world::entity::{
    loaded_entity::NativeLoadedEntity,
    tracking::{EntityTrackingSnapshot, MetadataItem, MetadataValue},
};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
#[path = "entity_tracking_fixture.rs"]
mod fixture;

type Result<T> = std::result::Result<T, EntityLookupError>;
fn unavailable(reason: impl Into<String>) -> EntityLookupError {
    EntityLookupError::LifecycleUnavailable(reason.into())
}
#[derive(Debug, Clone, PartialEq)]
pub struct TrackingPacket {
    pub name: &'static str,
    pub body: Vec<u8>,
}
#[derive(Debug, Clone, Copy)]
pub struct TrackingPlayer {
    pub id: i32,
    pub position: Vec3,
    pub view_distance_chunks: i32,
    pub chunk_tracked: bool,
    pub broadcast: bool,
    pub can_see: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct EntityTrackingConfig {
    pub world_uuid: Uuid,
    /// Actual Spigot configured ranges, after TrackingRange category choice.
    pub animal_range_blocks: i32,
    pub other_range_blocks: i32,
    /// Actual Paper Y-range category value; None is disabled/-1.
    pub animal_range_y: Option<f64>,
    pub other_range_y: Option<f64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackingWorldEvent {
    AddedToWorld,
    RemovedFromWorld,
    ScoreboardRemoved,
    TrackPlayer,
    UntrackPlayer,
}
pub trait TrackingAuthority {
    fn external_uuid_in_use(&self, uuid: &Uuid) -> bool;
    /// Actual level players in source encounter order and their actual chunk,
    /// broadcast/vanish decisions; not an invented all-visible player list.
    fn players(&mut self, entity: &NativeLoadedEntity) -> Result<Vec<TrackingPlayer>>;
    /// Actual canonical connection-identity set encounter order. Java uses
    /// ReferenceOpenHashSet (System.identityHashCode), not insertion order.
    /// The tracker stores membership only and never fabricates identity hashes.
    fn observer_order(&mut self, entity: &NativeLoadedEntity, members: &[i32]) -> Result<Vec<i32>>;
    /// TrackPlayer's boolean is the real cancellable PlayerTrackEntityEvent
    /// result. Other events are noncancellable (their result is ignored).
    fn event(
        &mut self,
        event: TrackingWorldEvent,
        entity: &NativeLoadedEntity,
        player: Option<i32>,
    ) -> Result<bool>;
    /// Source pairing is one bundle, not a sequence of independent sends.
    fn send_bundle(&mut self, player: i32, packets: &[TrackingPacket]) -> Result<()>;
    fn send(&mut self, player: i32, packet: &TrackingPacket) -> Result<()>;
    /// Genuine debug synchronizer subscriber bookkeeping, if any. Requiring
    /// authority distinguishes a known empty subscriber list from unavailable.
    fn debug_tracking(
        &mut self,
        entity: &NativeLoadedEntity,
        player: Option<i32>,
        start: bool,
    ) -> Result<()>;
    /// Current animals have no viewer inventory; a chest cart can have real
    /// viewers and must close those through actual world authority.
    fn close_inventory_viewers(&mut self, entity: &NativeLoadedEntity) -> Result<()>;
}
#[derive(Debug)]
pub struct ServerEntityTracker {
    base: Vec3,
    last_movement: Vec3,
    last_rotation: [i8; 2],
    last_head: i8,
    was_on_ground: bool,
    tick_count: i32,
    teleport_delay: i32,
    force_resync: bool,
    metadata: Vec<MetadataItem>,
    initial_dirty: bool,
}
impl ServerEntityTracker {
    pub fn new(snapshot: &EntityTrackingSnapshot) -> Result<Self> {
        if snapshot.experimental_minecart {
            return Err(unavailable(
                "experimental minecart interpolation tracking requires genuine NewMinecartBehavior lerp steps",
            ));
        }
        Ok(Self {
            base: snapshot.position,
            last_movement: snapshot.movement,
            last_rotation: [
                pack_degrees(snapshot.rotation[0]),
                pack_degrees(snapshot.rotation[1]),
            ],
            last_head: pack_degrees(snapshot.head_rotation),
            was_on_ground: snapshot.on_ground,
            tick_count: 0,
            teleport_delay: 0,
            force_resync: false,
            metadata: snapshot.metadata.clone(),
            initial_dirty: true,
        })
    }
    pub fn on_player_add(&mut self) {
        self.force_resync = true;
    }
    pub fn pairing(
        &self,
        entity: &NativeLoadedEntity,
        access: &RegistryAccess,
    ) -> Result<Vec<TrackingPacket>> {
        if entity.is_removed() {
            return Ok(Vec::new());
        }
        let type_id = *rivet_registry::generated::registries::ENTITY_TYPE_BY_NAME
            .get(entity.encode_id())
            .ok_or_else(|| {
                unavailable("native entity type absent from complete built-in registry")
            })? as i32;
        let snapshot = entity.tracking_snapshot();
        let packet = ClientboundAddEntityPacket {
            id: entity.entity_id(),
            uuid: entity.uuid(),
            entity_type_id: type_id,
            x: snapshot.position.x,
            y: snapshot.position.y,
            z: snapshot.position.z,
            movement: self.last_movement,
            x_rot: self.last_rotation[1],
            y_rot: self.last_rotation[0],
            y_head_rot: self.last_head,
            data: 0,
        };
        let mut buf = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
        add_entity_codec()
            .encode(&mut buf, &packet)
            .map_err(|e| unavailable(e.to_string()))?;
        let mut packets = vec![TrackingPacket {
            name: "add_entity",
            body: buf.inner_mut().as_slice().to_vec(),
        }];
        let values: Vec<_> = self
            .metadata
            .iter()
            .filter(|v| !v.is_default())
            .cloned()
            .collect();
        if !values.is_empty() {
            packets.push(metadata_packet(entity.entity_id(), &values));
        }
        // Supported native load/construction forbids equipment/passengers and
        // only instantiates follow_range (not client-syncable). Thus source
        // getSyncableAttributes/equipment/leash lists are genuinely empty.
        Ok(packets)
    }
    /// Exact ordinary unmounted ServerEntity branch. Native mutation currently
    /// does not expose passengers, hurtMarked or new-minecart lerp queues.
    pub fn send_changes(
        &mut self,
        id: i32,
        snapshot: &EntityTrackingSnapshot,
    ) -> Result<Vec<TrackingPacket>> {
        if snapshot.experimental_minecart {
            return Err(unavailable("NewMinecartBehavior tracking unavailable"));
        }
        let mut packets = Vec::new();
        if !snapshot.living {
            self.teleport_delay = 9999;
        }
        let dirty = self.initial_dirty || snapshot.metadata != self.metadata;
        if self.force_resync || self.tick_count % 3 == 0 || dirty {
            let rotation = [
                pack_degrees(snapshot.rotation[0]),
                pack_degrees(snapshot.rotation[1]),
            ];
            let rotated = (i32::from(rotation[0]) - i32::from(self.last_rotation[0])).abs() >= 1
                || (i32::from(rotation[1]) - i32::from(self.last_rotation[1])).abs() >= 1;
            self.teleport_delay = self.teleport_delay.wrapping_add(1);
            let d = subtract(snapshot.position, self.base);
            let pos = length_squared(d) >= 7.62939453125E-6 || self.tick_count % 60 == 0;
            let delta = [
                encode_delta(snapshot.position.x, self.base.x),
                encode_delta(snapshot.position.y, self.base.y),
                encode_delta(snapshot.position.z, self.base.z),
            ];
            let too_big = delta.iter().any(|v| !(-32768..=32767).contains(v));
            let mut sent_position = false;
            let mut sent_rotation = false;
            let movement = if self.force_resync
                || too_big
                || self.teleport_delay > 400
                || self.was_on_ground != snapshot.on_ground
            {
                self.was_on_ground = snapshot.on_ground;
                self.teleport_delay = 0;
                sent_position = true;
                sent_rotation = true;
                Some(position_sync_packet(id, snapshot))
            } else if pos && rotated {
                sent_position = true;
                sent_rotation = true;
                Some(relative_packet(
                    id,
                    Some(delta),
                    Some(rotation),
                    snapshot.on_ground,
                ))
            } else if pos {
                sent_position = true;
                Some(relative_packet(id, Some(delta), None, snapshot.on_ground))
            } else if rotated {
                sent_rotation = true;
                Some(relative_packet(
                    id,
                    None,
                    Some(rotation),
                    snapshot.on_ground,
                ))
            } else {
                None
            };
            // Current EntityTypes all trackDeltas=true. Motion precedes the
            // movement packet, even on a forced absolute position resync.
            let diff = length_squared(subtract(snapshot.movement, self.last_movement));
            if diff > 1.0E-7 || (diff > 0. && length_squared(snapshot.movement) == 0.) {
                self.last_movement = snapshot.movement;
                let mut buf = FriendlyByteBuf::new(BytesMut::new());
                buf.write_var_int(id);
                write_lp_vec3(&mut buf, snapshot.movement);
                packets.push(finish("set_entity_motion", buf));
            }
            if let Some(packet) = movement {
                packets.push(packet);
            }
            if dirty {
                let values: Vec<_> = snapshot
                    .metadata
                    .iter()
                    .filter(|v| {
                        if self.initial_dirty {
                            !v.is_default()
                        } else {
                            self.metadata
                                .iter()
                                .find(|old| old.id == v.id)
                                .is_none_or(|old| old.value != v.value)
                        }
                    })
                    .cloned()
                    .collect();
                if !values.is_empty() {
                    packets.push(metadata_packet(id, &values));
                }
                self.metadata = snapshot.metadata.clone();
                self.initial_dirty = false;
            }
            if sent_position {
                self.base = snapshot.position;
            }
            if sent_rotation {
                self.last_rotation = rotation;
            }
            let head = pack_degrees(snapshot.head_rotation);
            if (i32::from(head) - i32::from(self.last_head)).abs() >= 1 {
                let mut buf = FriendlyByteBuf::new(BytesMut::new());
                buf.write_var_int(id);
                buf.write_byte(head);
                packets.push(finish("rotate_head", buf));
                self.last_head = head;
            }
            self.force_resync = false;
        }
        self.tick_count = self.tick_count.wrapping_add(1);
        Ok(packets)
    }
}
#[derive(Debug)]
struct TrackedEntity {
    server: ServerEntityTracker,
    seen_by: Vec<i32>,
}
pub struct EntityTrackingRuntime {
    access: RegistryAccess,
    config: EntityTrackingConfig,
    tracked: HashMap<i32, TrackedEntity>,
    navigating_mobs: Vec<i32>,
    ticking: Vec<i32>,
}
impl EntityTrackingRuntime {
    pub fn world_uuid(&self) -> Uuid {
        self.config.world_uuid
    }
    pub fn new(access: RegistryAccess, config: EntityTrackingConfig) -> Self {
        Self {
            access,
            config,
            tracked: HashMap::new(),
            navigating_mobs: Vec::new(),
            ticking: Vec::new(),
        }
    }
    pub fn contains(&self, id: i32) -> bool {
        self.tracked.contains_key(&id)
    }
    pub fn navigating_mobs(&self) -> &[i32] {
        &self.navigating_mobs
    }
    pub fn ticking(&self) -> &[i32] {
        &self.ticking
    }
    pub fn lifecycle<'a>(
        &'a mut self,
        authority: &'a mut dyn TrackingAuthority,
    ) -> TrackingLifecycle<'a> {
        TrackingLifecycle {
            runtime: self,
            authority,
        }
    }
    pub fn update_players(
        &mut self,
        entity: &NativeLoadedEntity,
        authority: &mut dyn TrackingAuthority,
    ) -> Result<()> {
        let Some(tracked) = self.tracked.get_mut(&entity.entity_id()) else {
            return Ok(());
        };
        let living = entity.tracking_snapshot().living;
        let configured = if living {
            self.config.animal_range_blocks
        } else {
            self.config.other_range_blocks
        };
        // Pinned MinecraftServer.getScaledTrackingDistance returns baseRange.
        let range = configured;
        let range_y = if living {
            self.config.animal_range_y
        } else {
            self.config.other_range_y
        };
        let players = authority.players(entity)?;
        for player in &players {
            let d = subtract(player.position, entity.position());
            let visible_range = range.min(player.view_distance_chunks.wrapping_mul(16)) as f64;
            let visible = d.x * d.x + d.z * d.z <= visible_range * visible_range
                && range_y.is_none_or(|y| d.y * d.y <= y * y)
                && player.broadcast
                && player.chunk_tracked
                && player.can_see;
            let seen = tracked.seen_by.contains(&player.id);
            if visible && !seen {
                tracked.seen_by.push(player.id);
                if authority.event(TrackingWorldEvent::TrackPlayer, entity, Some(player.id))? {
                    authority
                        .send_bundle(player.id, &tracked.server.pairing(entity, &self.access)?)?;
                    if tracked.seen_by.len() == 1 {
                        authority.debug_tracking(entity, None, true)?;
                    }
                    authority.debug_tracking(entity, Some(player.id), true)?;
                }
                tracked.server.on_player_add();
            } else if !visible && seen {
                remove_player(tracked, entity, player.id, authority)?;
            }
        }
        let absent: Vec<_> = ordered_observers(authority, entity, &tracked.seen_by)?
            .into_iter()
            .filter(|id| !players.iter().any(|p| p.id == *id))
            .collect();
        for player in absent {
            remove_player(tracked, entity, player, authority)?;
        }
        Ok(())
    }
    pub fn tick_entity(
        &mut self,
        entity: &NativeLoadedEntity,
        entity_ticking: bool,
        authority: &mut dyn TrackingAuthority,
    ) -> Result<()> {
        self.update_players(entity, authority)?;
        if let Some(tracked) = self.tracked.get_mut(&entity.entity_id()) {
            if tracked.seen_by.is_empty() && !entity_ticking {
                return Ok(());
            }
            for packet in tracked
                .server
                .send_changes(entity.entity_id(), &entity.tracking_snapshot())?
            {
                for player in ordered_observers(authority, entity, &tracked.seen_by)? {
                    authority.send(player, &packet)?;
                }
            }
        }
        Ok(())
    }
}
pub struct TrackingLifecycle<'a> {
    runtime: &'a mut EntityTrackingRuntime,
    authority: &'a mut dyn TrackingAuthority,
}
impl EntityLifecycle for TrackingLifecycle<'_> {
    fn after_event(
        &mut self,
        event: EntityLifecycleEvent,
        entity: &NativeLoadedEntity,
    ) -> Result<()> {
        match event {
            EntityLifecycleEvent::TrackingStart => {
                self.authority
                    .event(TrackingWorldEvent::AddedToWorld, entity, None)?;
            }
            EntityLifecycleEvent::TrackingEnd => {
                self.authority
                    .event(TrackingWorldEvent::RemovedFromWorld, entity, None)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn external_uuid_in_use(&self, uuid: &Uuid) -> bool {
        self.authority.external_uuid_in_use(uuid)
    }
    fn on_event(&mut self, event: EntityLifecycleEvent, entity: &NativeLoadedEntity) -> Result<()> {
        if entity.world_uuid() != self.runtime.config.world_uuid {
            return Err(unavailable("entity tracking world UUID mismatch"));
        }
        let id = entity.entity_id();
        match event {
            EntityLifecycleEvent::Created => {} // lookup executes setOldPosAndRot.
            EntityLifecycleEvent::Destroyed => {
                self.authority
                    .event(TrackingWorldEvent::ScoreboardRemoved, entity, None)?;
            }
            EntityLifecycleEvent::TickingStart => {
                if !self.runtime.ticking.contains(&id) {
                    self.runtime.ticking.push(id);
                }
            }
            EntityLifecycleEvent::TickingEnd => {
                remove_id(&mut self.runtime.ticking, id);
            }
            EntityLifecycleEvent::TrackingStart => {
                if entity.tracking_snapshot().living && !self.runtime.navigating_mobs.contains(&id)
                {
                    self.runtime.navigating_mobs.push(id);
                }
                if !entity.lifecycle().valid {
                    return Err(unavailable(
                        "tracking requires lookup-owned valid=true transition",
                    ));
                }
                if self.runtime.tracked.contains_key(&id) {
                    return Err(unavailable("entity already tracked"));
                }
                let range = if entity.tracking_snapshot().living {
                    self.runtime.config.animal_range_blocks
                } else {
                    self.runtime.config.other_range_blocks
                };
                if range != 0 {
                    self.runtime.tracked.insert(
                        id,
                        TrackedEntity {
                            server: ServerEntityTracker::new(&entity.tracking_snapshot())?,
                            seen_by: Vec::new(),
                        },
                    );
                    self.runtime.update_players(entity, self.authority)?;
                }
            }
            EntityLifecycleEvent::TrackingEnd => {
                if matches!(entity, NativeLoadedEntity::ChestMinecart(_)) {
                    self.authority.close_inventory_viewers(entity)?;
                }
                if let Some(tracked) = self.runtime.tracked.remove(&id) {
                    for player in ordered_observers(self.authority, entity, &tracked.seen_by)? {
                        self.authority.event(
                            TrackingWorldEvent::UntrackPlayer,
                            entity,
                            Some(player),
                        )?;
                        self.authority.send(player, &remove_packet(id))?;
                    }
                }
                remove_id(&mut self.runtime.navigating_mobs, id);
                self.authority.debug_tracking(entity, None, false)?;
            }
            EntityLifecycleEvent::SectionChange => {} // no dynamic listener in these native families.
        }
        Ok(())
    }
}
fn ordered_observers(
    authority: &mut dyn TrackingAuthority,
    entity: &NativeLoadedEntity,
    members: &[i32],
) -> Result<Vec<i32>> {
    // An empty/singleton identity set has no ambiguous encounter order.
    if members.len() <= 1 {
        return Ok(members.to_vec());
    }
    let order = authority.observer_order(entity, members)?;
    let actual: HashSet<_> = order.iter().copied().collect();
    if order.len() != members.len()
        || actual.len() != members.len()
        || !members.iter().all(|id| actual.contains(id))
    {
        return Err(unavailable(
            "canonical observer order did not match actual tracker membership",
        ));
    }
    Ok(order)
}
fn remove_player(
    tracked: &mut TrackedEntity,
    entity: &NativeLoadedEntity,
    player: i32,
    authority: &mut dyn TrackingAuthority,
) -> Result<()> {
    remove_id(&mut tracked.seen_by, player);
    authority.event(TrackingWorldEvent::UntrackPlayer, entity, Some(player))?;
    authority.send(player, &remove_packet(entity.entity_id()))?;
    if tracked.seen_by.is_empty() {
        authority.debug_tracking(entity, None, false)?;
    }
    Ok(())
}
fn remove_id(ids: &mut Vec<i32>, id: i32) {
    if let Some(index) = ids.iter().position(|v| *v == id) {
        ids.swap_remove(index);
    }
}
fn finish(name: &'static str, buf: FriendlyByteBuf) -> TrackingPacket {
    TrackingPacket {
        name,
        body: buf.into_inner().to_vec(),
    }
}
fn pack_degrees(degrees: f32) -> i8 {
    (degrees * 256. / 360.).floor() as i32 as i8
}
fn encode_delta(value: f64, base: f64) -> i64 {
    ((value * 4096. + 0.5).floor() as i64).wrapping_sub((base * 4096. + 0.5).floor() as i64)
}
fn subtract(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}
fn length_squared(v: Vec3) -> f64 {
    v.x * v.x + v.y * v.y + v.z * v.z
}
fn remove_packet(id: i32) -> TrackingPacket {
    let mut buf = FriendlyByteBuf::new(BytesMut::new());
    buf.write_var_int(1);
    buf.write_var_int(id);
    finish("remove_entities", buf)
}
fn metadata_packet(id: i32, values: &[MetadataItem]) -> TrackingPacket {
    let mut buf = FriendlyByteBuf::new(BytesMut::new());
    buf.write_var_int(id);
    for item in values {
        buf.write_byte(item.id as i8);
        match item.value {
            MetadataValue::Byte(v) => {
                buf.write_var_int(0);
                buf.write_byte(v);
            }
            MetadataValue::Int(v) => {
                buf.write_var_int(1);
                buf.write_var_int(v);
            }
            MetadataValue::Float(v) => {
                buf.write_var_int(3);
                buf.write_float(v);
            }
            MetadataValue::Boolean(v) => {
                buf.write_var_int(8);
                buf.write_boolean(v);
            }
            MetadataValue::Registry { serializer, id } => {
                buf.write_var_int(serializer);
                buf.write_var_int(id as i32);
            }
            MetadataValue::OptionalBlockState(state) => {
                buf.write_var_int(15);
                buf.write_var_int(state.map_or(0, |id| id.wrapping_add(1)) as i32);
            }
        }
    }
    buf.write_byte(-1);
    finish("set_entity_data", buf)
}
fn relative_packet(
    id: i32,
    delta: Option<[i64; 3]>,
    rotation: Option<[i8; 2]>,
    ground: bool,
) -> TrackingPacket {
    let mut buf = FriendlyByteBuf::new(BytesMut::new());
    buf.write_var_int(id);
    if let Some(delta) = delta {
        for v in delta {
            buf.write_short(v as i16);
        }
    }
    if let Some([yaw, pitch]) = rotation {
        buf.write_byte(yaw);
        buf.write_byte(pitch);
    }
    buf.write_boolean(ground);
    finish(
        match (delta.is_some(), rotation.is_some()) {
            (true, true) => "move_entity_pos_rot",
            (true, false) => "move_entity_pos",
            _ => "move_entity_rot",
        },
        buf,
    )
}
fn position_sync_packet(id: i32, snapshot: &EntityTrackingSnapshot) -> TrackingPacket {
    let mut buf = FriendlyByteBuf::new(BytesMut::new());
    buf.write_var_int(id);
    for value in [
        snapshot.position.x,
        snapshot.position.y,
        snapshot.position.z,
        snapshot.movement.x,
        snapshot.movement.y,
        snapshot.movement.z,
    ] {
        buf.write_double(value);
    }
    buf.write_float(snapshot.rotation[0]);
    buf.write_float(snapshot.rotation[1]);
    buf.write_boolean(snapshot.on_ground);
    finish("entity_position_sync", buf)
}
