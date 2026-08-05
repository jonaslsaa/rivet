use std::collections::BTreeSet;
use std::env;
use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use azalea::app::{App, Plugin, Update};
use azalea::ecs::message::MessageReader;
use azalea::ecs::prelude::{Res, Resource};
use azalea::ecs::schedule::IntoScheduleConfigs;
use azalea::join::{ConnectionFailedEvent, poll_create_connection_task};
use azalea::prelude::*;
use azalea::core::game_type::GameMode;
use serde_json::{Value, json};

const DEFAULT_ADDRESS: &str = "127.0.0.1:25599";
const DEFAULT_USERNAME: &str = "RivetProbe";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
const AZALEA_REVISION: &str = "6249c295d353b9b3ef68f665b311cba39211fd19";
const TRANSCRIPT_PROTOCOL: u64 = 1;

/// After `Event::Spawn` we keep the client alive for a short observation window
/// so the observable outcome is stable (chunks arrived, health/inventory
/// populated, position settled) before emitting the canonical `joined` record.
const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(15);
const MIN_OBSERVATION: Duration = Duration::from_secs(3);
const QUIET_PERIOD: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug)]
struct Args {
    address: String,
    username: String,
    timeout: Duration,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut address = DEFAULT_ADDRESS.to_owned();
        let mut username = DEFAULT_USERNAME.to_owned();
        let mut timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
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
        })
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {option}"))
}

fn usage() -> String {
    format!(
        "Usage: rivet-client [--address HOST:PORT] [--username NAME] [--timeout-seconds N]\n\
         Defaults: --address {DEFAULT_ADDRESS} --username {DEFAULT_USERNAME} \
         --timeout-seconds {DEFAULT_TIMEOUT_SECONDS}"
    )
}

#[derive(Clone, Component)]
struct State {
    spawned: Arc<AtomicBool>,
    terminal_emitted: Arc<AtomicBool>,
    /// Chunk coordinates received so far (sorted at read time). Shared between
    /// the event handler (writer) and the observation task (reader).
    chunks: Arc<Mutex<BTreeSet<(i32, i32)>>>,
    runtime: tokio::runtime::Handle,
}

impl Default for State {
    fn default() -> Self {
        Self {
            spawned: Arc::new(AtomicBool::new(false)),
            terminal_emitted: Arc::new(AtomicBool::new(false)),
            chunks: Arc::new(Mutex::new(BTreeSet::new())),
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
        if now >= started + MIN_OBSERVATION && now.duration_since(last_change) >= QUIET_PERIOD {
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
        Ok(hunger) => (bot.health().ok(), Some(hunger.food), Some(hunger.saturation)),
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
            let position = bot.position().ok().map(round_position);
            emit(json!({
                "event": "spawn",
                "position": position,
            }));
            let runtime = state.runtime.clone();
            runtime.spawn(observe_and_emit(bot, state));
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

    emit(json!({
        "event": "starting",
        "address": args.address,
        "username": args.username,
        "timeout_seconds": args.timeout.as_secs(),
        "azalea_revision": AZALEA_REVISION,
    }));

    let state = State::default();
    let spawned = Arc::clone(&state.spawned);
    let terminal_emitted = Arc::clone(&state.terminal_emitted);
    let (failure_tx, failure_rx) = tokio::sync::oneshot::channel();
    let connection_failure = ConnectionFailure(Arc::new(Mutex::new(Some(failure_tx))));
    let account = Account::offline(&args.username);
    let client = ClientBuilder::new()
        .reconnect_after(None)
        .add_plugins(ConnectionFailurePlugin(connection_failure))
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
