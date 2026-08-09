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
use azalea::core::tick::GameTick;
use azalea::ecs::message::MessageReader;
use azalea::ecs::observer::On;
use azalea::ecs::prelude::{Query, Res, Resource};
use azalea::ecs::schedule::IntoScheduleConfigs;
use azalea::entity::{EntityGeometryUpdateSystems, LastSentPosition, Physics};
use azalea::join::{ConnectionFailedEvent, poll_create_connection_task};
use azalea::movement::{
    send_player_input_packet, send_position, send_sprinting_if_needed, update_pose,
};
use azalea::packet::game::SendGamePacketEvent;
use azalea::physics::PhysicsSystems;
use azalea::physics::client_movement::ClientMovementState;
use azalea::prelude::*;
use azalea::protocol::packets::game::{ClientboundGamePacket, ServerboundGamePacket};
use serde_json::{Value, json};

const DEFAULT_MODE: &str = "join";
const DEFAULT_ADDRESS: &str = "127.0.0.1:25599";
const DEFAULT_USERNAME: &str = "RivetProbe";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
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
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Join => "join",
            Mode::Move => "move",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "join" => Some(Mode::Join),
            "move" => Some(Mode::Move),
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
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut address = DEFAULT_ADDRESS.to_owned();
        let mut username = DEFAULT_USERNAME.to_owned();
        let mut timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
        let mut mode = Mode::Join;
        let mut args = env::args().skip(1);

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
                        format!("invalid --mode value: {value} (expected join|move)")
                    })?;
                }
                "--help" | "-h" => return Err(usage()),
                _ => return Err(format!("unknown argument: {argument}\n\n{}", usage())),
            }
        }

        if username.is_empty() {
            return Err("--username must not be empty".to_owned());
        }
        if timeout_seconds == 0 {
            return Err("--timeout-seconds must be greater than zero".to_owned());
        }

        Ok(Self {
            address,
            username,
            timeout: Duration::from_secs(timeout_seconds),
            mode,
        })
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {option}"))
}

