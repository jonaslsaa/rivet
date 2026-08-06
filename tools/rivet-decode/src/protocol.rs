//! The vanilla-id serverbound-play dispatch table used by the harness.
//!
//! [`ProtocolInfoBuilder`] assigns network ids in `addPacket` registration
//! order, so a slice of nine packets gets ids `0..8` — not the vanilla ids
//! (`accept_teleportation 0`, `chunk_batch_received 11`, ..., `player_action
//! 41`). A real capture is framed with the *vanilla* ids, so this harness does
//! not go through the builder: it builds a table of 69 entries indexed by the
//! generated protocol id (`generated::packets::play::serverbound::PacketType`,
//! where `id() == index`). The nine ported packets decode with their real
//! codecs (`rivet_protocol::game`); the other sixty are raw passthrough — their
//! body bytes are captured, not interpreted, exactly as the harness must treat
//! packets it does not yet have a codec for.
//!
//! Java fidelity notes:
//! - `IdDispatchCodec.decode` rejects an id out of the registered range with
//!   `"Received unknown packet id {n}"`; here the registered range is the full
//!   `0..69` vanilla table, so an unknown id is `>= 69` (or negative).
//! - `PacketDecoder.decode` requires the whole body to be consumed; a real
//!   codec that leaves trailing bytes is an error, not silently accepted.
//! - Unchecked Java exceptions (`ArrayIndexOutOfBoundsException` from
//!   `readEnum`, `RuntimeException("VarInt too big")`, the bytes-buffer short
//!   read) map to panics in this port. [`decode_frame`] catches them and
//!   returns the panic message as the error, mirroring how the Rust codec
//!   layer surfaces Java's unchecked exceptions.

use bytes::BytesMut;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, StreamEncoder, map};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::game::serversbound_accept_teleportation_packet::accept_teleportation_codec;
use rivet_protocol::game::serversbound_chunk_batch_received_packet::chunk_batch_received_codec;
use rivet_protocol::game::serversbound_client_command_packet::client_command_codec;
use rivet_protocol::game::serversbound_client_tick_end_packet::client_tick_end_codec;
use rivet_protocol::game::serversbound_move_player_packet::{
    ServerboundMovePlayerPacket, pos_codec, pos_rot_codec, rot_codec, status_only_codec,
};
use rivet_protocol::game::serversbound_player_action_packet::player_action_codec;
use rivet_protocol::game::{
    ServerboundAcceptTeleportationPacket, ServerboundChunkBatchReceivedPacket,
    ServerboundClientCommandPacket, ServerboundClientTickEndPacket, ServerboundPlayerActionPacket,
};
use rivet_protocol::generated::packets::play::serverbound::PacketType as PlayServerbound;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::LazyLock;

/// The erased packet value this harness decodes into. The nine ported packets
/// carry their real bodies; anything else is [`PlayPacket::Raw`] (id + the
/// untouched body bytes).
#[derive(Debug, Clone, PartialEq)]
pub enum PlayPacket {
    AcceptTeleportation(ServerboundAcceptTeleportationPacket),
    ChunkBatchReceived(ServerboundChunkBatchReceivedPacket),
    ClientCommand(ServerboundClientCommandPacket),
    ClientTickEnd,
    MovePlayerPos(ServerboundMovePlayerPacket),
    MovePlayerPosRot(ServerboundMovePlayerPacket),
    MovePlayerRot(ServerboundMovePlayerPacket),
    MovePlayerStatusOnly(ServerboundMovePlayerPacket),
    PlayerAction(ServerboundPlayerActionPacket),
    Raw { id: i32, body: Vec<u8> },
}

/// One vanilla table slot.
struct Entry {
    name: &'static str,
    codec: StreamCodec<FriendlyByteBuf, PlayPacket>,
    /// Whether the slot decodes a ported packet (raw slots capture bytes).
    real: bool,
}

/// The full 69-entry serverbound-play table, indexed by vanilla protocol id.
static TABLE: LazyLock<Vec<Entry>> = LazyLock::new(build_table);

