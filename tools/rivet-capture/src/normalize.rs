//! Byte-level normalization of the raw join-path capture into a deterministic
//! canonical form.
//!
//! The Paper server randomizes a handful of wire fields on every boot (see the
//! provenance docs in the fixture manifest for the full justification):
//!
//! - the player's spawn X/Z offset (PlayerSpawnFinder picks a random candidate
//!   around the fixed world spawn), which shifts the spawn position, the chunk
//!   view square, and every entity/position coordinate;
//! - the player entity id (a server-side counter);
//! - the player-list latency (Paper's keepalive RTT estimate);
//! - keepalive ids (the server emits a wall-clock challenge, the client echoes it);
//! - the `set_time` body (tick-dependent world clock state).
//!
//! Configuration keepalives are watchdog traffic emitted once per second while
//! Paper's asynchronous registry join is in progress. Their presence and
//! interleaving therefore depend on scheduling, so the canonical form omits
//! those configuration-only frames while raw relationship checks still validate
//! every request/echo pair. Everything else on the join path is byte-deterministic for a fixed seed +
//! superflat config + offline bot identity (verified across fresh boots by
//! `verify`). This module rewrites exactly those nondeterministic fields,
//! leaving every other byte untouched, and records each rewrite with a
//! justification so the fixture manifest stays self-documenting.
//!
//! `Direction` uses the protocol's own naming: clientbound packets travel
//! server → client, serverbound packets travel client → server.

use std::collections::BTreeMap;

use crate::frame;
use crate::packet::{CapturedPacket, Direction, State};
use crate::structured;
use rivet_decode::nbt::MAX_INITIAL_COLLECTION_SIZE;

/// A normalized packet: the raw (state, direction, id) plus the normalized body
/// and a human-readable note describing any rewrite applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPacket {
    pub state: State,
    pub direction: Direction,
    pub id: i32,
    pub body: Vec<u8>,
    /// Human-readable justification; empty when the body was unchanged.
    pub note: String,
}

/// Context discovered from the raw capture before per-packet normalization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnCtx {
    /// Player spawn X (absolute, from the first `player_position` body).
    pub x: f64,
    /// Player spawn Z (absolute, from the first `player_position` body).
    pub z: f64,
    /// Chunk column containing the spawn position.
    pub chunk_x: i32,
    pub chunk_z: i32,
}

/// Derive the spawn context from the raw capture: the first `player_position`
/// body carries the player's spawn coordinates.
pub fn find_spawn(packets: &[CapturedPacket]) -> Option<SpawnCtx> {
    for p in packets {
        if p.state == State::Play
            && p.direction == Direction::Clientbound
            && p.id == 72
            && let Some((x, z)) = parse_player_position(&p.body)
        {
            return Some(SpawnCtx {
                x,
                z,
                chunk_x: (x / 16.0).floor() as i32,
                chunk_z: (z / 16.0).floor() as i32,
            });
        }
    }
    None
}

/// Parse `(x, z)` out of a `player_position` body
/// (`[VarInt id][x f64][y f64][z f64]...`).
fn parse_player_position(body: &[u8]) -> Option<(f64, f64)> {
    let mut off = 0;
    frame::read_varint(body, &mut off)?;
    let x = frame::read_f64(body, &mut off)?;
    frame::read_f64(body, &mut off)?;
    let z = frame::read_f64(body, &mut off)?;
    Some((x, z))
}

/// Rewrite the leading VarInt entity id to `1`. Used for every packet whose
/// first field is the (per-boot randomized) server-assigned entity id.
fn rewrite_entity_id(body: &[u8], id: i32) -> Vec<u8> {
    let mut off = 0;
    let id_len = frame::read_varint(body, &mut off).map(|_| off).unwrap_or(0);
    let mut out = Vec::with_capacity(body.len());
    frame::write_varint(&mut out, id);
    out.extend_from_slice(&body[id_len..]);
    out
}

/// Zero a big-endian field in `body` if it fits.
fn zero_f64(body: &mut [u8], off: usize) {
    if body.len() >= off + 8 {
        body[off..off + 8].copy_from_slice(&0.0f64.to_be_bytes());
    }
}

fn zero_f32(body: &mut [u8], off: usize) {
    if body.len() >= off + 4 {
        body[off..off + 4].copy_from_slice(&0.0f32.to_be_bytes());
    }
}

fn zero_bytes(body: &mut [u8], off: usize, n: usize) {
    if body.len() >= off + n {
        body[off..off + n].fill(0);
    }
}

/// Rewrite every `UPDATE_LATENCY` value in a player-info update to zero.
///
/// Paper constructs the join packet with `ServerCommonPacketListenerImpl.latency()`.
/// That value is updated from the keepalive RTT calculator, so it depends on the
/// scheduling and loopback timing of the fresh capture. The packet's action mask
/// and entry fields are parsed according to `ClientboundPlayerInfoUpdatePacket`;
/// unsupported optional display-name payloads fail closed and remain byte-exact.
fn normalize_player_info_latency(body: &[u8]) -> Option<Vec<u8>> {
    let mut off = 0;
    let actions = *body.get(off)?;
    off += 1;
    let entry_count = frame::read_varint(body, &mut off)?;
    if entry_count < 0 {
        return None;
    }
    let mut latency_ranges = Vec::new();

    for _ in 0..entry_count {
        frame::read_bytes(body, &mut off, 16)?; // profile UUID
        if actions & (1 << 0) != 0 {
            read_string(body, &mut off)?; // profile name
            let property_count = frame::read_varint(body, &mut off)?;
            if property_count < 0 {
                return None;
            }
            for _ in 0..property_count {
                read_string(body, &mut off)?; // property name
                read_string(body, &mut off)?; // property value
                if *body.get(off)? != 0 {
                    off += 1;
                    read_string(body, &mut off)?; // property signature
                } else {
                    off += 1;
                }
            }
        }
        if actions & (1 << 1) != 0 {
            // A non-null chat session is outside this fixture's join shape. Keep
            // it byte-exact rather than guessing at its nested key encoding.
            if *body.get(off)? != 0 {
                return None;
            }
            off += 1;
        }
        if actions & (1 << 2) != 0 {
            frame::read_varint(body, &mut off)?; // game mode
        }
        if actions & (1 << 3) != 0 {
            off = off.checked_add(1)?; // listed
            body.get(off - 1)?;
        }
        if actions & (1 << 4) != 0 {
            let start = off;
            frame::read_varint(body, &mut off)?; // latency
            latency_ranges.push((start, off));
        }
        if actions & (1 << 5) != 0 {
            // The trusted display-name component is NBT. The join packet carries
            // null; fail closed for a non-null component.
            if *body.get(off)? != 0 {
                return None;
            }
            off += 1;
        }
        if actions & (1 << 6) != 0 {
            frame::read_varint(body, &mut off)?; // list order
        }
        if actions & (1 << 7) != 0 {
            off = off.checked_add(1)?; // show hat
            body.get(off - 1)?;
        }
    }
    if off != body.len() || latency_ranges.is_empty() {
        return None;
    }

    let mut out = Vec::with_capacity(body.len());
    let mut copied = 0;
    for (start, end) in latency_ranges {
        out.extend_from_slice(&body[copied..start]);
        frame::write_varint(&mut out, 0);
        copied = end;
    }
    out.extend_from_slice(&body[copied..]);
    Some(out)
}

