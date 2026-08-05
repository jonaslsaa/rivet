//! `rivet-codegen` — vanilla data extraction + registry codegen (epic #8).
//!
//! Subcommands:
//! - `extract` — dump the block registry + block states from the Paper 26.2
//!   bundler jar into `data/block_states.json`
//! - `generate` — read that JSON and emit Rust registry source into
//!   `crates/rivet-registry/src/generated/` (committed, feature-gated)
//!
//! The tool is excluded from the cargo workspace (see root `Cargo.toml`).

mod extract;
mod generate;
mod model;

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
            generate::run(input, output)
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
    args.windows(2).find(|w| w[0] == name).map(|w| Path::new(w[1].as_str()))
}

fn print_usage() {
    eprintln!(
        "rivet-codegen — vanilla data extraction + registry codegen\n\
         \n\
         USAGE:\n\
         \x20   rivet-codegen <extract|generate> [flags]\n\
         \n\
         SUBCOMMANDS:\n\
         \x20   extract   Extract the block registry + block states from the Paper 26.2\n\
         \x20             bundler jar into data/block_states.json.\n\
         \x20             Flags: --bundler <path>   path to paper-bundler-26.2*.jar\n\
         \x20                     --output <path>   output JSON (default data/block_states.json)\n\
         \x20   generate  Read data/block_states.json and emit Rust registry source\n\
         \x20             into crates/rivet-registry/src/generated/ (committed).\n\
         \x20             Flags: --input <path>    input JSON (default data/block_states.json)\n\
         \x20                     --output <dir>   output dir (default generated/)\n\
         \n\
         Requires: java + javac on PATH (or JAVA_HOME), and unzip.\n\
         Default bundler: working/Paper/paper-server/build/libs/paper-bundler-26.2.local-SNAPSHOT.jar"
    );
}
