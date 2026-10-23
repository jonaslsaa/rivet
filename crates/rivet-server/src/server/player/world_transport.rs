//! ChunkHolder's section batching over real sent watchers and canonical states.
//! World effects queue source changes; the session owner flushes actual packet
//! bodies through its PlaySender, never a replacement whole-chunk resend.
use super::{PlayerSessionManager, Session};
use crate::server::{
    level::{FullChunkStatus, ServerLevel, generated_chunk_owner::GenerationReadiness},
    network::connection_id::ConnectionId,
    player::play_sender::{PlaySendError, PlaySender},
    tick::registry::ConnectionRegistry,
};
use rivet_protocol::{
    generated::packets::play::clientbound::PacketType,
    protocol::game::clientbound_block_updates::{
        ClientboundBlockUpdatePacket, ClientboundLevelEventPacket,
        ClientboundSectionBlocksUpdatePacket,
    },
};
use rivet_registry::{
    RegistryAccess,
    block_state::BlockState,
    core::{BlockPos, ChunkPos, SectionPos, Vec3},
};
use rivet_util::short2object_open_hash_map::Short2ObjectOpenHashMap;
use rivet_world::{block_entity::NativeBlockEntityData, chunk::status::ChunkStatus};
use std::collections::{BTreeMap, HashMap, VecDeque};

#[derive(Debug, thiserror::Error)]
pub enum WorldTransportError {
    #[error("world packet encoding: {0}")]
    Encode(String),
    #[error("canonical world chunk {0} disappeared before block broadcast")]
    MissingChunk(ChunkPos),
    #[error("live block-entity update packet is unavailable at {0:?}")]
    BlockEntityUpdateUnavailable(BlockPos),
    #[error("lazy block-entity promotion authority is unavailable at {0:?}")]
    BlockEntityPromotionUnavailable(BlockPos),
    #[error("source-player visibility filtering is unavailable for this level event")]
    SourceVisibilityUnavailable,
}

#[derive(Default)]
pub struct WorldTransport {
    order: VecDeque<ChunkPos>,
    // Source visits sections bottom-up. Within each section append-only
    // Short2ObjectOpenHashMap<()> has ShortOpenHashSet's identical default
    // short hash/probing/growth and zero-first descending iterator order.
    blocks: HashMap<ChunkPos, BTreeMap<i32, Short2ObjectOpenHashMap<()>>>,
    events: VecDeque<LevelEvent>,
}
struct LevelEvent {
    packet: ClientboundLevelEventPacket,
    recipient: ConnectionId,
}

