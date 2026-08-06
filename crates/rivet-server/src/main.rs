//! `rivet-server` binary. Mirrors `DedicatedServer`/`MinecraftServer.runServer`
//! boot in the smallest end-to-end form: read the bind config, construct the
//! `Server`, and run the accept loop + sync tick thread (issues #145/#93).
//!
//! The full boot sequence (world/level load, registry/data load, tick-phase
//! systems) is M1 sub-issues #100/#101; this binary brings up the TCP listener,
//! pre-play connection state machines, and the empty tick spine.
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
    let server = Server::new(config);

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

/// Parse the bind config. `--host` and `--port` override the defaults so the
/// binary is runnable without a `server.properties` parser (a later M1 slice).
fn server_config_from_args() -> ServerConfig {
    config_from_args(std::env::args().skip(1))
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
            other => panic!("unknown argument {other:?} (expected --host/--port)"),
        }
        i += 1;
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn default_config_binds_any_interface_on_25565() {
        let config = config_from_args(Vec::<String>::new().into_iter());
        assert_eq!(config.bind_host, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(config.port, 25565);
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
}