/// Read a VarInt-prefixed UTF-8 string from a packet body.
fn read_string(body: &[u8], off: &mut usize) -> Option<()> {
    let len = frame::read_varint(body, off)?;
    if len < 0 {
        return None;
    }
    frame::read_bytes(body, off, len as usize).map(|_| ())
}

/// Normalize a single packet body. Returns the normalized body plus a note.
///
/// Every rewrite here is a field the Paper server varies per boot or schedule
/// (spawn offset, entity ids, teleport ids, player-list latency, keepalive ids,
/// tick-dependent set_time), documented with a one-line justification so the
/// fixture manifest stays self-describing.
pub fn normalize_packet(packet: &CapturedPacket, spawn: Option<SpawnCtx>) -> NormalizedPacket {
    let (body, note) = match (packet.state, packet.direction, packet.id) {
        // login_finished: [UUID playerUUID][String username][VarInt props]
        // [UUID sessionId]. The sessionId is a per-login random UUID; zero it.
        (State::Login, Direction::Clientbound, 2) => {
            let mut body = packet.body.clone();
            zero_bytes(&mut body, 28, 16);
            (
                body,
                "login_finished sessionId (per-login random UUID) -> 0".into(),
            )
        }
        // Handshake intention: zero the proxy's ephemeral listen port.
        // Body: [VarInt protocol][String address][u16 port][VarInt next_state].
        (State::Handshake, Direction::Serverbound, 0) => {
            let mut body = packet.body.clone();
            let mut off = 0;
            if frame::read_varint(&body, &mut off).is_some() {
                let addr_len = frame::read_varint(&body, &mut off).unwrap_or(0) as usize;
                if addr_len > 0 && body.len() >= off + addr_len + 2 {
                    off += addr_len;
                    body[off..off + 2].copy_from_slice(&0u16.to_be_bytes());
                }
            }
            (
                body,
                "handshake port (proxy's ephemeral listen port) -> 0".into(),
            )
        }
        // login: playerId is a writeInt at the head of the body.
        (State::Play, Direction::Clientbound, 49) => {
            let mut body = packet.body.clone();
            if body.len() >= 4 {
                body[0..4].copy_from_slice(&1i32.to_be_bytes());
            }
            (
                body,
                "player entity id (server-assigned counter) -> 1".into(),
            )
        }
        // player_info_update: Paper's initial player-list packet carries the
        // connection latency measured by its keepalive RTT calculator. Zero only
        // that UPDATE_LATENCY field; every profile, action, and list-order byte
        // remains protocol-exact.
        (State::Play, Direction::Clientbound, 70) => match normalize_player_info_latency(&packet.body) {
            Some(body) => (
                body,
                "player-list latency (fresh-join keepalive RTT) -> 0".into(),
            ),
            None => (
                packet.body.clone(),
                "player_info_update latency normalization FAILED — raw body kept".into(),
            ),
        },
        // player_position: rewrite the teleport id, x/z, delta dx/dz and the
        // rotation floats; keep y (superflat spawn height is deterministic).
        // Body: [VarInt id][x][y][z][dx][dy][dz][yaw][pitch][flags...]
        (State::Play, Direction::Clientbound, 72) => {
            let mut off = 0;
            let id_len = frame::read_varint(&packet.body, &mut off)
                .map(|_| off)
                .unwrap_or(0);
            let mut body = Vec::with_capacity(packet.body.len());
            frame::write_varint(&mut body, 0); // teleport id -> 0
            body.extend_from_slice(&packet.body[id_len..]);
            // body = [0][x][y][z][dx][dy][dz][yaw][pitch][flags...]
            zero_f64(&mut body, 1); // x
            // y at 9..17 kept (deterministic spawn height).
            zero_f64(&mut body, 17); // z
            zero_f64(&mut body, 25); // dx
            // dy at 33..41 kept (vertical delta).
            zero_f64(&mut body, 41); // dz
            zero_f32(&mut body, 49); // yaw
            zero_f32(&mut body, 53); // pitch
            (
                body,
                "teleport id, position x/z, delta dx/dz, yaw/pitch (spawn offset randomized) -> canonical".into(),
            )
        }
        // set_chunk_cache_center: [VarInt chunkX][VarInt chunkZ] — translate to
        // the spawn-relative origin (0,0) so the view square is deterministic.
        (State::Play, Direction::Clientbound, 94) => {
            let mut body = Vec::with_capacity(packet.body.len());
            let (sx, sz) = spawn.map(|s| (s.chunk_x, s.chunk_z)).unwrap_or((0, 0));
            let mut off = 0;
            let cx = frame::read_varint(&packet.body, &mut off).unwrap_or(0) - sx;
            let cz = frame::read_varint(&packet.body, &mut off).unwrap_or(0) - sz;
            frame::write_varint(&mut body, cx);
            frame::write_varint(&mut body, cz);
            (
                body,
                "chunk-cache center translated to spawn-chunk origin".into(),
            )
        }
        // level_chunk_with_light: chunkX/chunkZ are writeInts at the head
        // (translated to the spawn chunk), and the heightmap map iterates in a
        // per-boot order, so its entries are sorted by type id. The section
        // buffer and block-entity list are untouched.
        (State::Play, Direction::Clientbound, 45) => {
            let mut body = packet.body.clone();
            if body.len() >= 8 {
                let cx = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                let cz = i32::from_be_bytes([body[4], body[5], body[6], body[7]]);
                let (sx, sz) = spawn.map(|s| (s.chunk_x, s.chunk_z)).unwrap_or((0, 0));
                body[0..4].copy_from_slice(&(cx - sx).to_be_bytes());
                body[4..8].copy_from_slice(&(cz - sz).to_be_bytes());
            }
            match structured::canon_chunk(&body) {
                Some(c) => (
                    c,
                    "chunk coords -> spawn origin; heightmaps + block/biome palettes sorted (per-boot HashMap order) -> canonical".into(),
                ),
                None => (body, "chunk canonicalization FAILED — raw body kept".into()),
            }
        }
        // registry_data: entries and NBT compound fields iterate per-boot
        // (HashMap-backed); sort both.
        (State::Configuration, Direction::Clientbound, 7) => {
            match structured::canon_registry_data(&packet.body) {
                Some(body) => (
                    body,
                    "registry entries + NBT fields sorted (per-boot HashMap order) -> canonical"
                        .into(),
                ),
                None => (
                    packet.body.clone(),
                    "registry_data canonicalization FAILED — raw body kept".into(),
                ),
            }
        }
        // update_tags: registries, tags, and tag entry ids iterate per-boot;
        // sort all three levels.
        (State::Configuration, Direction::Clientbound, 13) => {
            match structured::canon_update_tags(&packet.body) {
                Some(body) => (
                    body,
                    "tag registries/tags/ids sorted (per-boot HashMap order) -> canonical".into(),
                ),
                None => (
                    packet.body.clone(),
                    "update_tags canonicalization FAILED — raw body kept".into(),
                ),
            }
        }
        // keep_alive: body is an 8-byte random long (server → client) or the
        // client's echo (client → server). Paper uses the same one-second
        // keepConnectionAlive tick in configuration and play, so the id is
        // nondeterministic in every state where the packet can appear.
        (State::Configuration, Direction::Clientbound, 4)
        | (State::Configuration, Direction::Serverbound, 4)
        | (State::Play, Direction::Clientbound, 44)
        | (State::Play, Direction::Serverbound, 28) => {
            let mut body = packet.body.clone();
            zero_bytes(&mut body, 0, 8);
            (body, "keepalive id (server-random long) -> 0".into())
        }
        // set_entity_data: entity id VarInt at the head.
        (State::Play, Direction::Clientbound, 99) => {
            let body = rewrite_entity_id(&packet.body, 1);
            (body, "entity id (server-assigned counter) -> 1".into())
        }
        // add_entity: [id VarInt][uuid 16][type VarInt][x][y][z][yRot][xRot][yHeadRot][data][vel...]
        (State::Play, Direction::Clientbound, 1) => {
            let mut body = rewrite_entity_id(&packet.body, 1);
            let mut type_off = 1 + 16;
            if let Some(type_end) = frame::read_varint(&body, &mut type_off).map(|_| type_off) {
                let x_off = type_end;
                // y at x_off+8 kept (deterministic spawn height).
                let z_off = type_end + 16;
                let yaw_off = type_end + 24;
                let pitch_off = type_end + 25;
                let head_off = type_end + 26;
                let mut data_off = type_end + 27;
                if let Some(data_end) = frame::read_varint(&body, &mut data_off).map(|_| data_off) {
                    let vx_off = data_end;
                    let vz_off = data_end + 4;
                    zero_f64(&mut body, x_off); // x
                    zero_f64(&mut body, z_off); // z
                    body[yaw_off] = 0; // yRot
                    body[pitch_off] = 0; // xRot
                    body[head_off] = 0; // yHeadRot
                    zero_bytes(&mut body, vx_off, 2); // velX
                    zero_bytes(&mut body, vz_off, 2); // velZ
                }
            }
            (
                body,
                "entity id -> 1; position x/z, rotation, velocity -> 0".into(),
            )
        }
        // entity_event: [Int entityId][Byte eventId] — Paper's
        // `ClientboundEntityEventPacket` uses `writeInt` for the entity id (not a
        // VarInt), then `writeByte` for the event.
        (State::Play, Direction::Clientbound, 34) => {
            let mut body = packet.body.clone();
            if body.len() >= 5 {
                body[0..4].copy_from_slice(&1i32.to_be_bytes()); // entity id -> 1
                body[4] = 0; // event id
            }
            (body, "entity id -> 1; event id -> 0".into())
        }
        // entity_position_sync: [id VarInt][x][y][z][dx][dy][dz][yaw][pitch][onGround].
        (State::Play, Direction::Clientbound, 35) => {
            let mut body = rewrite_entity_id(&packet.body, 1);
            zero_f64(&mut body, 1); // x
            // y at 9..17 kept.
            zero_f64(&mut body, 17); // z
            zero_f64(&mut body, 25); // dx
            zero_f64(&mut body, 41); // dz
            zero_f32(&mut body, 49); // yaw
            zero_f32(&mut body, 53); // pitch
            (body, "entity id -> 1; position x/z, yaw/pitch -> 0".into())
        }
        // move_entity_pos: [id][xa short][ya short][za short][onGround].
        (State::Play, Direction::Clientbound, 53) => {
            let mut body = rewrite_entity_id(&packet.body, 1);
            zero_bytes(&mut body, 1, 2); // xa
            zero_bytes(&mut body, 5, 2); // za
            (body, "entity id -> 1; horizontal deltas -> 0".into())
        }
        // move_entity_pos_rot: [id][xa][ya][za][yRot][xRot][onGround].
        (State::Play, Direction::Clientbound, 54) => {
            let mut body = rewrite_entity_id(&packet.body, 1);
            zero_bytes(&mut body, 1, 2); // xa
            zero_bytes(&mut body, 5, 2); // za
            zero_bytes(&mut body, 7, 2); // yRot xRot
            (
                body,
                "entity id -> 1; horizontal deltas, rotation -> 0".into(),
            )
        }
        // move_entity_rot: [id][yRot][xRot][onGround].
        (State::Play, Direction::Clientbound, 56) => {
            let mut body = rewrite_entity_id(&packet.body, 1);
            zero_bytes(&mut body, 1, 2); // yRot xRot
            (body, "entity id -> 1; rotation -> 0".into())
        }
        // rotate_head: [id][yHeadRot byte].
        (State::Play, Direction::Clientbound, 83) => {
            let mut body = rewrite_entity_id(&packet.body, 1);
            zero_bytes(&mut body, 1, 1);
            (body, "entity id -> 1; head yaw -> 0".into())
        }
        // set_entity_motion: [id][velX short][velY short][velZ short].
        (State::Play, Direction::Clientbound, 101) => {
            let mut body = rewrite_entity_id(&packet.body, 1);
            zero_bytes(&mut body, 1, 6);
            (body, "entity id -> 1; velocity -> 0".into())
        }
        // update_attributes: [id VarInt][list...] — attribute values for the
        // player are deterministic; the entity id and the snapshot list order
        // vary per boot.
        (State::Play, Direction::Clientbound, 131) => {
            let body = rewrite_entity_id(&packet.body, 1);
            match structured::canon_update_attributes(&body) {
                Some(c) => (
                    c,
                    "entity id -> 1; attribute snapshots sorted by id (per-boot collection order) -> canonical".into(),
                ),
                None => (body, "entity id (server-assigned counter) -> 1".into()),
            }
        }
        // update_advancements: the added/removed/progress lists iterate per-boot
        // (HashMap/HashSet-backed); sort all three, zero the obtained instants
        // (wall-clock when the fresh-join advancements were granted), and
        // structurally canonicalize each display payload (NBT compound field
        // order + DataComponentPatch entry order) — see rivet-decode::advancement.
        (State::Play, Direction::Clientbound, 130) => {
            match rivet_decode::advancement::canon_update_advancements(&packet.body) {
                Some(body) => (
                    body,
                    "advancement added/removed/progress + criteria sorted; display NBT/DataComponentPatch canonicalized; obtained instants -> 0 (per-boot) -> canonical".into(),
                ),
                None => (packet.body.clone(), "update_advancements canonicalization FAILED — raw body kept".into()),
            }
        }
        // update_recipes: the property-set map and each set's item list iterate
        // per-boot (HashMap/HashSet-backed); sort both.
        (State::Play, Direction::Clientbound, 133) => {
            match structured::canon_update_recipes(&packet.body) {
                Some(body) => (
                    body,
                    "recipe property-set map + item lists sorted (per-boot map order) -> canonical"
                        .into(),
                ),
                None => (
                    packet.body.clone(),
                    "update_recipes canonicalization FAILED — raw body kept".into(),
                ),
            }
        }
        // set_time: gameTime and the clock totalTicks are tick-dependent, but the
        // clock COUNT and holder ids are structurally fixed by the world's clock
        // registry. Re-encode the body structurally (gameTime/ticks -> 0) so the
        // canonical form is valid wire format while retaining the holder set.
        (State::Play, Direction::Clientbound, 113) => match canonical_set_time(&packet.body) {
            Some(body) => (
                body,
                "set_time tick-dependent (gameTime + clock ticks) -> 0; holder ids kept, sorted"
                    .into(),
            ),
            None => (
                packet.body.clone(),
                "set_time canonicalization FAILED — raw body kept".into(),
            ),
        },
        // accept_teleportation: [VarInt teleport id] (echo of player_position).
        (State::Play, Direction::Serverbound, 0) => {
            (vec![0u8], "teleport id (server counter) -> 0".into())
        }
        // move_player_pos (movement sample): x/z doubles, keep y.
        (State::Play, Direction::Serverbound, 30) => {
            let mut body = packet.body.clone();
            zero_f64(&mut body, 0); // x
            // y at 8..16 kept.
            zero_f64(&mut body, 16); // z
            (body, "movement x/z (spawn offset randomized) -> 0".into())
        }
        // move_player_pos_rot: x/z doubles + yaw/pitch floats, keep y.
        (State::Play, Direction::Serverbound, 31) => {
            let mut body = packet.body.clone();
            zero_f64(&mut body, 0); // x
            // y at 8..16 kept.
            zero_f64(&mut body, 16); // z
            zero_f32(&mut body, 24); // yRot
            zero_f32(&mut body, 28); // xRot
            (
                body,
                "movement x/z, yaw/pitch (spawn offset randomized) -> 0".into(),
            )
        }
        // move_player_rot: yaw/pitch floats.
        (State::Play, Direction::Serverbound, 32) => {
            let mut body = packet.body.clone();
            zero_f32(&mut body, 0); // yRot
            zero_f32(&mut body, 4); // xRot
            (body, "movement yaw/pitch -> 0".into())
        }
        _ => (packet.body.clone(), String::new()),
    };
    NormalizedPacket {
        state: packet.state,
        direction: packet.direction,
        id: packet.id,
        body,
        note,
    }
}

