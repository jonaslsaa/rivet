//! Id-matched and coordinate relationships over the RAW capture (#195).
//!
//! `normalize.rs` rewrites the teleport ids, keepalive ids, movement x/z, and
//! spawn coordinates to canonical constants, so those fields carry no signal
//! after normalization. The relationships below run before normalization, on
//! the raw bodies, where the real values are still comparable:
//!
//! - teleport → ack: every `accept_teleportation` id must echo an issued
//!   `player_position` id, in order (Paper gates `handleMovePlayer` on
//!   `awaitingPositionFromClient` being cleared by the ack).
//! - keepalive request → echo: every `serverbound keep_alive` body must equal a
//!   *prior* `clientbound keep_alive` body (the server disconnects on an
//!   unmatched or out-of-order echo).
//! - spawn consistency: the spawn `player_position` y equals the movement
//!   sample y and the `set_default_spawn_position` y; the chunk column of the
//!   spawn equals `set_chunk_cache_center`.
//! - entity ids: `login`'s writeInt playerId, `entity_event`'s writeInt entity
//!   id, and the `set_entity_data` VarInt entity id must all agree (the player
//!   is the only entity on this scenario).
//!
//! The cross-direction ordering of unrelated packets is a proxy merge artifact,
//! so response-chain checks here use id/content matching rather than position.

use crate::frame;
use crate::invariants::Failure;
use crate::packet::{CapturedPacket, Direction, State};

/// Parse the teleport id (leading VarInt) of a `player_position` body.
fn player_position_id(body: &[u8]) -> Option<i32> {
    let mut off = 0;
    frame::read_varint(body, &mut off)
}

/// Parse `(x, y, z)` out of a `player_position` body: `[VarInt id][x][y][z]...`.
fn player_position_xyz(body: &[u8]) -> Option<(f64, f64, f64)> {
    let mut off = 0;
    frame::read_varint(body, &mut off)?;
    let x = frame::read_f64(body, &mut off)?;
    let y = frame::read_f64(body, &mut off)?;
    let z = frame::read_f64(body, &mut off)?;
    Some((x, y, z))
}

/// Parse the movement sample `(x, y, z)` of a `move_player_pos`/`pos_rot` body
/// (serverbound, no teleport id).
fn move_xyz(body: &[u8]) -> Option<(f64, f64, f64)> {
    let mut off = 0;
    let x = frame::read_f64(body, &mut off)?;
    let y = frame::read_f64(body, &mut off)?;
    let z = frame::read_f64(body, &mut off)?;
    Some((x, y, z))
}

/// Decode a `set_default_spawn_position` body:
/// `[ResourceKey world][BlockPos as a fixed 8-byte Long][f32 yaw][f32 pitch]`,
/// where the packed BlockPos long is `x<<38 | z<<12 | (y & 0xFFF)`
/// (`FriendlyByteBuf.writeBlockPos` = `writeLong`, not a VarLong).
/// Returns `(world, x, y, z)`.
fn spawn_packed(body: &[u8]) -> Option<(String, i32, i32, i32)> {
    let mut off = 0;
    let len = frame::read_varint(body, &mut off)?;
    if len < 0 {
        return None;
    }
    let world = String::from_utf8(body.get(off..off + len as usize)?.to_vec()).ok()?;
    off += len as usize;
    let packed = frame::read_i64(body, &mut off)?;
    let x = (packed >> 38) as i32;
    let z = ((packed >> 12) & 0x3FF_FFFF) as i32;
    let y = (packed & 0xFFF) as i32;
    // Sign-extend the 12-bit y.
    let y = if y >= 2048 { y - 4096 } else { y };
    Some((world, x, y, z))
}

fn pkt_identity(state: State, direction: Direction, id: i32) -> String {
    crate::ordering::identity(state, direction, id)
}

/// Run the relationship detectors over the raw capture.
pub fn check(packets: &[CapturedPacket]) -> Vec<Failure> {
    let mut f = Vec::new();
    check_teleport_ack(packets, &mut f);
    check_keepalive_echo(packets, &mut f);
    check_spawn_consistency(packets, &mut f);
    check_entity_ids(packets, &mut f);
    f
}

