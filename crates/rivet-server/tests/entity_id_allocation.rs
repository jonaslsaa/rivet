//! `ServerLevel.getNextEntityId` / `ENTITY_COUNTER` (GitHub #222) — the
//! integration tests proving Paper-faithful entity-id allocation end to end.
//!
//! Java source of truth: `working/Paper/paper-server/src/minecraft/java/net/
//! minecraft/server/level/ServerLevel.java`:
//!
//! ```java
//! private static final AtomicInteger ENTITY_COUNTER = new AtomicInteger();
//! @Override public int getNextEntityId() {
//!     int id = 0;
//!     while (id == 0 || this.chunkSource.hasEntityWithId(id)) {
//!         id = ENTITY_COUNTER.incrementAndGet();
//!     }
//!     return id;
//! }
//! ```
//!
//! `ENTITY_COUNTER` is `static` — one counter shared by every `ServerLevel` in
//! the JVM — so Rivet keeps the allocator at the tick-thread server play scope
//! (the session manager), NOT on the level: a per-level allocator restarting at
//! 1 would hand different levels the same ids. Two live sessions share one
//! server-scope allocator, so a second join must allocate a distinct non-zero
//! id — the exact collision the old `player_id = 1` hardcode produced.
//! Disconnecting and rejoining must NOT reuse the freed id (the counter never
//! decrements); a still-live session's id stays in use (`hasEntityWithId`).
//! Allocating before a failed join burst consumes the id without registering
//! it, exactly as Paper never rolls the counter back. The skip-zero /
//! skip-in-use / wrap loop itself is unit-tested in `entity_id_allocator.rs`;
//! this file proves the server-scope semantics through the real session manager
//! + join-burst path.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use rivet_protocol::protocol::common::client_information::ClientInformation;
use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_registry::core::{GameProfile, create_offline_player_uuid};
use rivet_registry::registries;
use rivet_server::server::level::entity_id_allocator::EntityIdAllocator;
use rivet_server::server::level::server_level::{ServerLevel, ServerLevelConfig};
use rivet_server::server::network::connection_id::ConnectionId;
use rivet_server::server::player::session::{PlayerSessionManager, default_session_config};
use rivet_server::server::tick::TickContext;
use rivet_server::server::tick::channels::{InboundDrained, LifecycleEvent, OutboundEvent};
use rivet_server::server::tick::registry::ConnectionRegistry;

const REMOTE: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);

/// The two connection ids the sessions join over.
const FIRST: ConnectionId = ConnectionId(1);
const SECOND: ConnectionId = ConnectionId(2);

/// The offline probe profile for one connection. The profile name (and thus
/// the offline uuid) is derived from the `ConnectionId` so the two sessions are
/// *distinct players* — the `PlayerIndices` UUID↔connection maps are a
/// bijection, so two sessions sharing one uuid would collapse to a single index
/// entry and `session_count()` would under-report. A rejoin of `FIRST` reuses
/// its own name, so it is the same player coming back.
fn profile(id: ConnectionId) -> GameProfile {
    let name = format!("RivetProbe{}", id.0);
    GameProfile::new_without_properties(create_offline_player_uuid(&name), name)
}

/// A fresh registry with `id` connected over a bounded outbound channel.
/// Returns the connection's outbound receiver — the test must keep it alive (a
/// dropped receiver makes every queued send fail and the registry prune the
/// connection), even though the join burst is never drained and the assertions
/// are on session state, not on the outbound frames.
fn connect(
    registry: &mut ConnectionRegistry,
    id: ConnectionId,
) -> tokio::sync::mpsc::Receiver<OutboundEvent> {
    connect_with_outbound_capacity(registry, id, 256)
}

/// Like [`connect`] but with a caller-chosen outbound channel capacity. A
/// capacity too small for the join burst makes the burst overflow and the
/// connection is pruned — how the failed-join test forces a burst failure.
fn connect_with_outbound_capacity(
    registry: &mut ConnectionRegistry,
    id: ConnectionId,
    capacity: usize,
) -> tokio::sync::mpsc::Receiver<OutboundEvent> {
    let (in_tx, in_rx) = tokio::sync::mpsc::channel(4);
    let (out_tx, out_rx) = tokio::sync::mpsc::channel(capacity);
    registry.apply(LifecycleEvent::Connect {
        id,
        remote: REMOTE,
        in_rx,
        out_tx,
        drained: InboundDrained::new(),
    });
    let _ = in_tx;
    out_rx
}