/// Re-encode a `set_time` body structurally: keep the clock count, holder ids,
/// `partialTick`, and `rate` from the raw body (the world's clock registry and
/// the join-time clock state, stable across boots) but zero the tick-dependent
/// fields (`gameTime`, each clock's `totalTicks`). The raw body is `[i64
/// gameTime][VarInt count][count × (VarInt holder][VarLong totalTicks][f32
/// partialTick][f32 rate])]`; the re-encoded form is valid wire format, so a
/// ported codec round-trips it. The clock list is written sorted by holder id —
/// Java serializes the world-clock map per boot — so two boots canonicalize to
/// identical bytes. Returns `None` when the body is not structurally parseable;
/// the caller keeps the raw body and records a note so the fixture surfaces the
/// malformation.
fn canonical_set_time(body: &[u8]) -> Option<Vec<u8>> {
    let mut off = 0;
    frame::read_i64(body, &mut off)?; // gameTime (zeroed below)
    let count = frame::read_varint(body, &mut off)?;
    let mut holders = Vec::with_capacity((count.max(0) as usize).min(MAX_INITIAL_COLLECTION_SIZE));
    for _ in 0..count.max(0) {
        let holder = frame::read_varint(body, &mut off)?;
        frame::read_varlong(body, &mut off)?; // totalTicks (zeroed below)
        let partial = frame::read_f32(body, &mut off)?;
        let rate = frame::read_f32(body, &mut off)?;
        holders.push((holder, partial, rate));
    }
    if off != body.len() {
        return None;
    }
    holders.sort_by_key(|(holder, _, _)| *holder);

    let mut out = Vec::with_capacity(body.len());
    out.extend_from_slice(&0i64.to_be_bytes()); // gameTime -> 0
    frame::write_varint(&mut out, holders.len() as i32);
    for (holder, partial, rate) in holders {
        frame::write_varint(&mut out, holder); // holder id kept
        frame::write_varint(&mut out, 0); // totalTicks -> 0
        out.extend_from_slice(&partial.to_be_bytes());
        out.extend_from_slice(&rate.to_be_bytes());
    }
    Some(out)
}