fn usage() -> String {
    format!(
        "Usage: rivet-client [--mode join|move] [--address HOST:PORT] [--username NAME] \
         [--timeout-seconds N]\n\
         Defaults: --mode {DEFAULT_MODE} --address {DEFAULT_ADDRESS} --username {DEFAULT_USERNAME} \
         --timeout-seconds {DEFAULT_TIMEOUT_SECONDS}"
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
    /// Clientbound keepalive ids, observed by the event handler.
    keepalives: Arc<Mutex<Vec<u64>>>,
    /// Serverbound `keep_alive` ids (the echo), observed by the outbound packet
    /// observer (azalea auto-echoes every keepalive).
    keepalive_echoes: Arc<Mutex<Vec<u64>>>,
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
            spawned: Arc::new(AtomicBool::new(false)),
            terminal_emitted: Arc::new(AtomicBool::new(false)),
            chunks: Arc::new(Mutex::new(BTreeSet::new())),
            move_samples: Arc::new(Mutex::new(Vec::new())),
            last_sent: Arc::new(Mutex::new(None)),
            teleports: Arc::new(Mutex::new(Vec::new())),
            teleport_acks: Arc::new(Mutex::new(Vec::new())),
            keepalives: Arc::new(Mutex::new(Vec::new())),
            keepalive_echoes: Arc::new(Mutex::new(Vec::new())),
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

/// Handles shared by the outbound-packet observer: it records the serverbound
/// echoes (teleport acks, keepalive echoes) that azalea sends automatically, so
/// the move transcript can prove the request->echo relationship on the raw ids.
#[derive(Clone)]
struct MoveObservation {
    teleport_acks: Arc<Mutex<Vec<u32>>>,
    keepalive_echoes: Arc<Mutex<Vec<u64>>>,
}

/// Registers an observer on every `SendGamePacketEvent` (any serverbound packet
/// the client sends). Azalea's own packet handlers auto-ack teleports and
/// auto-echo keepalives; this observer records those echoes with their ids so
/// the `moved` transcript can verify them against the clientbound requests.
struct MoveObservationPlugin(MoveObservation);

impl Plugin for MoveObservationPlugin {
    fn build(&self, app: &mut App) {
        let teleport_acks = Arc::clone(&self.0.teleport_acks);
        let keepalive_echoes = Arc::clone(&self.0.keepalive_echoes);
        app.add_observer(move |event: On<SendGamePacketEvent>| match &event.packet {
            ServerboundGamePacket::AcceptTeleportation(p) => {
                teleport_acks
                    .lock()
                    .expect("teleport acks lock poisoned")
                    .push(p.id);
            }
            ServerboundGamePacket::KeepAlive(p) => {
                keepalive_echoes
                    .lock()
                    .expect("keepalive echoes lock poisoned")
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
async fn observe_and_emit(bot: Client, state: State) {
    let chunks = Arc::clone(&state.chunks);
    let started = Instant::now();
    let mut last_size = 0usize;
    let mut last_change = started;

    // Wait for the chunk stream to quiesce: no new chunk for QUIET_PERIOD, at
    // least MIN_OBSERVATION after spawn, and never longer than
    // OBSERVATION_TIMEOUT. The captured chunk set is the full view square the
    // server sent (its count is deterministic; its coordinates track the
    // player's spawn chunk), not a racy prefix.
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
            && now.duration_since(last_change) >= QUIET_PERIOD
        {
            break;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

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
/// trailing server correction land before the record is emitted.
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

    // Snapshot the observables. Each lock is taken and released per value (never
    // all at once). The walk finished earlier and MOVE_DRAIN let the write side
    // quiesce: no further teleports/corrections arrive once the player stops
    // moving, and the keepalive cadence (1s) is far longer than the drain
    // (200ms), so in practice these reads observe the final sets. If a keepalive
    // somehow landed between two reads, the echo relationship below would report
    // false and the run would FAIL — a spurious failure, never a false pass,
    // which is the honest direction for a differential harness.
    let (
        samples,
        teleports,
        teleport_acks,
        keepalives,
        keepalive_echoes,
        corrections,
        origin,
        last_sent,
    ) = {
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
        let keepalives = state
            .keepalives
            .lock()
            .expect("keepalives lock poisoned")
            .clone();
        let keepalive_echoes = state
            .keepalive_echoes
            .lock()
            .expect("keepalive echoes lock poisoned")
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
            keepalives,
            keepalive_echoes,
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
            // is fixed), so the record is normalized the same way — but it is
            // NOT guaranteed identical across boots: it is a snapshot of the
            // unsampled tail, the region a mid-walk server re-sync
            // (`player_position`) perturbation lands in. It is recorded as a
            // diagnostic and excluded from the #53 differential (see the
            // `walk.last_sent` entry in `excluded_move_fields`).
            "walk_ticks": MOVE_TICKS,
            "movement_ticks": MOVE_TICKS - 1,
            "sampled_ticks": samples.len(),
            "heading_degrees": -90.0,
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
            emit(json!({
                "event": "spawn",
                "position": position,
            }));
            let runtime = state.runtime.clone();
            match state.mode {
                Mode::Move => runtime.spawn(move_and_emit(bot, state)),
                Mode::Join => runtime.spawn(observe_and_emit(bot, state)),
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
            if state.mode == Mode::Move {
                state
                    .keepalives
                    .lock()
                    .expect("keepalives lock poisoned")
                    .push(id);
            }
        }
        Event::Disconnect(reason) => {
            state.terminal_emitted.store(true, Ordering::Release);
            emit(json!({
                "event": "disconnect",
                "reason": reason.map(|reason| format!("{reason:?}")),
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
        "azalea_revision": AZALEA_REVISION,
    }));

    let state = State {
        mode,
        ..State::default()
    };
    let spawned = Arc::clone(&state.spawned);
    let terminal_emitted = Arc::clone(&state.terminal_emitted);
    let teleport_acks = Arc::clone(&state.teleport_acks);
    let keepalive_echoes = Arc::clone(&state.keepalive_echoes);
    let (failure_tx, failure_rx) = tokio::sync::oneshot::channel();
    let connection_failure = ConnectionFailure(Arc::new(Mutex::new(Some(failure_tx))));
    let account = Account::offline(&args.username);
    let client = ClientBuilder::new()
        .reconnect_after(None)
        .add_plugins(ConnectionFailurePlugin(connection_failure))
        .add_plugins(MoveSamplerPlugin)
        .add_plugins(MoveObservationPlugin(MoveObservation {
            teleport_acks,
            keepalive_echoes,
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
