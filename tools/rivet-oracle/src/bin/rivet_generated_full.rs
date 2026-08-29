//! Dedicated Rivet generated-FULL producer entrypoint.
//!
//! This process intentionally does not read Paper output.  It owns a fresh
//! controller-supplied output root and is the seam where the real
//! OverworldGenerator -> FULL/light -> SerializableChunkData pipeline will be
//! connected.  The current checkout has the codec and chunk-status pieces but
//! not the production Level/registry/lighting orchestration needed to execute
//! that pipeline.  Returning BLOCKED is therefore the only honest result.
//!
//! RivetTodo(#54): connecting this producer to the real production pipeline is
//! the producer-bound normal-FULL parity lane owned by issue #54; until then
//! every run exits BLOCKED and no evidence is fabricated.

use std::env;
use std::fs;
use std::path::PathBuf;

const BLOCKED_PREFIX: &str = "RIVET_GENERATED_FULL_BLOCKED:";

fn main() {
    if let Err(message) = run() {
        eprintln!("{BLOCKED_PREFIX} {message}");
        std::process::exit(4);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut output = None;
    let mut config = None;
    let mut seed = None;
    let mut saw_generated_full = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--generated-full" => {
                saw_generated_full = true;
                i += 1;
            }
            "--seed" | "--config" | "--output" | "--coordinates" | "--nonce" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| format!("{} requires a value", args[i]))?
                    .clone();
                match args[i].as_str() {
                    "--seed" => seed = Some(value),
                    "--config" => config = Some(PathBuf::from(value)),
                    "--output" => output = Some(PathBuf::from(value)),
                    "--coordinates" | "--nonce" => {}
                    _ => unreachable!(),
                }
                i += 2;
            }
            other => return Err(format!("unknown producer argument {other}")),
        }
    }
    if !saw_generated_full || seed.is_none() || config.is_none() || output.is_none() {
        return Err(
            "usage requires --generated-full --seed <u64> --config <immutable config> --output <fresh root>"
                .to_string(),
        );
    }
    let config = config.expect("checked above");
    let output = output.expect("checked above");
    let config_bytes = fs::read(&config).map_err(|error| {
        format!(
            "controller-owned producer config {} cannot be read: {error}",
            config.display()
        )
    })?;
    if config_bytes.is_empty() {
        return Err("producer config is empty".to_string());
    }
    if output.exists() {
        return Err(format!(
            "controller output root {} already exists; producer will not reuse evidence",
            output.display()
        ));
    }
    // Do not create or touch the output root before the missing production API
    // is available. A failed launch must leave no partial evidence that could
    // later be mistaken for an absent prerequisite.
    Err(format!(
        "real OverworldGenerator FULL/light/SerializableChunkData producer is not yet exposed as one production API (needed: Level registry bootstrap, chunk-source dependency closure, light-engine completion, and FULL SerializableChunkData snapshot/write); seed {} was not fabricated",
        seed.expect("checked above")
    ))
}
