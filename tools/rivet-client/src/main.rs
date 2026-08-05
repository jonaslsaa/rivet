use std::env;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use azalea::app::{App, Plugin, Update};
use azalea::ecs::message::MessageReader;
use azalea::ecs::prelude::{Res, Resource};
use azalea::ecs::schedule::IntoScheduleConfigs;
use azalea::join::{ConnectionFailedEvent, poll_create_connection_task};
use azalea::prelude::*;
use serde_json::{Value, json};

const DEFAULT_ADDRESS: &str = "127.0.0.1:25599";
const DEFAULT_USERNAME: &str = "RivetProbe";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
const AZALEA_REVISION: &str = "6249c295d353b9b3ef68f665b311cba39211fd19";
const TRANSCRIPT_PROTOCOL: u64 = 1;

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
}

impl Default for State {
    fn default() -> Self {
        Self {
            spawned: Arc::new(AtomicBool::new(false)),
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

async fn handle(bot: Client, event: Event, state: State) {
    match event {
        Event::Init => emit(json!({ "event": "init" })),
        Event::Login => emit(json!({ "event": "login" })),
        Event::Spawn => {
            state.spawned.store(true, Ordering::Release);
            let position = bot.position().ok();
            emit(json!({
                "event": "spawn",
                "position": position.map(|position| json!({
                    "x": position.x,
                    "y": position.y,
                    "z": position.z,
                })),
            }));
            bot.exit();
        }
        Event::Disconnect(reason) => {
            emit(json!({
                "event": "disconnect",
                "reason": reason.map(|reason| format!("{reason:?}")),
                "after_spawn": state.spawned.load(Ordering::Acquire),
            }));
            bot.exit();
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
