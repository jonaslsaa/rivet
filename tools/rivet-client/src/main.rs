use std::collections::BTreeSet;
use std::env;
use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use azalea::WalkDirection;
use azalea::app::{App, Plugin, Update};
use azalea::attack::handle_attack_queued;
use azalea::core::game_type::GameMode;
use azalea::core::position::BlockPos;
use azalea::core::tick::GameTick;
use azalea::ecs::message::MessageReader;
use azalea::ecs::observer::On;
use azalea::ecs::prelude::{Query, Res, Resource};
use azalea::ecs::schedule::IntoScheduleConfigs;
use azalea::entity::{EntityGeometryUpdateSystems, LastSentPosition, Physics};
use azalea::join::{ConnectionFailedEvent, poll_create_connection_task};
use azalea::local_player::WorldHolder;
use azalea::movement::{
    send_player_input_packet, send_position, send_sprinting_if_needed, update_pose,
};
use azalea::packet::game::SendGamePacketEvent;
use azalea::physics::PhysicsSystems;
use azalea::physics::client_movement::ClientMovementState;
use azalea::prelude::*;
use azalea::protocol::common::movements::MoveFlags;
use azalea::protocol::packets::game::{ClientboundGamePacket, ServerboundGamePacket};
use serde_json::{Value, json};

const DEFAULT_MODE: &str = "join";
const DEFAULT_ADDRESS: &str = "127.0.0.1:25599";
const DEFAULT_USERNAME: &str = "RivetProbe";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
/// Default dwell window for `--mode dwell`: the wall-clock seconds the client
/// stays connected after spawn while answering every live keepalive. Zero means
/// the mode is inactive (dwell mode with zero is rejected at parse time).
const DEFAULT_DWELL_SECONDS: u64 = 0;
const AZALEA_REVISION: &str = "6249c295d353b9b3ef68f665b311cba39211fd19";
const TRANSCRIPT_PROTOCOL: u64 = 1;

/// Move-mode walk length. 120 game ticks = 6s of walking: long enough to cross
/// the 1s Paper keepalive cadence (proving keepalive echo while moving) and to
/// show a few send_position deltas, short enough that the walk stays inside the
/// spawn chunk (spawn is ~10 blocks from the chunk corner; walking +x stays
/// well within the loaded view).
const MOVE_TICKS: u32 = 120;
/// Game ticks of the walk that are sampled into the transcript. Kept below
/// `MOVE_TICKS` so the walk always continues a few unsampled ticks after the
/// last sample: if the server re-syncs the player (`player_position`) at some
/// arbitrary tick, the perturbation lands in the unsampled tail and cannot
/// corrupt the deterministic sample sequence (a re-sync mid-sample was observed
/// to knock the local player airborne for the final 3 samples on one boot).
const SAMPLE_TICKS: u32 = 100;
/// Wait this long after the walk stops before ending the client, so the final
/// sent positions and any trailing correction are recorded before exit.
const MOVE_DRAIN: Duration = Duration::from_millis(200);
/// How long the keepalive settle loop (dwell and move modes) waits between
/// coherence checks before snapshotting. The keepalive cadence is 1 s, so a
/// 50 ms check interval is ample: it lets an in-flight challenge/echo pair land
/// and be observed well before the next challenge.
const KEEPALIVE_SETTLE_INTERVAL: Duration = Duration::from_millis(50);
/// How long the keepalive settle loop waits for the challenge and echo streams
/// to reach 1:1 correspondence before giving up and snapshotting anyway. Bounded
/// so a genuinely missing echo (a client that stopped echoing, which the server
/// would kick) cannot hold the record open forever — the emitted mismatch still
/// fails the verdict. 1 s is far shorter than the move walk or the dwell
/// survival-window proof, and keeps the record honest.
const KEEPALIVE_SETTLE_TIMEOUT: Duration = Duration::from_secs(1);
/// The minimum wall-clock dwell window (s) `--mode dwell` accepts. The
/// transcript verdict requires the challenge span to reach 30 s, and the first
/// challenge lands ~1.2 s after spawn, so a 31 s window (the server's 30 s kick
/// limit plus 1) would span only ~29.8 s and fail on a healthy run. 35 s leaves
/// a comfortable margin. Mirrors `run-scenario`'s
/// `transcript::DWELL_MIN_DWELL_SECONDS`; kept in sync so a direct client
/// invocation cannot be told to run a window that cannot prove survival.
const DWELL_MIN_DWELL_SECONDS: u64 = 35;
/// Reserved client-side headroom (s) beyond the dwell window + settle timeout
/// that `--timeout-seconds` must accommodate. The timeout starts at process
/// launch while the dwell window only starts at `Event::Spawn` (after offline
/// login and configuration), so the timeout must reserve that pre-spawn time
/// too, or the timeout branch cuts the client off before it emits the `dwell`
/// record. Mirrors `run-scenario`'s `DWELL_TIMEOUT_HEADROOM_SECONDS` (5 s here
/// + the 1 s settle above = the 6 s the runner reserves).
const DWELL_LOGIN_HEADROOM_SECONDS: u64 = 5;

/// After `Event::Spawn` we keep the client alive for a short observation window
/// so the observable outcome is stable (chunks arrived, health/inventory
/// populated, position settled) before emitting the canonical `joined` record.
const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(15);
const MIN_OBSERVATION: Duration = Duration::from_secs(3);
const QUIET_PERIOD: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Join and observe the settled world state (the `join` scenario).
    Join,
    /// Join, then walk forward on a fixed heading for a bounded tick window,
    /// sampling position/velocity/on-ground each tick and recording the
    /// teleport/keepalive/correction packets observed along the way (the `move`
    /// scenario).
    Move,
    /// Join and stay connected for `--dwell-seconds` wall-clock seconds after
    /// spawn while azalea auto-echoes every live keepalive, then emit the
    /// `dwell` record (challenge ids, echo ids, wall-clock span). Proves a real
    /// client survives in PLAY past the server's keepalive kick limit (30s).
    Dwell,
    /// Join, then send a movement frame whose position is NaN so the server's
    /// `contains_invalid_values` anti-cheat gate answers with a
    /// `ClientboundDisconnectPacket` carrying
    /// `multiplayer.disconnect.invalid_player_movement`. The decoded reason
    /// (the translatable key) is emitted by the `disconnect` handler, proving
    /// the real client decodes Rivet's reason (the `kick` scenario, issue #86).
    Kick,
    /// Join a server that boots a real loaded world (`--level <copy>`, issue
    /// #374), wait for chunk quiescence, then sample the genuine per-coordinate
    /// block content the server served (surface/bedrock/under-feet names from
    /// the client's own loaded `ChunkStorage`) and emit the `loaded` record.
    /// The runner compares this against the read-only ground-truth manifest
    /// (`rivet-oracle extract-world`); a server that merely echoes repeated
    /// superflat bytes cannot match it. The runner classifies honestly
    /// (UNVERIFIED for non-FULL chunks and uncarried #369 capability flags)
    /// rather than reporting a fabricated PASS.
    Loaded,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Join => "join",
            Mode::Move => "move",
            Mode::Dwell => "dwell",
            Mode::Kick => "kick",
            Mode::Loaded => "loaded",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "join" => Some(Mode::Join),
            "move" => Some(Mode::Move),
            "dwell" => Some(Mode::Dwell),
            "kick" => Some(Mode::Kick),
            "loaded" => Some(Mode::Loaded),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct Args {
    address: String,
    username: String,
    timeout: Duration,
    mode: Mode,
    dwell: Duration,
}

impl Args {
    fn parse() -> Result<Self, String> {
        Self::parse_from(env::args().skip(1))
    }

