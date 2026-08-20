//! `rivet-codegen` — vanilla data extraction + registry codegen (epic #8).
//!
//! Subcommands:
//! - `extract` — dump the block registry + block states from the Paper 26.2
//!   bundler jar into `data/block_states.json`
//! - `generate` — read that JSON and emit Rust registry source into
//!   `crates/rivet-registry/src/generated/` (committed, feature-gated)
//! - `extract-feature-data` — dump the deterministic seed-42 feature-data
//!   foundation (reachable biomes + biome generation settings + the placed /
//!   configured feature closure) from a live Paper load into
//!   `data/feature_data.json`
//! - `reports` — run the vanilla `net.minecraft.data.Main --reports` datagen
//!   against the materialized Paper 26.2 jar and pin `packets.json`,
//!   `registries.json`, `blocks.json` with provenance in `data/reports/`
//!
//! The tool is excluded from the cargo workspace (see root `Cargo.toml`).

mod biomes_tags;
mod block_behaviors;
mod block_states;
mod extract;
mod extract_biomes_tags;
mod extract_block_behaviors;
mod extract_feature_data;
mod extract_worldgen;
mod feature_data;
mod feature_tables;
mod generate;
mod model;
mod mth_gen;
mod packets;
mod probe_biomes_tags;
mod probe_block_behaviors;
mod probe_block_states;
mod probe_feature_data;
mod probe_worldgen;
mod registries;
mod registry_data;
mod reports;
mod synchronized;
mod worldgen;

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
        Some("registries") => {
            let input = flag(&args, "--input");
            let output = flag(&args, "--output");
            registries::run(input, output)
        }
        Some("mth-gen") => {
            let bundler = flag(&args, "--bundler");
            let output = flag(&args, "--output");
            mth_gen::run(bundler, output)
        }
        Some("probe-block-states") => {
            let bundler = flag(&args, "--bundler");
            probe_block_states::run(bundler)
        }
        Some("extract-biomes-tags") => {
            let bundler = flag(&args, "--bundler");
            let output = flag(&args, "--output");
            extract_biomes_tags::run(bundler, output)
        }
        Some("probe-biomes-tags") => {
            let bundler = flag(&args, "--bundler");
            probe_biomes_tags::run(bundler)
        }
        Some("extract-block-behaviors") => {
            let bundler = flag(&args, "--bundler");
            let output = flag(&args, "--output");
            extract_block_behaviors::run(bundler, output)
        }
        Some("probe-block-behaviors") => {
            let bundler = flag(&args, "--bundler");
            probe_block_behaviors::run(bundler)
        }
        Some("extract-worldgen") => {
            let bundler = flag(&args, "--bundler");
            let output = flag(&args, "--output");
            extract_worldgen::run(bundler, output)
        }
        Some("probe-worldgen") => {
            let bundler = flag(&args, "--bundler");
            probe_worldgen::run(bundler)
        }
        Some("extract-feature-data") => {
            let bundler = flag(&args, "--bundler");
            let output = flag(&args, "--output");
            extract_feature_data::run(bundler, output)
        }
        Some("probe-feature-data") => {
            let bundler = flag(&args, "--bundler");
            probe_feature_data::run(bundler)
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
         \x20   rivet-codegen <extract|generate|registries|mth-gen|probe-block-states|extract-biomes-tags|probe-biomes-tags|extract-block-behaviors|probe-block-behaviors|extract-worldgen|probe-worldgen|extract-feature-data|probe-feature-data|reports> [flags]\n\
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
         \x20   registries  Read data/reports/registries.json and emit the static-builtin\n\
         \x20             id tables for the ordered registry surfaces M1 touches into\n\
         \x20             crates/rivet-registry/src/generated/registries.rs.\n\
         \x20             Flags: --input <path>    registry report (default data/reports/registries.json)\n\
         \x20                     --output <dir>   registry output dir (default crates/rivet-registry/src/generated)\n\
         \x20   mth-gen   Regenerate the Mth tables + golden tests in crates/rivet-util/src\n\
         \x20             from the real Paper Mth class (SIN/ASIN_TAB/COS_TAB + all\n\
         \x20             1156 golden vectors). Idempotent: `git diff` stays clean.\n\
         \x20             Flags: --bundler <path>   path to paper-bundler-26.2*.jar\n\
         \x20                     --output <dir>    repo root to write under (default repo root)\n\
         \x20   probe-block-states  Compile + run GlobalPaletteProbe.java against the real\n\
         \x20             Paper jar and cross-check every emitted block-state id, block, default,\n\
         \x20             and property via a complete digest, plus structural/anchor diagnostics\n\
         \x20             (issue #154). Flags: --bundler <path>  path to paper-bundler-26.2*.jar\n\
         \x20   extract-biomes-tags  Dump the deterministic biome id table + tag network\n\
         \x20             content from a live Paper registry load (BiomeTagExtractor.java)\n\
         \x20             into data/biomes_tags.json (+ provenance manifest), issue #49.\n\
         \x20             Flags: --bundler <path>   path to paper-bundler-26.2*.jar\n\
         \x20                     --output <path>   output JSON (default data/biomes_tags.json)\n\
         \x20   probe-biomes-tags  Re-run the biome+tag extractor against the real Paper jar\n\
         \x20             and require byte-identity with the committed data/biomes_tags.json,\n\
         \x20             plus the anchor counts (issue #49). Flags: --bundler <path>\n\
         \x20   extract-block-behaviors  Dump the compact per-StateId worldgen/heightmap/lighting\n\
         \x20             behavior table from a live Paper Block.BLOCK_STATE_REGISTRY load\n\
         \x20             (BlockBehaviourProbe.java) into data/block_behaviors.json\n\
         \x20             (+ provenance manifest), issue #228.\n\
         \x20             Flags: --bundler <path>   path to paper-bundler-26.2*.jar\n\
         \x20                     --output <path>   output JSON (default data/block_behaviors.json)\n\
         \x20   probe-block-behaviors  Re-run the behavior-table extractor against the real Paper jar\n\
         \x20             and require byte-identity with the committed data/block_behaviors.json,\n\
         \x20             presence of every probe anchor key, and state_count pinned to 32366\n\
         \x20             (anchor values are pinned by the rivet-registry decode tests), issue #228.\n\
         \x20             Flags: --bundler <path>\n\
         \x20   extract-worldgen  Dump the worldgen noise registry, per-biome climate config, and\n\
         \x20             multi-noise biome-source preset parameter points from a live Paper\n\
         \x20             registry load (WorldgenDataExtractor.java) into data/worldgen.json\n\
         \x20             (+ provenance manifest), issue #354.\n\
         \x20             Flags: --bundler <path>   path to paper-bundler-26.2*.jar\n\
         \x20                     --output <path>   output JSON (default data/worldgen.json)\n\
         \x20   probe-worldgen  Re-run the worldgen extractor against the real Paper jar and require\n\
         \x20             byte-identity with the committed data/worldgen.json, plus the anchor\n\
         \x20             counts (noise 63, biome 66, presets 2, nether 5, overworld 7594\n\
         \x20             points), issue #354. Flags: --bundler <path>\n\
         \x20   extract-feature-data  Dump the deterministic seed-42 feature-data foundation (reachable\n\
         \x20             biomes over the committed grid's decoration context, their full biome\n\
         \x20             generation settings, and the transitive placed/configured feature closure\n\
         \x20             as RegistryOps JSON) from a live Paper load into data/feature_data.json\n\
         \x20             (+ provenance manifest), seed-42 FEATURES checkpoint. Flags: --bundler <path>\n\
         \x20                     --output <path>   output JSON (default data/feature_data.json)\n\
         \x20   probe-feature-data  Re-run the feature-data extractor against the real Paper jar and require\n\
         \x20             byte-identity with the committed data/feature_data.json, plus the anchor\n\
         \x20             counts (5 reachable biomes, 203 placed, 170 configured), the paper pin, and\n\
         \x20             non-vacuity. Flags: --bundler <path>\n\
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
