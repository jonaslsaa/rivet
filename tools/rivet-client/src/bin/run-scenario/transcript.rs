//! Normalization of a client run's raw JSONL transcript into a canonical
//! observable-outcome object for the `join` and `move` scenarios.
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

/// Nondeterministic fields of the `move` transcript, with justification.
///
/// The client already normalizes per-tick positions to spawn-relative `dx/dz`
/// deltas at full precision, so the sampled walk (position, velocity,
/// on-ground) is byte-identical across boots. Teleport ids are deterministic
/// too — Paper's `awaitingTeleport` counter (ServerGamePacketListenerImpl) is
/// per-connection and starts at 0, so on a fresh boot the spawn teleport is
/// always the first teleport (id 1) — so `walk.teleports` / `walk.teleport_acks`
/// are compared, and the teleport->ack echo relationship is additionally
/// checked structurally. What is genuinely per-boot nondeterministic and must
/// be excluded:
///
/// - `walk.keepalives` / `walk.keepalive_echoes`: Paper's keepalive challengeId
///   is `Util.getMillis()` wall-clock (ServerCommonPacketListenerImpl), so the
///   raw ids differ every boot. The relationship — every keepalive has exactly
///   one matching echo — is compared structurally via `keepalive_echo` set.
/// - `walk.corrections` / `walk.corrections_count`: `entity_position_sync`
///   packets are a timing-dependent server observation — how many client
///   position packets land before each server tick decides how often the server
///   re-syncs the player entity. Both the count and the coordinates (which
///   wander far outside the +x walk line) vary across fresh boots. Azalea is
///   client-authoritative for the player, so these corrections never move the
///   client and the sampled walk above is unaffected; they are recorded as a
///   diagnostic so the invariant "server corrections occurred while walking"
///   stays observable, but they are excluded from parity.
fn excluded_move_fields() -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert(
        "walk.keepalives".to_owned(),
        json!("Paper's keepalive challengeId is Util.getMillis() wall-clock (ServerCommonPacketListenerImpl), so raw keepalive ids differ per boot; the keepalive->echo relationship is compared structurally via keepalive_echo set equality"),
    );
    map.insert(
        "walk.keepalive_echoes".to_owned(),
        json!("echo of the per-boot keepalive ids; relationship compared structurally via keepalive_echo set equality"),
    );
    map.insert(
        "walk.corrections".to_owned(),
        json!("entity_position_sync coordinates carry the server's per-boot entity context and timing, so both the coordinates and the count vary per boot; recorded as a diagnostic, excluded from parity"),
    );
    map.insert(
        "walk.corrections_count".to_owned(),
        json!("the number of entity_position_sync packets is timing-dependent (how many client position packets land before each server tick), verified to vary across fresh boots (46-118 across test boots); recorded as a diagnostic, excluded from parity"),
    );
    map
}

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

/// Multiset equality of two JSON scalar arrays (as sorted multisets). Used to
/// compare the teleport->ack and keepalive->echo relationships on the raw ids:
/// every request must have exactly one matching echo, and every echo must have
/// exactly one matching request. The keepalive ids are per-boot and excluded
/// from parity, so the echo relationship is what carries the signal there; the
/// teleport ids are deterministic and compared directly as well.
fn set_equality(a: &Value, b: &Value) -> bool {
    // Compare scalar arrays as sorted multisets by their canonical JSON
    // serialization (serde_json::Value is not PartialOrd).
    let key = |v: &Value| -> Vec<String> {
        let mut keys: Vec<String> = v
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|x| x.to_string())
            .collect();
        keys.sort();
        keys
    };
    key(a) == key(b)
}