/// A raw slot: decode consumes the whole body and records it; encode writes it
/// back verbatim (the id is written by the caller).
fn raw_codec(id: i32) -> StreamCodec<FriendlyByteBuf, PlayPacket> {
    rivet_protocol::codec::of(
        move |out: &mut FriendlyByteBuf, p: &PlayPacket| match p {
            PlayPacket::Raw { body, .. } => {
                out.write_bytes(body);
                Ok(())
            }
            _ => unreachable!("raw slot encodes a Raw packet"),
        },
        move |input: &mut FriendlyByteBuf| {
            let body = input.read_slice(input.readable_bytes() as i32);
            Ok(PlayPacket::Raw { id, body })
        },
    )
}

fn build_table() -> Vec<Entry> {
    let mut table = Vec::with_capacity(69);
    for id in 0..69u32 {
        let name = PlayServerbound::from_id(id).unwrap().name();
        let entry = match id {
            0 => Entry {
                name,
                codec: map(
                    accept_teleportation_codec(),
                    |v: &ServerboundAcceptTeleportationPacket| PlayPacket::AcceptTeleportation(*v),
                    |p: &PlayPacket| match p {
                        PlayPacket::AcceptTeleportation(v) => *v,
                        _ => unreachable!(),
                    },
                ),
                real: true,
            },
            11 => Entry {
                name,
                codec: map(
                    chunk_batch_received_codec(),
                    |v: &ServerboundChunkBatchReceivedPacket| PlayPacket::ChunkBatchReceived(*v),
                    |p: &PlayPacket| match p {
                        PlayPacket::ChunkBatchReceived(v) => *v,
                        _ => unreachable!(),
                    },
                ),
                real: true,
            },
            12 => Entry {
                name,
                codec: map(
                    client_command_codec(),
                    |v: &ServerboundClientCommandPacket| PlayPacket::ClientCommand(*v),
                    |p: &PlayPacket| match p {
                        PlayPacket::ClientCommand(v) => *v,
                        _ => unreachable!(),
                    },
                ),
                real: true,
            },
            13 => Entry {
                name,
                codec: map(
                    client_tick_end_codec(),
                    |_: &ServerboundClientTickEndPacket| PlayPacket::ClientTickEnd,
                    |p: &PlayPacket| match p {
                        PlayPacket::ClientTickEnd => ServerboundClientTickEndPacket,
                        _ => unreachable!(),
                    },
                ),
                real: true,
            },
            30 => Entry {
                name,
                codec: map(
                    pos_codec(),
                    |v: &ServerboundMovePlayerPacket| PlayPacket::MovePlayerPos(v.clone()),
                    |p: &PlayPacket| match p {
                        PlayPacket::MovePlayerPos(v) => v.clone(),
                        _ => unreachable!(),
                    },
                ),
                real: true,
            },
            31 => Entry {
                name,
                codec: map(
                    pos_rot_codec(),
                    |v: &ServerboundMovePlayerPacket| PlayPacket::MovePlayerPosRot(v.clone()),
                    |p: &PlayPacket| match p {
                        PlayPacket::MovePlayerPosRot(v) => v.clone(),
                        _ => unreachable!(),
                    },
                ),
                real: true,
            },
            32 => Entry {
                name,
                codec: map(
                    rot_codec(),
                    |v: &ServerboundMovePlayerPacket| PlayPacket::MovePlayerRot(v.clone()),
                    |p: &PlayPacket| match p {
                        PlayPacket::MovePlayerRot(v) => v.clone(),
                        _ => unreachable!(),
                    },
                ),
                real: true,
            },
            33 => Entry {
                name,
                codec: map(
                    status_only_codec(),
                    |v: &ServerboundMovePlayerPacket| PlayPacket::MovePlayerStatusOnly(v.clone()),
                    |p: &PlayPacket| match p {
                        PlayPacket::MovePlayerStatusOnly(v) => v.clone(),
                        _ => unreachable!(),
                    },
                ),
                real: true,
            },
            41 => Entry {
                name,
                codec: map(
                    player_action_codec(),
                    |v: &ServerboundPlayerActionPacket| PlayPacket::PlayerAction(*v),
                    |p: &PlayPacket| match p {
                        PlayPacket::PlayerAction(v) => *v,
                        _ => unreachable!(),
                    },
                ),
                real: true,
            },
            _ => Entry {
                name,
                codec: raw_codec(id as i32),
                real: false,
            },
        };
        table.push(entry);
    }
    table
}

