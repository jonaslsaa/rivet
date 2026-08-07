//! Port of `ca.spottedleaf.moonrise.patches.chunk_system.player.RegionizedPlayerChunkLoader`
//! (MC 26.2, Paper) — the Moonrise direct chunk-send path (issue #100).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/
//! moonrise/patches/chunk_system/player/RegionizedPlayerChunkLoader.java`
//! (1104 lines).
//!
//! Owned by the `ca.spottedleaf.moonrise.patches.chunk_system.player` manifest
//! unit (MANIFEST line 788). This slice ports the **direct-send half** only —
//! `PlayerChunkLoaderData.add()` + the send-queue drain — reduced to the M1
//! superflat world. Moonrise bypasses vanilla `PlayerChunkSender.sendNextChunks`
//! batching: each chunk is sent as a bare `ClientboundLevelChunkWithLightPacket`
//! (the pinned #153 join capture contains no `ChunkBatchStart`/`ChunkBatchFinished`).
//!
//! **Chunk ordering is a deterministic canonicalization, not Paper's wire
//! order.** Paper's raw receive order is the `sendQueue` heap drain (squared
//! distance from the cache center), whose equal-distance tie-break depends on
//! chunk-load timing and differs across boots. `rivet-capture` therefore
//! excludes chunk order from the parity contract (`ordering.rs`:
//! "chunk order is outside the parity contract") and `canonicalize` sorts the
//! capture's chunk packets by coordinate (`normalize.rs`). This port emits that
//! deterministic X-major/Z-minor coordinate raster via
//! `ChunkTrackingView.for_each` (corners skipped) — the order the canonical
//! fixture byte-matches. A faithful port of the `sendQueue` distance order is
//! deliberately not attempted: it is non-deterministic across Paper boots.
//!
//! The chunk *pipeline* (tickets, distance maps, `SingleUserAreaMap` load/tick
//! tracking, the rate limiters, the per-stage queues) is deferred with the
//! owning unit's remaining scope (#185). The M1 world holds one loaded spawn
//! chunk and every other view position has byte-identical deterministic
//! superflat content (the #194 fixture proves all 117 bodies differ only in the
//! 8-byte coordinate header), so the send path resolves each view chunk's
//! content directly.
//!
//! Ownership per OWNERSHIP §Network: this runs on the tick thread and produces
//! play-state packets for a connection's bounded outbound channel. The packets
//! are plain values (`id` + body); the caller frames them for the wire and
//! queues them (`ConnectionRegistry::send`). Compression stays per-connection:
//! the tick thread frames with the config threshold the M1 login applies to
//! every connection, exactly as `Connection::send_packet` does on the tokio
//! side (the #96 per-connection refinement is a `RivetTodo`).
//!
//! RivetTodo(#101): the `ServerPlayer`/`placeNewPlayer` join burst that calls
//! `addPlayer` + `update` each tick — this slice is invoked directly by tests
//! and exposes the M1 send-set. RivetTodo(#185): the per-stage send/load queues
//! and rate limiters the full `updateQueues` drains.

use bytes::{Bytes, BytesMut};

use rivet_protocol::codec::StreamEncoder;
use rivet_protocol::compression_encoder::CompressionEncoder;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::packets::play::clientbound::PacketType as PlayClientbound;
use rivet_protocol::protocol::game::clientbound_level_chunk_with_light::ClientboundLevelChunkWithLightPacket;
use rivet_protocol::protocol::game::clientbound_set_chunk_cache_center::ClientboundSetChunkCacheCenterPacket;
use rivet_protocol::protocol::game::clientbound_set_chunk_cache_radius::ClientboundSetChunkCacheRadiusPacket;
use rivet_protocol::protocol::game::clientbound_set_simulation_distance::ClientboundSetSimulationDistancePacket;
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_protocol::var_int;
use rivet_protocol::varint21_length_field_prepender::encode_frame;
use rivet_registry::RegistryAccess;
use rivet_registry::core::ChunkPos;

use super::chunk_tracking_view::ChunkTrackingView;
use super::server_level::ServerLevel;

