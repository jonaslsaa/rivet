//! `rivet-server` binary. Mirrors `DedicatedServer`/`MinecraftServer.runServer`
//! boot in the smallest end-to-end form: read the bind config, construct the
//! `Server`, and run the accept loop + sync tick thread (issues #145/#93).
//!
//! The full boot sequence (world/level load, registry/data load, tick-phase
//! systems) is M1 sub-issues #100/#101; this binary brings up the TCP listener,
//! the login/configuration/play state machines, the tick spine, and — with
//! `enable_join` set — the live play path: an offline client that completes
//! login + configuration joins the superflat world and receives the join burst
//! (issue #101 Slice B) sized by its `ClientInformation` view distance — the
//! capture client's 8 caps at `load - 1` and resolves the 117-chunk send-set, a
//! `create_default` client's 2 the 81-chunk set.
//!
//! ## Machine-readable protocol (stdout)
//!
//! stdout carries exactly one line, `RIVET_READY`, printed once the TCP
//! listener is bound and the accept loop is about to serve. The scenario runner
//! (`tools/rivet-client/src/bin/run-scenario/server.rs`) waits for this marker
//! as a hard gate. All human-oriented tracing goes to stderr, so stdout stays a
//! clean machine channel.
//!
//! SIGTERM triggers the orderly shutdown path: the accept loop stops, per-
//! connection tasks close, the tick thread drains and exits, and the process
//! exits 0. The harness sends SIGTERM after the scenario and treats exit 0 as a
//! clean shutdown.

use std::process::ExitCode;

use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;

use rivet_server::server::{Server, ServerConfig};

