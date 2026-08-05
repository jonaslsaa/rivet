//! Normalization of a client run's raw JSONL transcript into a canonical
//! observable-outcome object for the `join` scenario.
//!
//! The client emits a JSON-lines stream on stdout (`protocol:1`). Not every
//! line is an observable outcome: `starting` is launch metadata and
//! `disconnect`/`connection_failed`/`timeout` are terminal states. This module
//! projects the stream onto the canonical shape the comparator diffs:
//!
//! ```json
//! {
//!   "protocol": 1,
//!   "scenario": "join",
//!   "outcome": "spawned",
//!   "lifecycle": ["init", "login", "spawn"],
//!   "position": {"x": 9.5, "y": -60.0, "z": -3.5},
//!   "world": "minecraft:overworld",
//!   "gamemode": "survival",
//!   "health": {"health": 20.0, "food": 20, "saturation": 5.0},
//!   "experience": {"level": 0, "progress": 0.0, "total": 0},
//!   "inventory": {"selected_slot": 0, "items": []},
//!   "chunk_count": 117,
//!   "chunks": [[-4, -4], ...],
//!   "excluded": {
//!     "position.x": "...why nondeterministic...",
//!     "position.z": "...why nondeterministic..."
//!   }
//! }
//! ```
//!
//! The `excluded` map is the explicit, justified nondeterminism declaration
//! (WORKFLOWS.md §headless-client-driver): the Paper server randomizes the
//! player's spawn X/Z offset each boot (verified across fresh boots), and the
//! chunk coordinate list is centered on that randomized spawn chunk, so it too
//! shifts per boot. `position.y` (superflat spawn height) and the chunk count
//! are deterministic and stay compared. The comparator skips exactly these
//! fields and reports them as excluded, never silently.

use serde_json::{Value, json};

/// Protocol version emitted by `rivet-client`. Consumers must reject any other
/// version rather than guessing at the event shape.
pub const PROTOCOL: u64 = 1;

/// Events that count as the observable join lifecycle, in emit order.
const LIFECYCLE_EVENTS: [&str; 3] = ["init", "login", "spawn"];

/// Nondeterministic fields of the join transcript, with justification. The
/// Paper server randomizes the player's spawn X/Z offset around the (fixed)
/// world spawn on every boot (verified across fresh boots). `position.y`
/// (superflat spawn height), the received chunk *count*, and everything else
/// are deterministic. The chunk *coordinate list* is a function of the
/// randomized player spawn chunk, so it is recorded as a diagnostic but
/// excluded from parity.
fn excluded_fields() -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert(
        "position.x".to_owned(),
        json!("player spawn X offset is randomized by the server per boot (world spawn is fixed at the level.dat spawn block); only position.y is deterministic"),
    );
    map.insert(
        "position.z".to_owned(),
        json!("player spawn Z offset is randomized by the server per boot (world spawn is fixed at the level.dat spawn block); only position.y is deterministic"),
    );
    map.insert(
        "chunks".to_owned(),
        json!("the received chunk coordinate list is centered on the player's (randomized) spawn chunk, so it shifts per boot; the chunk count is deterministic and is compared"),
    );
    map
}

/// Parse a raw JSON-lines stream into records, rejecting malformed lines and
/// unsupported protocol versions.
pub fn parse_records(raw: &str) -> Result<Vec<Value>, String> {
    let records: Vec<Value> = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<Value>(line).map_err(|e| format!("invalid JSON line: {e}: {line}"))
        })
        .collect::<Result<_, _>>()?;
    for record in &records {
        if record.get("protocol") != Some(&json!(PROTOCOL)) {
            return Err(format!(
                "unsupported transcript protocol {:?} (expected {PROTOCOL})",
                record.get("protocol")
            ));
        }
    }
    Ok(records)
}

/// Determine the terminal outcome from the raw records.
fn outcome(records: &[Value]) -> &'static str {
    if records
        .iter()
        .any(|r| r.get("event") == Some(&json!("joined")))
    {
        return "spawned";
    }
    if records.iter().any(|r| r.get("event") == Some(&json!("timeout"))) {
        return "timeout";
    }
    if records
        .iter()
        .any(|r| r.get("event") == Some(&json!("connection_failed")))
    {
        return "connection_failed";
    }
    "unknown"
}

/// Project a client run onto the canonical `join` transcript.
pub fn normalize_join(raw: &str) -> Result<Value, String> {
    let records = parse_records(raw)?;

    let lifecycle: Vec<String> = records
        .iter()
        .filter_map(|r| r.get("event").and_then(Value::as_str))
        .filter(|e| LIFECYCLE_EVENTS.contains(e))
        .map(str::to_owned)
        .collect();

    let joined = records
        .iter()
        .find(|r| r.get("event") == Some(&json!("joined")))
        .cloned();

    let mut transcript = json!({
        "protocol": PROTOCOL,
        "scenario": "join",
        "outcome": outcome(&records),
        "lifecycle": lifecycle,
        "excluded": excluded_fields(),
    });

    if let Some(mut joined) = joined {
        // Copy every observable field the client already normalizes.
        let joined_obj = joined.as_object_mut().expect("joined is an object");
        for key in [
            "position",
            "world",
            "gamemode",
            "health",
            "experience",
            "inventory",
            "chunk_count",
            "chunks",
        ] {
            if let Some(value) = joined_obj.remove(key) {
                transcript[key] = value;
            }
        }
    }

    Ok(transcript)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_records() -> String {
        [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":40,"azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":9.5,"y":-60.0,"z":-3.5},"protocol":1}"#,
            r#"{"event":"joined","position":{"x":9.5,"y":-59.0,"z":-3.5},"world":"minecraft:overworld","gamemode":"survival","health":{"health":20.0,"food":20,"saturation":5.0},"experience":{"level":0,"progress":0.0,"total":0},"inventory":{"selected_slot":0,"items":[]},"chunk_count":81,"chunks":[[-4,-4],[-4,-3],[0,0]],"observation_ms":4123,"protocol":1}"#,
        ]
        .join("\n")
    }

    #[test]
    fn normalizes_a_successful_join() {
        let t = normalize_join(&client_records()).expect("normalize");
        assert_eq!(t["outcome"], "spawned");
        assert_eq!(t["lifecycle"], json!(["init", "login", "spawn"]));
        assert_eq!(t["position"]["y"], json!(-59.0));
        assert_eq!(t["chunk_count"], json!(81));
        assert_eq!(t["chunks"], json!([[-4, -4], [-4, -3], [0, 0]]));
        // `observation_ms` is wall-clock and must not leak into the transcript.
        assert!(t.get("observation_ms").is_none());
        assert!(t.get("event").is_none());
        // The explicit nondeterminism declaration is always present.
        assert!(t["excluded"].is_object());
        assert!(t["excluded"]["position.x"].is_string());
        assert!(t["excluded"]["chunks"].is_string());
    }

    #[test]
    fn failed_join_normalizes_without_observables() {
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":5,"azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"connection_failed","reason":"failed to create connection","protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_join(&raw).expect("normalize");
        assert_eq!(t["outcome"], "connection_failed");
        assert_eq!(t["lifecycle"], json!(["init"]));
        assert!(t.get("position").is_none());
    }

    #[test]
    fn rejects_unknown_protocol() {
        let raw = r#"{"event":"starting","protocol":999}"#;
        assert!(normalize_join(raw).is_err());
    }
}