// `ServerChunkCache.setSendViewDistance` + `ChunkMap.setServerViewDistance` map
// to Moonrise `setSendDistance(viewDistance)` / `setLoadDistance(viewDistance + 1)`
// (the world half of the `ViewDistances` record). The M1 world pins these to
// the `view-distance=4` fixture; the per-player overrides are absent (values
// `-1`), so the Moonrise distance ladder below collapses to
// `tick = min(simulation, load - 1)`, `load = view + 1`, `send = view`.
//
// The `±1` steps wrap like Java ints (PORTING.md: all arithmetic wrapping
// unless proven range-safe): the `-1`/`+1` in the Java ladder are int
// additions/subtractions, so `wrapping_sub`/`wrapping_add` preserve the wrap at
// i32 extremes. The reachable M1 inputs (world distances in `[2, 32]`, the -1
// sentinels) never approach the extremes, so the wrap is unreachable today.

/// `PlatformHooks.configAutoConfigSendDistance()` — the Paper default
/// `auto-config-send-distance` is true; Rivet has no config knob yet, so the
/// `sendViewDistance` ladder resolves the world's send distance when the client
/// requests none. RivetTodo(#236): the Paper config knob.
const AUTO_CONFIG_SEND_DISTANCE: bool = true;

/// `RegionizedPlayerChunkLoader.getClientViewDistance(player)` —
/// `player.requestedViewDistance()`, `null` → -1 (the "no request" sentinel the
/// ladder resolves via auto-config). The M1 callers pass `None` (issue #101 is
/// not wired), but the capture client itself requests 8 — see
/// `add_and_send_chunks`.
pub fn get_client_view_distance(requested_view_distance: Option<i32>) -> i32 {
    requested_view_distance.map(|vd| vd.max(0)).unwrap_or(-1)
}

/// `PlayerChunkLoaderData.getTickDistance` — `min(playerTick < 0 ? worldTick :
/// playerTick, playerLoad < 0 ? worldLoad - 1 : playerLoad - 1)`.
pub fn get_tick_distance(
    player_tick_view_distance: i32,
    world_tick_view_distance: i32,
    player_load_view_distance: i32,
    world_load_view_distance: i32,
) -> i32 {
    (if player_tick_view_distance < 0 {
        world_tick_view_distance
    } else {
        player_tick_view_distance
    })
    .min(if player_load_view_distance < 0 {
        world_load_view_distance.wrapping_sub(1)
    } else {
        player_load_view_distance.wrapping_sub(1)
    })
}

/// `PlayerChunkLoaderData.getLoadViewDistance` — `max(tickViewDistance + 1,
/// playerLoad < 0 ? worldLoad : playerLoad)`.
pub fn get_load_view_distance(
    tick_view_distance: i32,
    player_load_view_distance: i32,
    world_load_view_distance: i32,
) -> i32 {
    tick_view_distance
        .wrapping_add(1)
        .max(if player_load_view_distance < 0 {
            world_load_view_distance
        } else {
            player_load_view_distance
        })
}

/// `PlayerChunkLoaderData.getSendViewDistance` — `min(load - 1, playerSend < 0
/// ? (!autoConfigSendDistance || client < 0 ? (worldSend < 0 ? load - 1 :
/// worldSend) : client + 1) : playerSend)`.
pub fn get_send_view_distance(
    load_view_distance: i32,
    client_view_distance: i32,
    player_send_view_distance: i32,
    world_send_view_distance: i32,
) -> i32 {
    let inner = if player_send_view_distance < 0 {
        if !AUTO_CONFIG_SEND_DISTANCE || client_view_distance < 0 {
            if world_send_view_distance < 0 {
                load_view_distance.wrapping_sub(1)
            } else {
                world_send_view_distance
            }
        } else {
            client_view_distance.wrapping_add(1)
        }
    } else {
        player_send_view_distance
    };
    load_view_distance.wrapping_sub(1).min(inner)
}