impl WorldTransport {
    /// Source blockChanged refuses unless a real watcher has received a
    /// send-ready FULL chunk. It never loads/generates merely for notification.
    pub fn queue_block_change(
        &mut self,
        level: &ServerLevel,
        pos: BlockPos,
        has_sent_watchers: bool,
    ) -> bool {
        let key = ChunkPos::containing(&pos);
        if !has_sent_watchers
            || level.chunk_map().get_chunk(key).is_none()
            || pos.get_y() < level.get_min_y()
            || pos.get_y() > level.get_max_y()
        {
            return false;
        }
        if let Some(owner) = level.generated_chunk_owner() {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    let neighbor = ChunkPos::new(key.x() + dx, key.z() + dz);
                    if !owner.is_promoted(neighbor)
                        || !matches!(
                            owner.readiness(neighbor, ChunkStatus::Full),
                            GenerationReadiness::Ready
                        )
                        || !owner
                            .current_full_status(neighbor)
                            .is_or_after(FullChunkStatus::Full)
                    {
                        return false;
                    }
                }
            }
        }
        if let Some(owner) = level.generated_chunk_owner()
            && owner.is_promoted(key)
            && (!matches!(
                owner.readiness(key, ChunkStatus::Full),
                GenerationReadiness::Ready
            ) || !owner
                .current_full_status(key)
                .is_or_after(FullChunkStatus::Full))
        {
            return false;
        }
        let first = !self.blocks.contains_key(&key);
        let sections = self.blocks.entry(key).or_insert_with(|| {
            self.order.push_back(key);
            BTreeMap::new()
        });
        sections
            .entry(pos.get_y() >> 4)
            .or_default()
            .put(SectionPos::section_relative_pos(&pos), ());
        first
    }
    /// `global=true` means the caller has already resolved GLOBAL_SOUND_EVENTS
    /// and selected globalLevelEvent; local lava fizz always passes false.
    pub fn queue_level_event(
        &mut self,
        event: i32,
        pos: BlockPos,
        data: i32,
        global: bool,
        except: Option<ConnectionId>,
        players: impl IntoIterator<Item = (ConnectionId, Vec3)>,
    ) -> Result<(), WorldTransportError> {
        if !global && except.is_some() {
            return Err(WorldTransportError::SourceVisibilityUnavailable);
        }
        let source = ClientboundLevelEventPacket {
            event,
            pos,
            data,
            global,
        };
        for (id, at) in players {
            let packet = if global {
                let (dx, dy, dz) = (
                    f64::from(pos.get_x()) + 0.5 - at.x,
                    f64::from(pos.get_y()) + 0.5 - at.y,
                    f64::from(pos.get_z()) + 0.5 - at.z,
                );
                let length = (dx * dx + dy * dy + dz * dz).sqrt();
                let sound = if length < 32.0 {
                    BlockPos::new(pos.get_x(), pos.get_y(), pos.get_z())
                } else {
                    BlockPos::new(
                        (at.x + dx / length * 32.0).floor() as i32,
                        (at.y + dy / length * 32.0).floor() as i32,
                        (at.z + dz / length * 32.0).floor() as i32,
                    )
                };
                ClientboundLevelEventPacket {
                    pos: sound,
                    ..source.clone()
                }
            } else {
                let (dx, dy, dz) = (
                    f64::from(pos.get_x()) - at.x,
                    f64::from(pos.get_y()) - at.y,
                    f64::from(pos.get_z()) - at.z,
                );
                if dx * dx + dy * dy + dz * dz >= 64.0 * 64.0 {
                    continue;
                }
                source.clone()
            };
            self.events.push_back(LevelEvent {
                packet,
                recipient: id,
            });
        }
        Ok(())
    }

    fn flush(
        &mut self,
        level: &ServerLevel,
        sessions: &HashMap<ConnectionId, Session>,
        sender: &mut PlaySender,
        connections: &mut ConnectionRegistry,
    ) -> Result<(), WorldTransportError> {
        while let Some(event) = self.events.pop_front() {
            let body = sender
                .encode_body(ClientboundLevelEventPacket::stream_codec(), &event.packet)
                .map_err(WorldTransportError::Encode)?;
            send(
                sender,
                connections,
                event.recipient,
                PacketType::LevelEvent.id(),
                &body,
            )?;
        }
        while let Some(&key) = self.order.front() {
            let targets: Vec<_> = sessions
                .iter()
                .filter_map(|(&id, session)| session.loader.has_sent(key).then_some(id))
                .collect();
            let sections = self.blocks.get_mut(&key).expect("queued source chunk");
            while let Some((y, positions)) = sections.pop_first() {
                if targets.is_empty() {
                    continue;
                }
                let chunk = level
                    .chunk_map()
                    .get_chunk(key)
                    .ok_or(WorldTransportError::MissingChunk(key))?;
                let section = SectionPos::of(key.x(), y, key.z());
                let changes: Vec<_> = positions
                    .entries()
                    .map(|(local, _)| {
                        let pos = section.relative_to_block_pos(local);
                        (
                            local,
                            BlockState::new(chunk.get_block_state(
                                pos.get_x(),
                                pos.get_y(),
                                pos.get_z(),
                            )),
                        )
                    })
                    .collect();
                if changes.len() == 1 {
                    let (local, state) = changes[0];
                    let packet = ClientboundBlockUpdatePacket {
                        pos: section.relative_to_block_pos(local),
                        state,
                    };
                    let body = sender
                        .encode_registry_body(
                            ClientboundBlockUpdatePacket::stream_codec(),
                            &packet,
                            &RegistryAccess::empty(),
                        )
                        .map_err(WorldTransportError::Encode)?;
                    for &id in &targets {
                        send(sender, connections, id, PacketType::BlockUpdate.id(), &body)?;
                    }
                } else {
                    let packet = ClientboundSectionBlocksUpdatePacket {
                        section,
                        changes: changes.clone(),
                    };
                    let body = sender
                        .encode_body(
                            ClientboundSectionBlocksUpdatePacket::stream_codec(),
                            &packet,
                        )
                        .map_err(WorldTransportError::Encode)?;
                    for &id in &targets {
                        send(
                            sender,
                            connections,
                            id,
                            PacketType::SectionBlocksUpdate.id(),
                            &body,
                        )?;
                    }
                }
                // Source sends the block packet before querying update packets.
                // Chest/hive inherit BlockEntity.getUpdatePacket == null;
                // spawner/heart require actual nonempty update packet bodies.
                for (local, state) in changes {
                    if !crate::server::level::structure_snapshot_data::has_block_entity(state) {
                        continue;
                    }
                    let pos = section.relative_to_block_pos(local);
                    if let Some(entity) = chunk.get_block_entity(&pos) {
                        match entity.data() {
                            NativeBlockEntityData::Chest(_) | NativeBlockEntityData::Beehive(_) => {
                            }
                            NativeBlockEntityData::Spawner(_)
                            | NativeBlockEntityData::CreakingHeart(_) => {
                                return Err(WorldTransportError::BlockEntityUpdateUnavailable(pos));
                            }
                        }
                    } else {
                        return Err(WorldTransportError::BlockEntityPromotionUnavailable(pos));
                    }
                }
            }
            self.order.pop_front();
            self.blocks.remove(&key);
        }
        Ok(())
    }
}
fn send(
    sender: &mut PlaySender,
    connections: &mut ConnectionRegistry,
    id: ConnectionId,
    packet: u32,
    body: &[u8],
) -> Result<(), WorldTransportError> {
    match sender.send_packet(connections, id, packet, body) {
        Ok(()) => Ok(()),
        // ConnectionRegistry already disconnects a gone/overflowed receiver;
        // it is not a failure of the world's canonical mutation.
        Err(PlaySendError::Outbound(error)) => {
            tracing::warn!(%id,%error,"sending world update");
            Ok(())
        }
        Err(PlaySendError::Encode(error)) => Err(WorldTransportError::Encode(error)),
    }
}
impl PlayerSessionManager {
    pub fn flush_world_transport(
        &mut self,
        connections: &mut ConnectionRegistry,
    ) -> Result<(), WorldTransportError> {
        self.world_transport
            .flush(&self.level, &self.sessions, &mut self.sender, connections)
    }
}

#[cfg(test)]
#[path = "world_transport_test.rs"]
mod tests;