/// Project a client run onto the canonical `move` transcript.
///
/// The client's `moved` record already carries per-tick spawn-relative deltas
/// (so the sampled walk is invariant to the server's randomized spawn X/Z), the
/// walk geometry (`walk_ticks` / `movement_ticks` / `sampled_ticks`), the
/// teleport and keepalive request ids, their echoes, and any server corrections.
/// This normalizer projects that record onto the canonical shape, adds the
/// structural echo-relationship flags (set equality on the raw ids — every
/// teleport/keepalive must have exactly one matching echo) and the correction
/// count, and attaches the explicit nondeterminism declaration (keepalive ids
/// and corrections are per-boot; teleport ids are deterministic).
///
/// A `move` run that never reaches spawn (timeout / connection_failed /
/// disconnect before `moved`) normalizes to the same outcome shapes as `join`,
/// minus the movement observables — so a failed move run cannot accidentally
/// pass parity against a successful one. A run that *does* emit `moved` but
/// whose samples show no meaningful forward progress is classified as `noop`
/// (see [`walk_moved`]) so that identical all-zero Paper boots FAIL the
/// Paper-vs-Paper self-check instead of passing it vacuously.
pub fn normalize_move(raw: &str) -> Result<Value, String> {
    let records = parse_records(raw)?;

    let lifecycle: Vec<String> = records
        .iter()
        .filter_map(|r| r.get("event").and_then(Value::as_str))
        .filter(|e| LIFECYCLE_EVENTS.contains(e))
        .map(str::to_owned)
        .collect();

    let moved = records
        .iter()
        .find(|r| r.get("event") == Some(&json!("moved")))
        .cloned();

    let mut transcript = json!({
        "protocol": PROTOCOL,
        "scenario": "move",
        "outcome": if moved.is_some() { "moved" } else { outcome(&records) },
        "lifecycle": lifecycle,
        "excluded": excluded_move_fields(),
    });

    if let Some(moved) = moved {
        let walk = moved.get("walk").cloned().unwrap_or(Value::Null);
        let teleports = walk.get("teleports").cloned().unwrap_or(json!([]));
        let teleport_acks = walk.get("teleport_acks").cloned().unwrap_or(json!([]));
        let keepalives = walk.get("keepalives").cloned().unwrap_or(json!([]));
        let keepalive_echoes = walk.get("keepalive_echoes").cloned().unwrap_or(json!([]));
        let corrections = walk.get("corrections").cloned().unwrap_or(json!([]));
        let samples = walk.get("samples").cloned().unwrap_or(json!([]));
        let sampled_ticks = samples.as_array().map(|a| a.len()).unwrap_or(0);

        transcript["walk"] = json!({
            "walk_ticks": walk.get("walk_ticks").cloned().unwrap_or(Value::Null),
            "movement_ticks": walk.get("movement_ticks").cloned().unwrap_or(Value::Null),
            "sampled_ticks": sampled_ticks,
            "heading_degrees": walk.get("heading_degrees").cloned().unwrap_or(Value::Null),
            "samples": samples,
            "teleport_ack_echo": set_equality(&teleports, &teleport_acks),
            "keepalive_echo": set_equality(&keepalives, &keepalive_echoes),
            "corrections_count": corrections.as_array().map(|a| a.len()).unwrap_or(0),
            "teleports": teleports,
            "teleport_acks": teleport_acks,
            "keepalives": keepalives,
            "keepalive_echoes": keepalive_echoes,
            "corrections": corrections,
        });

        // Semantic invariant: the sampled walk must show meaningful forward
        // progress. A no-op boot (the walk direction was never applied, or the
        // player never actually moved) is a harness failure, not a valid `moved`
        // outcome — reporting it as `noop` makes the runner FAIL rather than
        // compare two identically frozen walks.
        if !walk_moved(&transcript["walk"]["samples"]) {
            transcript["outcome"] = json!("noop");
        }
    }

    Ok(transcript)
}

/// Minimum forward progress (in blocks) a `move` walk must show across its
/// sampled ticks to count as having actually moved. A real 100-tick walk
/// travels ~21 blocks; a no-op boot shows ~0.
const MOVED_DISTANCE_BLOCKS: f64 = 0.5;

/// Forward progress of the sampled walk, in blocks: the last sample's
/// spawn-relative `dx` minus the first's. The samples are ordered by tick and
/// `dx` grows monotonically on the +x walk, so this is the net displacement.
fn walk_progress(samples: &[Value]) -> f64 {
    let dx = |v: &Value| v.get("dx").and_then(Value::as_f64).unwrap_or(0.0);
    samples.last().map(dx).unwrap_or(0.0) - samples.first().map(dx).unwrap_or(0.0)
}