/// Racy packet ids: their per-boot COUNT or exact placement in the stream is
/// timing-dependent, so the canonical form keeps only the first occurrence as a
/// deterministic sample (compared for byte identity, not count).
fn is_racy(state: State, direction: Direction, id: i32) -> bool {
    matches!(
        (state, direction, id),
        // Play keepalives arrive at a server-chosen instant, possibly interleaved.
        // Configuration keepalives are omitted entirely by `omit_from_canonical`.
        (State::Play, Direction::Clientbound, 44)
            | (State::Play, Direction::Serverbound, 28)
            // per-tick client traffic: count depends on tick alignment.
            | (State::Play, Direction::Serverbound, 33) // move_player_status_only
            | (State::Play, Direction::Serverbound, 13) // client_tick_end
            // the movement sample: the first position packet after spawn.
            | (State::Play, Direction::Serverbound, 30) // move_player_pos
            // chunk-batch acks: one per batch, timing-dependent.
            | (State::Play, Direction::Serverbound, 11) // chunk_batch_received
            // set_time: canonicalized, so a single sample suffices.
            | (State::Play, Direction::Clientbound, 113)
    )
}

/// Paper's `ServerCommonPacketListenerImpl.keepConnectionAlive` emits a
/// configuration keepalive every second while the asynchronous registry join is
/// in progress. Whether that watchdog crosses the capture's configuration
/// window depends on boot and loopback scheduling, so it is not part of the
/// fixture's protocol-content contract. Raw invariants still validate every
/// request/echo pair before this canonical-only omission.
fn omit_from_canonical(state: State, direction: Direction, id: i32) -> bool {
    state == State::Configuration
        && id == 4
        && matches!(direction, Direction::Clientbound | Direction::Serverbound)
}