/// One tick-thread-produced play-state packet: the play clientbound packet id
/// plus the encoded body (packet id NOT included — the caller frames it). Plain
/// owned values so the tick thread can queue them without holding game state
/// (OWNERSHIP §Network).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayPacket {
    /// The play clientbound packet id (`varint` on the wire).
    pub id: u32,
    /// The encoded packet body.
    pub body: Vec<u8>,
}

impl PlayPacket {
    /// `new Packet(id, body)`.
    pub fn new(id: u32, body: Vec<u8>) -> Self {
        PlayPacket { id, body }
    }
}

/// `RegionizedPlayerChunkLoader.PlayerChunkLoaderData` — the per-player chunk
/// send state, reduced to the M1 direct-send path.
///
/// The M1 player stands still at the spawn chunk, so only `add()` (the initial
/// send-set) is reachable; the movement-driven `update()` re-center, the
/// `sentChunks` delta tracking, and the per-stage queues are deferred with
/// #185/#101.
pub struct PlayerChunkLoader {
    /// `this.lastChunkX` / `lastChunkZ` — the player's chunk position (the
    /// `ClientboundSetChunkCacheCenterPacket` center).
    last_chunk_x: i32,
    last_chunk_z: i32,
    /// `this.lastSendDistance` — the cache radius sent to the client.
    last_send_distance: i32,
    /// `this.lastTickDistance` — the simulation distance sent to the client.
    last_tick_distance: i32,
    /// Test seam (issue #100): forces a view-chunk encode failure in
    /// `add_and_send_chunks` (compiled out of non-test builds).
    #[cfg(test)]
    fail_chunk_encoding: bool,
}

impl PlayerChunkLoader {
    /// `new PlayerChunkLoaderData(world, player)` for a player at `center`.
    pub fn new(center: ChunkPos) -> Self {
        PlayerChunkLoader {
            last_chunk_x: center.x(),
            last_chunk_z: center.z(),
            last_send_distance: i32::MIN,
            last_tick_distance: i32::MIN,
            #[cfg(test)]
            fail_chunk_encoding: false,
        }
    }