/// Apply the configuration→play handoff for `id` (the network side's
/// `LifecycleEvent::EnterPlay`).
fn enter_play(registry: &mut ConnectionRegistry, id: ConnectionId) {
    registry.apply(LifecycleEvent::EnterPlay {
        id,
        profile: profile(id),
        client_information: ClientInformation::create_default(),
    });
}

/// Run one tick of the session manager at `now_ms = 0` (the epoch reading new
/// sessions are seeded with).
fn run_tick(
    mut manager: PlayerSessionManager,
    registry: &mut ConnectionRegistry,
) -> PlayerSessionManager {
    let mut ctx = TickContext {
        tick: 1,
        now_ns: 0,
        now_ms: 0,
        connections: registry,
        inbound: Vec::new(),
    };
    manager.tick(&mut ctx);
    manager
}

/// A manager with `ids` connected + handed off, one tick run (every handoff
/// spawns a session + fires its join burst). Returns the manager, the registry,
/// and the outbound receivers held alive for the session's lifetime (see
/// [`connect`]).
fn spawned_manager(
    ids: &[ConnectionId],
) -> (
    PlayerSessionManager,
    ConnectionRegistry,
    Vec<tokio::sync::mpsc::Receiver<OutboundEvent>>,
) {
    let mut registry = ConnectionRegistry::new();
    let mut receivers = Vec::new();
    for id in ids {
        receivers.push(connect(&mut registry, *id));
        enter_play(&mut registry, *id);
    }
    let manager = run_tick(
        PlayerSessionManager::new(default_session_config(256, 42)),
        &mut registry,
    );
    (manager, registry, receivers)
}

#[test]
fn two_sessions_get_distinct_nonzero_entity_ids() {
    let (manager, _registry, _receivers) = spawned_manager(&[FIRST, SECOND]);
    assert_eq!(manager.session_count(), 2, "both sessions live");

    let first = manager
        .player_entity_id(FIRST)
        .expect("first session has an id");
    let second = manager
        .player_entity_id(SECOND)
        .expect("second session has an id");
    assert_ne!(first, 0, "entity id 0 is never allocated (skip-zero)");
    assert_ne!(second, 0, "entity id 0 is never allocated (skip-zero)");
    assert_ne!(
        first, second,
        "concurrent sessions get distinct ids — the old player_id=1 hardcode would collide"
    );
}

#[test]
fn rejoin_after_disconnect_gets_a_fresh_id_and_frees_the_old_one() {
    let (mut manager, mut registry, _receivers) = spawned_manager(&[FIRST, SECOND]);

    let first_before = manager.player_entity_id(FIRST).unwrap();
    let second = manager.player_entity_id(SECOND).unwrap();
    assert_ne!(first_before, second);

    // `FIRST` leaves; the next tick's `prune_lost` releases its id (the
    // world's `hasEntityWithId` no longer reports it) and drops the session.
    registry.apply(LifecycleEvent::Disconnect {
        id: FIRST,
        reason: rivet_server::server::network::packet_listener::DisconnectReason::EndOfStream,
    });
    manager = run_tick(manager, &mut registry);
    assert_eq!(
        manager.session_count(),
        1,
        "the disconnected session is pruned"
    );
    assert_eq!(manager.player_entity_id(FIRST), None, "session gone");

    // `FIRST` rejoins: the server-scope counter never decremented, so it gets a
    // FRESH id — not the freed one, and not the still-live `SECOND`'s id.
    // The new receiver is held for the new connection's lifetime (a dropped
    // receiver prunes the connection).
    let _rejoined_out = connect(&mut registry, FIRST);
    enter_play(&mut registry, FIRST);
    manager = run_tick(manager, &mut registry);
    assert_eq!(manager.session_count(), 2);

    let first_after = manager.player_entity_id(FIRST).expect("rejoined session");
    assert_ne!(first_after, 0);
    assert_ne!(
        first_after, first_before,
        "a freed id is never reused (the counter does not decrement)"
    );
    assert_ne!(
        first_after, second,
        "a still-in-use id is never handed out (hasEntityWithId skip)"
    );
}

