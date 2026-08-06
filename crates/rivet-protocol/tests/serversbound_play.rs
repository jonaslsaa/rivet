//! Java-grounded tests for the issue #97 serverbound play packet bodies
//! (`crates/rivet-protocol/src/game/`).
//!
//! The six Java classes port here (ServerboundMovePlayerPacket + its four
//! subclasses, ServerboundChunkBatchReceivedPacket,
//! ServerboundAcceptTeleportationPacket, ServerboundClientCommandPacket,
//! ServerboundClientTickEndPacket, ServerboundPlayerActionPacket) are the
//! `mc.network.protocol.game.serverbound` manifest unit (MANIFEST line 263).
//! Their 9 concrete packets register in `GamePacketTypes`/`GameProtocols`
//! order. Because this slice registers only those 9 of the ~66 serverbound play
//! packets, `ProtocolInfoBuilder` assigns sequential registration-order ids
//! (0..8); the *vanilla* ids those land on in the full serverbound template are
//! pinned by the generated table (#50) — `generated::packets::play::serverbound`.
//! These tests pin that the real codecs:
//!   - register in `GamePacketTypes` order (registration id `n` == the `n`-th
//!     addPacket call), with the generated vanilla ids as the cross-check;
//!   - round-trip `[varint id + body]` through the bound `IdDispatchCodec`,
//!     byte-identically (the `rivet-protocol` slice of the #97 decode harness
//!     DoD);
//!   - reject an unknown id with Java's `DecoderException` text;
//!   - reject a mismatched flow with `ProtocolCodecBuilder.add`'s panic.
//!
//! Gated on the `packets` feature (the `game` module lives behind it).

use bytes::BytesMut;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, StreamEncoder, map};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::game::serversbound_accept_teleportation_packet::accept_teleportation_codec;
use rivet_protocol::game::serversbound_chunk_batch_received_packet::chunk_batch_received_codec;
use rivet_protocol::game::serversbound_client_command_packet::{
    Action as ClientCommandAction, client_command_codec,
};
use rivet_protocol::game::serversbound_client_tick_end_packet::client_tick_end_codec;
use rivet_protocol::game::serversbound_move_player_packet::{
    pos_codec, pos_rot_codec, rot_codec, status_only_codec,
};
use rivet_protocol::game::serversbound_player_action_packet::{
    Action as PlayerActionAction, player_action_codec,
};
use rivet_protocol::game::{
    ServerboundAcceptTeleportationPacket, ServerboundChunkBatchReceivedPacket,
    ServerboundClientCommandPacket, ServerboundClientTickEndPacket, ServerboundMovePlayerPacket,
    ServerboundPlayerActionPacket,
};
use rivet_protocol::generated::packets::play::serverbound::PacketType as PlayServerbound;
use rivet_protocol::generated::protocol::{ConnectionProtocol, PacketFlow};
use rivet_protocol::protocol::{Packet, PacketType, ProtocolInfoBuilder, serverbound_protocol};
use std::fmt;
use std::panic::catch_unwind;

/// The erased play/serverbound packet value: every real body variant.
#[derive(Debug, Clone, PartialEq)]
enum PlayServerboundPacket {
    AcceptTeleportation(ServerboundAcceptTeleportationPacket),
    ChunkBatchReceived(ServerboundChunkBatchReceivedPacket),
    ClientCommand(ServerboundClientCommandPacket),
    ClientTickEnd,
    MovePlayerPos(ServerboundMovePlayerPacket),
    MovePlayerPosRot(ServerboundMovePlayerPacket),
    MovePlayerRot(ServerboundMovePlayerPacket),
    MovePlayerStatusOnly(ServerboundMovePlayerPacket),
    PlayerAction(ServerboundPlayerActionPacket),
}

impl Packet for PlayServerboundPacket {
    fn packet_type(&self) -> PacketType {
        match self {
            PlayServerboundPacket::AcceptTeleportation(_) => {
                PacketType::serverbound("accept_teleportation")
            }
            PlayServerboundPacket::ChunkBatchReceived(_) => {
                PacketType::serverbound("chunk_batch_received")
            }
            PlayServerboundPacket::ClientCommand(_) => PacketType::serverbound("client_command"),
            PlayServerboundPacket::ClientTickEnd => PacketType::serverbound("client_tick_end"),
            PlayServerboundPacket::MovePlayerPos(_) => PacketType::serverbound("move_player_pos"),
            PlayServerboundPacket::MovePlayerPosRot(_) => {
                PacketType::serverbound("move_player_pos_rot")
            }
            PlayServerboundPacket::MovePlayerRot(_) => PacketType::serverbound("move_player_rot"),
            PlayServerboundPacket::MovePlayerStatusOnly(_) => {
                PacketType::serverbound("move_player_status_only")
            }
            PlayServerboundPacket::PlayerAction(_) => PacketType::serverbound("player_action"),
        }
    }
}