    /// `PlayerChunkLoaderData.add()` + the send-queue drain, in one M1 step:
    /// emit the three cache packets (radius → simulation distance → center) then
    /// the 117 bare `ClientboundLevelChunkWithLightPacket`s in the deterministic
    /// coordinate raster the canonical fixture byte-matches. Returns the ordered
    /// send-set.
    ///
    /// Like Java's `add()`, `lastSendDistance`/`lastTickDistance` are committed
    /// right after the three cache packets and before the chunk drain — they
    /// record the distances sent in those packets, not the chunk set's delivery.
    ///
    /// `requested_view_distance` is `ServerPlayer.requestedViewDistance()`
    /// (issue #101); the M1 callers pass `None`. Note the capture client itself
    /// requests 8 (`client_information` view distance in `capture.jsonl`), but
    /// the ladder still resolves send = 4 (`min(load - 1, client + 1)`), so the
    /// M1 send-set is unchanged.
    pub fn add_and_send_chunks(
        &mut self,
        world: &ServerLevel,
        requested_view_distance: Option<i32>,
    ) -> Result<Vec<PlayPacket>, String> {
        // `add()`: derive the per-player distances from the world's Moonrise
        // distances (no per-player overrides — the M1 player holds the world
        // defaults).
        let tick = get_tick_distance(
            -1,
            world.get_simulation_distance(),
            -1,
            world.load_view_distance(),
        );
        let load = get_load_view_distance(tick, -1, world.load_view_distance());
        let client = get_client_view_distance(requested_view_distance);
        let send = get_send_view_distance(load, client, -1, world.send_view_distance());
        debug_assert!(
            send <= load && tick <= load,
            "send/tick view cannot exceed load view"
        );

        // `add()` packet order — `this.player.connection.send(...)` in Moonrise
        // source order: cache radius, then simulation distance, then cache
        // center (the center last so the client's cache is sized and located
        // before any chunk arrives).
        // 3 cache packets + the view square's bounding box (with the Moonrise
        // `includeNeighbors` margin the box is `(2·send+3)²`; for send 4 that is
        // 121 cells, 117 of which are contained).
        let mut packets = Vec::with_capacity(3 + ((2 * send + 3) * (2 * send + 3)) as usize);
        packets.push(PlayPacket::new(
            PlayClientbound::SetChunkCacheRadius.id(),
            encode_body(
                ClientboundSetChunkCacheRadiusPacket::stream_codec(),
                &ClientboundSetChunkCacheRadiusPacket::new(send),
            )?,
        ));
        packets.push(PlayPacket::new(
            PlayClientbound::SetSimulationDistance.id(),
            encode_body(
                ClientboundSetSimulationDistancePacket::stream_codec(),
                &ClientboundSetSimulationDistancePacket::new(tick),
            )?,
        ));
        packets.push(PlayPacket::new(
            PlayClientbound::SetChunkCacheCenter.id(),
            encode_body(
                ClientboundSetChunkCacheCenterPacket::stream_codec(),
                &ClientboundSetChunkCacheCenterPacket::new(self.last_chunk_x, self.last_chunk_z),
            )?,
        ));

        // `add()` state commit — Java `PlayerChunkLoaderData.add()` assigns
        // `lastSendDistance`/`lastTickDistance` right after sending the three
        // cache packets and before the chunk queue drain. The fields record the
        // cache-packet distances actually sent, not that the chunk set was
        // delivered; the M1 drain is synchronous below, so a chunk-encode
        // failure aborts with `Err` after this commit — the same ordering as
        // Java, where the async queue drain follows `add()`.
        self.last_send_distance = send;
        self.last_tick_distance = tick;

        // `update()` send-set + `updateQueues()` drain: `wantChunkSent` accepts
        // exactly `ChunkTrackingView.isWithinDistance(center, send, chunk, true)`
        // within the `squareDistance <= send + 1` bound — the `for_each` raster
        // of `ChunkTrackingView.of(center, send)`, which for the M1 center
        // (0,0) / send 4 is the 117-chunk square. Each chunk is sent as a bare
        // `ClientboundLevelChunkWithLightPacket` (no `ChunkBatchStart`/`Finished`).
        // A failed encode aborts the whole send-set: a dropped chunk is a
        // protocol error (Paper disconnects on send failure), never a silent
        // 116-chunk set.
        let view = ChunkTrackingView::of(ChunkPos::new(self.last_chunk_x, self.last_chunk_z), send);
        let mut bodies = Vec::with_capacity(view.chunk_count());
        view.for_each(|pos| bodies.push(encode_chunk_with_light(pos, world)));
        // Test seam (issue #100): the M1 superflat slice has no reachable
        // chunk-encode failure, so the ordering regression injects one here.
        #[cfg(test)]
        if self.fail_chunk_encoding {
            bodies.push(Err(
                "forced view-chunk encode failure (test seam)".to_string()
            ));
        }
        let bodies: Vec<Vec<u8>> = bodies.into_iter().collect::<Result<_, _>>()?;

        for body in bodies {
            packets.push(PlayPacket::new(
                PlayClientbound::LevelChunkWithLight.id(),
                body,
            ));
        }

        Ok(packets)
    }

    /// `PlayerChunkLoaderData.lastSendDistance` — the cache radius emitted by
    /// the last `add`.
    pub fn last_send_distance(&self) -> i32 {
        self.last_send_distance
    }

    /// `PlayerChunkLoaderData.lastTickDistance` — the simulation distance
    /// emitted by the last `add`.
    pub fn last_tick_distance(&self) -> i32 {
        self.last_tick_distance
    }

    /// Test seam (issue #100): force the view-chunk encode step of the next
    /// `add_and_send_chunks` call to fail. Compiles out of non-test builds.
    #[cfg(test)]
    fn set_fail_chunk_encoding(&mut self, fail: bool) {
        self.fail_chunk_encoding = fail;
    }
}

