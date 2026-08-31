//! Dedicated Rivet generated-FULL producer entrypoint.
//!
//! This process intentionally does not read Paper output. It owns a fresh
//! controller-supplied output root and is the seam where the real
//! OverworldGenerator -> FULL/light -> SerializableChunkData pipeline will be
//! connected. The current checkout has the codec and chunk-status pieces but
//! not the production Level/registry/lighting orchestration needed to execute
//! that pipeline. A valid launch therefore returns the explicit BLOCKED status;
//! malformed launches and ordinary failures return FAIL.
//!
//! RivetTodo(#54): connecting this producer to the real production pipeline is
//! the producer-bound normal-FULL parity lane owned by issue #54; until then a
//! valid launch exits BLOCKED and no evidence is fabricated.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;

const BLOCKED_PREFIX: &str = "RIVET_GENERATED_FULL_BLOCKED:";
const EXPECTED_COORDINATES: [(i32, i32); 8] = [
    (0, 0),
    (15, 15),
    (31, 31),
    (-1, -1),
    (-16, -16),
    (-31, -31),
    (-1, 0),
    (0, -1),
];

#[derive(Debug)]
enum ProducerError {
    Failed(String),
    Blocked(String),
}

impl From<String> for ProducerError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

fn validate_nonce(nonce: &str) -> Result<(), ProducerError> {
    let mut parts = nonce.split('-');
    let timestamp = parts.next().unwrap_or_default();
    let pid = parts.next().unwrap_or_default();
    let attempt = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || timestamp.is_empty()
        || pid.is_empty()
        || attempt.is_empty()
        || !timestamp
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || !pid.bytes().all(|byte| byte.is_ascii_digit())
        || !attempt.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ProducerError::Failed(format!(
            "producer nonce {nonce:?} does not match <lowercase-hex>-<pid>-<attempt>"
        )));
    }
    Ok(())
}

fn parse_coordinates(value: &str) -> Result<Vec<(i32, i32)>, ProducerError> {
    let parsed: serde_json::Value = serde_json::from_str(value)
        .map_err(|error| format!("producer coordinates are invalid JSON: {error}"))?;
    let entries = parsed.as_array().ok_or_else(|| {
        ProducerError::Failed("producer coordinates must be a JSON array".to_string())
    })?;
    if entries.is_empty() {
        return Err(ProducerError::Failed(
            "producer coordinates must not be empty".to_string(),
        ));
    }

    let mut coordinates = Vec::with_capacity(entries.len());
    let mut unique = HashSet::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let object = entry.as_object().ok_or_else(|| {
            ProducerError::Failed(format!(
                "producer coordinate [{index}] must be an object with exactly x and z"
            ))
        })?;
        if object.len() != 2 || !object.contains_key("x") || !object.contains_key("z") {
            return Err(ProducerError::Failed(format!(
                "producer coordinate [{index}] must contain exactly integer x and z"
            )));
        }
        let parse_field = |name: &str| {
            let value = object[name].as_i64().ok_or_else(|| {
                ProducerError::Failed(format!(
                    "producer coordinate [{index}].{name} must be an integer"
                ))
            })?;
            i32::try_from(value).map_err(|_| {
                ProducerError::Failed(format!(
                    "producer coordinate [{index}].{name} is outside the i32 range"
                ))
            })
        };
        let x = parse_field("x")?;
        let z = parse_field("z")?;
        if !unique.insert((x, z)) {
            return Err(ProducerError::Failed(format!(
                "producer coordinates contain duplicate ({x}, {z})"
            )));
        }
        coordinates.push((x, z));
    }

    let expected = EXPECTED_COORDINATES.into_iter().collect::<HashSet<_>>();
    if unique != expected {
        return Err(ProducerError::Failed(
            "producer coordinates do not match the generated-FULL v1 contract corpus".to_string(),
        ));
    }
    Ok(coordinates)
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(ProducerError::Failed(message)) => {
            eprintln!("generated-full producer failed: {message}");
            std::process::exit(1);
        }
        Err(ProducerError::Blocked(message)) => {
            eprintln!("{BLOCKED_PREFIX} {message}");
            std::process::exit(4);
        }
    }
}

fn run() -> Result<(), ProducerError> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut output = None;
    let mut config = None;
    let mut seed = None;
    let mut coordinates = None;
    let mut nonce = None;
    let mut saw_generated_full = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--generated-full" => {
                if saw_generated_full {
                    return Err("duplicate producer argument --generated-full"
                        .to_string()
                        .into());
                }
                saw_generated_full = true;
                i += 1;
            }
            "--seed" | "--config" | "--output" | "--coordinates" | "--nonce" => {
                let flag = args[i].as_str();
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| format!("{flag} requires a value"))?
                    .clone();
                match flag {
                    "--seed" => {
                        if seed.is_some() {
                            return Err("duplicate producer argument --seed".to_string().into());
                        }
                        seed = Some(value.parse::<u64>().map_err(|error| {
                            format!("producer seed must be an unsigned 64-bit integer: {error}")
                        })?);
                    }
                    "--config" => {
                        if config.is_some() {
                            return Err("duplicate producer argument --config".to_string().into());
                        }
                        config = Some(PathBuf::from(value));
                    }
                    "--output" => {
                        if output.is_some() {
                            return Err("duplicate producer argument --output".to_string().into());
                        }
                        output = Some(PathBuf::from(value));
                    }
                    "--coordinates" => {
                        if coordinates.is_some() {
                            return Err("duplicate producer argument --coordinates"
                                .to_string()
                                .into());
                        }
                        coordinates = Some(parse_coordinates(&value)?);
                    }
                    "--nonce" => {
                        if nonce.is_some() {
                            return Err("duplicate producer argument --nonce".to_string().into());
                        }
                        validate_nonce(&value)?;
                        nonce = Some(value);
                    }
                    _ => unreachable!(),
                }
                i += 2;
            }
            other => return Err(format!("unknown producer argument {other}").into()),
        }
    }
    if !saw_generated_full
        || seed.is_none()
        || config.is_none()
        || output.is_none()
        || coordinates.is_none()
        || nonce.is_none()
    {
        return Err(ProducerError::Failed(
            "usage requires --generated-full --seed <u64> --coordinates <contract corpus JSON> --nonce <lowercase-hex-pid-attempt> --config <immutable config> --output <fresh root>"
                .to_string(),
        ));
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
        return Err(ProducerError::Failed(
            "producer config is empty".to_string(),
        ));
    }
    let config_json: serde_json::Value = serde_json::from_slice(&config_bytes)
        .map_err(|error| format!("producer config is invalid JSON: {error}"))?;
    if !config_json.is_object() {
        return Err(ProducerError::Failed(
            "producer config root must be a JSON object".to_string(),
        ));
    }
    if output.exists() {
        return Err(ProducerError::Failed(format!(
            "controller output root {} already exists; producer will not reuse evidence",
            output.display()
        )));
    }
    // Do not create or touch the output root before the missing production API
    // is available. A failed launch must leave no partial evidence that could
    // later be mistaken for an absent prerequisite.
    Err(ProducerError::Blocked(format!(
        "real OverworldGenerator FULL/light/SerializableChunkData producer is not yet exposed as one production API (needed: Level registry bootstrap, chunk-source dependency closure, light-engine completion, and FULL SerializableChunkData snapshot/write); seed {} was not fabricated",
        seed.expect("checked above")
    )))
}