impl fmt::Display for PlayServerboundPacket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.packet_type())
    }
}

/// `StreamCodec.map` wrap/unwrap for a concrete body codec into the erased enum
/// value — Java's dispatch-table erasure
/// (`StreamCodec<? super B, ? extends V>` -> `StreamCodec<B, V>`).
fn wrap_codec<V: 'static, E: 'static>(
    codec: StreamCodec<FriendlyByteBuf, V>,
    wrap: impl Fn(&V) -> E + Send + Sync + 'static,
    unwrap: impl Fn(&E) -> V + Send + Sync + 'static,
) -> StreamCodec<FriendlyByteBuf, E> {
    map(codec, wrap, unwrap)
}

/// Registers the nine ported packets in `GamePacketTypes` order — the
/// `addPacket` calls of the play/serverbound template for exactly these
/// classes.
fn play_serverbound(b: &mut ProtocolInfoBuilder<PlayServerboundPacket, ()>) {
    b.add_packet(
        PacketType::serverbound("accept_teleportation"),
        wrap_codec(
            accept_teleportation_codec(),
            |v: &ServerboundAcceptTeleportationPacket| {
                PlayServerboundPacket::AcceptTeleportation(*v)
            },
            |p: &PlayServerboundPacket| match p {
                PlayServerboundPacket::AcceptTeleportation(v) => *v,
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        PacketType::serverbound("chunk_batch_received"),
        wrap_codec(
            chunk_batch_received_codec(),
            |v: &ServerboundChunkBatchReceivedPacket| PlayServerboundPacket::ChunkBatchReceived(*v),
            |p: &PlayServerboundPacket| match p {
                PlayServerboundPacket::ChunkBatchReceived(v) => *v,
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        PacketType::serverbound("client_command"),
        wrap_codec(
            client_command_codec(),
            |v: &ServerboundClientCommandPacket| PlayServerboundPacket::ClientCommand(*v),
            |p: &PlayServerboundPacket| match p {
                PlayServerboundPacket::ClientCommand(v) => *v,
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        PacketType::serverbound("client_tick_end"),
        wrap_codec(
            client_tick_end_codec(),
            |_: &ServerboundClientTickEndPacket| PlayServerboundPacket::ClientTickEnd,
            |p: &PlayServerboundPacket| match p {
                PlayServerboundPacket::ClientTickEnd => ServerboundClientTickEndPacket,
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        PacketType::serverbound("move_player_pos"),
        wrap_codec(
            pos_codec(),
            |v: &ServerboundMovePlayerPacket| PlayServerboundPacket::MovePlayerPos(v.clone()),
            |p: &PlayServerboundPacket| match p {
                PlayServerboundPacket::MovePlayerPos(v) => v.clone(),
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        PacketType::serverbound("move_player_pos_rot"),
        wrap_codec(
            pos_rot_codec(),
            |v: &ServerboundMovePlayerPacket| PlayServerboundPacket::MovePlayerPosRot(v.clone()),
            |p: &PlayServerboundPacket| match p {
                PlayServerboundPacket::MovePlayerPosRot(v) => v.clone(),
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        PacketType::serverbound("move_player_rot"),
        wrap_codec(
            rot_codec(),
            |v: &ServerboundMovePlayerPacket| PlayServerboundPacket::MovePlayerRot(v.clone()),
            |p: &PlayServerboundPacket| match p {
                PlayServerboundPacket::MovePlayerRot(v) => v.clone(),
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        PacketType::serverbound("move_player_status_only"),
        wrap_codec(
            status_only_codec(),
            |v: &ServerboundMovePlayerPacket| {
                PlayServerboundPacket::MovePlayerStatusOnly(v.clone())
            },
            |p: &PlayServerboundPacket| match p {
                PlayServerboundPacket::MovePlayerStatusOnly(v) => v.clone(),
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        PacketType::serverbound("player_action"),
        wrap_codec(
            player_action_codec(),
            |v: &ServerboundPlayerActionPacket| PlayServerboundPacket::PlayerAction(*v),
            |p: &PlayServerboundPacket| match p {
                PlayServerboundPacket::PlayerAction(v) => *v,
                _ => unreachable!(),
            },
        ),
    );
}

fn buf() -> FriendlyByteBuf {
    FriendlyByteBuf::new(BytesMut::new())
}

fn panic_message<F: FnOnce() -> R, R>(f: F) -> String {
    let err = match catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(_) => panic!("expected the closure to panic"),
        Err(err) => err,
    };
    err.downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

// ---------------------------------------------------------------------------
// addPacket order == vanilla network ids (pinned against GamePacketTypes).
// ---------------------------------------------------------------------------

#[test]
fn registration_order_and_generated_vanilla_ids() {
    // addPacket order in GamePacketTypes: accept_teleportation, chunk_batch_received,
    // client_command, client_tick_end, move_player_pos, move_player_pos_rot,
    // move_player_rot, move_player_status_only, player_action. This slice
    // registers only those 9 of the ~66 serverbound play packets, so the
    // builder assigns sequential registration-order ids (0..8).
    let template = serverbound_protocol::<PlayServerboundPacket>(ConnectionProtocol::Play, |b| {
        play_serverbound(b);
    });
    assert_eq!(
        template.details().list_packets(),
        &[
            (PacketType::serverbound("accept_teleportation"), 0),
            (PacketType::serverbound("chunk_batch_received"), 1),
            (PacketType::serverbound("client_command"), 2),
            (PacketType::serverbound("client_tick_end"), 3),
            (PacketType::serverbound("move_player_pos"), 4),
            (PacketType::serverbound("move_player_pos_rot"), 5),
            (PacketType::serverbound("move_player_rot"), 6),
            (PacketType::serverbound("move_player_status_only"), 7),
            (PacketType::serverbound("player_action"), 8),
        ]
    );
    // The generated table (#50) pins where these land in the *full* vanilla
    // serverbound template (GameProtocols), matching the Java GamePacketTypes
    // addPacket order.
    assert_eq!(PlayServerbound::AcceptTeleportation.id(), 0);
    assert_eq!(PlayServerbound::ChunkBatchReceived.id(), 11);
    assert_eq!(PlayServerbound::ClientCommand.id(), 12);
    assert_eq!(PlayServerbound::ClientTickEnd.id(), 13);
    assert_eq!(PlayServerbound::MovePlayerPos.id(), 30);
    assert_eq!(PlayServerbound::MovePlayerPosRot.id(), 31);
    assert_eq!(PlayServerbound::MovePlayerRot.id(), 32);
    assert_eq!(PlayServerbound::MovePlayerStatusOnly.id(), 33);
    assert_eq!(PlayServerbound::PlayerAction.id(), 41);
}

// ---------------------------------------------------------------------------
// Byte-identity round-trip through the bound IdDispatchCodec.
// ---------------------------------------------------------------------------

fn round_trip(
    codec: &StreamCodec<FriendlyByteBuf, PlayServerboundPacket>,
    value: &PlayServerboundPacket,
) -> Vec<u8> {
    let mut out = buf();
    codec.encode(&mut out, value).unwrap();
    let bytes = out.into_inner().to_vec();
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    assert_eq!(&codec.decode(&mut input).unwrap(), value);
    assert_eq!(input.readable_bytes(), 0, "no trailing bytes");
    bytes
}

#[test]
fn bound_codec_round_trips_all_nine_bodies_byte_identically() {
    let template = serverbound_protocol::<PlayServerboundPacket>(ConnectionProtocol::Play, |b| {
        play_serverbound(b);
    });
    let info = template.bind();
    let codec = info.codec();

    use rivet_registry::core::{BlockPos, Direction};

    // Registration-order ids (this slice registers 9 of the ~66 serverbound
    // play packets): accept_teleportation 0, chunk_batch_received 1,
    // client_command 2, client_tick_end 3, move_player_pos 4, pos_rot 5,
    // rot 6, status_only 7, player_action 8. The vanilla ids (0/11/12/13/
    // 30/31/32/33/41) are pinned in `registration_order_and_generated_vanilla_ids`.

    // accept_teleportation id=0 -> wire [0, 0x00].
    let bytes = round_trip(
        codec,
        &PlayServerboundPacket::AcceptTeleportation(ServerboundAcceptTeleportationPacket::new(0)),
    );
    assert_eq!(bytes, vec![0x00, 0x00]);

    // chunk_batch_received id=1 -> [0x01, float].
    let bytes = round_trip(
        codec,
        &PlayServerboundPacket::ChunkBatchReceived(ServerboundChunkBatchReceivedPacket::new(3.5)),
    );
    let mut expected = vec![0x01];
    expected.extend_from_slice(&3.5f32.to_be_bytes());
    assert_eq!(bytes, expected);

    // client_command id=2, PERFORM_RESPAWN (0) -> [0x02, 0x00].
    let bytes = round_trip(
        codec,
        &PlayServerboundPacket::ClientCommand(ServerboundClientCommandPacket::new(
            ClientCommandAction::PerformRespawn,
        )),
    );
    assert_eq!(bytes, vec![0x02, 0x00]);

    // client_tick_end id=3, unit codec -> [0x03] (no body).
    let bytes = round_trip(codec, &PlayServerboundPacket::ClientTickEnd);
    assert_eq!(bytes, vec![0x03]);

    // move_player_pos id=4 -> [0x04, 3 doubles + flags].
    let bytes = round_trip(
        codec,
        &PlayServerboundPacket::MovePlayerPos(ServerboundMovePlayerPacket::Pos {
            x: 1.5,
            y: -2.25,
            z: 3.5,
            on_ground: true,
            horizontal_collision: false,
        }),
    );
    assert_eq!(bytes.len(), 26); // id + 25-byte body

    // move_player_pos_rot id=5 -> [0x05, 3 doubles + 2 floats + flags].
    let bytes = round_trip(
        codec,
        &PlayServerboundPacket::MovePlayerPosRot(ServerboundMovePlayerPacket::PosRot {
            x: 10.0,
            y: 64.0,
            z: -8.0,
            y_rot: 90.0,
            x_rot: -45.0,
            on_ground: true,
            horizontal_collision: true,
        }),
    );
    assert_eq!(bytes.len(), 34); // id + 33-byte body

    // move_player_rot id=6 -> [0x06, 2 floats + flags].
    let bytes = round_trip(
        codec,
        &PlayServerboundPacket::MovePlayerRot(ServerboundMovePlayerPacket::Rot {
            y_rot: 180.0,
            x_rot: 0.5,
            on_ground: false,
            horizontal_collision: true,
        }),
    );
    assert_eq!(bytes.len(), 10); // id + 9-byte body

    // move_player_status_only id=7 -> [0x07, flags].
    let bytes = round_trip(
        codec,
        &PlayServerboundPacket::MovePlayerStatusOnly(ServerboundMovePlayerPacket::StatusOnly {
            on_ground: true,
            horizontal_collision: false,
        }),
    );
    assert_eq!(bytes, vec![0x07, 0x01]);

    // player_action id=8 -> [0x08, varint ordinal + 8 packed longs + direction + varint seq].
    let bytes = round_trip(
        codec,
        &PlayServerboundPacket::PlayerAction(ServerboundPlayerActionPacket::new(
            PlayerActionAction::StartDestroyBlock,
            BlockPos::new(1, 2, 3),
            Direction::Down,
            0,
        )),
    );
    assert_eq!(bytes.len(), 12); // id + 11-byte body
}

#[test]
fn bound_codec_rejects_unknown_id_with_java_message() {
    let template = serverbound_protocol::<PlayServerboundPacket>(ConnectionProtocol::Play, |b| {
        play_serverbound(b);
    });
    let info = template.bind();
    let mut input = buf();
    input.write_var_int(42); // serverbound move_vehicle, not in this slice
    let err = info.codec().decode(&mut input).unwrap_err();
    assert_eq!(err.message, "Received unknown packet id 42");
}

#[test]
fn flow_mismatch_panics_with_java_message() {
    // A clientbound type into the serverbound template -> ProtocolCodecBuilder.add's panic.
    let mut b = ProtocolInfoBuilder::<PlayServerboundPacket, ()>::new(
        ConnectionProtocol::Play,
        PacketFlow::Serverbound,
    );
    let msg = panic_message(|| {
        b.add_packet(
            PacketType::clientbound("player_position"),
            wrap_codec(
                pos_rot_codec(),
                |v: &ServerboundMovePlayerPacket| {
                    PlayServerboundPacket::MovePlayerPosRot(v.clone())
                },
                |p: &PlayServerboundPacket| match p {
                    PlayServerboundPacket::MovePlayerPosRot(v) => v.clone(),
                    _ => unreachable!(),
                },
            ),
        );
        b.build_unbound(());
    });
    assert_eq!(
        msg,
        "Invalid packet flow for packet clientbound/minecraft:player_position, expected SERVERBOUND"
    );
}

#[test]
fn move_player_variants_encode_distinct_vanilla_ids() {
    // The four MovePlayer subclasses are four distinct packets with four ids.
    let template = serverbound_protocol::<PlayServerboundPacket>(ConnectionProtocol::Play, |b| {
        play_serverbound(b);
    });
    let info = template.bind();
    let codec = info.codec();

    let pos = ServerboundMovePlayerPacket::Pos {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        on_ground: false,
        horizontal_collision: false,
    };
    let mut out = buf();
    codec
        .encode(&mut out, &PlayServerboundPacket::MovePlayerPos(pos))
        .unwrap();
    // Registration-order id (4); vanilla id 30 is pinned by the generated table.
    assert_eq!(out.into_inner().as_ref()[0], 4);

    let rot = ServerboundMovePlayerPacket::Rot {
        y_rot: 0.0,
        x_rot: 0.0,
        on_ground: false,
        horizontal_collision: false,
    };
    let mut out = buf();
    codec
        .encode(&mut out, &PlayServerboundPacket::MovePlayerRot(rot))
        .unwrap();
    // Registration-order id (6); vanilla id 32 is pinned by the generated table.
    assert_eq!(out.into_inner().as_ref()[0], 6);
}