/// Build the canonical, deterministic packet list from a raw capture.
///
/// Grouping is by (state, direction, id); within a group, racy ids are reduced
/// to their first occurrence and chunk packets are sorted by their (now
/// spawn-relative) coordinates. The output is a stable, ordered list suitable
/// for byte-for-byte fixture comparison.
pub fn canonicalize(packets: &[CapturedPacket]) -> Vec<NormalizedPacket> {
    let spawn = find_spawn(packets);
    let normalized: Vec<NormalizedPacket> =
        packets.iter().map(|p| normalize_packet(p, spawn)).collect();

    // Group by (state, direction, id), preserving capture order.
    let mut groups: BTreeMap<(u8, u8, i32), Vec<NormalizedPacket>> = BTreeMap::new();
    for p in normalized {
        let state = state_rank(p.state);
        let dir = direction_rank(p.direction);
        groups.entry((state, dir, p.id)).or_default().push(p);
    }

    let mut out = Vec::new();
    for (_key, group) in groups {
        if omit_from_canonical(group[0].state, group[0].direction, group[0].id) {
            continue;
        }
        if is_racy(group[0].state, group[0].direction, group[0].id) {
            // Sample the first occurrence only.
            if let Some(first) = group.first() {
                out.push(first.clone());
            }
        } else if group[0].id == 45 && group[0].state == State::Play {
            // Sort chunk packets by translated coordinates for a deterministic
            // order (the receive order is not part of the parity contract).
            let mut chunks: Vec<NormalizedPacket> = group;
            chunks.sort_by_key(|c| {
                let mut off = 0;
                let cx = frame::read_i32(&c.body, &mut off).unwrap_or(i32::MAX);
                let cz = frame::read_i32(&c.body, &mut off).unwrap_or(i32::MAX);
                (cx, cz)
            });
            out.extend(chunks);
        } else {
            out.extend(group);
        }
    }
    out
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

fn direction_rank(d: Direction) -> u8 {
    match d {
        Direction::Serverbound => 0,
        Direction::Clientbound => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::write_varint;
    use crate::frame::{read_f32, read_i64};
    use std::path::PathBuf;

    fn pkt(state: State, direction: Direction, id: i32, body: Vec<u8>) -> CapturedPacket {
        CapturedPacket {
            state,
            direction,
            id,
            body,
        }
    }

    #[test]
    fn find_spawn_from_player_position() {
        let mut body = Vec::new();
        write_varint(&mut body, 0); // teleport id
        body.extend_from_slice(&8.5f64.to_be_bytes());
        body.extend_from_slice(&(-60.0f64).to_be_bytes());
        body.extend_from_slice(&(-3.5f64).to_be_bytes());
        let packets = vec![pkt(State::Play, Direction::Clientbound, 72, body)];
        let spawn = find_spawn(&packets).expect("spawn");
        assert_eq!(spawn.chunk_x, 0);
        assert_eq!(spawn.chunk_z, -1);
    }

    #[test]
    fn normalize_login_player_id() {
        let body = 42i32.to_be_bytes().to_vec();
        let n = normalize_packet(&pkt(State::Play, Direction::Clientbound, 49, body), None);
        assert_eq!(&n.body[0..4], &1i32.to_be_bytes());
        assert!(!n.note.is_empty());
    }

    #[test]
    fn normalize_chunk_coords_translate() {
        let mut body = Vec::new();
        body.extend_from_slice(&3i32.to_be_bytes());
        body.extend_from_slice(&(-2i32).to_be_bytes());
        body.extend_from_slice(&[0xAB; 10]);
        let spawn = SpawnCtx {
            x: 0.0,
            z: 0.0,
            chunk_x: 3,
            chunk_z: -2,
        };
        let n = normalize_packet(
            &pkt(State::Play, Direction::Clientbound, 45, body),
            Some(spawn),
        );
        let mut off = 0;
        assert_eq!(frame::read_i32(&n.body, &mut off), Some(0));
        assert_eq!(frame::read_i32(&n.body, &mut off), Some(0));
    }

    fn player_info_body(latency: i32) -> Vec<u8> {
        // All eight actions, one offline RivetProbe entry, null chat/display
        // names, and the initial list order/show-hat fields.
        let mut body = vec![0xff, 1];
        body.extend_from_slice(&[0; 16]); // profile UUID
        write_varint(&mut body, 10);
        body.extend_from_slice(b"RivetProbe");
        body.push(0); // profile properties
        body.push(0); // chat session
        write_varint(&mut body, 0); // game mode
        body.push(1); // listed
        write_varint(&mut body, latency);
        body.push(0); // display name
        write_varint(&mut body, 0); // list order
        body.push(1); // show hat
        body
    }

    #[test]
    fn normalize_player_info_latency_to_zero() {
        let n = normalize_packet(
            &pkt(
                State::Play,
                Direction::Clientbound,
                70,
                player_info_body(28),
            ),
            None,
        );
        assert_eq!(n.body, player_info_body(0));
        assert!(n.note.contains("latency"));
    }

    #[test]
    fn normalize_player_info_does_not_mask_profile_mutation() {
        let mut tampered = player_info_body(28);
        // The profile name starts after the action mask, count, UUID, and its
        // ten-byte VarInt-prefixed name length.
        tampered[19] ^= 1;
        let normalized = normalize_packet(
            &pkt(State::Play, Direction::Clientbound, 70, tampered),
            None,
        );
        assert_ne!(normalized.body, player_info_body(0));
    }

    #[test]
    fn normalize_keepalive_zeroes_long() {
        let body = 0xDEADBEEFCAFEBABEu64 as i64 as i128;
        let body = (body as i64).to_be_bytes().to_vec();
        for (state, dir, id) in [
            (State::Configuration, Direction::Clientbound, 4),
            (State::Configuration, Direction::Serverbound, 4),
            (State::Play, Direction::Clientbound, 44),
            (State::Play, Direction::Serverbound, 28),
        ] {
            let n = normalize_packet(&pkt(state, dir, id, body.clone()), None);
            assert_eq!(&n.body[0..8], &0i64.to_be_bytes());
        }
    }

    #[test]
    fn normalize_set_time_is_canonical_and_parseable() {
        // A real full-sync body: gameTime 16, holders {0,1} at ticks 16.
        let mut raw = Vec::new();
        raw.extend_from_slice(&16i64.to_be_bytes());
        frame::write_varint(&mut raw, 2);
        for h in [0, 1] {
            frame::write_varint(&mut raw, h);
            raw.push(16); // totalTicks (1-byte VarLong)
            raw.extend_from_slice(&0.0f32.to_be_bytes());
            raw.extend_from_slice(&1.0f32.to_be_bytes());
        }
        let n = normalize_packet(&pkt(State::Play, Direction::Clientbound, 113, raw), None);
        let mut off = 0;
        assert_eq!(read_i64(&n.body, &mut off), Some(0)); // gameTime -> 0
        assert_eq!(frame::read_varint(&n.body, &mut off), Some(2)); // 2 clocks kept
        // entry 1
        assert_eq!(frame::read_varint(&n.body, &mut off), Some(0)); // overworld holder kept
        assert_eq!(frame::read_varint(&n.body, &mut off), Some(0)); // totalTicks -> 0
        assert_eq!(read_f32(&n.body, &mut off), Some(0.0));
        assert_eq!(read_f32(&n.body, &mut off), Some(1.0));
        // entry 2
        assert_eq!(frame::read_varint(&n.body, &mut off), Some(1)); // the_end holder kept
        assert_eq!(frame::read_varint(&n.body, &mut off), Some(0));
        assert_eq!(read_f32(&n.body, &mut off), Some(0.0));
        assert_eq!(read_f32(&n.body, &mut off), Some(1.0));
        assert_eq!(off, n.body.len(), "no trailing bytes");
    }

    #[test]
    fn normalize_set_time_sorts_holder_order_deterministically() {
        // Two raw full-sync bodies whose clock-holder order differs across boots
        // (Java serializes the world-clock map per boot) must canonicalize to
        // identical bytes — this is the raw1/raw2 regression.
        fn raw(game_time: i64, holders: &[i32]) -> Vec<u8> {
            let mut body = Vec::new();
            body.extend_from_slice(&game_time.to_be_bytes());
            frame::write_varint(&mut body, holders.len() as i32);
            for &h in holders {
                frame::write_varint(&mut body, h);
                frame::write_varint(&mut body, 16); // totalTicks (1-byte VarLong)
                body.extend_from_slice(&0.0f32.to_be_bytes());
                body.extend_from_slice(&1.0f32.to_be_bytes());
            }
            body
        }
        let a = normalize_packet(
            &pkt(State::Play, Direction::Clientbound, 113, raw(16, &[0, 1])),
            None,
        );
        let b = normalize_packet(
            &pkt(State::Play, Direction::Clientbound, 113, raw(7, &[1, 0])),
            None,
        );
        assert_eq!(
            a.body, b.body,
            "reversed holder order must canonicalize identically"
        );
        assert_eq!(a.body.len(), 29);

        let mut off = 0;
        assert_eq!(read_i64(&a.body, &mut off), Some(0)); // gameTime -> 0
        assert_eq!(frame::read_varint(&a.body, &mut off), Some(2));
        // Holder ids retained, written ascending.
        assert_eq!(frame::read_varint(&a.body, &mut off), Some(0));
        assert_eq!(frame::read_varint(&a.body, &mut off), Some(0)); // totalTicks -> 0
        assert_eq!(read_f32(&a.body, &mut off), Some(0.0));
        assert_eq!(read_f32(&a.body, &mut off), Some(1.0));
        assert_eq!(frame::read_varint(&a.body, &mut off), Some(1));
        assert_eq!(frame::read_varint(&a.body, &mut off), Some(0));
        assert_eq!(read_f32(&a.body, &mut off), Some(0.0));
        assert_eq!(read_f32(&a.body, &mut off), Some(1.0));
        assert_eq!(off, a.body.len(), "no trailing bytes");
    }

    #[test]
    fn normalize_set_time_keeps_holder_ids_from_raw() {
        // A malformed-schema body with holder id 5 must keep holder 5.
        let mut raw = Vec::new();
        raw.extend_from_slice(&42i64.to_be_bytes());
        frame::write_varint(&mut raw, 1);
        frame::write_varint(&mut raw, 5);
        raw.push(0);
        raw.extend_from_slice(&0.0f32.to_be_bytes());
        raw.extend_from_slice(&1.0f32.to_be_bytes());
        let n = normalize_packet(&pkt(State::Play, Direction::Clientbound, 113, raw), None);
        let mut off = 0;
        assert_eq!(read_i64(&n.body, &mut off), Some(0));
        assert_eq!(frame::read_varint(&n.body, &mut off), Some(1));
        assert_eq!(
            frame::read_varint(&n.body, &mut off),
            Some(5),
            "holder id kept"
        );
    }

    #[test]
    fn canonicalize_reduces_racy_ids_to_first_occurrence() {
        let packets = vec![
            pkt(State::Play, Direction::Serverbound, 13, vec![]),
            pkt(State::Play, Direction::Serverbound, 13, vec![]),
            pkt(State::Play, Direction::Serverbound, 13, vec![]),
        ];
        let canon = canonicalize(&packets);
        assert_eq!(canon.len(), 1);
        assert_eq!(canon[0].id, 13);
    }

    #[test]
    fn canonicalize_omits_configuration_keepalives_but_keeps_known_packs() {
        let packets = vec![
            pkt(
                State::Configuration,
                Direction::Clientbound,
                4,
                123i64.to_be_bytes().to_vec(),
            ),
            pkt(
                State::Configuration,
                Direction::Serverbound,
                4,
                123i64.to_be_bytes().to_vec(),
            ),
            pkt(State::Configuration, Direction::Serverbound, 7, vec![0]),
        ];
        let canon = canonicalize(&packets);
        assert_eq!(canon.len(), 1);
        assert_eq!(canon[0].id, 7);
        assert_eq!(canon[0].direction, Direction::Serverbound);
    }

    #[test]
    fn canonicalize_does_not_mask_known_pack_response_payload() {
        let empty = canonicalize(&[pkt(
            State::Configuration,
            Direction::Serverbound,
            7,
            vec![0],
        )]);
        let nonempty = canonicalize(&[pkt(
            State::Configuration,
            Direction::Serverbound,
            7,
            vec![1],
        )]);
        assert_ne!(empty[0].body, nonempty[0].body);
    }

    // -- update_advancements display-canonicalization e2e (#221) --------------
    // The builders below hand-craft raw wire bytes (VarInt counts, VarInt-
    // prefixed identifiers, big-endian NBT strings, unsorted compound fields)
    // so the tests exercise the REAL normalize_packet pipeline against display
    // payloads that genuinely vary in per-boot order, independent of any writer
    // that would sort on emit.

    fn wstr(out: &mut Vec<u8>, s: &str) {
        write_varint(out, s.len() as i32);
        out.extend_from_slice(s.as_bytes());
    }

    /// Raw NBT string payload: `[u16 len][chars]` (no type byte).
    fn raw_str(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(s.len() as u16).to_be_bytes());
        out.extend_from_slice(s.as_bytes());
        out
    }

    /// Raw NBT compound in the GIVEN field order (unsorted allowed) —
    /// `[type 10][field]*[type 0]`.
    fn raw_compound(fields: &[(&str, u8, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(10);
        for (name, type_id, payload) in fields {
            out.push(*type_id);
            out.extend_from_slice(&(name.len() as u16).to_be_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(payload);
        }
        out.push(0);
        out
    }

    /// A `DataComponentPatch` with the positive entries in the given order.
    fn patch(entries: &[(u32, Vec<u8>)], neg: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        write_varint(&mut out, entries.len() as i32);
        write_varint(&mut out, neg.len() as i32);
        for (id, value) in entries {
            write_varint(&mut out, *id as i32);
            out.extend_from_slice(value);
        }
        for id in neg {
            write_varint(&mut out, *id as i32);
        }
        out
    }

    /// A `DisplayInfo` value: title/desc as raw NBT, an icon whose patch entry
    /// order follows `order`, frame 0, flags 0, fixed position floats.
    fn display(
        title: &[u8],
        desc: &[u8],
        components: &[(u32, Vec<u8>)],
        neg: &[u32],
        order: &[usize],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(title);
        out.extend_from_slice(desc);
        write_varint(&mut out, 926); // icon item
        write_varint(&mut out, 1); // icon count
        let mut entries = Vec::new();
        for &i in order {
            entries.push(components[i].clone());
        }
        out.extend_from_slice(&patch(&entries, neg));
        write_varint(&mut out, 0); // frame
        out.extend_from_slice(&0i32.to_be_bytes()); // flags
        out.extend_from_slice(&0.5f32.to_be_bytes());
        out.extend_from_slice(&(-1.25f32).to_be_bytes());
        out
    }

    /// An advancement value: `[id][no parent][display?][requirements][telemetry]`.
    fn advancement(id: &str, display: Option<&[u8]>) -> Vec<u8> {
        let mut out = Vec::new();
        wstr(&mut out, id);
        out.push(0); // no parent
        match display {
            Some(d) => {
                out.push(1);
                out.extend_from_slice(d);
            }
            None => out.push(0),
        }
        write_varint(&mut out, 1); // one requirement group
        write_varint(&mut out, 1); // one name
        wstr(&mut out, "unlock_right_away");
        out.push(0); // telemetry
        out
    }

    /// An `update_advancements` body with no removed/progress entries.
    fn adv_body(reset: bool, added: &[Vec<u8>], show: bool) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(reset as u8);
        write_varint(&mut out, added.len() as i32);
        for a in added {
            out.extend_from_slice(a);
        }
        write_varint(&mut out, 0); // removed
        write_varint(&mut out, 0); // progress
        out.push(show as u8);
        out
    }

    #[test]
    fn normalize_advancements_canonicalizes_display_in_the_real_pipeline() {
        // #221 e2e through the REAL normalize path (normalize_packet), not the
        // rivet-decode unit API. Two raw update_advancements bodies are
        // semantically identical but their display wire bytes differ in
        // per-boot order: the title NBT compound field order and the
        // DataComponentPatch positive-entry order. Both must normalize to the
        // SAME canonical bytes.
        //
        // This is non-vacuous for deletion/bypass: if someone removes the
        // `rivet_decode::advancement` wiring from normalize_packet, each body is
        // passed through raw, and because the two raws differ the equality
        // assertion fails.
        let title_unsorted =
            raw_compound(&[("text", 8, raw_str("A")), ("color", 8, raw_str("red"))]);
        let title_sorted = raw_compound(&[("color", 8, raw_str("red")), ("text", 8, raw_str("A"))]);
        let desc = raw_compound(&[("text", 8, raw_str("D"))]);

        let components = &[
            (6u32, raw_compound(&[("text", 8, raw_str("x"))])), // custom_name
            (0u32, raw_compound(&[("extra", 8, raw_str("y"))])), // custom_data
        ];

        // body_a: unsorted compound + unsorted patch entry order [6, 0].
        let body_a = adv_body(
            false,
            &[advancement(
                "story:root",
                Some(&display(&title_unsorted, &desc, components, &[], &[0, 1])),
            )],
            true,
        );
        // body_b: sorted compound + sorted patch entry order [0, 6].
        let body_b = adv_body(
            false,
            &[advancement(
                "story:root",
                Some(&display(&title_sorted, &desc, components, &[], &[1, 0])),
            )],
            true,
        );
        assert_ne!(body_a, body_b, "the two raw bodies must differ");

        let na = normalize_packet(
            &pkt(State::Play, Direction::Clientbound, 130, body_a.clone()),
            None,
        );
        let nb = normalize_packet(&pkt(State::Play, Direction::Clientbound, 130, body_b), None);
        assert_eq!(
            na.body, nb.body,
            "display-bearing bodies must canonicalize identically through normalize_packet"
        );
        assert!(
            na.note.contains("display"),
            "normalization note must document the display rewrite: {:?}",
            na.note
        );
        // The canonical output must differ from the unsorted raw input — if the
        // display path were bypassed (raw passed through), this fails.
        assert_ne!(
            na.body, body_a,
            "the display payload must actually be rewritten, not passed through"
        );
        // Idempotent through the pipeline.
        let twice = normalize_packet(
            &pkt(State::Play, Direction::Clientbound, 130, na.body.clone()),
            None,
        );
        assert_eq!(twice.body, na.body);
    }

    #[test]
    fn normalize_advancements_keeps_real_no_display_fixture_byte_identical() {
        // The pinned join fixture's update_advancements carries no display
        // payload and its lists/criteria are already in canonical order, so
        // normalize_packet must pass it through byte-identically. Proving this
        // against the committed fixture guards against a regression where the
        // display path rewrites bytes that the real (no-display) capture
        // already produced.
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/join");
        let packets = crate::fixture::read_capture(&dir).expect("fixture loads");
        let adv = packets
            .iter()
            .find(|p| {
                p.state == State::Play && p.direction == Direction::Clientbound && p.id == 130
            })
            .expect("fixture contains update_advancements");
        let n = normalize_packet(
            &CapturedPacket {
                state: State::Play,
                direction: Direction::Clientbound,
                id: 130,
                body: adv.body.clone(),
            },
            None,
        );
        assert_eq!(
            n.body, adv.body,
            "the real no-display fixture body must pass through byte-identically"
        );
        assert!(
            n.note.contains("display"),
            "note should still describe the display canonicalization path"
        );
    }

    #[test]
    fn normalize_entity_event_rewrites_int_id_and_event() {
        // Paper's ClientboundEntityEventPacket uses writeInt for the entity id
        // (not a VarInt) followed by a writeByte event. Raw: Int 0 + Byte 0x18.
        let mut body = Vec::new();
        body.extend_from_slice(&0i32.to_be_bytes());
        body.push(0x18);
        let n = normalize_packet(&pkt(State::Play, Direction::Clientbound, 34, body), None);
        assert_eq!(&n.body[0..4], &1i32.to_be_bytes(), "entity id (Int) -> 1");
        assert_eq!(n.body[4], 0, "event id -> 0");
        assert_eq!(n.body.len(), 5);
        assert!(!n.note.is_empty());
    }

    #[test]
    fn normalize_add_entity_rewrites_id_and_position() {
        let mut body = Vec::new();
        write_varint(&mut body, 5); // entity id
        body.extend_from_slice(&[0xAA; 16]); // uuid
        write_varint(&mut body, 63); // type (player)
        body.extend_from_slice(&8.5f64.to_be_bytes()); // x
        body.extend_from_slice(&(-60.0f64).to_be_bytes()); // y
        body.extend_from_slice(&(-3.5f64).to_be_bytes()); // z
        body.extend_from_slice(&[0u8; 9]); // yaw/pitch/headYaw/data/velocity...
        let n = normalize_packet(&pkt(State::Play, Direction::Clientbound, 1, body), None);
        let mut off = 0;
        assert_eq!(
            frame::read_varint(&n.body, &mut off),
            Some(1),
            "entity id -> 1"
        );
        off += 16; // uuid
        let type_end = frame::read_varint(&n.body, &mut off).unwrap();
        let _ = type_end; // type value
        let x = frame::read_f64(&n.body, &mut off).unwrap();
        assert_eq!(x, 0.0, "x normalized to 0");
    }
}