    fn parse_from(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut address = DEFAULT_ADDRESS.to_owned();
        let mut username = DEFAULT_USERNAME.to_owned();
        let mut timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
        let mut dwell_seconds = DEFAULT_DWELL_SECONDS;
        let mut dwell_explicit = false;
        let mut mode = Mode::Join;

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--address" => address = next_value(&mut args, "--address")?,
                "--username" => username = next_value(&mut args, "--username")?,
                "--timeout-seconds" => {
                    let value = next_value(&mut args, "--timeout-seconds")?;
                    timeout_seconds = value
                        .parse()
                        .map_err(|_| format!("invalid --timeout-seconds value: {value}"))?;
                }
                "--mode" => {
                    let value = next_value(&mut args, "--mode")?;
                    mode = Mode::parse(&value).ok_or_else(|| {
                        format!(
                            "invalid --mode value: {value} (expected join|move|dwell|kick|loaded)"
                        )
                    })?;
                }
                "--dwell-seconds" => {
                    let value = next_value(&mut args, "--dwell-seconds")?;
                    dwell_seconds = value
                        .parse()
                        .map_err(|_| format!("invalid --dwell-seconds value: {value}"))?;
                    dwell_explicit = true;
                }
                "--help" | "-h" => return Err(usage()),
                _ => return Err(format!("unknown argument: {argument}\n\n{}", usage())),
            }
        }

        // --dwell-seconds only has meaning in dwell mode; on join/move/kick an
        // explicit value would be a silent no-op, so it is rejected (exit 64)
        // rather than ignored.
        if dwell_explicit && mode != Mode::Dwell {
            return Err(
                "--dwell-seconds only applies to --mode dwell; join/move/kick/loaded never dwell, \
                 so an explicit value would be a silent no-op — drop it"
                    .to_owned(),
            );
        }
        if username.is_empty() {
            return Err("--username must not be empty".to_owned());
        }
        if timeout_seconds == 0 {
            return Err("--timeout-seconds must be greater than zero".to_owned());
        }
        if mode == Mode::Dwell && dwell_seconds < DWELL_MIN_DWELL_SECONDS {
            return Err(format!(
                "--mode dwell requires --dwell-seconds of at least {DWELL_MIN_DWELL_SECONDS} (the \
                 server's 30 s keepalive kick limit plus the first-challenge offset and margin; a \
                 shorter window cannot prove survival or span the required 30 s of challenges)"
            ));
        }
        // The timeout starts at process launch while the dwell window only
        // starts at spawn; after the window the client spends up to
        // KEEPALIVE_SETTLE_TIMEOUT settling the keepalive stream before
        // emitting the `dwell` record. `dwell < timeout` is therefore not
        // enough — the timeout must reserve the settle loop AND the pre-spawn
        // login/configuration time, or the timeout branch cuts the client off
        // before it emits.
        if mode == Mode::Dwell
            && timeout_seconds
                <= dwell_seconds + KEEPALIVE_SETTLE_TIMEOUT.as_secs() + DWELL_LOGIN_HEADROOM_SECONDS
        {
            return Err(format!(
                "--timeout-seconds must exceed --dwell-seconds by more than {}s (the client \
                 spends up to {}s settling the keepalive stream after the dwell window, plus {}s \
                 of login/configuration time before spawn, and must emit the dwell record before \
                 the timeout fires)",
                KEEPALIVE_SETTLE_TIMEOUT.as_secs() + DWELL_LOGIN_HEADROOM_SECONDS,
                KEEPALIVE_SETTLE_TIMEOUT.as_secs(),
                DWELL_LOGIN_HEADROOM_SECONDS
            ));
        }

        Ok(Self {
            address,
            username,
            timeout: Duration::from_secs(timeout_seconds),
            mode,
            dwell: Duration::from_secs(dwell_seconds),
        })
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {option}"))
}

fn usage() -> String {
    format!(
        "Usage: rivet-client [--mode join|move|dwell|kick|loaded] [--address HOST:PORT] [--username NAME] \
         [--timeout-seconds N] [--dwell-seconds N]\n\
         Defaults: --mode {DEFAULT_MODE} --address {DEFAULT_ADDRESS} --username {DEFAULT_USERNAME} \
         --timeout-seconds {DEFAULT_TIMEOUT_SECONDS} --dwell-seconds {DEFAULT_DWELL_SECONDS}"
    )
}

/// One per-tick movement sample collected by the move mode: the client's local
/// physics state immediately after a game tick. `velocity` is azalea's post-tick
/// velocity (which the server sees as the movement delta), and `on_ground` is
/// the ground-contact flag. Both are server-independent client-side values, so
/// they are deterministic given a deterministic server and a fixed walk.
#[derive(Clone)]
struct MoveSample {
    position: azalea::Vec3,
    velocity: azalea::Vec3,
    on_ground: bool,
}

/// The walk-length-window sampler for `--mode move`.
///
/// Sampling happens in a [`GameTick`] system, not in the async driver, because
/// the async `get_tick_broadcaster()` returns a capacity-1 broadcast that can
/// drop ticks under scheduling pressure: the driver would then count fewer
/// "ticks" than the ECS actually ran, misaligning samples to real game ticks
/// (observed: one boot captured two ticks of movement in a single sample). A
/// synchronous system runs once per real game tick on the tick thread, so it
/// cannot drop a tick, and it reads the post-physics ECS state directly with no
/// async lock interplay.
///
/// The sampler owns the walk itself: on the first tick it observes it snaps
/// `ClientMovementState.move_direction` to `WalkDirection::Forward`, so the walk
/// start is exactly aligned to a real tick boundary (there is no async
/// start-walk race). That first observed tick records no sample — its physics
/// already ran without a direction, so the player did not move and there is no
/// pre-movement state to capture. The subsequent `SAMPLE_TICKS` ticks are the
/// walk's moving ticks and each is sampled post-physics; on the `MOVE_TICKS`th
/// observed tick the sampler snaps the player back to `WalkDirection::None` and
/// notifies the async driver that the window is complete. Each run of the system
/// is exactly one game tick, so the internal `tick_count` is a true tick count.
#[derive(Component)]
struct MoveSampler {
    /// Number of game ticks this sampler has observed, in `[0, MOVE_TICKS]`.
    /// Tick 0 starts the walk and records no sample; ticks `1..=SAMPLE_TICKS`
    /// each record the post-movement state; the walk stops once the count
    /// reaches `MOVE_TICKS`.
    tick_count: u32,
    /// Signals the async driver once the walk window has fully elapsed.
    notify: Arc<tokio::sync::Notify>,
    /// The samples captured so far, shared with the driver for the final record.
    samples: Arc<Mutex<Vec<MoveSample>>>,
    /// The last movement frame actually sent across the whole walk (the
    /// `LastSentPosition` at the tick the walk stopped), shared with the driver
    /// for the final record's `walk.last_sent`.
    last_sent: Arc<Mutex<Option<azalea::Vec3>>>,
}

/// A [`GameTick`] system that drives the move-walk sampler synchronously.
///
/// Ordered after [`PhysicsSystems`] and the movement/send chain by
/// [`MoveSamplerPlugin`], so the observed state is the tick's final post-physics
/// state (the physics for the current tick already ran with the previous
/// `move_direction`). On its first observed tick it sets the walk direction,
/// which the *next* tick's physics then applies. It samples `SAMPLE_TICKS`
/// post-movement ticks — the walk's 1st..=100th moving tick — and on the
/// `MOVE_TICKS`th observed tick stops the walk and wakes the driver. Exactly
/// `SAMPLE_TICKS` samples are recorded, matching the old async sampler's
/// transcript contract (sample 0 is the first tick the walk moved).
fn move_sampler_system(
    mut players: Query<(
        &azalea::entity::Position,
        &Physics,
        &mut ClientMovementState,
        &LastSentPosition,
        &mut MoveSampler,
    )>,
) {
    for (position, physics, mut movement, last_sent, mut sampler) in &mut players {
        // Once the walk window has fully elapsed the sampler is inert: it no
        // longer samples, re-stops the walk, or re-notifies the driver.
        if sampler.tick_count >= MOVE_TICKS {
            continue;
        }
        if sampler.tick_count == 0 {
            // First tick observed: start the walk. Do not sample this tick —
            // its physics ran before the direction was set, so it did not move.
            movement.move_direction = WalkDirection::Forward;
            sampler.tick_count += 1;
            continue;
        }
        // A tick whose physics ran while walking: sample it.
        if sampler.tick_count <= SAMPLE_TICKS {
            sampler
                .samples
                .lock()
                .expect("move samples lock poisoned")
                .push(MoveSample {
                    position: **position,
                    velocity: physics.velocity,
                    on_ground: physics.on_ground(),
                });
        }
        sampler.tick_count += 1;
        if sampler.tick_count >= MOVE_TICKS {
            movement.move_direction = WalkDirection::None;
            // This run happens after `send_position`, so `LastSentPosition` is
            // the position the walk's final movement frame was actually sent
            // with (the frame sent this tick — the walk still moved, direction
            // is only cleared here). The sampled prefix stops at SAMPLE_TICKS,
            // so the remaining `MOVE_TICKS - 1 - SAMPLE_TICKS` moving ticks are
            // unsampled; this snapshot is the last of them.
            *sampler.last_sent.lock().expect("last sent lock poisoned") = Some(**last_sent);
            sampler.notify.notify_one();
        }
    }
}

/// Registers the move-walk sampler for `--mode move`.
struct MoveSamplerPlugin;

