//! Fuzz target: the play-protocol serverbound packet decode paths.
//!
//! Feeds arbitrary bytes through the real registration-built dispatch codec
//! (`serverbound_protocol` + `ProtocolInfoBuilder` + `IdDispatchCodec`) for the
//! nine ported serverbound play bodies (issue #97): `accept_teleportation`,
//! `chunk_batch_received`, `client_command`, `client_tick_end`, the four
//! `move_player` variants (pos / pos_rot / rot / status_only), and
//! `player_action`.
//!
//! The interesting hostile surfaces here are the raw-scalar bodies: a
//! `client_command`/`player_action` ordinal outside the enum panics faithfully
//! (`Index n out of bounds for length m`), and a short read panics faithfully
//! (EOF). An unknown packet id returns `Err` (`"Received unknown packet id n"`).
//! Every other hostile shape resolves to `Err` or a faithful panic — anything
//! else aborts the fuzzer and writes an artifact.
#![no_main]
use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, map};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::game::serversbound_accept_teleportation_packet::{
    ServerboundAcceptTeleportationPacket, accept_teleportation_codec,
};
use rivet_protocol::game::serversbound_chunk_batch_received_packet::{
    ServerboundChunkBatchReceivedPacket, chunk_batch_received_codec,
};
use rivet_protocol::game::serversbound_client_command_packet::{
    ServerboundClientCommandPacket, client_command_codec,
};
use rivet_protocol::game::serversbound_client_tick_end_packet::{
    ServerboundClientTickEndPacket, client_tick_end_codec,
};
use rivet_protocol::game::serversbound_move_player_packet::{
    ServerboundMovePlayerPacket, pos_codec, pos_rot_codec, rot_codec, status_only_codec,
};
use rivet_protocol::game::serversbound_player_action_packet::{
    ServerboundPlayerActionPacket, player_action_codec,
};
use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::protocol::{Packet, PacketType, ProtocolInfoBuilder, serverbound_protocol};
use std::sync::OnceLock;

mod guard;
use guard::guarded;

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

/// The erased play/serverbound packet value: every ported body variant,
/// registered in `GamePacketTypes` order (see `tests/serversbound_play.rs`).
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

fn dispatch_codec() -> &'static StreamCodec<FriendlyByteBuf, PlayServerboundPacket> {
    static CODEC: OnceLock<StreamCodec<FriendlyByteBuf, PlayServerboundPacket>> = OnceLock::new();
    CODEC.get_or_init(|| {
        let template =
            serverbound_protocol::<PlayServerboundPacket>(ConnectionProtocol::Play, |b| {
                play_serverbound(b);
            });
        template.bind().codec().clone()
    })
}

fuzz_target!(|data: &[u8]| {
    if data.len() > guard::MAX_INPUT_LEN {
        return;
    }
    guarded(|| {
        let mut input = FriendlyByteBuf::new(BytesMut::from(data));
        let _ = dispatch_codec().decode(&mut input);
    });
});
