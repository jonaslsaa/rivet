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

    let config = server_config_from_args();
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

/// Parse the bind config. `--host`, `--port`, `--level`, and `--seed` override
/// the defaults so the binary is runnable without a `server.properties` parser.
/// `--seed <i64>` is the generated-world seed (the `generated-world`
/// capability contract: the scenario runner boots `rivet-server --seed 42`);
/// the no-level superflat boot carries it into the world object and the login
/// packet's obfuscated seed.
///
/// The production binary always enables the live play path: `Server::new`
/// wires the tick-owned session manager that consumes the configuration→play
/// handoff and fires the join burst (issue #101 Slice B). `ServerConfig::default()`
/// leaves `enable_join` off so the offline-login tests exercise the handoff
/// seam without the burst; this entry point turns it on.
fn server_config_from_args() -> ServerConfig {
    production_config_from_args(std::env::args().skip(1))
}

/// The M1 production config: the parsed bind config with `enable_join` forced
/// on (see [`server_config_from_args`]). Pure so tests can assert the binary's
/// join behavior without touching real process args.
fn production_config_from_args(args: impl Iterator<Item = String>) -> ServerConfig {
    ServerConfig {
        enable_join: true,
        ..config_from_args(args)
    }
}

fn config_from_args(args: impl Iterator<Item = String>) -> ServerConfig {
    let mut config = ServerConfig::default();
    let mut i = 0;
    let args: Vec<String> = args.collect();
    while i < args.len() {
        match args[i].as_str() {
            "--host" => {
                i += 1;
                let raw = args.get(i).expect("--host requires a value (e.g. 0.0.0.0)");
                config.bind_host = raw
                    .parse()
                    .expect("invalid --host (expected an IPv4/IPv6 address)");
            }
            "--port" => {
                i += 1;
                let raw = args.get(i).expect("--port requires a value (e.g. 25565)");
                config.port = raw.parse().expect("invalid --port (expected 0-65535)");
            }
            "--level" => {
                i += 1;
                let raw = args
                    .get(i)
                    .expect("--level requires a disposable world directory path");
                config.level_path = Some(raw.into());
            }
            "--seed" => {
                i += 1;
                let raw = args.get(i).expect("--seed requires a value (e.g. 42)");
                config.seed = raw
                    .parse()
                    .expect("invalid --seed (expected a signed 64-bit integer)");
            }
            other => panic!("unknown argument {other:?} (expected --host/--port/--level/--seed)"),
        }
        i += 1;
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    use rivet_server::server::level::ServerLevelConfig;

    #[test]
    fn default_config_binds_any_interface_on_25565() {
        let config = config_from_args(Vec::<String>::new().into_iter());
        assert_eq!(config.bind_host, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(config.port, 25565);
        assert!(config.level_path.is_none());
    }

    #[test]
    fn parses_host_and_port_overrides() {
        let config = config_from_args(
            ["--host", "127.0.0.1", "--port", "25599"]
                .into_iter()
                .map(str::to_owned),
        );
        assert_eq!(config.bind_host, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(config.port, 25599);
    }

    #[test]
    fn parses_disposable_level_path() {
        let config = config_from_args(
            ["--level", "/tmp/rivet-disposable-world"]
                .into_iter()
                .map(str::to_owned),
        );
        assert_eq!(
            config.level_path.as_deref(),
            Some(std::path::Path::new("/tmp/rivet-disposable-world"))
        );
    }

    #[test]
    fn parses_level_with_bind_overrides() {
        let config = config_from_args(
            [
                "--host",
                "127.0.0.1",
                "--level",
                "/tmp/rivet-disposable-world",
                "--port",
                "25599",
            ]
            .into_iter()
            .map(str::to_owned),
        );
        assert_eq!(config.bind_host, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(config.port, 25599);
        assert_eq!(
            config.level_path.as_deref(),
            Some(std::path::Path::new("/tmp/rivet-disposable-world"))
        );
    }

    #[test]
    #[should_panic(expected = "--level requires a disposable world directory path")]
    fn level_requires_a_path() {
        let _ = config_from_args(["--level"].into_iter().map(str::to_owned));
    }

    #[test]
    fn production_config_enables_live_join() {
        let config = production_config_from_args(Vec::<String>::new().into_iter());
        assert!(
            config.enable_join,
            "the M1 binary must run the live play path (issue #101 Slice B)"
        );
    }

    #[test]
    fn parses_seed_override() {
        let config = config_from_args(["--seed", "7"].into_iter().map(str::to_owned));
        assert_eq!(config.seed, 7);
    }

    #[test]
    fn seed_defaults_to_the_m1_fixture() {
        // No `--seed` keeps the M1 superflat fixture seed, so the byte-exact
        // join burst is unchanged.
        let config = config_from_args(Vec::<String>::new().into_iter());
        assert_eq!(config.seed, ServerLevelConfig::M1_FIXTURE_SEED);
    }

    #[test]
    fn parses_seed_negative_and_i64_boundaries() {
        for (raw, expected) in [
            ("-1", -1i64),
            ("0", 0i64),
            (&i64::MIN.to_string(), i64::MIN),
            (&i64::MAX.to_string(), i64::MAX),
        ] {
            let config = config_from_args(["--seed", raw].into_iter().map(str::to_owned));
            assert_eq!(config.seed, expected, "seed {raw}");
        }
    }

    #[test]
    fn parses_seed_with_bind_overrides() {
        let config = config_from_args(
            [
                "--host",
                "127.0.0.1",
                "--seed",
                "7",
                "--level",
                "/tmp/rivet-disposable-world",
                "--port",
                "25599",
            ]
            .into_iter()
            .map(str::to_owned),
        );
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
    #[should_panic(expected = "--seed requires a value")]
    fn seed_requires_a_value() {
        let _ = config_from_args(["--seed"].into_iter().map(str::to_owned));
    }

    #[test]
    #[should_panic(expected = "invalid --seed (expected a signed 64-bit integer)")]
    fn seed_rejects_non_integer() {
        let _ = config_from_args(["--seed", "not-a-seed"].into_iter().map(str::to_owned));
    }
}