fn check_teleport_ack(packets: &[CapturedPacket], f: &mut Vec<Failure>) {
    let issued: Vec<i32> = packets
        .iter()
        .filter(|p| p.state == State::Play && p.direction == Direction::Clientbound && p.id == 72)
        .filter_map(|p| player_position_id(&p.body))
        .collect();
    let acks: Vec<i32> = packets
        .iter()
        .filter(|p| p.state == State::Play && p.direction == Direction::Serverbound && p.id == 0)
        .filter_map(|p| frame::read_varint(&p.body, &mut 0))
        .collect();

    if issued.is_empty() && acks.is_empty() {
        return;
    }
    // The ack sequence must be a prefix of the issued-id sequence: the client
    // acks teleports in order and only teleports the server issued.
    let mut i = 0;
    for ack in &acks {
        if issued.get(i).is_some_and(|id| id == ack) {
            i += 1;
            continue;
        }
        f.push(Failure::new(
            "teleport-ack",
            format!(
                "{} with teleport id {ack}",
                pkt_identity(State::Play, Direction::Serverbound, 0)
            ),
            format!("acknowledges a teleport the server did not issue (issued: {issued:?})"),
        ));
    }
    if i < issued.len() {
        f.push(Failure::new(
            "teleport-ack",
            format!(
                "{} with teleport id {}",
                pkt_identity(State::Play, Direction::Clientbound, 72),
                issued[i]
            ),
            "issued but never acknowledged by accept_teleportation",
        ));
    }
}

fn check_keepalive_echo(packets: &[CapturedPacket], f: &mut Vec<Failure>) {
    let mut challenges: Vec<Vec<u8>> = Vec::new();
    for p in packets {
        if p.state == State::Play {
            if p.direction == Direction::Clientbound && p.id == 44 {
                challenges.push(p.body.clone());
            } else if p.direction == Direction::Serverbound && p.id == 28 {
                let echoed = challenges.iter().find(|c| **c == p.body);
                if echoed.is_none() {
                    f.push(Failure::new(
                        "keepalive",
                        format!(
                            "{} with body {}",
                            pkt_identity(State::Play, Direction::Serverbound, 28),
                            crate::fixture::hex(&p.body)
                        ),
                        "serverbound keep_alive does not echo any prior clientbound keep_alive body",
                    ));
                }
            }
        }
    }
}

fn check_spawn_consistency(packets: &[CapturedPacket], f: &mut Vec<Failure>) {
    // The first player_position is the spawn teleport.
    let spawn = packets
        .iter()
        .find(|p| p.state == State::Play && p.direction == Direction::Clientbound && p.id == 72)
        .and_then(|p| player_position_xyz(&p.body));

    let mut cache_center: Option<(i32, i32)> = None;
    let mut default_spawn: Option<(String, i32, i32, i32)> = None;
    for p in packets {
        if p.state != State::Play {
            continue;
        }
        match (p.direction, p.id) {
            (Direction::Clientbound, 94) => {
                let mut off = 0;
                let cx = frame::read_varint(&p.body, &mut off);
                let cz = frame::read_varint(&p.body, &mut off);
                if let (Some(cx), Some(cz)) = (cx, cz) {
                    cache_center = Some((cx, cz));
                }
            }
            (Direction::Clientbound, 97) => {
                default_spawn = spawn_packed(&p.body);
            }
            _ => {}
        }
    }

    if let Some((_, sy, _)) = spawn {
        // The first post-spawn movement sample (pos or pos_rot) keeps the same y.
        let sample = packets.iter().find(|p| {
            p.state == State::Play
                && p.direction == Direction::Serverbound
                && matches!(p.id, 30 | 31)
        });
        let move_y = sample.and_then(|p| move_xyz(&p.body)).map(|(_, y, _)| y);
        if let Some(my) = move_y.filter(|my| (my - sy).abs() > 0.0001)
            && let Some(sample) = sample
        {
            f.push(Failure::new(
                "spawn",
                format!(
                    "{} y {my}",
                    pkt_identity(State::Play, Direction::Serverbound, sample.id)
                ),
                format!("movement sample y {my} disagrees with the spawn player_position y {sy}"),
            ));
        }
    }

    if let Some((world, x, y, z)) = default_spawn {
        if world != "minecraft:overworld" {
            f.push(Failure::new(
                "spawn",
                "play/clientbound set_default_spawn_position",
                format!("world {world} is not minecraft:overworld"),
            ));
        }
        if let Some((sx, sy, sz)) = spawn {
            // set_default_spawn_position is a block position (floor of the
            // player's exact position), so compare x/z against the floor.
            if x as f64 != sx.floor() || z as f64 != sz.floor() || y as f64 != sy {
                f.push(Failure::new(
                    "spawn",
                    "play/clientbound set_default_spawn_position",
                    format!(
                        "block position ({x}, {y}, {z}) does not match the spawn player_position ({sx}, {sy}, {sz})"
                    ),
                ));
            }
        }
    }

    if let Some((sx, sz)) =
        spawn.map(|(x, _, z)| ((x / 16.0).floor() as i32, (z / 16.0).floor() as i32))
        && let Some((cx, cz)) = cache_center.filter(|c| *c != (sx, sz))
    {
        f.push(Failure::new(
            "spawn",
            "play/clientbound set_chunk_cache_center",
            format!("center ({cx}, {cz}) is not the spawn chunk column ({sx}, {sz})"),
        ));
    }
}