/// The outcome of decoding one packet frame body (the bytes after the varint21
/// frame header, i.e. `[packet id varint + body]`).
#[derive(Debug)]
pub struct Decoded {
    pub id: i32,
    pub name: String,
    pub packet: PlayPacket,
    pub body_hex: String,
}

/// Decode a single packet frame body. Unchecked Java exceptions (readEnum
/// out-of-range, `VarInt too big`, short reads) surface as panics in this
/// port; they are caught and returned as `Err` with the exact panic text,
/// exactly as a caller would observe the unchecked exception from the Java
/// codec.
pub fn decode_frame(body: &[u8]) -> Result<Decoded, String> {
    match catch_unwind(AssertUnwindSafe(|| decode_frame_inner(body))) {
        Ok(result) => result,
        Err(payload) => Err(payload_message(payload)),
    }
}

fn payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

fn decode_frame_inner(body: &[u8]) -> Result<Decoded, String> {
    let mut input = FriendlyByteBuf::new(BytesMut::from(body));
    let id = input.read_var_int();
    let entry = match TABLE.get(id as usize) {
        Some(e) if id >= 0 => e,
        _ => return Err(format!("Received unknown packet id {id}")),
    };
    let body_hex = hex(body);
    let packet = if entry.real {
        let value = entry
            .codec
            .decode(&mut input)
            .map_err(|e| format!("Failed to decode packet '{}': {}", entry.name, e.message))?;
        if input.readable_bytes() != 0 {
            return Err(format!(
                "Failed to decode packet '{}': {} trailing bytes",
                entry.name,
                input.readable_bytes()
            ));
        }
        value
    } else {
        entry
            .codec
            .decode(&mut input)
            .map_err(|e| format!("Failed to decode packet '{}': {}", entry.name, e.message))?
    };
    Ok(Decoded {
        id,
        name: entry.name.to_string(),
        packet,
        body_hex,
    })
}

/// Encode a packet (id varint + body) for corpus capture.
pub fn encode_packet(id: i32, packet: &PlayPacket) -> Result<Vec<u8>, String> {
    let entry = match TABLE.get(id as usize) {
        Some(e) => e,
        None => return Err(format!("no table entry for id {id}")),
    };
    let mut out = FriendlyByteBuf::new(BytesMut::new());
    out.write_var_int(id);
    entry
        .codec
        .encode(&mut out, packet)
        .map_err(|e| format!("Failed to encode packet '{}': {}", entry.name, e.message))?;
    Ok(out.into_inner().to_vec())
}

/// The canonical packet name for an id (from the generated table).
pub fn packet_name(id: i32) -> Option<&'static str> {
    PlayServerbound::from_id(id as u32).map(|t| t.name())
}