impl Plugin for MoveSamplerPlugin {
    fn build(&self, app: &mut App) {
        // The sampler must observe the local player's final state for the tick.
        // Order it after every azalea GameTick system that reads or writes the
        // sampled data (`Position`, `Physics`, `ClientMovementState`): the
        // physics sets (which move the player), the movement/send chain (input,
        // pose, sprinting, position send), the geometry update that rewrites the
        // bounding box, and the attack system that mutates `Physics`. The
        // explicit edges keep the sampled timing deterministic even if azalea's
        // internal chain order changes, and suppress the schedule-ambiguity
        // warnings azalea's AmbiguityLoggerPlugin otherwise reports for this
        // system.
        app.add_systems(
            GameTick,
            move_sampler_system
                .after(PhysicsSystems)
                .after(EntityGeometryUpdateSystems)
                .after(send_position)
                .after(send_sprinting_if_needed)
                .after(update_pose)
                .after(send_player_input_packet)
                .after(handle_attack_queued),
        );
    }
}

#[derive(Clone, Component)]
struct State {
    mode: Mode,
    /// Dwell-mode wall-clock window (real time, monotonic). Zero for non-dwell
    /// modes; validated at parse time to be non-zero and strictly less than the
    /// outer `--timeout-seconds` bound for dwell.
    dwell: Duration,
    /// Monotonic `Instant` at `Event::Spawn`, the start of the dwell window.
    spawn_instant: Arc<Mutex<Option<Instant>>>,
    spawned: Arc<AtomicBool>,
    terminal_emitted: Arc<AtomicBool>,
    /// Chunk coordinates received so far (sorted at read time). Shared between
    /// the event handler (writer) and the observation task (reader).
    chunks: Arc<Mutex<BTreeSet<(i32, i32)>>>,
    /// Per-tick movement samples, appended by the move task and read once when
    /// it emits the canonical `moved` record.
    move_samples: Arc<Mutex<Vec<MoveSample>>>,
    /// The last movement frame actually sent across the walk (the sampler's
    /// `walk.last_sent` snapshot), read once when the `moved` record is emitted.
    last_sent: Arc<Mutex<Option<azalea::Vec3>>>,
    /// Clientbound teleport ids (`player_position`), observed by the event
    /// handler. On a fresh boot the spawn teleport is the first (id 1 — Paper's
    /// `awaitingTeleport` is per-connection and starts at 0), so the ids are
    /// deterministic and compared; the echo relationship is additionally
    /// checked structurally.
    teleports: Arc<Mutex<Vec<u32>>>,
    /// Serverbound `accept_teleportation` ids, observed by the outbound packet
    /// observer (azalea auto-acks every teleport).
    teleport_acks: Arc<Mutex<Vec<u32>>>,
    /// Keepalive challenges, their receipt instants, and their echoes, shared
    /// by move and dwell modes under a single lock so a snapshot never splits a
    /// challenge from its echo (see [`KeepaliveLog`]).
    keepalive_log: Arc<Mutex<KeepaliveLog>>,
    /// Server-issued position corrections (`entity_position_sync`) observed
    /// during the walk: Paper re-syncs the player entity periodically, so both
    /// the count and the coordinates vary per boot (46-118 across test boots).
    /// Azalea is client-authoritative for the local player, so these never move
    /// the client and the sampled walk is unaffected; they are recorded as a
    /// diagnostic and excluded from parity (see `excluded_move_fields`).
    corrections: Arc<Mutex<Vec<azalea::Vec3>>>,
    /// The player's position at `Event::Spawn` (the server's randomized spawn
    /// point). Move samples are normalized to deltas from this origin at full
    /// precision, so the transcript is invariant to the per-boot spawn X/Z
    /// offset instead of excluding the whole coordinate.
    spawn_origin: Arc<Mutex<Option<azalea::Vec3>>>,
    runtime: tokio::runtime::Handle,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mode: Mode::Join,
            dwell: Duration::ZERO,
            spawn_instant: Arc::new(Mutex::new(None)),
            spawned: Arc::new(AtomicBool::new(false)),
            terminal_emitted: Arc::new(AtomicBool::new(false)),
            chunks: Arc::new(Mutex::new(BTreeSet::new())),
            move_samples: Arc::new(Mutex::new(Vec::new())),
            last_sent: Arc::new(Mutex::new(None)),
            teleports: Arc::new(Mutex::new(Vec::new())),
            teleport_acks: Arc::new(Mutex::new(Vec::new())),
            keepalive_log: Arc::new(Mutex::new(KeepaliveLog::default())),
            corrections: Arc::new(Mutex::new(Vec::new())),
            spawn_origin: Arc::new(Mutex::new(None)),
            runtime: tokio::runtime::Handle::current(),
        }
    }
}

fn emit(mut value: Value) {
    value
        .as_object_mut()
        .expect("events must be JSON objects")
        .insert("protocol".to_owned(), json!(TRANSCRIPT_PROTOCOL));
    println!("{value}");
}

/// Flush stdout then exit with `code`.
///
/// Used in the terminal paths (after `joined`/`disconnect`). Azalea's swarm
/// builder self-deadlocks when a queued bot event references an entity cleared
/// by `ecs.clear_all()` (it holds the ECS write lock and then takes a read lock
/// to print a missing-state error). A hard exit avoids that race entirely; this
/// is a headless test client whose transcript is fully emitted before exiting.
/// `println!` flushes per line, so `flush` here is belt-and-braces.
fn hard_exit(code: i32) -> ! {
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}

/// Round a float so transcript values are stable across runs even if the
/// underlying value has spurious trailing digits. Keeps 3 decimals for
/// positions and 2 for small metrics.
fn round_to(v: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (v * f).round() / f
}

fn round_position(p: azalea::core::position::Vec3) -> Value {
    json!({
        "x": round_to(p.x, 3),
        "y": round_to(p.y, 3),
        "z": round_to(p.z, 3),
    })
}

/// The translation key of a disconnect reason that azalea decoded as a
/// translatable component, or `None` for a literal/plain-text reason. The
/// `kick` scenario needs the raw key (e.g.
/// `multiplayer.disconnect.invalid_player_movement`) — not its Debug rendering
/// or a resolved-language string — so it can assert Rivet's exact reason was
/// decoded and reported. `Event::Disconnect(Some(reason))` carries azalea's
/// `FormattedText`; a plain-text reason is `FormattedText::Text`, and the
/// translatable reason from `ClientboundDisconnectPacket` is
/// `FormattedText::Translatable` whose `key` field is the wire key.
fn translatable_key(reason: &azalea::FormattedText) -> Option<String> {
    match reason {
        azalea::FormattedText::Translatable(c) => Some(c.key.clone()),
        _ => None,
    }
}

#[derive(Clone, Resource)]
struct ConnectionFailure(Arc<Mutex<Option<tokio::sync::oneshot::Sender<String>>>>);

struct ConnectionFailurePlugin(ConnectionFailure);

impl Plugin for ConnectionFailurePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone()).add_systems(
            Update,
            capture_connection_failure.after(poll_create_connection_task),
        );
    }
}

/// The keepalive observables a run records: every clientbound challenge id, its
/// receipt instant (dwell mode only), and every serverbound echo id. All three
/// live under one lock so a snapshot is coherent — a challenge and its echo can
/// never be split across two reads, and the challenge/instant pair is written
/// atomically (the same handler records both).
#[derive(Clone, Default)]
struct KeepaliveLog {
    challenges: Vec<u64>,
    instants: Vec<Instant>,
    echoes: Vec<u64>,
}

impl KeepaliveLog {
    /// Whether the challenge and echo streams have reached 1:1 correspondence.
    /// The server challenges at a 1 s cadence and azalea echoes within a tick,
    /// but the two streams are observed on different paths — the echo observer
    /// records synchronously when the challenge packet is processed, while the
    /// challenge itself is recorded only after azalea's async event channel
    /// drains to the client handler — so either stream can briefly lead the
    /// other at a snapshot boundary. Requiring exact 1:1 (not `echoes >=
    /// challenges`) means a settled log is always a coherent state: it cannot
    /// observe a challenge whose echo is still in flight, and it cannot observe
    /// an echo whose challenge record has not landed yet. A genuinely missing
    /// echo (a client that stopped echoing would be kicked) keeps this false, so
    /// the settle window expires and the emitted mismatch still fails the
    /// verdict.
    fn settled(&self) -> bool {
        self.echoes.len() == self.challenges.len()
    }
}

/// Handles shared by the outbound-packet observer: it records the serverbound
/// echoes (teleport acks, keepalive echoes) that azalea sends automatically, so
/// the move transcript can prove the request->echo relationship on the raw ids.
#[derive(Clone)]
struct MoveObservation {
    teleport_acks: Arc<Mutex<Vec<u32>>>,
    keepalive_log: Arc<Mutex<KeepaliveLog>>,
}

/// Registers an observer on every `SendGamePacketEvent` (any serverbound packet
/// the client sends). Azalea's own packet handlers auto-ack teleports and
/// auto-echo keepalives; this observer records those echoes with their ids so
/// the `moved` transcript can verify them against the clientbound requests.
struct MoveObservationPlugin(MoveObservation);