fn check_entity_ids(packets: &[CapturedPacket], f: &mut Vec<Failure>) {
    // login playerId and entity_event entity id are writeInts at the head of the
    // body; every other entity-id packet (set_entity_data, update_attributes,
    // set_entity_motion, the move_entity_* trio, rotate_head, add_entity) carries
    // the entity id as a leading VarInt. The scenario has exactly one entity
    // (the joining player), so every entity id must be 1.
    let mut seen: Vec<(&str, i32)> = Vec::new();

    // writeInt heads (49, 34).
    for (id, name) in [(49, "login"), (34, "entity_event")] {
        if let Some(p) = packets
            .iter()
            .find(|p| p.state == State::Play && p.direction == Direction::Clientbound && p.id == id)
            && let Some(b) = p.body.get(0..4)
        {
            seen.push((name, i32::from_be_bytes([b[0], b[1], b[2], b[3]])));
        }
    }

    // VarInt heads: 99 set_entity_data, 131 update_attributes, 101
    // set_entity_motion, 53/54/56 move_entity_pos{,rot}/{rot}, 83 rotate_head,
    // 1 add_entity.
    for (id, name) in [
        (99, "set_entity_data"),
        (131, "update_attributes"),
        (101, "set_entity_motion"),
        (53, "move_entity_pos"),
        (54, "move_entity_pos_rot"),
        (56, "move_entity_rot"),
        (83, "rotate_head"),
        (1, "add_entity"),
    ] {
        if let Some(p) = packets
            .iter()
            .find(|p| p.state == State::Play && p.direction == Direction::Clientbound && p.id == id)
            && let Some(v) = frame::read_varint(&p.body, &mut 0)
        {
            seen.push((name, v));
        }
    }

    for (name, v) in &seen {
        if *v != 1 {
            f.push(Failure::new(
                "entity-id",
                format!("play/clientbound {name}"),
                format!("entity id {v} is not 1 (the sole player entity on this scenario)"),
            ));
        }
    }
    // All present entity ids must agree (the scenario has exactly one entity).
    if seen.len() >= 2 && seen.windows(2).any(|w| w[0].1 != w[1].1) {
        f.push(Failure::new(
            "entity-id",
            format!("{} vs {}", seen[0].0, seen[1].0),
            format!("entity ids disagree: {seen:?}"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::write_varint;

    fn pkt(state: State, direction: Direction, id: i32, body: Vec<u8>) -> CapturedPacket {
        CapturedPacket {
            state,
            direction,
            id,
            body,
        }
    }

    fn spawn_position_body(id: i32) -> Vec<u8> {
        let mut b = Vec::new();
        write_varint(&mut b, id);
        b.extend_from_slice(&0.5f64.to_be_bytes());
        b.extend_from_slice(&(-63.0f64).to_be_bytes());
        b.extend_from_slice(&0.5f64.to_be_bytes());
        b
    }

    fn base_play() -> Vec<CapturedPacket> {
        vec![
            pkt(
                State::Play,
                Direction::Clientbound,
                49,
                1i32.to_be_bytes().to_vec(),
            ),
            pkt(
                State::Play,
                Direction::Clientbound,
                72,
                spawn_position_body(1),
            ),
            pkt(State::Play, Direction::Serverbound, 0, vec![0x01]),
            pkt(State::Play, Direction::Serverbound, 30, {
                let mut b = Vec::new();
                b.extend_from_slice(&0.5f64.to_be_bytes());
                b.extend_from_slice(&(-63.0f64).to_be_bytes());
                b.extend_from_slice(&0.5f64.to_be_bytes());
                b.push(0);
                b
            }),
        ]
    }

    #[test]
    fn teleport_ack_valid() {
        let fails = check(&base_play());
        assert!(!fails.iter().any(|x| x.kind == "teleport-ack"), "{fails:?}");
    }

    #[test]
    fn teleport_ack_wrong_id_fails() {
        let mut v = base_play();
        v[2] = pkt(State::Play, Direction::Serverbound, 0, vec![0x03]);
        let fails = check(&v);
        assert!(
            fails
                .iter()
                .any(|x| x.kind == "teleport-ack" && x.identity.contains("teleport id 3")),
            "{fails:?}"
        );
    }

    #[test]
    fn teleport_ack_deleted_fails() {
        let mut v = base_play();
        v.remove(2); // drop accept_teleportation
        let fails = check(&v);
        assert!(
            fails
                .iter()
                .any(|x| x.kind == "teleport-ack" && x.message.contains("never acknowledged")),
            "{fails:?}"
        );
    }

    #[test]
    fn keepalive_echo_must_match_prior() {
        let mut v = base_play();
        v.push(pkt(
            State::Play,
            Direction::Clientbound,
            44,
            vec![1, 2, 3, 4, 5, 6, 7, 8],
        ));
        v.push(pkt(
            State::Play,
            Direction::Serverbound,
            28,
            vec![1, 2, 3, 4, 5, 6, 7, 8],
        ));
        let fails = check(&v);
        assert!(!fails.iter().any(|x| x.kind == "keepalive"), "{fails:?}");

        let mut bad = base_play();
        bad.push(pkt(State::Play, Direction::Clientbound, 44, vec![9; 8]));
        bad.push(pkt(State::Play, Direction::Serverbound, 28, vec![8; 8]));
        let fails = check(&bad);
        assert!(fails.iter().any(|x| x.kind == "keepalive"), "{fails:?}");
    }

    #[test]
    fn keepalive_before_any_request_fails() {
        let mut v = base_play();
        v.push(pkt(State::Play, Direction::Serverbound, 28, vec![0; 8]));
        let fails = check(&v);
        assert!(fails.iter().any(|x| x.kind == "keepalive"), "{fails:?}");
    }

    #[test]
    fn spawn_y_mismatch_fails() {
        let mut v = base_play();
        // bump the movement sample's y by 1.
        let mut b = v[3].body.clone();
        b[8..16].copy_from_slice(&(-62.0f64).to_be_bytes());
        v[3] = pkt(State::Play, Direction::Serverbound, 30, b);
        let fails = check(&v);
        assert!(
            fails
                .iter()
                .any(|x| x.kind == "spawn" && x.message.contains("movement sample y -62")),
            "{fails:?}"
        );
    }

    #[test]
    fn cache_center_mismatch_fails() {
        let mut v = base_play();
        v.push(pkt(
            State::Play,
            Direction::Clientbound,
            94,
            vec![0x01, 0x00],
        ));
        let fails = check(&v);
        assert!(
            fails
                .iter()
                .any(|x| x.kind == "spawn" && x.identity.contains("set_chunk_cache_center")),
            "{fails:?}"
        );
    }

    #[test]
    fn entity_ids_agree() {
        let fails = check(&base_play());
        assert!(!fails.iter().any(|x| x.kind == "entity-id"), "{fails:?}");
    }
}