/// Lowercase hex of raw bytes.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Parse a hex string back into bytes (whitespace ignored).
pub fn unhex(s: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !cleaned.len().is_multiple_of(2) {
        return Err(format!("odd hex length: {}", cleaned.len()));
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    let mut iter = cleaned.bytes();
    while let (Some(hi), Some(lo)) = (iter.next(), iter.next()) {
        let hi = (hi as char).to_digit(16).ok_or("non-hex char")?;
        let lo = (lo as char).to_digit(16).ok_or("non-hex char")?;
        out.push((hi * 16 + lo) as u8);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Normalized transcript
// ---------------------------------------------------------------------------

/// The normalized JSON object for one decoded packet. Deterministic: every
/// field key is emitted in the same order, floats are raw IEEE-754 bits as
/// lowercase hex (NaN/Inf included), enums use the Java constant name.
pub fn transcript_line(seq: usize, decoded: &Decoded) -> String {
    serde_json::to_string(&json!({
        "seq": seq,
        "id": decoded.id,
        "name": decoded.name,
        "kind": kind(&decoded.packet),
        "fields": fields_for(&decoded.packet),
        "frame_hex": decoded.body_hex,
    }))
    .expect("json serialization is infallible")
}

fn kind(packet: &PlayPacket) -> &'static str {
    match packet {
        PlayPacket::AcceptTeleportation(_) => "accept_teleportation",
        PlayPacket::ChunkBatchReceived(_) => "chunk_batch_received",
        PlayPacket::ClientCommand(_) => "client_command",
        PlayPacket::ClientTickEnd => "client_tick_end",
        PlayPacket::MovePlayerPos(_) => "move_player_pos",
        PlayPacket::MovePlayerPosRot(_) => "move_player_pos_rot",
        PlayPacket::MovePlayerRot(_) => "move_player_rot",
        PlayPacket::MovePlayerStatusOnly(_) => "move_player_status_only",
        PlayPacket::PlayerAction(_) => "player_action",
        PlayPacket::Raw { .. } => "raw",
    }
}

fn f32_bits(v: f32) -> String {
    format!("0x{:08x}", v.to_bits())
}

fn f64_bits(v: f64) -> String {
    format!("0x{:016x}", v.to_bits())
}

fn fields_for(packet: &PlayPacket) -> Value {
    match packet {
        PlayPacket::AcceptTeleportation(p) => json!({ "teleport_id": p.get_id() }),
        PlayPacket::ChunkBatchReceived(p) => {
            json!({ "desired_chunks_per_tick_bits": f32_bits(p.desired_chunks_per_tick()) })
        }
        PlayPacket::ClientCommand(p) => json!({ "action": format!("{:?}", p.get_action()) }),
        PlayPacket::ClientTickEnd => json!({}),
        PlayPacket::MovePlayerPos(p) => json!({
            "x_bits": f64_bits(p.get_x(0.0)),
            "y_bits": f64_bits(p.get_y(0.0)),
            "z_bits": f64_bits(p.get_z(0.0)),
            "on_ground": p.is_on_ground(),
            "horizontal_collision": p.horizontal_collision(),
        }),
        PlayPacket::MovePlayerPosRot(p) => json!({
            "x_bits": f64_bits(p.get_x(0.0)),
            "y_bits": f64_bits(p.get_y(0.0)),
            "z_bits": f64_bits(p.get_z(0.0)),
            "y_rot_bits": f32_bits(p.get_y_rot(0.0)),
            "x_rot_bits": f32_bits(p.get_x_rot(0.0)),
            "on_ground": p.is_on_ground(),
            "horizontal_collision": p.horizontal_collision(),
        }),
        PlayPacket::MovePlayerRot(p) => json!({
            "y_rot_bits": f32_bits(p.get_y_rot(0.0)),
            "x_rot_bits": f32_bits(p.get_x_rot(0.0)),
            "on_ground": p.is_on_ground(),
            "horizontal_collision": p.horizontal_collision(),
        }),
        PlayPacket::MovePlayerStatusOnly(p) => json!({
            "on_ground": p.is_on_ground(),
            "horizontal_collision": p.horizontal_collision(),
        }),
        PlayPacket::PlayerAction(p) => {
            let pos = p.get_pos();
            json!({
                "action": format!("{:?}", p.get_action()),
                "position": [pos.get_x(), pos.get_y(), pos.get_z()],
                "direction": format!("{:?}", p.get_direction()),
                "sequence": p.get_sequence(),
            })
        }
        PlayPacket::Raw { body, .. } => json!({ "body_hex": hex(body) }),
    }
}