impl Plugin for MoveObservationPlugin {
    fn build(&self, app: &mut App) {
        let teleport_acks = Arc::clone(&self.0.teleport_acks);
        let keepalive_log = Arc::clone(&self.0.keepalive_log);
        app.add_observer(move |event: On<SendGamePacketEvent>| match &event.packet {
            ServerboundGamePacket::AcceptTeleportation(p) => {
                teleport_acks
                    .lock()
                    .expect("teleport acks lock poisoned")
                    .push(p.id);
            }
            ServerboundGamePacket::KeepAlive(p) => {
                keepalive_log
                    .lock()
                    .expect("keepalive log lock poisoned")
                    .echoes
                    .push(p.id);
            }
            _ => {}
        });
    }
}

fn capture_connection_failure(
    mut failures: MessageReader<ConnectionFailedEvent>,
    captured: Res<ConnectionFailure>,
) {
    for failure in failures.read() {
        if let Some(sender) = captured
            .0
            .lock()
            .expect("connection failure lock poisoned")
            .take()
        {
            let _ = sender.send(failure.error.to_string());
        }
    }
}

/// Compact, deterministic description of the player inventory: only non-empty
/// slots, keyed by protocol slot index (0-based across the player menu).
fn describe_inventory(bot: &Client) -> Value {
    let menu = match bot.menu() {
        Ok(menu) => menu,
        Err(_) => return json!(null),
    };
    let selected = bot.selected_hotbar_slot().ok();
    let mut items: Vec<Value> = Vec::new();
    for (i, stack) in menu.slots().iter().enumerate() {
        if stack.is_present() {
            items.push(json!({
                "slot": i,
                "kind": stack.kind().to_string(),
                "count": stack.count(),
            }));
        }
    }
    json!({
        "selected_slot": selected,
        "items": items,
    })
}

