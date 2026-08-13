//! The ordered-sequence model over the RAW capture (#195).
//!
//! `canonicalize` erases order: it groups packets by `(state, direction, id)`,
//! sorts chunks by coordinate, and samples racy ids, so a join that reorders
//! required packets still byte-matches the fixture. This module consumes the raw
//! capture and replays the proxy's own state machine, asserting the ordering
//! that IS the join protocol.
//!
//! The proxy merges two independent relay tasks (c2s and s2c) into one list, so
//! positional adjacency between *unrelated* serverbound and clientbound packets
//! is a scheduling artifact. Only two classes of ordering are reliable:
//!
//! - **Within-direction**: a single relay task appends its direction's packets
//!   in wire order, so server-only emit order (e.g. `login_compression` before
//!   `login_finished`) and client-only emit order are exact.
//! - **Response chains**: a serverbound response can only be generated after the
//!   client received its clientbound request, and the s2c relay appends a frame
//!   *before* forwarding it to the client. So `cb request` is always appended
//!   before `sb response` (e.g. `login_finished` before `login_acknowledged`).
//!
//! Id-matched pairs (teleport→ack, keepalive request→echo) are additionally
//! checked in `relationships.rs`.

use std::collections::HashMap;

use crate::invariants::Failure;
use crate::packet::{CapturedPacket, Direction, State};

fn key(state: State, direction: Direction, id: i32) -> (State, Direction, i32) {
    (state, direction, id)
}

/// Stable human identity for a (state, direction, id) tuple.
pub fn identity(state: State, direction: Direction, id: i32) -> String {
    format!(
        "{}/{} id {} ({})",
        state,
        direction.flow(),
        id,
        crate::packet::packet_name(state, direction, id).unwrap_or("minecraft:unknown")
    )
}

fn state_rank(s: State) -> u8 {
    match s {
        State::Handshake => 0,
        State::Status => 1,
        State::Login => 2,
        State::Configuration => 3,
        State::Play => 4,
    }
}

/// First/last occurrence indices of every (state, direction, id) tuple in the
/// merged raw list, plus the first play index.
struct Indexes {
    first: HashMap<(State, Direction, i32), usize>,
    last: HashMap<(State, Direction, i32), usize>,
    first_play: Option<usize>,
}

impl Indexes {
    fn build(packets: &[CapturedPacket]) -> Self {
        let mut first = HashMap::new();
        let mut last = HashMap::new();
        let mut first_play = None;
        for (i, p) in packets.iter().enumerate() {
            first.entry(key(p.state, p.direction, p.id)).or_insert(i);
            last.insert(key(p.state, p.direction, p.id), i);
            if p.state == State::Play && first_play.is_none() {
                first_play = Some(i);
            }
        }
        Self {
            first,
            last,
            first_play,
        }
    }

    fn first(&self, state: State, direction: Direction, id: i32) -> Option<usize> {
        self.first.get(&key(state, direction, id)).copied()
    }

    fn last(&self, state: State, direction: Direction, id: i32) -> Option<usize> {
        self.last.get(&key(state, direction, id)).copied()
    }
}

/// One required ordering edge. `mode` selects the comparison: `FirstFirst`
/// requires `a`'s first occurrence before `b`'s first; `AllFirst` requires every
/// `a` before `b`'s first.
enum Mode {
    FirstFirst,
    AllFirst,
}

struct Rule {
    state: State,
    a_dir: Direction,
    a_id: i32,
    b_dir: Direction,
    b_id: i32,
    mode: Mode,
    /// Short human description of the required edge.
    why: &'static str,
}