fn main() -> ExitCode {
    let config = match production_action_from_args(std::env::args().skip(1)) {
        Ok(CliAction::Run(config)) => config,
        Ok(CliAction::Help) => {
            println!("{}", usage_text());
            return ExitCode::SUCCESS;
        }
        Ok(CliAction::Version) => {
            println!("{}", version_text());
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    // tracing output mirrors Paper's log level. Default to INFO; RIVET_LOG
    // overrides globally (e.g. RIVET_LOG=debug enables this crate's debug! logs).
    // No per-target `rivet_server=...` directive is added: in EnvFilter a
    // target-specific directive shadows the global level, which would make
    // RIVET_LOG unable to lower this crate below INFO.
    //
    // Logs go to stderr; stdout is reserved for the machine-readable protocol
    // (the `RIVET_READY` marker).
    let filter = EnvFilter::try_from_env("RIVET_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let server = match Server::try_new(config) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("RIVET_WORLD_UNVERIFIED: {error}");
            return ExitCode::FAILURE;
        }
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(run_server(server))
}

/// Bind, announce readiness, serve until SIGTERM, then exit cleanly.
async fn run_server(server: Server) -> ExitCode {
    let listener = match server.bind().await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("server error: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("RIVET_READY");

    // SIGTERM -> graceful shutdown: the accept loop stops, the tick thread
    // drains and exits, and `serve` returns so the process exits 0.
    let shutdown = server.shutdown_handle();
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(sigterm) => sigterm,
        Err(e) => {
            eprintln!("failed to install SIGTERM handler: {e}");
            return ExitCode::FAILURE;
        }
    };
    tokio::spawn(async move {
        let _ = sigterm.recv().await;
        shutdown.request();
    });

    if let Err(e) = server.serve(listener).await {
        eprintln!("server error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[derive(Debug)]
enum CliAction {
    Run(ServerConfig),
    Help,
    Version,
}

fn version_text() -> String {
    format!(
        "{} {} (Minecraft {})",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        rivet_core::VERSION_NAME
    )
}

fn usage_text() -> String {
    format!(
        "{}\n\n\
         Usage: {} [OPTIONS]\n\n\
         Options:\n\
         \x20     --host <ADDR>    Bind address (default: 0.0.0.0)\n\
         \x20     --port <PORT>    TCP port (default: 25565)\n\
         \x20     --level <PATH>   Disposable world directory path\n\
         \x20     --seed <SEED>    Signed 64-bit world generation seed\n\
         \x20 -h, --help           Print help\n\
         \x20 -V, --version        Print version",
        version_text(),
        env!("CARGO_PKG_NAME"),
    )
}

/// Parse the bind config. `--host`, `--port`, `--level`, and `--seed` override
/// the defaults so the binary is runnable without a `server.properties` parser.
/// `--seed <i64>` is the generated-world seed (the `generated-world`
/// capability contract: the scenario runner boots `rivet-server --seed 42`);
/// the no-level superflat boot carries it into the world object and the login
/// packet's obfuscated seed. A disk `--level` keeps its persisted seed from
/// `world_gen_settings.dat`; a combined `--seed` is then ignored (loaded-world
/// behavior is untouched, so no rejection).
///
/// The production binary always enables the live play path: `Server::new`
/// wires the tick-owned session manager that consumes the configuration→play
/// handoff and fires the join burst (issue #101 Slice B). `ServerConfig::default()`
/// leaves `enable_join` off so the offline-login tests exercise the handoff
/// seam without the burst; this entry point turns it on.
fn production_action_from_args(args: impl Iterator<Item = String>) -> Result<CliAction, String> {
    match action_from_args(args)? {
        CliAction::Run(config) => Ok(CliAction::Run(ServerConfig {
            enable_join: true,
            ..config
        })),
        other => Ok(other),
    }
}

fn action_from_args(args: impl Iterator<Item = String>) -> Result<CliAction, String> {
    let mut config = ServerConfig::default();
    let mut i = 0;
    let args: Vec<String> = args.collect();
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => return Ok(CliAction::Help),
            "--version" | "-V" => return Ok(CliAction::Version),
            "--host" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or_else(|| "--host requires a value (e.g. 0.0.0.0)".to_owned())?;
                config.bind_host = raw.parse().map_err(|_| {
                    format!("invalid --host {raw:?} (expected an IPv4/IPv6 address)")
                })?;
            }
            "--port" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or_else(|| "--port requires a value (e.g. 25565)".to_owned())?;
                config.port = raw
                    .parse()
                    .map_err(|_| format!("invalid --port {raw:?} (expected 0-65535)"))?;
            }
            "--level" => {
                i += 1;
                let raw = args.get(i).ok_or_else(|| {
                    "--level requires a disposable world directory path".to_owned()
                })?;
                config.level_path = Some(raw.into());
            }
            "--seed" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or_else(|| "--seed requires a value (e.g. 42)".to_owned())?;
                config.seed = raw.parse().map_err(|_| {
                    format!("invalid --seed {raw:?} (expected a signed 64-bit integer)")
                })?;
            }
            other => {
                return Err(format!(
                    "unknown argument {other:?} (expected --host/--port/--level/--seed; run --help for usage)"
                ));
            }
        }
        i += 1;
    }
    Ok(CliAction::Run(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    use rivet_server::server::level::ServerLevelConfig;

    fn expect_run(args: impl IntoIterator<Item = &'static str>) -> ServerConfig {
        match action_from_args(args.into_iter().map(str::to_owned)) {
            Ok(CliAction::Run(config)) => config,
            other => panic!("expected CliAction::Run, got {other:?}"),
        }
    }

    #[test]
    fn default_config_binds_any_interface_on_25565() {
        let config = expect_run([]);
        assert_eq!(config.bind_host, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(config.port, 25565);
        assert!(config.level_path.is_none());
    }

    #[test]
    fn parses_host_and_port_overrides() {
        let config = expect_run(["--host", "127.0.0.1", "--port", "25599"]);
        assert_eq!(config.bind_host, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(config.port, 25599);
    }

    #[test]
    fn parses_disposable_level_path() {
        let config = expect_run(["--level", "/tmp/rivet-disposable-world"]);
        assert_eq!(
            config.level_path.as_deref(),
            Some(std::path::Path::new("/tmp/rivet-disposable-world"))
        );
    }

    #[test]
    fn parses_level_with_bind_overrides() {
        let config = expect_run([
            "--host",
            "127.0.0.1",
            "--level",
            "/tmp/rivet-disposable-world",
            "--port",
            "25599",
        ]);
        assert_eq!(config.bind_host, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(config.port, 25599);
        assert_eq!(
            config.level_path.as_deref(),
            Some(std::path::Path::new("/tmp/rivet-disposable-world"))
        );
    }

    #[test]
    fn level_requires_a_path() {
        let err = action_from_args(["--level"].into_iter().map(str::to_owned)).unwrap_err();
        assert!(
            err.contains("--level requires a disposable world directory path"),
            "{err}"
        );
    }

    #[test]
    fn production_config_enables_live_join() {
        let config = match production_action_from_args(Vec::<String>::new().into_iter()) {
            Ok(CliAction::Run(config)) => config,
            other => panic!("expected CliAction::Run, got {other:?}"),
        };
        assert!(
            config.enable_join,
            "the M1 binary must run the live play path (issue #101 Slice B)"
        );
    }

    #[test]
    fn parses_seed_override() {
        let config = expect_run(["--seed", "7"]);
        assert_eq!(config.seed, 7);
    }

    #[test]
    fn seed_defaults_to_the_m1_fixture() {
        // No `--seed` keeps the M1 superflat fixture seed, so the byte-exact
        // join burst is unchanged.
        let config = expect_run([]);
        assert_eq!(config.seed, ServerLevelConfig::M1_FIXTURE_SEED);
    }

    #[test]
    fn parses_seed_negative_and_i64_boundaries() {
        let min = i64::MIN.to_string();
        let max = i64::MAX.to_string();
        for (raw, expected) in [
            ("-1", -1i64),
            ("0", 0i64),
            (min.as_str(), i64::MIN),
            (max.as_str(), i64::MAX),
        ] {
            let config = match action_from_args(["--seed", raw].into_iter().map(str::to_owned)) {
                Ok(CliAction::Run(config)) => config,
                other => panic!("expected CliAction::Run for seed {raw}, got {other:?}"),
            };
            assert_eq!(config.seed, expected, "seed {raw}");
        }
    }

    #[test]
    fn parses_seed_with_bind_overrides() {
        let config = expect_run([
            "--host",
            "127.0.0.1",
            "--seed",
            "7",
            "--level",
            "/tmp/rivet-disposable-world",
            "--port",
            "25599",
        ]);
        assert_eq!(config.bind_host, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(config.port, 25599);
        // A non-default seed proves the `--seed` arm overrides in the
        // combined-flag path (the default equals M1_FIXTURE_SEED, so 42 would
        // pass even if the arm were dropped).
        assert_eq!(config.seed, 7);
        assert_eq!(
            config.level_path.as_deref(),
            Some(std::path::Path::new("/tmp/rivet-disposable-world"))
        );
    }

    #[test]
    fn seed_requires_a_value() {
        let err = action_from_args(["--seed"].into_iter().map(str::to_owned)).unwrap_err();
        assert!(err.contains("--seed requires a value"), "{err}");
    }

    #[test]
    fn seed_rejects_non_integer() {
        let err =
            action_from_args(["--seed", "not-a-seed"].into_iter().map(str::to_owned)).unwrap_err();
        assert!(
            err.contains("invalid --seed") && err.contains("expected a signed 64-bit integer"),
            "{err}"
        );
    }

    #[test]
    fn parses_help_and_version_flags() {
        for flag in ["--help", "-h"] {
            assert!(matches!(
                action_from_args([flag].into_iter().map(str::to_owned)),
                Ok(CliAction::Help)
            ));
        }
        for flag in ["--version", "-V"] {
            assert!(matches!(
                action_from_args([flag].into_iter().map(str::to_owned)),
                Ok(CliAction::Version)
            ));
        }
    }

    #[test]
    fn rejects_unknown_argument_without_panicking() {
        let err = action_from_args(["--bogus"].into_iter().map(str::to_owned)).unwrap_err();
        assert!(
            err.contains("unknown argument") && err.contains("--bogus"),
            "{err}"
        );
    }
}