/// Emit the canonical `joined` record — the normalized observable outcome —
/// once the world state has settled, then end the client.
/// Wait for the client's received-chunk stream to quiesce: no new chunk for
/// `QUIET_PERIOD`, at least `MIN_OBSERVATION` after spawn, and never longer
/// than `OBSERVATION_TIMEOUT`. The captured chunk set is then the full view
/// square the server sent (its count is deterministic; its coordinates track
/// the player's spawn chunk), not a racy prefix. An empty set never quiesces —
/// a stalled or regressed chunk stream surfaces as `chunk_count: 0`, not as
/// stability. Shared by the join (`observe_and_emit`) and loaded
/// (`loaded_and_emit`) modes.
async fn wait_chunk_quiescence(chunks: &Arc<Mutex<BTreeSet<(i32, i32)>>>) {
    let started = Instant::now();
    let mut last_size = 0usize;
    let mut last_change = started;
    loop {
        let size = chunks.lock().expect("chunks lock poisoned").len();
        if size != last_size {
            last_size = size;
            last_change = Instant::now();
        }
        let now = Instant::now();
        if now >= started + OBSERVATION_TIMEOUT {
            break;
        }
        // Never quiesce on an empty chunk set: a stalled or regressed chunk
        // stream must surface as `chunk_count: 0`, not be accepted as stable.
        if size > 0
            && now >= started + MIN_OBSERVATION
            && now.saturating_duration_since(last_change) >= QUIET_PERIOD
        {
            break;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn observe_and_emit(bot: Client, state: State) {
    let chunks = Arc::clone(&state.chunks);
    let started = Instant::now();
    wait_chunk_quiescence(&chunks).await;

    let position = bot.position().ok().map(round_position);
    let world = bot
        .world_name()
        .ok()
        .map(|name| name.0.to_string())
        .unwrap_or_else(|| "?".to_string());
    let gamemode = bot
        .component::<GameMode>()
        .ok()
        .map(|mode| mode.name().to_string())
        .unwrap_or_else(|| "?".to_string());

    let (health, food, saturation) = match bot.hunger() {
        Ok(hunger) => (
            bot.health().ok(),
            Some(hunger.food),
            Some(hunger.saturation),
        ),
        Err(_) => (None, None, None),
    };
    let experience = bot.experience().ok();

    let chunk_list: Vec<[i32; 2]> = chunks
        .lock()
        .expect("chunks lock poisoned")
        .iter()
        .map(|&(x, z)| [x, z])
        .collect();
    let chunk_count = chunk_list.len();

    emit(json!({
        "event": "joined",
        "position": position,
        "world": world,
        "gamemode": gamemode,
        "health": health.map(|h| json!({
            "health": round_to(h as f64, 2),
            "food": food,
            "saturation": saturation.map(|s| round_to(s as f64, 2)),
        })),
        "experience": experience.map(|e| json!({
            "level": e.level,
            "progress": round_to(e.progress as f64, 3),
            "total": e.total,
        })),
        "inventory": describe_inventory(&bot),
        "chunk_count": chunk_count,
        "chunks": chunk_list,
        "observation_ms": started.elapsed().as_millis() as u64,
    }));

    hard_exit(0);
}

/// The loaded-world sampling contract (issue #374): the client waits for chunk
/// quiescence, then reads the *genuine per-coordinate block content* the
/// server served, from its own loaded `ChunkStorage`, at fixed world
/// coordinates near the spawn chunk.
///
/// The sampled Y levels mirror the read-only ground-truth extractor
/// (`rivet-oracle extract-world`): the surface (highest non-air block per
/// column), `y=-60` (the copied world's bedrock slab) and `y=-61` (one block
/// below the bedrock — the dense stone body a superflat floor does not have).
/// A server that merely echoes repeated superflat bytes for every chunk cannot
/// match a genuine terrain chunk's distinct block set at these coordinates.
///
/// The record is deliberately small and canonical: the runner compares
/// `samples` against the manifest and refuses PASS when any #369 capability
/// flag is present. After sampling, the client takes a short bounded forward
/// walk and reports its `before`/`after` position as the walk evidence (the
/// per-coordinate grid, not the walk delta, is the load-bearing content
/// check). Non-FULL chunks and uncarried #369 capability flags are classified
/// UNVERIFIED rather than a fabricated PASS.
async fn loaded_and_emit(bot: Client, state: State) {
    let chunks = Arc::clone(&state.chunks);
    let started = Instant::now();
    wait_chunk_quiescence(&chunks).await;

    let position = bot.position().ok().map(round_position);
    // The spawn chunk — the deterministic center of the view square the
    // server sent. Everything below samples near it, where the loaded world
    // is guaranteed.
    let spawn_pos = state
        .spawn_origin
        .lock()
        .expect("spawn origin lock poisoned")
        .map(|p| azalea::core::position::BlockPos {
            x: p.x.floor() as i32,
            y: p.y.floor() as i32,
            z: p.z.floor() as i32,
        });
    let spawn_chunk = spawn_pos.map(|p| (p.x.div_euclid(16), p.z.div_euclid(16)));

    // Sample the block content the server served. `sample_cell` walks the
    // loaded world downward from the world ceiling, so the runner compares
    // genuine per-coordinate content. The `WorldHolder` read guard is scoped
    // and dropped before the walk below, so the future stays `Send`.
    let samples: Vec<Value> = {
        let holder = bot
            .component::<WorldHolder>()
            .map_err(|_| "client has no loaded WorldHolder".to_owned())
            .expect("loaded mode requires a joined client");
        match (spawn_chunk, position.as_ref()) {
            (Some((cx, cz)), Some(_)) => {
                let shared = holder.shared.read();
                let mut out = Vec::new();
                for dx in -2..=2i32 {
                    for dz in -2..=2i32 {
                        let chunk_x = cx + dx;
                        let chunk_z = cz + dz;
                        // World-anchored sample point: chunk origin + center
                        // offset, so the manifest's per-coordinate surface
                        // matches.
                        let origin_x = chunk_x * 16 + 8;
                        let origin_z = chunk_z * 16 + 8;
                        let (surface, bedrock, below_feet) =
                            sample_cell(&shared, origin_x, origin_z);
                        out.push(json!({
                            "chunk_x": chunk_x,
                            "chunk_z": chunk_z,
                            "sample_x": origin_x,
                            "sample_z": origin_z,
                            "surface": surface,
                            "bedrock": bedrock,
                            "below_feet": below_feet,
                        }));
                    }
                }
                out
            }
            _ => Vec::new(),
        }
    };

    // A bounded forward walk proves the loaded client can move in the served
    // world (the task's join/dwell/walk evidence). The position delta is
    // recorded, not asserted — the per-coordinate sample grid is the
    // load-bearing content check.
    let walk = loaded_walk(&bot).await;

    emit(json!({
        "event": "loaded",
        "position": position,
        "spawn_chunk": spawn_chunk.map(|(x, z)| [x, z]),
        "chunk_count": chunks.lock().expect("chunks lock poisoned").len(),
        "samples": samples,
        "walk": walk,
        "observation_ms": started.elapsed().as_millis() as u64,
    }));

    hard_exit(0);
}

/// How long the loaded-mode walk runs (wall-clock): long enough to move at
/// least one block in the served world, short enough to stay well inside the
/// client timeout.
const LOADED_WALK: Duration = Duration::from_millis(1500);

/// Drive a short bounded forward walk (yaw -90 faces +x) and report the
/// position before and after as the walk evidence. A server that accepted the
/// client but served a frozen world yields `before == after`; the runner
/// records it honestly rather than fabricating movement.
async fn loaded_walk(bot: &Client) -> Value {
    let before = bot.position().ok().map(round_position);
    let _ = bot.set_direction(-90.0, 0.0);
    bot.walk(WalkDirection::Forward);
    tokio::time::sleep(LOADED_WALK).await;
    bot.walk(WalkDirection::None);
    let after = bot.position().ok().map(round_position);
    json!({ "before": before, "after": after })
}

/// The overworld world-ceiling Y (the copied world is full-height 384).
const WORLD_CEILING_Y: i32 = 320;
/// The bedrock-slab sample Y (mirrors the extractor's `BEDROCK_Y`).
const LOADED_BEDROCK_Y: i32 = -60;
/// One block below the bedrock slab (mirrors the extractor's
/// `BELOW_BEDROCK_Y`).
const LOADED_BELOW_BEDROCK_Y: i32 = -61;

/// Canonicalize an azalea block id for the transcript: azalea's
/// `BlockTrait::id()` returns the bare registry name (`grass_block`), while the
/// ground-truth manifest stores namespaced names (`minecraft:grass_block`).
/// The loaded transcript must match the manifest's namespace so the runner's
/// per-coordinate comparison is not defeated by a representation mismatch.
fn namespaced_block_name(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// Sample one world column at `(x, z)`: the surface block (highest non-air),
/// the block at the bedrock slab, and the block below it. `world` is the
/// client's loaded `World` (its `ChunkStorage`). Returns `minecraft:air` for
/// any coordinate the client has not loaded — the runner treats a missing
/// chunk as a FAIL (the server did not serve the loaded world), never as a
/// vacuous pass. Every emitted name is namespaced (see
/// [`namespaced_block_name`]).
fn sample_cell(world: &azalea::world::World, x: i32, z: i32) -> (String, String, String) {
    let block_at = |y: i32| {
        world
            .get_block_state(BlockPos { x, y, z })
            .map(|s| namespaced_block_name(s.to_trait().id()))
            .unwrap_or_else(|| "minecraft:air".to_owned())
    };
    let mut surface = "minecraft:air".to_owned();
    for y in (LOADED_BELOW_BEDROCK_Y..=WORLD_CEILING_Y).rev() {
        let state = world.get_block_state(BlockPos { x, y, z });
        if let Some(s) = state
            && !s.is_air()
        {
            surface = namespaced_block_name(s.to_trait().id());
            break;
        }
    }
    let bedrock = block_at(LOADED_BEDROCK_Y);
    let below_feet = block_at(LOADED_BELOW_BEDROCK_Y);
    (surface, bedrock, below_feet)
}

/// Emit the canonical `moved` record — the move scenario's observable outcome —
/// after a bounded forward walk, then end the client.
///
/// Faces +x (yaw -90), then hands the walk to the [`MoveSampler`] component:
/// a [`GameTick`] system (registered by [`MoveSamplerPlugin`]) starts the walk
/// on a real tick boundary, samples the client's own post-physics state
/// (position, velocity, on-ground) for the first `SAMPLE_TICKS` moving ticks,
/// stops the walk after `MOVE_TICKS` observed ticks and wakes this task.
/// Sampling is synchronous on the tick thread — one sample per real game tick,
/// immune to the dropped-tick race of the broadcast-based `get_tick_broadcaster()`
/// approach — so the position/velocity sequence is deterministic across boots.
///
/// After the walk this task drains briefly so the final sent positions and any
/// trailing server correction land before the record is emitted, then settles
/// the keepalive stream (see `settle_and_snapshot`) so the transcript's
/// challenge/echo relationship is observed coherently across the boundary.
///
/// The walk's first observed tick is a setup tick (direction set, no movement),
/// so the walk spans `MOVE_TICKS` observed ticks of which `MOVE_TICKS - 1` move
/// the player. The first `SAMPLE_TICKS` moving ticks are sampled; the remaining
/// `MOVE_TICKS - 1 - SAMPLE_TICKS` moving ticks continue unsampled after the
/// last sample: if the server re-syncs the player (`player_position`, which
/// azalea applies directly to local physics) at an arbitrary tick, the
/// perturbation lands in the unsampled tail and cannot corrupt the sampled
/// sequence. The emitted `walk_ticks`, `movement_ticks` and `sampled_ticks`
/// report these three counts while `samples` holds the sampled prefix.
async fn move_and_emit(bot: Client, state: State) {
    // yaw -90 = facing +x (azalea: forward input (0,1) rotated by yaw).
    let _ = bot.set_direction(-90.0, 0.0);

    let samples = Arc::clone(&state.move_samples);
    let last_sent = Arc::clone(&state.last_sent);
    let notify = Arc::new(tokio::sync::Notify::new());
    {
        let mut ecs = bot.ecs.write();
        ecs.entity_mut(bot.entity).insert(MoveSampler {
            tick_count: 0,
            notify: Arc::clone(&notify),
            samples,
            last_sent,
        });
    }
    // Wait for the sampler to finish the walk window.
    notify.notified().await;

    // Let the last sent positions and any trailing server correction flush.
    tokio::time::sleep(MOVE_DRAIN).await;

    // Snapshot the keepalive log via the bounded settle-and-coherent-snapshot
    // mechanism dwell mode uses: a challenge can arrive exactly as MOVE_DRAIN
    // elapses, and because azalea records the echo synchronously while the
    // challenge record only lands after the async event channel drains, either
    // stream can straddle the boundary (observed: a Paper boot with one more
    // echo than challenge — the mirror of the dwell in-flight-pair case). The
    // settle loop waits for 1:1 correspondence (bounded by
    // KEEPALIVE_SETTLE_TIMEOUT) so the transcript's `keepalive_echo` relationship
    // is observed coherently; a genuinely missing echo still times out and fails.
    let keepalive_log = settle_and_snapshot(
        &state.keepalive_log,
        KEEPALIVE_SETTLE_TIMEOUT,
        KEEPALIVE_SETTLE_INTERVAL,
    )
    .await;
    let keepalives = keepalive_log.challenges;
    let keepalive_echoes = keepalive_log.echoes;

    // Snapshot the remaining observables, each under its own lock. The walk
    // finished earlier and MOVE_DRAIN let the write side quiesce: no further
    // teleports/corrections arrive once the player stops moving, so in practice
    // these reads observe the final sets.
    let (samples, teleports, teleport_acks, corrections, origin, last_sent) = {
        let samples = state
            .move_samples
            .lock()
            .expect("move samples lock poisoned")
            .clone();
        let teleports = state
            .teleports
            .lock()
            .expect("teleports lock poisoned")
            .clone();
        let teleport_acks = state
            .teleport_acks
            .lock()
            .expect("teleport acks lock poisoned")
            .clone();
        let corrections = state
            .corrections
            .lock()
            .expect("corrections lock poisoned")
            .clone();
        let origin = *state
            .spawn_origin
            .lock()
            .expect("spawn origin lock poisoned");
        let last_sent = *state.last_sent.lock().expect("last sent lock poisoned");
        (
            samples,
            teleports,
            teleport_acks,
            corrections,
            origin,
            last_sent,
        )
    };
    let origin = origin.expect("move mode requires a recorded spawn position");
    let last_sent = last_sent.expect("move mode requires the walk's last sent position");

    // Samples are normalized to spawn-relative X/Z deltas at full precision
    // (subtract the origin, then round), so the walk is identical across boots
    // even though the server randomizes the spawn X/Z offset each boot. `y` is
    // absolute: the superflat spawn height is deterministic. Velocity is already
    // a per-tick delta (blocks/tick), hence spawn-independent.
    let sample_json = |s: &MoveSample| {
        json!({
            "dx": round_to(s.position.x - origin.x, 3),
            "y": round_to(s.position.y, 3),
            "dz": round_to(s.position.z - origin.z, 3),
            "vx": round_to(s.velocity.x, 4),
            "vy": round_to(s.velocity.y, 4),
            "vz": round_to(s.velocity.z, 4),
            "on_ground": s.on_ground,
        })
    };

    emit(json!({
        "event": "moved",
        "walk": {
            // The walk spans MOVE_TICKS observed game ticks; the first is the
            // setup tick (direction set, no movement), so the player actually
            // moves for MOVE_TICKS - 1 of them. The first SAMPLE_TICKS moving
            // ticks are sampled into `samples`; the remaining
            // MOVE_TICKS - 1 - SAMPLE_TICKS moving ticks are unsampled.
            //
            // `last_sent` is the last movement frame actually sent across all
            // MOVE_TICKS - 1 moving ticks (the `LastSentPosition` at the tick
            // the walk stopped). It is NOT the final sampled position: the
            // samples stop at SAMPLE_TICKS, so the last frame was sent 19
            // moving ticks after the last sample — comparing `last_sent` to the
            // final sample would always disagree and is not done here. Like the
            // samples, X/Z are spawn-relative (the server randomizes the spawn
            // offset each boot) and `y` is absolute (the superflat spawn height
            // is fixed), so the record is normalized the same way. `last_sent`
            // is a *compared* field in the #53 differentials: the evidence
            // across fresh boots and Paper-vs-Rivet runs (the
            // `differing_last_sent_is_compared` parity test) shows it is
            // deterministic per server and Paper-vs-Rivet equal on X/Z, so a
            // differing `last_sent` is a real movement divergence, not per-boot
            // noise — it is no longer excluded (there is no `walk.last_sent`
            // entry in `excluded_move_fields`). The one honest caveat is that
            // `last_sent` snapshots the unsampled tail, the region a mid-walk
            // server re-sync (`player_position`) perturbation would land in; if
            // one ever does land there, the differential now FAILS on the
            // divergence rather than waving it through — the intended strict
            // behavior.
            //
            // `spawn_origin` is the full-precision spawn position the X/Z
            // normalization subtracted. It is carried (not rounded) so the
            // harness can invert the normalization losslessly: the runner adds
            // it back to `last_sent` to reconstruct the absolute position for
            // the Rivet-trace cross-check (run-scenario's
            // `check_rivet_authoritative`). It is per-boot nondeterministic
            // (Paper randomizes the spawn X/Z offset) and excluded from parity.
            "walk_ticks": MOVE_TICKS,
            "movement_ticks": MOVE_TICKS - 1,
            "sampled_ticks": samples.len(),
            "heading_degrees": -90.0,
            "spawn_origin": json!({
                "x": origin.x,
                "y": origin.y,
                "z": origin.z,
            }),
            "last_sent": json!({
                "x": round_to(last_sent.x - origin.x, 3),
                "y": round_to(last_sent.y, 3),
                "z": round_to(last_sent.z - origin.z, 3),
            }),
            "samples": samples.iter().map(sample_json).collect::<Vec<_>>(),
            "teleports": teleports,
            "teleport_acks": teleport_acks,
            "keepalives": keepalives,
            "keepalive_echoes": keepalive_echoes,
            "corrections": corrections.iter().map(|p| round_position(*p)).collect::<Vec<_>>(),
        },
    }));

    hard_exit(0);
}

/// Wait until the challenge and echo streams reach 1:1 correspondence (bounded
/// by `timeout`), then return a coherent snapshot of the log.
///
/// The 1 s cadence means a challenge/echo pair can straddle the emit boundary:
/// a challenge can arrive exactly as the window elapses with azalea's echo
/// still in flight (dwell mode), or — because the echo observer records
/// synchronously while the challenge record only lands after azalea's async
/// event channel drains (move mode) — an echo can be observed with its
/// challenge still in flight. Snapshotting immediately in either case records a
/// pair half and spuriously fails the 1:1 challenge->echo relationship. This
/// waits (polling every `interval`) for the in-flight pair to land and be
/// observed coherently. A pair half whose match truly never arrives (a client
/// that stopped echoing, which the server would kick) keeps the log unsettled
/// until `timeout`; the snapshot then preserves the mismatch, so a genuinely
/// missing echo still fails the verdict — the settle window never masks it.
///
/// `timeout` and `interval` are parameters so the straddle/missing-echo
/// counterfactuals can be driven deterministically in tests.
async fn settle_and_snapshot(
    log: &Arc<Mutex<KeepaliveLog>>,
    timeout: Duration,
    interval: Duration,
) -> KeepaliveLog {
    let settle_deadline = Instant::now() + timeout;
    loop {
        // Hold the log's single lock across the settled decision AND the clone:
        // no writer can interleave, so a returned snapshot is guaranteed to be a
        // settled, coherent state (every challenge it contains has its echo). A
        // challenge that lands only after the clone is simply outside the
        // snapshot — a coherent prefix, never a split pair. The guard is scoped
        // to this block so it is dropped before the sleep below (it is not Send).
        let snapshot = {
            let guard = log.lock().expect("keepalive log lock poisoned");
            if guard.settled() || Instant::now() >= settle_deadline {
                Some(guard.clone())
            } else {
                None
            }
        };
        if let Some(snapshot) = snapshot {
            return snapshot;
        }
        tokio::time::sleep(interval).await;
    }
}

/// Elapsed wall-clock seconds from `start` to `now`, saturating at zero. A
/// receipt instant recorded before the dwell window start (a keepalive that
/// landed before `Event::Spawn` set the origin) must produce 0, never a panic
/// from [`Instant::duration_since`]'s "later instant" precondition.
fn elapsed_secs_f64(start: Instant, now: Instant) -> f64 {
    now.saturating_duration_since(start).as_secs_f64()
}

/// Wall-clock offset (ms) of a keepalive receipt from the dwell window start,
/// saturating at zero for a receipt before spawn (same invariant as
/// [`elapsed_secs_f64`]).
fn receipt_offset_ms(receipt: Instant, start: Instant) -> u64 {
    receipt.saturating_duration_since(start).as_millis() as u64
}

/// The challenge span across the dwell window: the last receipt offset minus
/// the first. `None` when there is no offset pair, or the offsets are inverted
/// (a corrupt/out-of-order stream) — the transcript then carries `null` and the
/// verdict refuses PASS rather than trusting a declared span.
fn challenge_span_ms(offsets: &[u64]) -> Option<u64> {
    match (offsets.first(), offsets.last()) {
        (Some(first), Some(last)) if last >= first => Some(last - first),
        _ => None,
    }
}

/// Emit the `dwell` record — the keepalive-survival observable — after staying
/// connected for `state.dwell` wall-clock seconds past spawn, then end the
/// client.
///
/// The dwell window is measured on the client's monotonic wall clock (`Instant`)
/// from `Event::Spawn` — real time, independent of the server's tick clock. While
/// connected the client stays passive (no movement) and azalea auto-echoes every
/// serverbound keepalive; the record reports each clientbound challenge id and
/// its receipt offset from spawn, plus the serverbound echo ids, so the 1:1
/// challenge->echo pairing and the ~1/s cadence are directly provable from the
/// transcript. The wall span proves the client survived in PLAY past the server's
/// 30s keepalive kick limit.
async fn dwell_and_emit(bot: Client, state: State) {
    let dwell = state.dwell;
    // Connected wall-clock elapsed since spawn. If spawn_instant is somehow
    // unset (should not happen: dwell only starts from the spawn handler), fall
    // back to an elapsed of zero and the following sleep still bounds the run.
    let start = state
        .spawn_instant
        .lock()
        .expect("spawn instant lock poisoned")
        .unwrap_or_else(Instant::now);

    tokio::time::sleep(dwell).await;

    // Settle the keepalive stream, then snapshot it coherently (see
    // `settle_and_snapshot`): a challenge that arrives exactly as the dwell
    // window elapses, with azalea's echo still in flight, is observed together
    // with its echo instead of spuriously failing the relationship.
    let log = settle_and_snapshot(
        &state.keepalive_log,
        KEEPALIVE_SETTLE_TIMEOUT,
        KEEPALIVE_SETTLE_INTERVAL,
    )
    .await;
    let keepalives = log.challenges;
    let keepalive_instants = log.instants;
    let keepalive_echoes = log.echoes;

    let now = Instant::now();
    let connected_wall_seconds = elapsed_secs_f64(start, now);
    // Wall-clock offset of each challenge receipt from spawn (ms). Challenges
    // arrive on the play socket after the join burst settles; the first is at
    // roughly the keepalive cadence after spawn.
    let offsets_ms: Vec<u64> = keepalive_instants
        .iter()
        .map(|t| receipt_offset_ms(*t, start))
        .collect();
    let first_offset_ms = offsets_ms.first().copied();
    let last_offset_ms = offsets_ms.last().copied();
    let challenge_span_ms = challenge_span_ms(&offsets_ms);

    emit(json!({
        "event": "dwell",
        "requested_dwell_seconds": dwell.as_secs(),
        "connected_wall_seconds": round_to(connected_wall_seconds, 3),
        "challenge_count": keepalives.len(),
        "echo_count": keepalive_echoes.len(),
        "challenge_ids": keepalives,
        "echo_ids": keepalive_echoes,
        "first_challenge_offset_ms": first_offset_ms,
        "last_challenge_offset_ms": last_offset_ms,
        "challenge_span_ms": challenge_span_ms,
    }));

    // The bot handle keeps the connection alive for the duration of the dwell;
    // it is intentionally never read beyond that.
    let _ = bot;
    hard_exit(0);
}

/// Send one movement frame whose position is NaN so Rivet's
/// `contains_invalid_values` anti-cheat gate (session.rs `dispatch_move_player`)
/// answers with a `ClientboundDisconnectPacket` carrying the
/// `multiplayer.disconnect.invalid_player_movement` translatable. The
/// `Event::Disconnect` handler decodes that reason and emits the transcript
/// terminal; the `kick` scenario asserts the decoded key. After sending, this
/// parks forever: the disconnect handler's `hard_exit` ends the process, and if
/// the server never kicks (a regression in the gate or the disconnect path), the
/// outer `--timeout-seconds` branch exits with `timeout` and the scenario FAILs.
async fn kick_and_wait(bot: Client, _state: State) {
    bot.write_packet(
        azalea::protocol::packets::game::ServerboundMovePlayerPosRot {
            pos: azalea::Vec3 {
                x: f64::NAN,
                y: 0.0,
                z: 0.0,
            },
            look_direction: azalea::entity::LookDirection::new(0.0, 0.0),
            flags: MoveFlags::default(),
        },
    );
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn handle(bot: Client, event: Event, state: State) {
    match event {
        Event::Init => emit(json!({ "event": "init" })),
        Event::Login => emit(json!({ "event": "login" })),
        Event::ReceiveChunk(pos) => {
            state
                .chunks
                .lock()
                .expect("chunks lock poisoned")
                .insert((pos.x, pos.z));
        }
        Event::Spawn => {
            state.spawned.store(true, Ordering::Release);
            let raw_position = bot.position().ok();
            let position = raw_position.map(round_position);
            *state
                .spawn_origin
                .lock()
                .expect("spawn origin lock poisoned") = raw_position;
            *state
                .spawn_instant
                .lock()
                .expect("spawn instant lock poisoned") = Some(Instant::now());
            emit(json!({
                "event": "spawn",
                "position": position,
            }));
            let runtime = state.runtime.clone();
            match state.mode {
                Mode::Move => runtime.spawn(move_and_emit(bot, state)),
                Mode::Join => runtime.spawn(observe_and_emit(bot, state)),
                Mode::Dwell => runtime.spawn(dwell_and_emit(bot, state)),
                Mode::Kick => runtime.spawn(kick_and_wait(bot, state)),
                Mode::Loaded => runtime.spawn(loaded_and_emit(bot, state)),
            };
        }
        // Move-mode packet observables. Recorded alongside the per-tick samples
        // so the transcript can prove the teleport->ack and keepalive->echo
        // relationships (rivet-capture's relationships.rs patterns) on the raw
        // ids, then normalize discards the per-boot id values.
        Event::Packet(packet) => {
            if state.mode != Mode::Move {
                return;
            }
            match &*packet {
                ClientboundGamePacket::PlayerPosition(p) => {
                    state
                        .teleports
                        .lock()
                        .expect("teleports lock poisoned")
                        .push(p.id);
                }
                ClientboundGamePacket::EntityPositionSync(p) => {
                    state
                        .corrections
                        .lock()
                        .expect("corrections lock poisoned")
                        .push(p.values.pos);
                }
                _ => {}
            }
        }
        Event::KeepAlive(id) => {
            // Record every clientbound keepalive challenge id and (in dwell
            // mode) its receipt instant. Both move and dwell modes report the
            // challenge/echo relationship; dwell needs a full ~1/s sequence to
            // prove the server kept issuing live challenges while the client
            // stayed connected, plus each receipt's wall-clock instant to report
            // the first/last offset and span. The challenge and instant are
            // written under the log's single lock, so a snapshot always pairs a
            // challenge with the instant recorded for it.
            if state.mode != Mode::Join {
                let now = Instant::now();
                let mut log = state
                    .keepalive_log
                    .lock()
                    .expect("keepalive log lock poisoned");
                log.challenges.push(id);
                if state.mode == Mode::Dwell {
                    log.instants.push(now);
                }
            }
        }
        Event::Disconnect(reason) => {
            state.terminal_emitted.store(true, Ordering::Release);
            emit(json!({
                "event": "disconnect",
                "reason": reason.as_ref().map(|reason| format!("{reason:?}")),
                "reason_key": reason.as_ref().and_then(translatable_key),
                "after_spawn": state.spawned.load(Ordering::Acquire),
            }));
            // Preserve the original exit-code contract: 0 if we got to spawn,
            // 1 if the server disconnected us before we spawned.
            let spawned = state.spawned.load(Ordering::Acquire);
            hard_exit(if spawned { 0 } else { 1 });
        }
        _ => {}
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(64);
        }
    };

    let mode = args.mode;
    emit(json!({
        "event": "starting",
        "address": args.address,
        "username": args.username,
        "timeout_seconds": args.timeout.as_secs(),
        "mode": mode.as_str(),
        "dwell_seconds": args.dwell.as_secs(),
        "azalea_revision": AZALEA_REVISION,
    }));

    let state = State {
        mode,
        dwell: args.dwell,
        ..State::default()
    };
    let spawned = Arc::clone(&state.spawned);
    let terminal_emitted = Arc::clone(&state.terminal_emitted);
    let teleport_acks = Arc::clone(&state.teleport_acks);
    let keepalive_log = Arc::clone(&state.keepalive_log);
    let (failure_tx, failure_rx) = tokio::sync::oneshot::channel();
    let connection_failure = ConnectionFailure(Arc::new(Mutex::new(Some(failure_tx))));
    let account = Account::offline(&args.username);
    let client = ClientBuilder::new()
        .reconnect_after(None)
        .add_plugins(ConnectionFailurePlugin(connection_failure))
        .add_plugins(MoveSamplerPlugin)
        .add_plugins(MoveObservationPlugin(MoveObservation {
            teleport_acks,
            keepalive_log,
        }))
        .set_handler(handle)
        .set_state(state)
        .start(account, args.address);

    tokio::select! {
        reason = failure_rx => {
            let reason = reason.unwrap_or_else(|_| "connection failure channel closed".to_owned());
            emit(json!({
                "event": "connection_failed",
                "reason": reason,
            }));
            ExitCode::FAILURE
        }
        _ = client => {
            if spawned.load(Ordering::Acquire) {
                ExitCode::SUCCESS
            } else {
                if !terminal_emitted.load(Ordering::Acquire) {
                    emit(json!({
                        "event": "connection_failed",
                        "reason": "Azalea exited before spawning; see stderr for its resolution or startup error",
                    }));
                }
                ExitCode::FAILURE
            }
        }
        _ = tokio::time::sleep(args.timeout) => {
            emit(json!({
                "event": "timeout",
                "timeout_seconds": args.timeout.as_secs(),
            }));
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keepalive log with `n` challenges (and matching receipt instants) and
    /// `echoed` echoes, the first `echoed` challenge ids echoed.
    fn log_with(n: usize, echoed: usize) -> KeepaliveLog {
        let ids: Vec<u64> = (1001..1001 + n as u64).collect();
        KeepaliveLog {
            challenges: ids.clone(),
            instants: vec![Instant::now(); n],
            echoes: ids.into_iter().take(echoed).collect(),
        }
    }

    #[tokio::test]
    async fn dwell_settle_snapshots_an_in_flight_pair_coherently() {
        // Counterfactual for the spurious-FAIL fix: the dwell window elapses
        // exactly as the server issues a new challenge, with azalea's echo
        // still in flight. A snapshot taken at that moment would record the
        // challenge without its echo and spuriously fail the 1:1
        // challenge->echo relationship. The settle loop must wait for the
        // in-flight echo, then snapshot challenge+echo together so the
        // relationship holds and the verdict does not false-FAIL.
        let log = Arc::new(Mutex::new(log_with(4, 3)));
        // The straddling echo lands 50 ms later — well inside the settle timeout
        // but definitely after the loop's first coherence check.
        let log_for_echo = Arc::clone(&log);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            log_for_echo
                .lock()
                .expect("test lock poisoned")
                .echoes
                .push(1004);
        });

        let started = Instant::now();
        let snapshot =
            settle_and_snapshot(&log, Duration::from_secs(2), Duration::from_millis(5)).await;

        // The snapshot observed the straddling challenge together with its echo:
        // the 1:1 relationship holds (challenge_count == echo_count) and the log
        // is settled. If the settle loop had snapshot immediately, this would be
        // 4 challenges vs 3 echoes and the verdict would FAIL.
        assert_eq!(snapshot.challenges, vec![1001, 1002, 1003, 1004]);
        assert_eq!(snapshot.echoes, vec![1001, 1002, 1003, 1004]);
        assert!(snapshot.settled());
        // The echo physically cannot land before 50 ms, so the settle loop must
        // have waited for it — the pass above is not a race where the echo beat
        // the first coherence check.
        assert!(
            started.elapsed() >= Duration::from_millis(50),
            "settle returned before the in-flight echo could land"
        );
    }

    #[tokio::test]
    async fn move_settle_snapshots_an_echo_without_challenge_coherently() {
        // Counterfactual for the move-mode flake (observed gating PR #562):
        // MOVE_DRAIN elapses exactly as azalea records an echo synchronously
        // (the `SendGamePacketEvent` observer fires while the challenge packet
        // is being processed) but before the challenge record lands on the
        // async event channel — the echo stream then leads the challenge stream
        // (Paper transcript: 6 challenges / 7 echoes). A snapshot taken at that
        // moment records an echo with no matching challenge, which fails the 1:1
        // `keepalive_echo` relationship and false-FAILs parity. The settle loop
        // must wait for the missing challenge, then snapshot echo+challenge
        // together so the relationship holds.
        let log = Arc::new(Mutex::new(KeepaliveLog {
            challenges: vec![1001, 1002, 1003],
            instants: vec![],
            echoes: vec![1001, 1002, 1003, 1004],
        }));
        // The straddling challenge lands 50 ms later — well inside the settle
        // timeout but definitely after the loop's first coherence check.
        let log_for_challenge = Arc::clone(&log);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            log_for_challenge
                .lock()
                .expect("test lock poisoned")
                .challenges
                .push(1004);
        });

        let started = Instant::now();
        let snapshot =
            settle_and_snapshot(&log, Duration::from_secs(2), Duration::from_millis(5)).await;

        // The snapshot observed the straddling echo together with its challenge:
        // the 1:1 relationship holds (challenge_count == echo_count). If the
        // settle loop had snapshot immediately, this would be 3 challenges vs 4
        // echoes and the verdict would FAIL. Under the pre-fix `echoes >=
        // challenges` predicate the settle loop would also return immediately —
        // the pass above proves the strict 1:1 predicate waits for coherence.
        assert_eq!(snapshot.challenges, vec![1001, 1002, 1003, 1004]);
        assert_eq!(snapshot.echoes, vec![1001, 1002, 1003, 1004]);
        assert!(snapshot.settled());
        // The challenge physically cannot land before 50 ms, so the settle loop
        // must have waited for it — the pass above is not a race where the
        // challenge beat the first coherence check.
        assert!(
            started.elapsed() >= Duration::from_millis(50),
            "settle returned before the in-flight challenge could land"
        );
    }

    #[tokio::test]
    async fn dwell_settle_never_masks_a_missing_echo() {
        // Counterfactual for the "no masking" half of the fix: a challenge is
        // recorded whose echo never arrives (a client that stopped echoing,
        // which the server would kick). The settle loop must NOT wait forever —
        // it times out and the snapshot preserves the mismatch, so the verdict
        // still fails on the genuinely missing echo.
        let log = Arc::new(Mutex::new(log_with(4, 3)));
        let snapshot =
            settle_and_snapshot(&log, Duration::from_millis(20), Duration::from_millis(5)).await;

        assert_eq!(snapshot.challenges.len(), 4);
        assert_eq!(snapshot.echoes.len(), 3);
        assert!(!snapshot.settled());
    }

    #[tokio::test]
    async fn dwell_settle_returns_immediately_when_already_settled() {
        // A log whose echoes already caught up must not sleep at all: the settle
        // window only delays the record when there is an in-flight pair to
        // drain, never on a healthy stream.
        let log = Arc::new(Mutex::new(log_with(4, 4)));
        let started = Instant::now();
        let snapshot =
            settle_and_snapshot(&log, Duration::from_secs(1), Duration::from_millis(5)).await;
        assert!(snapshot.settled());
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "a settled log must not be delayed by the settle loop"
        );
    }

    #[test]
    fn duration_helpers_saturate_instead_of_panicking() {
        // Counterfactual for the latent panic paths: a receipt instant recorded
        // before the dwell window start (a keepalive that landed before
        // `Event::Spawn` set the origin) must yield 0, not panic via
        // `Instant::duration_since`'s "later instant" precondition.
        let start = Instant::now();
        let later = start + Duration::from_secs(2);
        assert_eq!(receipt_offset_ms(start, later), 0);
        assert_eq!(elapsed_secs_f64(later, start), 0.0);
        // Normal ordering yields the expected offsets.
        assert_eq!(receipt_offset_ms(later, start), 2000);
        assert_eq!(elapsed_secs_f64(start, later), 2.0);
    }

    #[test]
    fn challenge_span_derives_from_offsets_without_panicking() {
        assert_eq!(challenge_span_ms(&[1200, 41_100]), Some(39_900));
        assert_eq!(challenge_span_ms(&[]), None);
        // A single challenge spans 0 ms (first == last) — the verdict rejects it
        // as below the 30 s minimum — and an inverted pair (corrupt/out-of-order
        // stream) yields None rather than a subtraction panic.
        assert_eq!(challenge_span_ms(&[41_100]), Some(0));
        assert_eq!(challenge_span_ms(&[5000, 1000]), None);
    }

    fn parse(v: &[&str]) -> Result<Args, String> {
        Args::parse_from(v.iter().map(|s| s.to_string()))
    }

    #[test]
    fn loaded_mode_parses_and_rejects_silent_dwell() {
        // `loaded` is a first-class client mode: it joins, waits for chunk
        // quiescence, samples the genuine block content the server served, and
        // emits the `loaded` record for the #374 runner.
        let a = parse(&["--mode", "loaded"]).unwrap();
        assert_eq!(a.mode, Mode::Loaded);
        assert_eq!(a.mode.as_str(), "loaded");

        // --dwell-seconds is dwell-mode-only; an explicit value on loaded is a
        // silent no-op and must be rejected (same policy as join/move).
        let err = parse(&["--mode", "loaded", "--dwell-seconds", "41"]).unwrap_err();
        assert!(
            err.contains("--dwell-seconds") && err.contains("silent no-op"),
            "loaded must reject --dwell-seconds as a silent no-op, got {err}"
        );
    }

    #[test]
    fn non_dwell_modes_reject_explicit_dwell_seconds() {
        // --dwell-seconds is dwell-mode-only; an explicit value on join/move is
        // a silent no-op and must be rejected (the caller's error exits 64).
        for mode in ["join", "move"] {
            let err = parse(&["--mode", mode, "--dwell-seconds", "41"]).unwrap_err();
            assert!(
                err.contains("--dwell-seconds") && err.contains("silent no-op"),
                "{mode} must reject --dwell-seconds as a silent no-op, got {err}"
            );
        }
        // dwell accepts an explicit value (with a timeout that reserves the
        // settle/login headroom; the 30 s default timeout cannot fit any valid
        // dwell window).
        assert!(
            parse(&[
                "--mode",
                "dwell",
                "--dwell-seconds",
                "41",
                "--timeout-seconds",
                "48"
            ])
            .is_ok()
        );
    }

    #[test]
    fn dwell_rejects_a_window_below_the_span_floor() {
        // A 31 s window would span only ~29.8 s of challenges and fail the
        // verdict; the client enforces the same floor as the runner.
        let err = parse(&["--mode", "dwell", "--dwell-seconds", "34"]).unwrap_err();
        assert!(
            err.contains("at least 35"),
            "error must state the minimum window, got {err}"
        );
        assert!(
            parse(&[
                "--mode",
                "dwell",
                "--dwell-seconds",
                "35",
                "--timeout-seconds",
                "42"
            ])
            .is_ok()
        );
    }

    #[test]
    fn dwell_timeout_must_reserve_settle_and_login_headroom() {
        // dwell < timeout is not enough: the timeout starts at process launch
        // while the dwell window starts at spawn, and after the window the
        // client spends up to 1 s settling before emitting. 47 = 41 + 1 (settle)
        // + 5 (login) must be rejected; 48 leaves a strict margin.
        let err = parse(&[
            "--mode",
            "dwell",
            "--dwell-seconds",
            "41",
            "--timeout-seconds",
            "47",
        ])
        .unwrap_err();
        assert!(
            err.contains("--timeout-seconds") && err.contains("settling"),
            "error must explain the reserved settle headroom, got {err}"
        );
        assert!(
            parse(&[
                "--mode",
                "dwell",
                "--dwell-seconds",
                "41",
                "--timeout-seconds",
                "48",
            ])
            .is_ok()
        );
    }
}
