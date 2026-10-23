use super::super::{MoveState, SessionMovement, TeleportAckState};
use super::*;
use crate::server::{
    level::{ServerLevelConfig, player_chunk_loader::PlayerChunkLoader},
    player::ServerPlayer,
    tick::channels::{InboundDrained, LifecycleEvent, OutboundEvent},
};
use bytes::BytesMut;
use rivet_protocol::{
    codec::StreamDecoder, friendly_byte_buf::FriendlyByteBuf,
    registry_friendly_byte_buf::RegistryFriendlyByteBuf,
};
use rivet_registry::core::{GameProfile, GameType, create_offline_player_uuid};
use rivet_world::block::Block;

#[test]
fn batch_short_iteration_matches_actual_paper() {
    for row in
        include_str!("../../../../../tools/rivet-oracle/fixtures/chunk-block-batch.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(row).unwrap();
        let mut set = Short2ObjectOpenHashMap::default();
        for value in row["insertions"].as_array().unwrap() {
            set.put(value.as_i64().unwrap() as i16, ());
        }
        let actual: Vec<_> = set.entries().map(|(key, _)| i64::from(key)).collect();
        let expected: Vec<_> = row["iteration"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert_eq!(actual, expected);
    }
}

fn session(id: ConnectionId, at: Vec3, loader: PlayerChunkLoader) -> Session {
    Session {
        player: ServerPlayer::new(
            id,
            GameProfile::new_without_properties(
                create_offline_player_uuid("Transport"),
                "Transport".into(),
            ),
            id.0 as i32,
            at,
            0.,
            0.,
            GameType::Survival,
        ),
        movement: SessionMovement {
            teleport: TeleportAckState::new(),
            anchors: MoveState::new(at.x, at.y, at.z),
        },
        loader,
        requested_view_distance: None,
        accepted_frames: 0,
    }
}
fn connect(
    registry: &mut ConnectionRegistry,
    id: ConnectionId,
) -> tokio::sync::mpsc::Receiver<OutboundEvent> {
    let (_tx, in_rx) = tokio::sync::mpsc::channel(4);
    let (out_tx, out_rx) = tokio::sync::mpsc::channel(32);
    registry.apply(LifecycleEvent::Connect {
        id,
        remote: "127.0.0.1:25565".parse().unwrap(),
        in_rx,
        out_tx,
        drained: InboundDrained::new(),
    });
    out_rx
}
fn frame(receiver: &mut tokio::sync::mpsc::Receiver<OutboundEvent>) -> (u32, FriendlyByteBuf) {
    let OutboundEvent::Packet { frame } = receiver.try_recv().unwrap() else {
        panic!("expected packet")
    };
    let mut buffer = FriendlyByteBuf::new(BytesMut::from(frame.as_ref()));
    let size = buffer.read_var_int();
    assert_eq!(size as usize, buffer.as_slice().len());
    // PlaySender retains the compression envelope even for threshold -1;
    // declared length zero means the following packet payload is uncompressed.
    assert_eq!(buffer.read_var_int(), 0);
    (buffer.read_var_int() as u32, buffer)
}
fn write_fixture(level: &mut ServerLevel, pos: BlockPos, state: BlockState) {
    let base = level
        .chunk_map_mut()
        .get_chunk_mut(ChunkPos::ZERO)
        .unwrap()
        .generation_base_mut();
    let section = ((pos.get_y() - base.get_min_y()) >> 4) as usize;
    base.get_section_mut(section).set_block_state(
        pos.get_x() & 15,
        pos.get_y() & 15,
        pos.get_z() & 15,
        state.id(),
        &|id| BlockState::new(*id).is_air(),
        &|_| false,
        &|id| BlockState::new(*id).fluid_empty(),
        &|_| false,
        &|_| false,
    );
}

#[test]
fn outbound_batches_use_sent_watchers_and_current_states() {
    let mut level = ServerLevel::new(ServerLevelConfig::default());
    let mut sent = PlayerChunkLoader::new(ChunkPos::ZERO);
    sent.add_and_send_chunks(&mut level, None).unwrap();
    assert!(sent.has_sent(ChunkPos::ZERO));
    let mut sessions = HashMap::from([
        (
            ConnectionId(1),
            session(ConnectionId(1), Vec3::new(0., 64., 0.), sent),
        ),
        (
            ConnectionId(2),
            session(
                ConnectionId(2),
                Vec3::new(0., 64., 0.),
                PlayerChunkLoader::new(ChunkPos::ZERO),
            ),
        ),
    ]);
    let mut connections = ConnectionRegistry::new();
    let mut watcher = connect(&mut connections, ConnectionId(1));
    let mut not_sent = connect(&mut connections, ConnectionId(2));
    let mut sender = PlaySender::new(-1, RegistryAccess::empty(), RegistryAccess::empty());
    let mut transport = WorldTransport::default();
    let first = BlockPos::new(1, 64, 1);
    let stone = Block::from_name("minecraft:stone")
        .unwrap()
        .default_block_state();
    assert!(!transport.queue_block_change(&level, first, false));
    assert!(transport.queue_block_change(&level, first, true));
    assert!(!transport.queue_block_change(&level, first, true));
    write_fixture(&mut level, first, stone);
    transport
        .flush(&level, &sessions, &mut sender, &mut connections)
        .unwrap();
    let (id, body) = frame(&mut watcher);
    assert_eq!(id, PacketType::BlockUpdate.id());
    let mut body =
        RegistryFriendlyByteBuf::new(BytesMut::from(body.as_slice()), RegistryAccess::empty());
    assert_eq!(
        ClientboundBlockUpdatePacket::stream_codec()
            .decode(&mut body)
            .unwrap(),
        ClientboundBlockUpdatePacket {
            pos: first,
            state: stone
        }
    );
    assert!(not_sent.try_recv().is_err());
    let second = BlockPos::new(2, 64, 1);
    transport.queue_block_change(&level, second, true);
    transport.queue_block_change(&level, first, true);
    transport
        .flush(&level, &sessions, &mut sender, &mut connections)
        .unwrap();
    let (id, mut body) = frame(&mut watcher);
    assert_eq!(id, PacketType::SectionBlocksUpdate.id());
    let packet = ClientboundSectionBlocksUpdatePacket::stream_codec()
        .decode(&mut body)
        .unwrap();
    assert_eq!(packet.changes.len(), 2);
    assert!(
        packet
            .changes
            .contains(&(SectionPos::section_relative_pos(&first), stone))
    );
    assert!(
        packet.changes.contains(&(
            SectionPos::section_relative_pos(&second),
            BlockState::new(
                level
                    .chunk_map()
                    .get_chunk(ChunkPos::ZERO)
                    .unwrap()
                    .get_block_state(2, 64, 1)
            )
        ))
    );
    assert!(not_sent.try_recv().is_err());
    // Event recipients are captured at the call boundary, not at flush after movement.
    transport
        .queue_level_event(
            1501,
            first,
            0,
            false,
            None,
            [
                (ConnectionId(1), Vec3::new(1., 64., 1.)),
                (ConnectionId(2), Vec3::new(65., 64., 1.)),
            ],
        )
        .unwrap();
    sessions
        .get_mut(&ConnectionId(1))
        .unwrap()
        .player
        .abs_snap_to(200., 64., 1., 0., 0.);
    transport
        .flush(&level, &sessions, &mut sender, &mut connections)
        .unwrap();
    let (id, mut body) = frame(&mut watcher);
    assert_eq!(id, PacketType::LevelEvent.id());
    assert_eq!(
        ClientboundLevelEventPacket::stream_codec()
            .decode(&mut body)
            .unwrap()
            .pos,
        first
    );
    assert!(not_sent.try_recv().is_err());
    transport
        .flush(&level, &sessions, &mut sender, &mut connections)
        .unwrap();
    assert!(watcher.try_recv().is_err());
}
