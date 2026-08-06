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
            serde_json::from_str::<Value>(line)
                .map_err(|e| format!("invalid JSON line: {e}: {line}"))
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
    if records
        .iter()
        .any(|r| r.get("event") == Some(&json!("timeout")))
    {
        return "timeout";
    }
    if records
        .iter()
        .any(|r| r.get("event") == Some(&json!("connection_failed")))
    {
        return "connection_failed";
    }
    if records
        .iter()
        .any(|r| r.get("event") == Some(&json!("disconnect")))
    {
        return "disconnected";
    }
    "unknown"
}

/// Verify a normalized transcript is the honest *pre-play* boundary of a
/// server that has not implemented login/configuration (Rivet, issue #96).
///
/// Returns a human-readable description of how far the client got. Errors when
/// the transcript claims a completed join (`spawned` — Rivet reached play, so
/// the pre-play assumption is stale and the harness must be updated), shows no
/// lifecycle events at all (malformed transcript), or has an outcome other
/// than `disconnected`.
///
/// The outcome is the connection proof, not the lifecycle: azalea fires
/// `Event::Init` on ECS-entity creation *before* any TCP connection is
/// established (`LocalPlayerEvents` is inserted immediately after the join
/// callback, and `init_listener` fires on `Added<LocalPlayerEvents>`), so a
/// non-empty lifecycle alone proves nothing. `connection_failed` fires only
/// when creating the connection fails (azalea's `ConnectionFailedEvent`) and
/// `timeout` means no session completed — neither proves the client reached
/// the server. Only `disconnected` (the server closed a connected client) is
/// evidence of a real pre-play exchange. The companion server-side check
/// (`connection established` + the login listener's `unsupported:` rejection
/// in the rivet log) is the genuinely Rivet-specific half of that proof.
pub fn preplay_verdict(t: &Value) -> Result<&'static str, String> {
    let outcome = t
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if outcome == "spawned" {
        return Err(
            "rivet reached play (outcome=spawned); the pre-play assumption no longer holds — \
             update the harness when login/configuration lands (issues #96/#159)"
                .to_owned(),
        );
    }
    if outcome != "disconnected" {
        return Err(format!(
            "rivet transcript outcome is {outcome} (expected disconnected): the client never \
             completed a session against the Rivet port. connection_failed/timeout mean the \
             connect or first write failed, and the init event alone fires before any connect, \
             so the harness did not actually target the server"
        ));
    }
    let lifecycle: Vec<&str> = t
        .get("lifecycle")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if lifecycle.is_empty() {
        return Err(format!(
            "rivet transcript has no lifecycle events (outcome={outcome}) — malformed transcript"
        ));
    }
    Ok("login (pre-play; Rivet login/configuration not implemented, issue #96)")
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

    #[test]
    fn disconnect_classifies_as_disconnected() {
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":5,"azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"disconnect","reason":"Server closed the connection before login","after_spawn":false,"protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_join(&raw).expect("normalize");
        assert_eq!(t["outcome"], "disconnected");
        assert_eq!(t["lifecycle"], json!(["init"]));
    }

    #[test]
    fn preplay_verdict_accepts_a_disconnected_login_boundary() {
        // A genuine Rivet pre-play run: the server closes the connected client
        // at the login listener (issue #96), which azalea surfaces as a
        // `disconnect` event — the one outcome that proves an established
        // session.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":5,"azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"disconnect","reason":"Server closed the connection before login","after_spawn":false,"protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_join(&raw).expect("normalize");
        let verdict = preplay_verdict(&t).expect("pre-play verdict");
        assert!(
            verdict.contains("pre-play"),
            "verdict must name the pre-play boundary, got {verdict}"
        );
    }

    #[test]
    fn preplay_verdict_rejects_connection_failed() {
        // `connection_failed` is azalea's `ConnectionFailedEvent`, sent only
        // when creating the connection fails (connect refused / first write
        // failed) — the client never reached the server, so this must not be
        // accepted as a pre-play boundary.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":5,"azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"connection_failed","reason":"failed to create connection","protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_join(&raw).expect("normalize");
        assert!(
            preplay_verdict(&t).is_err(),
            "connection_failed must not be accepted: the client never completed a session"
        );
    }

    #[test]
    fn preplay_verdict_rejects_timeout() {
        // A hung/dead endpoint that never responds is a `timeout`, not proof of
        // a pre-play exchange.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":5,"azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"timeout","timeout_seconds":5,"protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_join(&raw).expect("normalize");
        assert!(
            preplay_verdict(&t).is_err(),
            "timeout must not be accepted: the client never completed a session"
        );
    }

    #[test]
    fn preplay_verdict_rejects_a_spawned_transcript() {
        let t = json!({
            "outcome": "spawned",
            "lifecycle": ["init", "login", "spawn"],
        });
        assert!(preplay_verdict(&t).is_err(), "spawned is not pre-play");
    }

    #[test]
    fn preplay_verdict_rejects_an_empty_transcript() {
        let t = json!({
            "outcome": "disconnected",
            "lifecycle": [],
        });
        assert!(
            preplay_verdict(&t).is_err(),
            "a disconnected transcript with no lifecycle is malformed"
        );
    }
}