#[test]
fn first_session_on_a_fresh_server_gets_id_one() {
    // `ENTITY_COUNTER` starts at 0 and `incrementAndGet` runs before the check,
    // so the very first allocation on a fresh server scope is 1 — the capture's
    // `entityId -> 1` normalization.
    let (manager, _registry, _receivers) = spawned_manager(&[FIRST]);
    assert_eq!(
        manager.player_entity_id(FIRST),
        Some(1),
        "the M1 capture's first player entity id"
    );
}

/// The server-scope counterfactual: `ENTITY_COUNTER` is `static` — one counter
/// per server, shared across every `ServerLevel` — so two *distinct level
/// instances* must never collide. This test drives two real `ServerLevel`
/// instances through one server-scope allocator and proves each level's first
/// id is a distinct non-zero value.
///
/// Scope honesty: this slice leaves `ServerLevel` with no entity-id allocation
/// surface (the allocator lives at the server play scope — the session
/// manager), so this test exercises the standalone allocator directly and
/// cannot by itself distinguish server-scope from per-level allocation. The
/// per-level counterfactual (a per-level allocator restarting at 1 would hand
/// the overworld's and nether's first entities the same id) is a design
/// rationale documented in `entity_id_allocator.rs`'s module docs — argued
/// here, not pinned by this test.
#[test]
fn two_distinct_levels_share_one_server_counter() {
    let overworld = ServerLevel::new(ServerLevelConfig::default());
    let nether = ServerLevel::new(ServerLevelConfig {
        dimension: ResourceKey::create(
            &*registries::DIMENSION,
            Identifier::with_default_namespace("the_nether"),
        ),
        ..ServerLevelConfig::default()
    });
    assert_ne!(
        overworld.dimension(),
        nether.dimension(),
        "two distinct level instances"
    );

    // One server-scope allocator serves both levels (the session manager owns
    // it, mirroring Paper's static counter).
    let mut server = EntityIdAllocator::new();
    let overworld_first = server.next_id();
    server.mark_in_use(overworld_first);
    let nether_first = server.next_id();
    server.mark_in_use(nether_first);
    assert_ne!(
        overworld_first, nether_first,
        "a shared server-scope counter never hands two levels the same id"
    );
    assert_ne!(overworld_first, 0);
    assert_ne!(nether_first, 0);
}

/// The faithful failure behavior: `spawn_session` allocates the entity id
/// BEFORE firing the join burst (`Entity`'s constructor runs `getNextEntityId()`
/// first), so a burst that fails consumes the id without registering it — the
/// counter never rolls back. This test forces a burst failure (an outbound
/// channel too small for the ~135-frame burst overflows, pruning the
/// connection) and proves the consumed id is never handed out and never marked
/// in use.
#[test]
fn failed_join_burst_consumes_the_id_without_registering_it() {
    let mut registry = ConnectionRegistry::new();
    // Capacity 1: the first burst frame fits, the second overflows and prunes
    // the connection, so `place_new_player` returns `Err` and `spawn_session`
    // rolls back before registering the session.
    let _stuck_out = connect_with_outbound_capacity(&mut registry, FIRST, 1);
    enter_play(&mut registry, FIRST);
    let manager = run_tick(
        PlayerSessionManager::new(default_session_config(256, 42)),
        &mut registry,
    );
    assert_eq!(
        manager.session_count(),
        0,
        "the failed burst rolls back the session"
    );
    assert_eq!(
        manager.player_entity_id(FIRST),
        None,
        "no session registered"
    );

    // The consumed id is gone: the next successful join gets a FRESH id, not
    // the id the failed join consumed.
    let _second_out = connect(&mut registry, SECOND);
    enter_play(&mut registry, SECOND);
    let manager = run_tick(manager, &mut registry);
    assert_eq!(manager.session_count(), 1);
    assert_eq!(
        manager.player_entity_id(SECOND),
        Some(2),
        "the failed join consumed id 1 without registering it — the next id is 2"
    );
}