/// `PlayerChunkSender.sendChunk` — the bare `ClientboundLevelChunkWithLightPacket`
/// body for one view chunk. The M1 world loads exactly the spawn chunk (#156);
/// the flat-generator content is position-independent (the #194 fixture proves
/// all 117 bodies differ only in the 8-byte coordinate header), so any other
/// view position resolves the spawn chunk's content.
///
/// RivetTodo(#185): the chunk pipeline loads every view chunk; until then the
/// content is the deterministic superflat build for every position.
fn encode_chunk_with_light(pos: ChunkPos, world: &ServerLevel) -> Result<Vec<u8>, String> {
    let content = match world.chunk_map().get_chunk(pos) {
        Some(chunk) => chunk.content(),
        None => world
            .chunk_map()
            .get_chunk(world.view().center())
            .expect("spawn chunk loaded")
            .content(),
    };
    let packet = ClientboundLevelChunkWithLightPacket::new(
        pos.x(),
        pos.z(),
        content.chunk_packet_data(),
        content.light_data.clone(),
    );
    encode_chunk_body(&packet)
}

/// Encode a plain-`FriendlyByteBuf` packet body (the cache packets; packet id
/// NOT included).
fn encode_body<T>(
    codec: impl StreamEncoder<FriendlyByteBuf, T>,
    value: &T,
) -> Result<Vec<u8>, String> {
    let mut out = FriendlyByteBuf::new(BytesMut::new());
    codec
        .encode(&mut out, value)
        .map_err(|e| format!("encoding play packet: {}", e.message))?;
    Ok(out.into_inner().to_vec())
}

/// Encode a `ClientboundLevelChunkWithLightPacket` body over the
/// registry-aware buffer (the block-entity list resolves through
/// `BLOCK_ENTITY_TYPE`; the superflat chunk carries no block entities, so the
/// empty `RegistryAccess` is never consulted).
fn encode_chunk_body(packet: &ClientboundLevelChunkWithLightPacket) -> Result<Vec<u8>, String> {
    let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
    ClientboundLevelChunkWithLightPacket::stream_codec()
        .encode(&mut out, packet)
        .map_err(|e| format!("encoding level_chunk_with_light: {}", e.message))?;
    Ok(out.into_inner().to_vec())
}