/// Whether the sampled walk actually moved a meaningful distance.
fn walk_moved(samples: &Value) -> bool {
    let samples = samples.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    walk_progress(samples) >= MOVED_DISTANCE_BLOCKS
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

    fn r3(v: f64) -> f64 {
        (v * 1000.0).round() / 1000.0
    }

    fn r4(v: f64) -> f64 {
        (v * 10000.0).round() / 10000.0
    }

    /// A synthetic but realistic sampled walk: 100 moving ticks advancing +x at
    /// ~0.214 blocks/tick to ~21 blocks total, velocity ramping 0.05 -> 0.12
    /// blocks/tick, standing on the superflat surface (y -60, dz 0, on_ground).
    fn realistic_samples() -> Vec<Value> {
        (0..100)
            .map(|i| {
                let i = i as f64;
                json!({
                    "dx": r3(0.098 + i * 0.2139),
                    "y": -60.0,
                    "dz": 0.0,
                    "vx": r4(0.05 + i * 0.0007),
                    "vy": -0.0784,
                    "vz": 0.0,
                    "on_ground": true,
                })
            })
            .collect()
    }

    /// A no-op walk: 100 samples that never move (the walk direction was never
    /// applied, or movement is broken).
    fn noop_samples() -> Vec<Value> {
        (0..100)
            .map(|_| {
                json!({
                    "dx": 0.0,
                    "y": -60.0,
                    "dz": 0.0,
                    "vx": 0.0,
                    "vy": 0.0,
                    "vz": 0.0,
                    "on_ground": true,
                })
            })
            .collect()
    }

    /// Raw move-mode client records with the given sampled walk.
    fn move_records_with(samples: &[Value]) -> String {
        let moved = json!({
            "event": "moved",
            "walk": {
                "walk_ticks": 120,
                "movement_ticks": 119,
                "sampled_ticks": samples.len(),
                "heading_degrees": -90.0,
                "samples": samples,
                "teleports": [1],
                "teleport_acks": [1],
                "keepalives": [1874340021547i64],
                "keepalive_echoes": [1874340021547i64],
                "corrections": [],
            },
            "protocol": 1,
        });
        let moved = moved.to_string();
        [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":40,"mode":"move","azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":9.5,"y":-60.0,"z":-3.5},"protocol":1}"#,
            &moved,
        ]
        .join("\n")
    }

    fn move_records() -> String {
        move_records_with(&realistic_samples())
    }

    #[test]
    fn normalizes_a_successful_move() {
        let t = normalize_move(&move_records()).expect("normalize");
        assert_eq!(t["outcome"], "moved");
        assert_eq!(t["scenario"], "move");
        assert_eq!(t["lifecycle"], json!(["init", "login", "spawn"]));
        // Walk geometry: 120 observed ticks, 119 moving (the first observed tick
        // is a setup tick that does not move), 100 sampled.
        assert_eq!(t["walk"]["walk_ticks"], json!(120));
        assert_eq!(t["walk"]["movement_ticks"], json!(119));
        assert_eq!(t["walk"]["sampled_ticks"], json!(100));
        assert_eq!(t["walk"]["heading_degrees"], json!(-90.0));
        assert_eq!(t["walk"]["samples"].as_array().unwrap().len(), 100);
        // Sample 0 is the first *moving* tick — there is no pre-movement sample.
        assert_eq!(t["walk"]["samples"][0]["dx"], json!(0.098));
        // Structural echo relationships on the raw ids.
        assert_eq!(t["walk"]["teleport_ack_echo"], json!(true));
        assert_eq!(t["walk"]["keepalive_echo"], json!(true));
        assert_eq!(t["walk"]["corrections_count"], json!(0));
        // Teleport ids are deterministic on a fresh boot (Paper's per-connection
        // awaitingTeleport starts at 0), so they are compared, not excluded;
        // keepalive ids are per-boot and excluded but still recorded as
        // diagnostics.
        assert!(t["walk"]["teleports"].is_array());
        assert!(t["walk"]["keepalives"].is_array());
        assert!(t["excluded"].get("walk.teleports").is_none());
        assert!(t["excluded"]["walk.keepalives"].is_string());
        assert!(t["excluded"]["walk.corrections"].is_string());
        assert!(t["excluded"]["walk.corrections_count"].is_string());
    }

    #[test]
    fn move_noop_walk_is_classified_as_noop() {
        // A boot that emits `moved` but never actually moved must not pass as a
        // valid walk: two identical no-op boots would otherwise compare
        // identically and the Paper-vs-Paper self-check would pass vacuously.
        let raw = move_records_with(&noop_samples());
        let t = normalize_move(&raw).expect("normalize");
        assert_eq!(t["outcome"], "noop");
        assert_eq!(t["walk"]["sampled_ticks"], json!(100));
        assert_eq!(t["walk"]["samples"][0]["dx"], json!(0.0));
    }

    #[test]
    fn walk_progress_measures_forward_displacement() {
        let realistic = realistic_samples();
        let noop = noop_samples();
        assert!(walk_progress(&realistic) >= 20.0);
        assert_eq!(walk_progress(&noop), 0.0);
        assert!(walk_moved(&json!(realistic)));
        assert!(!walk_moved(&json!(noop)));
    }

    #[test]
    fn move_echo_relationship_detects_a_missing_ack() {
        // A keepalive with no matching echo is a relationship violation: the
        // transcript must report keepalive_echo: false.
        let raw = move_records().replace(
            "\"keepalive_echoes\":[1874340021547]",
            "\"keepalive_echoes\":[]",
        );
        let t = normalize_move(&raw).expect("normalize");
        assert_eq!(t["walk"]["keepalive_echo"], json!(false));
        assert_eq!(t["walk"]["teleport_ack_echo"], json!(true));
    }

    #[test]
    fn move_echo_relationship_detects_a_mismatched_id() {
        let raw = move_records().replace("\"teleport_acks\":[1]", "\"teleport_acks\":[7]");
        let t = normalize_move(&raw).expect("normalize");
        assert_eq!(t["walk"]["teleport_ack_echo"], json!(false));
    }

    #[test]
    fn failed_move_normalizes_without_movement_observables() {
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":5,"mode":"move","azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"connection_failed","reason":"failed to create connection","protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_move(&raw).expect("normalize");
        assert_eq!(t["outcome"], "connection_failed");
        assert_eq!(t["lifecycle"], json!(["init"]));
        assert!(t.get("walk").is_none());
    }

    #[test]
    fn move_rejects_unknown_protocol() {
        let raw = r#"{"event":"starting","protocol":999}"#;
        assert!(normalize_move(raw).is_err());
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