/// The deterministic play clientbound join burst: Paper's fixed send order on
/// the pinned commit (fixture provenance `26.2-DEV-main@0a99345`), before the
/// chunk stream. `canonicalize` groups by id, so this order is otherwise erased
/// — a port that reorders the burst still byte-matches the fixture. Each burst
/// packet's first occurrence must not regress against this sequence (packets
/// absent from a capture are skipped). Excluded because their wire position is
/// timing-flexible, not fixed send order:
///
/// - chunks (45) and keep_alive (44): within-stream placement is timing-flexible
///   and chunk order is outside the parity contract.
/// - commands (16): `Commands.sendCommands` dispatches to the fixed
///   `COMMAND_SENDING_POOL` (2 threads) and `sendAsync` reschedules to the
///   server thread, so the clientbound packet lands wherever that completes
///   relative to the synchronous join-tick sends (e.g. the tracker's
///   `set_entity_data`, 99) — it has no fixed wire position.
pub const PLAY_BURST_ORDER: &[i32] = &[
    49,  // login
    10,  // change_difficulty
    64,  // player_abilities
    105, // set_held_slot
    133, // update_recipes
    34,  // entity_event
    76,  // recipe_book_settings
    74,  // recipe_book_add
    72,  // player_position
    86,  // server_data
    43,  // initialize_border
    113, // set_time
    97,  // set_default_spawn_position
    38,  // game_event
    127, // ticking_state
    128, // ticking_step
    18,  // container_set_content
    20,  // container_set_slot
    121, // system_chat
    70,  // player_info_update
    95,  // set_chunk_cache_radius
    111, // set_simulation_distance
    94,  // set_chunk_cache_center
    99,  // set_entity_data
    130, // update_advancements
    104, // set_health
    103, // set_experience
    131, // update_attributes
];

fn rules() -> Vec<Rule> {
    use Direction::{Clientbound, Serverbound};
    use State::{Configuration, Login, Play};
    vec![
        // Login: client sends hello, then (after compression + finished) acks.
        Rule {
            state: Login,
            a_dir: Serverbound,
            a_id: 0,
            b_dir: Serverbound,
            b_id: 3,
            mode: Mode::FirstFirst,
            why: "login hello precedes login_acknowledged (client send order)",
        },
        // Login: server negotiates compression before finishing.
        Rule {
            state: Login,
            a_dir: Clientbound,
            a_id: 3,
            b_dir: Clientbound,
            b_id: 2,
            mode: Mode::FirstFirst,
            why: "login_compression precedes login_finished (server send order)",
        },
        // Login: the client acks only after the server finished (response chain).
        Rule {
            state: Login,
            a_dir: Clientbound,
            a_id: 2,
            b_dir: Serverbound,
            b_id: 3,
            mode: Mode::FirstFirst,
            why: "login_finished precedes login_acknowledged (client acks after finished)",
        },
        // Configuration clientbound: update_enabled_features precedes
        // select_known_packs (server send order), before the registry stream.
        Rule {
            state: Configuration,
            a_dir: Clientbound,
            a_id: 12,
            b_dir: Clientbound,
            b_id: 14,
            mode: Mode::FirstFirst,
            why: "update_enabled_features precedes select_known_packs (server send order)",
        },
        // Configuration serverbound: the client's custom_payload brand packet
        // then client_information, then the select_known_packs response.
        Rule {
            state: Configuration,
            a_dir: Serverbound,
            a_id: 2,
            b_dir: Serverbound,
            b_id: 0,
            mode: Mode::FirstFirst,
            why: "the brand custom_payload precedes client_information (client send order)",
        },
        Rule {
            state: Configuration,
            a_dir: Serverbound,
            a_id: 0,
            b_dir: Serverbound,
            b_id: 7,
            mode: Mode::FirstFirst,
            why: "client_information precedes the known-packs response (client send order)",
        },
        // Configuration: the known-packs response gates the registry sync.
        Rule {
            state: Configuration,
            a_dir: Clientbound,
            a_id: 14,
            b_dir: Clientbound,
            b_id: 7,
            mode: Mode::FirstFirst,
            why: "select_known_packs precedes the registry stream (server send order)",
        },
        Rule {
            state: Configuration,
            a_dir: Serverbound,
            a_id: 7,
            b_dir: Clientbound,
            b_id: 7,
            mode: Mode::FirstFirst,
            why: "the known-packs response precedes the registry stream (registry sync is gated on it)",
        },
        // Configuration: all registries, then tags, then the server finishes.
        Rule {
            state: Configuration,
            a_dir: Clientbound,
            a_id: 7,
            b_dir: Clientbound,
            b_id: 13,
            mode: Mode::AllFirst,
            why: "every registry_data precedes update_tags (server send order)",
        },
        Rule {
            state: Configuration,
            a_dir: Clientbound,
            a_id: 13,
            b_dir: Clientbound,
            b_id: 3,
            mode: Mode::FirstFirst,
            why: "update_tags precedes finish_configuration (server send order)",
        },
        // Configuration handoff into play: cb finish, then the client's sb ack,
        // then the first play packet (response chain + proxy construction).
        Rule {
            state: Configuration,
            a_dir: Clientbound,
            a_id: 3,
            b_dir: Serverbound,
            b_id: 3,
            mode: Mode::FirstFirst,
            why: "finish_configuration request precedes the client's ack",
        },
        // Play: the deterministic join burst starts with login, then the teleport.
        Rule {
            state: Play,
            a_dir: Clientbound,
            a_id: 49,
            b_dir: Clientbound,
            b_id: 72,
            mode: Mode::FirstFirst,
            why: "login precedes the spawn teleport (player_position)",
        },
        // The teleport precedes chunk delivery.
        Rule {
            state: Play,
            a_dir: Clientbound,
            a_id: 72,
            b_dir: Clientbound,
            b_id: 45,
            mode: Mode::FirstFirst,
            why: "the spawn teleport precedes the first chunk",
        },
        // The view square (cache center + radius) precedes the chunks that fill it.
        Rule {
            state: Play,
            a_dir: Clientbound,
            a_id: 94,
            b_dir: Clientbound,
            b_id: 45,
            mode: Mode::FirstFirst,
            why: "set_chunk_cache_center precedes the first chunk",
        },
        Rule {
            state: Play,
            a_dir: Clientbound,
            a_id: 95,
            b_dir: Clientbound,
            b_id: 45,
            mode: Mode::FirstFirst,
            why: "set_chunk_cache_radius precedes the first chunk",
        },
        // LEVEL_CHUNKS_LOAD_START game_event precedes the chunks it brackets.
        Rule {
            state: Play,
            a_dir: Clientbound,
            a_id: 38,
            b_dir: Clientbound,
            b_id: 45,
            mode: Mode::FirstFirst,
            why: "game_event (chunks-load-start) precedes the first chunk",
        },
        // The client acks the teleport (response chain) before sending movement.
        Rule {
            state: Play,
            a_dir: Clientbound,
            a_id: 72,
            b_dir: Serverbound,
            b_id: 0,
            mode: Mode::FirstFirst,
            why: "player_position precedes accept_teleportation (client acks the teleport)",
        },
        Rule {
            state: Play,
            a_dir: Serverbound,
            a_id: 0,
            b_dir: Serverbound,
            b_id: 30,
            mode: Mode::FirstFirst,
            why: "accept_teleportation precedes the client's post-teleport movement",
        },
        // The client acks the world load only after receiving a chunk (response chain).
        Rule {
            state: Play,
            a_dir: Clientbound,
            a_id: 45,
            b_dir: Serverbound,
            b_id: 44,
            mode: Mode::FirstFirst,
            why: "the first chunk precedes player_loaded (client acks the loaded world)",
        },
    ]
}

