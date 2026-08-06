//! `rivet-server` binary. Mirrors `DedicatedServer`/`MinecraftServer.runServer`
//! boot in the smallest end-to-end form: read the bind config, construct the
//! `Server`, and run the accept loop + sync tick thread (issues #145/#93).
//!
//! The full boot sequence (world/level load, registry/data load, tick-phase
//! systems) is M1 sub-issues #100/#101; this binary brings up the TCP listener,
//! pre-play connection state machines, and the empty tick spine.

use tracing_subscriber::EnvFilter;

use rivet_server::server::{Server, ServerConfig};

fn main() {
    // tracing output mirrors Paper's log level. Default to INFO; RIVET_LOG
    // overrides globally (e.g. RIVET_LOG=debug enables this crate's debug! logs).
    // No per-target `rivet_server=...` directive is added: in EnvFilter a
    // target-specific directive shadows the global level, which would make
    // RIVET_LOG unable to lower this crate below INFO.
    let filter = EnvFilter::try_from_env("RIVET_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let config = server_config_from_args();
    let server = Server::new(config);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        if let Err(e) = server.run().await {
            eprintln!("server error: {e}");
            std::process::exit(1);
        }
    });
}

/// Parse the bind config. `--host` and `--port` override the defaults so the
/// binary is runnable without a `server.properties` parser (a later M1 slice).
fn server_config_from_args() -> ServerConfig {
    let args: Vec<String> = std::env::args().collect();
    let mut config = ServerConfig::default();
    let mut i = 1;
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