/// Frame one play packet for the wire: `varint21(varint(declaredLen) ++
/// payload)` where `payload = varint(id) ++ body`. When `compression_threshold
/// >= 0` the payload is run through the protocol `CompressionEncoder` (zlib for
/// payloads at/over the threshold, `varint(0) ++ payload` below) — the exact
/// framing `Connection::send_packet` produces on the tokio side, so a frame
/// queued here writes byte-identically to a pre-play `send_packet` frame.
///
/// RivetTodo(#96): the per-connection threshold lives in the config for the M1
/// world (login applies `config.compression_threshold` to every connection);
/// the per-player refinement is deferred.
pub fn encode_play_frame(packet: &PlayPacket, compression_threshold: i32) -> Result<Bytes, String> {
    let mut payload = Vec::with_capacity(5 + packet.body.len());
    var_int::write(&mut payload, packet.id as i32);
    payload.extend_from_slice(&packet.body);
    let wire = if compression_threshold >= 0 {
        let mut encoder = CompressionEncoder::new(compression_threshold);
        encoder
            .encode(&payload)
            .map_err(|e| format!("compressing play packet: {}", e.message))?
    } else {
        BytesMut::from(&payload[..])
    };
    let frame = encode_frame(&wire).map_err(|e| e.message)?;
    Ok(Bytes::from(frame.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_protocol::compression_decoder::CompressionDecoder;
    use rivet_protocol::varint21_frame_decoder::Varint21FrameDecoder;

    /// The #194 capture fixture's first `level_chunk_with_light` body (coords
    /// -5/-4). All 117 bodies in the capture are byte-identical apart from the
    /// 8-byte BE coordinate header.
    const GOLDEN_FULL: &str =
        include_str!("../../../../rivet-protocol/tests/fixtures/chunk_golden_full.hex");

    fn hex(s: &str) -> Vec<u8> {
        let trimmed: String = s.trim().chars().filter(|c| !c.is_whitespace()).collect();
        (0..trimmed.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&trimmed[i..i + 2], 16).unwrap())
            .collect()
    }

    fn overworld() -> ServerLevel {
        ServerLevel::new(super::super::server_level::ServerLevelConfig::default())
    }

    /// The expected M1 send-set order: the `-5..5` × `-5..5` raster skipping
    /// the four corners (X-major, Z-minor) — the deterministic coordinate sort
    /// `rivet-capture` canonicalizes the fixture to (`normalize.rs`), not
    /// Paper's timing-dependent wire order.
    fn expected_raster() -> Vec<ChunkPos> {
        let mut out = Vec::new();
        for x in -5i32..=5 {
            for z in -5i32..=5 {
                if x.abs() == 5 && z.abs() == 5 {
                    continue;
                }
                out.push(ChunkPos::new(x, z));
            }
        }
        out
    }

    #[test]
    fn distance_ladder_matches_the_m1_fixture() {
        // view-distance=4, simulation-distance=4: world (tick=4, load=5, send=4).
        let world = overworld();
        let tick = get_tick_distance(
            -1,
            world.get_simulation_distance(),
            -1,
            world.load_view_distance(),
        );
        assert_eq!(tick, 4);
        let load = get_load_view_distance(tick, -1, world.load_view_distance());
        assert_eq!(load, 5);
        // No client request (null -> -1): auto-config resolves world send = 4.
        let send = get_send_view_distance(load, -1, -1, world.send_view_distance());
        assert_eq!(send, 4);
        // The capture client requests 8 (`client_information` view distance), but
        // the ladder caps it at load - 1 = 4 — the M1 send-set is unchanged.
        let send = get_send_view_distance(load, 8, -1, world.send_view_distance());
        assert_eq!(send, 4);
        // A client requesting its own view distance is capped by load - 1.
        let send = get_send_view_distance(load, 6, -1, world.send_view_distance());
        assert_eq!(send, 4);
    }

    #[test]
    fn m1_send_set_is_117_chunks_in_deterministic_raster_order() {
        let world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let packets = loader.add_and_send_chunks(&world, None).unwrap();
        assert_eq!(packets.len(), 3 + 117);
        assert_eq!(loader.last_send_distance(), 4);
        assert_eq!(loader.last_tick_distance(), 4);

        let chunk_packets: Vec<&PlayPacket> = packets[3..].iter().collect();
        let coords: Vec<ChunkPos> = chunk_packets
            .iter()
            .map(|p| {
                // The chunk body starts with the BE i32 x/z header.
                let x = i32::from_be_bytes([p.body[0], p.body[1], p.body[2], p.body[3]]);
                let z = i32::from_be_bytes([p.body[4], p.body[5], p.body[6], p.body[7]]);
                ChunkPos::new(x, z)
            })
            .collect();
        assert_eq!(
            coords,
            expected_raster(),
            "send order is the deterministic coordinate raster the fixture canonicalizes to"
        );
        // The shape: every (-5..5, -5..5) cell except the four corners.
        let set: std::collections::HashSet<ChunkPos> = coords.iter().copied().collect();
        assert_eq!(set.len(), 117, "no duplicate chunks");
        for x in -5i32..=5 {
            for z in -5i32..=5 {
                let corner = x.abs() == 5 && z.abs() == 5;
                assert_eq!(set.contains(&ChunkPos::new(x, z)), !corner, "({x},{z})");
            }
        }
    }

    #[test]
    fn cache_packets_precede_chunks_in_moonrise_order() {
        let world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let packets = loader.add_and_send_chunks(&world, None).unwrap();
        // radius, then simulation distance, then center — the Moonrise `add`
        // source order, before any chunk.
        assert_eq!(packets[0].id, PlayClientbound::SetChunkCacheRadius.id());
        assert_eq!(packets[0].body, vec![0x04]); // radius 4
        assert_eq!(packets[1].id, PlayClientbound::SetSimulationDistance.id());
        assert_eq!(packets[1].body, vec![0x04]); // simulation distance 4
        assert_eq!(packets[2].id, PlayClientbound::SetChunkCacheCenter.id());
        assert_eq!(packets[2].body, vec![0x00, 0x00]); // center (0, 0)
        for p in &packets[3..] {
            assert_eq!(p.id, PlayClientbound::LevelChunkWithLight.id());
        }
    }

    #[test]
    fn every_chunk_body_is_byte_identical_to_the_fixture_apart_from_coords() {
        let world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let packets = loader.add_and_send_chunks(&world, None).unwrap();
        let golden = hex(GOLDEN_FULL);
        // The first send chunk is (-5,-4) — the fixture's coordinates.
        assert_eq!(
            packets[3].body, golden,
            "first body matches the fixture exactly"
        );
        // Every other body matches apart from the 8-byte BE coordinate header.
        for p in &packets[3..] {
            assert_eq!(p.body[8..], golden[8..], "body matches the fixture");
            let x = i32::from_be_bytes([p.body[0], p.body[1], p.body[2], p.body[3]]);
            let z = i32::from_be_bytes([p.body[4], p.body[5], p.body[6], p.body[7]]);
            assert_eq!(
                &p.body[..8],
                &[x.to_be_bytes(), z.to_be_bytes()].concat(),
                "coordinate header is the BE x/z"
            );
        }
    }

    #[test]
    fn frames_round_trip_through_the_wire_framing() {
        let world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let packets = loader.add_and_send_chunks(&world, None).unwrap();
        // Frame every packet at the fixture's compression threshold 256.
        let frames: Vec<Bytes> = packets
            .iter()
            .map(|p| encode_play_frame(p, 256).unwrap())
            .collect();

        // Decode each frame back: VarInt21 framing + decompression yields the
        // packet id and the original body.
        let mut decoder = CompressionDecoder::new(256, true);
        for (packet, frame) in packets.iter().zip(&frames) {
            let mut buf = bytes::BytesMut::from(&frame[..]);
            let raw = Varint21FrameDecoder::new(None)
                .decode(&mut buf)
                .unwrap()
                .expect("full frame");
            let mut decoded = decoder.decode(&raw).unwrap();
            // `decoded` = varint(id) ++ body.
            let id = rivet_protocol::var_int::read(&mut decoded);
            assert_eq!(id as u32, packet.id);
            assert_eq!(&decoded[..], packet.body.as_slice());
        }
    }

    #[test]
    fn uncompressed_frames_are_the_plain_varint21_wire_form() {
        let world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let packets = loader.add_and_send_chunks(&world, None).unwrap();
        let frame = encode_play_frame(&packets[0], -1).unwrap();
        // payload = varint(95) ++ [0x04]; length 2 -> varint21 header 0x02.
        assert_eq!(frame.to_vec(), vec![0x02, 0x5F, 0x04]);
    }

    #[test]
    fn chunk_encode_error_returns_err_after_the_java_state_commit() {
        // Regression (issue #100 review): Java `PlayerChunkLoaderData.add()`
        // commits `lastSendDistance`/`lastTickDistance` right after the three
        // cache packets and BEFORE the async chunk queue drain. The M1 drain is
        // folded synchronously into this call, so a chunk-encode failure returns
        // `Err` only after the distances are committed — the Java ordering, not a
        // transactional all-or-nothing commit. The `Err` carries no send-set: the
        // cache packets built so far are dropped, never returned as a partial set.
        let world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        loader.set_fail_chunk_encoding(true);
        let err = loader.add_and_send_chunks(&world, None).unwrap_err();
        assert!(
            err.contains("chunk"),
            "error names the chunk encode step: {err}"
        );
        // Distances committed before the drain — the exact Java ordering.
        assert_eq!(loader.last_send_distance(), 4);
        assert_eq!(loader.last_tick_distance(), 4);

        // Clearing the seam restores the full 117-chunk send-set.
        loader.set_fail_chunk_encoding(false);
        let packets = loader.add_and_send_chunks(&world, None).unwrap();
        assert_eq!(packets.len(), 3 + 117);
        assert_eq!(loader.last_send_distance(), 4);
        assert_eq!(loader.last_tick_distance(), 4);
    }
}
