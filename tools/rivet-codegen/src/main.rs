//! `rivet-codegen` — vanilla data extraction + registry codegen (epic #8).
//!
//! Subcommands:
//! - `extract` — dump the block registry + block states from the Paper 26.2
//!   bundler jar into `data/block_states.json`
//! - `generate` — read that JSON and emit Rust registry source into
//!   `crates/rivet-registry/src/generated/` (committed, feature-gated)
//! - `reports` — run the vanilla `net.minecraft.data.Main --reports` datagen
//!   against the materialized Paper 26.2 jar and pin `packets.json`,
//!   `registries.json`, `blocks.json` with provenance in `data/reports/`
//!
//! The tool is excluded from the cargo workspace (see root `Cargo.toml`).

mod extract;
mod generate;
mod model;
mod mth_gen;
mod packets;
mod reports;

use std::path::Path;

use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("extract") => {
            let bundler = flag(&args, "--bundler");
            let output = flag(&args, "--output");
            extract::run(bundler, output)
        }
        Some("generate") => {
            let input = flag(&args, "--input");
            let output = flag(&args, "--output");
            let packets = flag(&args, "--packets");
            let packets_output = flag(&args, "--packets-output");
            generate::run(input, output, packets, packets_output)
        }
        Some("mth-gen") => {
            let bundler = flag(&args, "--bundler");
            let output = flag(&args, "--output");
            mth_gen::run(bundler, output)
        }
        Some("reports") => {
            let jar = flag(&args, "--jar");
            let output = flag(&args, "--output");
            let verify = args.contains(&"--verify".to_string());
            reports::run(jar, output, verify)
        }
        Some("help" | "--help" | "-h") | None => {
            print_usage();
            Ok(())
        }
        Some(other) => {
            eprintln!("error: unknown subcommand `{other}`");
            print_usage();
            std::process::exit(2);
        }
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a Path> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| Path::new(w[1].as_str()))
}

fn print_usage() {
    eprintln!(
        "rivet-codegen — vanilla data extraction + registry codegen\n\
         \n\
         USAGE:\n\
         \x20   rivet-codegen <extract|generate|mth-gen|reports> [flags]\n\
         \n\
         SUBCOMMANDS:\n\
         \x20   extract   Extract the block registry + block states from the Paper 26.2\n\
         \x20             bundler jar into data/block_states.json.\n\
         \x20             Flags: --bundler <path>   path to paper-bundler-26.2*.jar\n\
         \x20                     --output <path>   output JSON (default data/block_states.json)\n\
         \x20   generate  Read data/block_states.json and emit Rust registry source\n\
         \x20             into crates/rivet-registry/src/generated/ (committed) and\n\
         \x20             crates/rivet-protocol/src/generated/ (committed).\n\
         \x20             Flags: --input <path>    block registry input (default data/block_states.json)\n\
         \x20                     --output <dir>   registry output dir (default crates/rivet-registry/src/generated)\n\
         \x20                     --packets <path> packet report input (default data/reports/packets.json)\n\
         \x20                     --packets-output <dir>  protocol output dir (default crates/rivet-protocol/src/generated)\n\
         \x20   mth-gen   Regenerate the Mth tables + golden tests in crates/rivet-util/src\n\
         \x20             from the real Paper Mth class (SIN/ASIN_TAB/COS_TAB + all\n\
         \x20             1156 golden vectors). Idempotent: `git diff` stays clean.\n\
         \x20             Flags: --bundler <path>   path to paper-bundler-26.2*.jar\n\
         \x20                     --output <dir>    repo root to write under (default repo root)\n\
         \x20   reports   Run the vanilla net.minecraft.data.Main --reports datagen against the\n\
         \x20             materialized Paper 26.2 server jar and pin packets.json, registries.json,\n\
         \x20             blocks.json with provenance under data/reports/.\n\
         \x20             Flags: --jar <path>      materialized server jar (default\n\
         \x20                     tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar)\n\
         \x20                     --output <dir>    output dir (default data/reports/)\n\
         \x20                     --verify          no-drift gate: fresh --reports runs must be\n\
         \x20                                       byte-identical to the committed fixtures\n\
         \n\
         Requires: java + javac on PATH (or JAVA_HOME), rustfmt, and unzip.\n\
         Default bundler: working/Paper/paper-server/build/libs/paper-bundler-26.2.local-SNAPSHOT.jar"
    );
}