/// Assert the ordering constraints of the join state machine over the raw
/// capture. Every violation names the pair of packet identities and the index
/// that broke the edge.
pub fn check(packets: &[CapturedPacket]) -> Vec<Failure> {
    let mut f = Vec::new();
    let idx = Indexes::build(packets);

    // The handshake intention must be the very first packet.
    if let Some(p) = packets.first().filter(|p| {
        p.state != State::Handshake || p.direction != Direction::Serverbound || p.id != 0
    }) {
        f.push(Failure::new(
            "ordering",
            format!("{} index 0", identity(p.state, p.direction, p.id)),
            "the handshake intention (handshake/serverbound id 0) must be the first packet",
        ));
    }

    // The observed-state sequence must be non-decreasing: handshake → login →
    // configuration → play, with no packet recorded in a later state before the
    // transition into it.
    for w in packets.windows(2) {
        if state_rank(w[1].state) < state_rank(w[0].state) {
            f.push(Failure::new(
                "ordering",
                format!(
                    "{} at index (after {})",
                    identity(w[1].state, w[1].direction, w[1].id),
                    identity(w[0].state, w[0].direction, w[0].id)
                ),
                "the connection state must never move backwards",
            ));
        }
    }

    for rule in rules() {
        let (a, b) = match (
            idx.first(rule.state, rule.a_dir, rule.a_id),
            idx.first(rule.state, rule.b_dir, rule.b_id),
        ) {
            (Some(a), Some(b)) => (a, b),
            _ => continue, // presence is checked separately below.
        };
        let a_last = idx.last(rule.state, rule.a_dir, rule.a_id).unwrap_or(a);
        let (a, violated) = match rule.mode {
            Mode::FirstFirst => (a, a >= b),
            Mode::AllFirst => (a_last, a_last >= b),
        };
        if violated {
            let (state, a_dir, a_id, b_dir, b_id) =
                (rule.state, rule.a_dir, rule.a_id, rule.b_dir, rule.b_id);
            f.push(Failure::new(
                "ordering",
                format!(
                    "{} at [{}] precedes {}",
                    identity(state, a_dir, a_id),
                    a,
                    identity(state, b_dir, b_id)
                ),
                format!("at [{}] — {}", b, rule.why),
            ));
        }
    }

    // The deterministic play burst: the first occurrence of every burst packet
    // must keep the server's fixed send order (a mid-burst reorder would still
    // byte-match the id-grouped canonical fixture, so it must be caught here).
    {
        let mut last: Option<(usize, i32)> = None; // (index, id)
        for &id in PLAY_BURST_ORDER {
            let Some(first) = idx.first(State::Play, Direction::Clientbound, id) else {
                continue; // absent from this capture
            };
            if let Some((prev_idx, prev_id)) = last
                && first < prev_idx
            {
                f.push(Failure::new(
                    "ordering",
                    format!(
                        "{} at [{}]",
                        identity(State::Play, Direction::Clientbound, id),
                        first
                    ),
                    format!(
                        "precedes {} at [{}] — the deterministic play join burst must keep Paper's send order",
                        identity(State::Play, Direction::Clientbound, prev_id),
                        prev_idx
                    ),
                ));
            }
            last = Some((first, id));
        }
    }

    // The transition into play: the client's finish_configuration (configuration
    // serverbound) must precede the first play packet — guaranteed by the proxy
    // state machine, so a raw capture that violates it was reordered.
    if let (Some(sb_finish), Some(fp)) = (
        idx.first(State::Configuration, Direction::Serverbound, 3),
        idx.first_play,
    ) && sb_finish >= fp
    {
        f.push(Failure::new(
            "ordering",
            format!(
                "{} at [{}] precedes the first play packet at [{}]",
                identity(State::Configuration, Direction::Serverbound, 3),
                sb_finish,
                fp
            ),
            "the client's finish_configuration must hand off into play before any play packet",
        ));
    }

    // Presence: every required join-path packet must appear at least once.
    const REQUIRED: &[(State, Direction, i32)] = &[
        (State::Handshake, Direction::Serverbound, 0),
        (State::Login, Direction::Serverbound, 0),
        (State::Login, Direction::Clientbound, 3),
        (State::Login, Direction::Clientbound, 2),
        (State::Login, Direction::Serverbound, 3),
        (State::Configuration, Direction::Clientbound, 7),
        (State::Configuration, Direction::Serverbound, 3),
        (State::Configuration, Direction::Clientbound, 3),
        (State::Play, Direction::Clientbound, 49),
        (State::Play, Direction::Clientbound, 72),
        (State::Play, Direction::Clientbound, 45),
        (State::Play, Direction::Serverbound, 44),
    ];
    for &(s, d, id) in REQUIRED {
        if idx.first(s, d, id).is_none() {
            f.push(Failure::new(
                "ordering",
                identity(s, d, id),
                "required packet missing from the join capture",
            ));
        }
    }

    f
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

    /// A minimal-but-real join raw capture that satisfies every ordering rule.
    fn valid_join() -> Vec<CapturedPacket> {
        let mut v = Vec::new();
        // handshake intention (next_state = 2).
        let mut hb = Vec::new();
        write_varint(&mut hb, 776);
        write_varint(&mut hb, 9);
        hb.extend_from_slice(b"127.0.0.1");
        hb.extend_from_slice(&25598u16.to_be_bytes());
        write_varint(&mut hb, 2);
        v.push(pkt(State::Handshake, Direction::Serverbound, 0, hb));
        // login
        v.push(pkt(State::Login, Direction::Serverbound, 0, vec![0x00]));
        let mut comp = Vec::new();
        write_varint(&mut comp, 256);
        v.push(pkt(State::Login, Direction::Clientbound, 3, comp));
        v.push(pkt(State::Login, Direction::Clientbound, 2, vec![0x00; 44]));
        v.push(pkt(State::Login, Direction::Serverbound, 3, vec![]));
        // configuration: enabled features, known-packs request+response, one
        // registry_data, update_tags, finish request+ack.
        v.push(pkt(
            State::Configuration,
            Direction::Clientbound,
            12,
            vec![0x00],
        ));
        v.push(pkt(
            State::Configuration,
            Direction::Clientbound,
            14,
            vec![0x00],
        ));
        // The client's brand custom_payload, then client_information, then the
        // known-packs response.
        v.push(pkt(
            State::Configuration,
            Direction::Serverbound,
            2,
            vec![0x00],
        ));
        v.push(pkt(
            State::Configuration,
            Direction::Serverbound,
            0,
            vec![0x00],
        ));
        v.push(pkt(
            State::Configuration,
            Direction::Serverbound,
            7,
            vec![0x00],
        ));
        let mut rd = Vec::new();
        write_varint(&mut rd, 14);
        write_varint(&mut rd, 1);
        rd.push(b'r');
        rd.push(1);
        rd.push(0);
        v.push(pkt(State::Configuration, Direction::Clientbound, 7, rd));
        let mut ut = Vec::new();
        write_varint(&mut ut, 1);
        write_varint(&mut ut, 10);
        write_varint(&mut ut, 1);
        write_varint(&mut ut, 1);
        write_varint(&mut ut, 1);
        write_varint(&mut ut, 0);
        v.push(pkt(State::Configuration, Direction::Clientbound, 13, ut));
        v.push(pkt(State::Configuration, Direction::Clientbound, 3, vec![]));
        v.push(pkt(State::Configuration, Direction::Serverbound, 3, vec![]));
        // play: the deterministic join burst (Paper's fixed send order), then
        // one chunk + a keepalive, then the serverbound ack + movement +
        // player_loaded. The ordering check only inspects (state, dir, id), so
        // the bodies are minimal. commands (16) is not in PLAY_BURST_ORDER (it
        // is async) but is a real join packet: insert it at its observed
        // reference position, between set_chunk_cache_center and set_entity_data.
        for id in PLAY_BURST_ORDER {
            v.push(pkt(State::Play, Direction::Clientbound, *id, vec![]));
        }
        let set_entity_data = v
            .iter()
            .position(|p| {
                p.state == State::Play && p.direction == Direction::Clientbound && p.id == 99
            })
            .expect("set_entity_data in the burst");
        v.insert(
            set_entity_data,
            pkt(State::Play, Direction::Clientbound, 16, vec![]),
        );
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&0i32.to_be_bytes());
        chunk.extend_from_slice(&0i32.to_be_bytes());
        write_varint(&mut chunk, 0); // no heightmaps
        write_varint(&mut chunk, 0); // empty section buffer
        chunk.push(0); // no block entities
        v.push(pkt(State::Play, Direction::Clientbound, 45, chunk));
        v.push(pkt(State::Play, Direction::Clientbound, 44, vec![0; 8]));
        v.push(pkt(State::Play, Direction::Serverbound, 0, vec![0x01]));
        let mut move_p = Vec::new();
        move_p.extend_from_slice(&0.5f64.to_be_bytes());
        move_p.extend_from_slice(&(-63.0f64).to_be_bytes());
        move_p.extend_from_slice(&0.5f64.to_be_bytes());
        move_p.push(0);
        v.push(pkt(State::Play, Direction::Serverbound, 30, move_p));
        v.push(pkt(State::Play, Direction::Serverbound, 44, vec![]));
        v
    }

    #[test]
    fn valid_join_passes() {
        let fails = check(&valid_join());
        assert!(fails.is_empty(), "{fails:?}");
    }

    #[test]
    fn reorder_login_compression_finished_fails() {
        let mut v = valid_join();
        let (i, j) = (2, 3); // login_compression, login_finished — adjacent.
        v.swap(i, j);
        let fails = check(&v);
        let names: Vec<_> = fails
            .iter()
            .filter(|x| x.kind == "ordering")
            .map(|x| x.identity.clone())
            .collect();
        assert!(
            names
                .iter()
                .any(|s| s.contains("login_compression") && s.contains("login_finished")),
            "expected the login_compression/login_finished edge to be named, got {names:?}"
        );
    }

    #[test]
    fn commands_may_follow_set_entity_data() {
        // Paper's commands packet is sent asynchronously (`Commands.sendCommands`
        // dispatches to COMMAND_SENDING_POOL, then `sendAsync` reschedules to the
        // server thread), so its wire position relative to the synchronous
        // join-tick sends is timing-flexible. It must be allowed to land after
        // set_entity_data — treating it as fixed mid-burst order is the #101
        // false positive. Re-adding 16 to PLAY_BURST_ORDER makes this fail.
        let mut v = valid_join();
        // Drop any commands packets the burst construction may have emitted, so
        // the single packet placed below is the capture's only occurrence of 16.
        v.retain(|p| {
            !(p.state == State::Play && p.direction == Direction::Clientbound && p.id == 16)
        });
        let after_set_entity_data = v
            .iter()
            .position(|p| {
                p.state == State::Play && p.direction == Direction::Clientbound && p.id == 99
            })
            .expect("set_entity_data")
            + 1;
        v.insert(
            after_set_entity_data,
            pkt(State::Play, Direction::Clientbound, 16, vec![]),
        );
        let fails = check(&v);
        assert!(
            fails.is_empty(),
            "commands after set_entity_data is a valid Paper layout, got {fails:?}"
        );
    }

    #[test]
    fn reorder_mid_burst_fails() {
        // swap entity_event (34) and update_recipes (133) — both in the burst.
        let mut v = valid_join();
        let i = v
            .iter()
            .position(|p| p.id == 34 && p.state == State::Play)
            .unwrap();
        let j = v
            .iter()
            .position(|p| p.id == 133 && p.state == State::Play)
            .unwrap();
        v.swap(i, j);
        let fails = check(&v);
        assert!(
            fails
                .iter()
                .any(|x| x.kind == "ordering" && x.identity.contains("entity_event")),
            "expected the burst order to name the displaced entity_event, got {fails:?}"
        );
    }

    #[test]
    fn reorder_config_serverbound_fails() {
        // swap client_information (0) and select_known_packs response (7).
        let mut v = valid_join();
        let i = v
            .iter()
            .position(|p| {
                p.state == State::Configuration
                    && p.direction == Direction::Serverbound
                    && p.id == 0
            })
            .unwrap();
        let j = v
            .iter()
            .position(|p| {
                p.state == State::Configuration
                    && p.direction == Direction::Serverbound
                    && p.id == 7
            })
            .unwrap();
        v.swap(i, j);
        let fails = check(&v);
        assert!(
            fails
                .iter()
                .any(|x| x.kind == "ordering" && x.identity.contains("client_information")),
            "expected the config serverbound order to name client_information, got {fails:?}"
        );
    }

    #[test]
    fn missing_required_packet_is_named() {
        let mut v = valid_join();
        v.retain(|p| {
            !(p.state == State::Play && p.direction == Direction::Serverbound && p.id == 44)
        });
        let fails = check(&v);
        assert!(
            fails.iter().any(|x| x.identity.contains("player_loaded")),
            "{fails:?}"
        );
    }

    #[test]
    fn play_before_finish_configuration_is_named() {
        let mut v = valid_join();
        // Move the first play packet ahead of the configuration finish ack.
        let play_idx = v
            .iter()
            .position(|p| p.state == State::Play)
            .expect("play packet");
        let fin_idx = v
            .iter()
            .position(|p| {
                p.state == State::Configuration
                    && p.direction == Direction::Serverbound
                    && p.id == 3
            })
            .expect("sb finish");
        let play = v.remove(play_idx);
        v.insert(fin_idx, play);
        let fails = check(&v);
        assert!(
            fails
                .iter()
                .any(|x| x.identity.contains("first play packet")),
            "{fails:?}"
        );
    }
}
