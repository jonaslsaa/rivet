//! Normalization of a client run's raw JSONL transcript into a canonical
//! observable-outcome object for the `join` and `move` scenarios.
//!
//! The client emits a JSON-lines stream on stdout (`protocol:1`). Not every
//! line is an observable outcome: `starting` is launch metadata (including the
//! pinned Azalea revision) and `disconnect`/`connection_failed`/`timeout` are
//! terminal states. This module projects the stream onto the canonical shape
//! the comparator diffs:
//!
//! Strict JSONL policies come from `rivet-harness-common::transcript`:
//! [`parse_lines`](rivet_harness_common::transcript::parse_lines) rejects any
//! malformed (unparseable) line, and
//! [`check_terminal`](rivet_harness_common::transcript::check_terminal)
//! rejects a stream whose terminal event is duplicated (corrupt) or missing
//! (the run never completed its outcome). The per-run terminal is derived from
//! the outcome the records imply (`spawned`→`joined`, `moved`→`moved`,
//! failures→their failure event) — the client emits exactly one terminal and
//! then exits.
//!
//! ```json
//! {
//!   "protocol": 1,
//!   "scenario": "join",
//!   "outcome": "spawned",
//!   "lifecycle": ["init", "login", "spawn"],
//!   "azalea_revision": "6249c295d353b9b3ef68f665b311cba39211fd19",
//!   "position": {"x": 9.5, "y": -63.0, "z": -3.5},
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
//!
//! `azalea_revision` is the exact Git revision the client was built against
//! (see [`PINNED_AZALEA_REVISION`]); [`rivet_play_verdict`] requires it so a
//! stale or locally-modified client binary cannot stand in for the pinned
//! headless client.

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
///   is `Util.getMillis()` — `System.nanoTime() / 1e6`, monotonic milliseconds
///   since the JVM started (ServerCommonPacketListenerImpl), so the raw ids
///   differ every boot. The relationship — every keepalive has exactly one
///   matching echo — is compared structurally via `keepalive_echo` set.
/// - `walk.corrections` / `walk.corrections_count`: `entity_position_sync`
///   packets are a timing-dependent server observation — how many client
///   position packets land before each server tick decides how often the server
///   re-syncs the player entity. Both the count and the coordinates (which
///   wander far outside the +x walk line) vary across fresh boots. Azalea is
///   client-authoritative for the player, so these corrections never move the
///   client and the sampled walk above is unaffected; they are recorded as a
///   diagnostic so the invariant "server corrections occurred while walking"
///   stays observable, but they are excluded from parity.
/// - `walk.spawn_origin`: the full-precision spawn position the client
///   subtracted to normalize the samples and `last_sent` to spawn-relative X/Z.
///   Paper randomizes the spawn X/Z offset per boot, so the origin varies per
///   boot and is excluded — the whole point of the normalization is that the
///   walk is compared independent of where the player spawned. It is carried
///   (rather than dropped) because the harness needs it: `check_rivet_authoritative`
///   adds the origin back to `last_sent` to reconstruct the absolute position
///   for the cross-check against Rivet's absolute authoritative trace.
fn excluded_move_fields() -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert(
        "walk.keepalives".to_owned(),
        json!("Paper's keepalive challengeId is Util.getMillis() — System.nanoTime()/1e6, monotonic milliseconds since JVM start (ServerCommonPacketListenerImpl) — so raw keepalive ids differ per boot; the keepalive->echo relationship is compared structurally via keepalive_echo set equality"),
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
    map.insert(
        "walk.spawn_origin".to_owned(),
        json!("the full-precision spawn position subtracted to normalize the samples and last_sent to spawn-relative X/Z; Paper randomizes the spawn X/Z offset per boot, so the origin varies per boot — the walk is compared spawn-relative, and the origin is only carried so check_rivet_authoritative can reconstruct the absolute last_sent"),
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

/// Parse a raw JSON-lines stream into records, rejecting malformed lines
/// (the shared blank-skip + strict-parse policy) and unsupported protocol
/// versions.
pub fn parse_records(raw: &str) -> Result<Vec<Value>, String> {
    let records = rivet_harness_common::transcript::parse_lines(raw, |line| {
        serde_json::from_str::<Value>(line).map_err(|e| format!("invalid JSON line: {e}: {line}"))
    })?;
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

/// Strict terminal policy for the client's transcript (shared
/// `rivet-harness-common::transcript::check_terminal`): exactly one record
/// must carry the terminal event the outcome implies. A duplicate is a corrupt
/// stream (the client emits its terminal once, then exits); an absent terminal
/// means the run never completed that outcome.
fn check_outcome_terminal(records: &[Value], outcome: &str) -> Result<(), String> {
    let terminal = match outcome {
        "spawned" => "joined",
        "moved" => "moved",
        "dwelled" => "dwell",
        "timeout" => "timeout",
        "connection_failed" => "connection_failed",
        "disconnected" => "disconnect",
        other => {
            return Err(format!(
                "transcript has no terminal event (outcome {other}) — the run never completed \
                 an observable outcome"
            ));
        }
    };
    rivet_harness_common::transcript::check_terminal(records, terminal, |r| {
        r.get("event").and_then(Value::as_str)
    })
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

/// The Azalea revision the harness requires the client to have been built
/// against (issue #192). The client emits its actual build revision in the
/// `starting` record; the verdict compares it to this pin so a locally-modified
/// or stale client binary cannot stand in for the unmodified pinned headless
/// client.
pub const PINNED_AZALEA_REVISION: &str = "6249c295d353b9b3ef68f665b311cba39211fd19";

/// The deterministic chunk send-set a Rivet join must deliver. Rivet's join
/// burst sends the Moonrise view-distance-4 square for the client's resolved
/// view distance. The bounds are `±(view_distance + 1)` (the `includeNeighbors`
/// margin): for view distance 4 that is the 11×11 raster `-5..5`, and the
/// four corners `(±5, ±5)` are excluded by `isWithinDistance` (`3²+3²=18 ≥
/// 4²=16`), leaving exactly 117. With the pinned Azalea client
/// (`ClientInformation::default().view_distance = 8`, resolved through
/// `client + 1` capped at `load - 1 = 4`) that resolves to view distance 4 and
/// the 117-chunk send-set.
pub const JOIN_CHUNK_COUNT: u64 = 117;

/// Rivet's fixed superflat spawn height (`BlockPos(0, -63, 0)`), deterministic
/// across boots. The Paper reference in the Rivet-vs-Paper differential boots
/// the single-stone superflat fixture and spawns at the same y=-63.0 (issue
/// #159), so `position.y` is a genuinely compared field on both sides — never
/// excluded or normalized. This is required as the genuine-Rivet marker.
pub const JOIN_SPAWN_Y: f64 = -63.0;

/// Verify a normalized transcript is the honest *play* boundary of a genuine
/// Rivet boot: the unmodified pinned Azalea client completed offline login,
/// configuration (registry sync), the play handoff, spawned, and received
/// exactly the deterministic 117-chunk send-set.
///
/// Returns a human-readable description of the play boundary reached. Errors
/// when the transcript does not prove that:
///
/// - the outcome is anything but `spawned`. `connection_failed`/`timeout` mean
///   the connect or first write failed, and `disconnected` means the server
///   closed the client before play — exactly what a fake or non-Rivet endpoint
///   (one that never completes login/configuration) produces. The pre-play
///   Rivet build produced `disconnected` at the login boundary, so this is also
///   the counterfactual that fails against a stale pre-play server.
/// - the lifecycle does not contain both `login` and `spawn` (malformed
///   transcript, or the client never reached play).
/// - `chunk_count != 117` — the server did not send the deterministic
///   view-distance-4 send-set.
/// - `position.y != JOIN_SPAWN_Y` — the server did not spawn the player at
///   Rivet's fixed superflat spawn (the default-flat Paper spawn fails here).
/// - `azalea_revision != PINNED_AZALEA_REVISION` — the client binary was built
///   against a different Azalea revision than the pinned one.
///
/// The `spawned` outcome is a stronger connection proof than the pre-play
/// `disconnected` ever was: azalea fires `Event::Init` before any TCP connect,
/// and `connection_failed`/`timeout` fire without a completed session, but the
/// `joined` event that produces `spawned` is emitted only after the client
/// observed login, configuration, the play handoff, chunk quiescence, and the
/// player entity spawn. The companion server-side check (`connection
/// established` in the rivet log) is the genuinely Rivet-specific half of that
/// proof.
pub fn rivet_play_verdict(t: &Value) -> Result<&'static str, String> {
    let outcome = t
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if outcome != "spawned" {
        return Err(format!(
            "rivet transcript outcome is {outcome} (expected spawned): the client never \
             completed login/configuration into play against the Rivet port. \
             connection_failed/timeout mean the connect or first write failed, and disconnected \
             means the server closed the client before play — what a fake or non-Rivet endpoint, \
             or a stale pre-play Rivet build, produces"
        ));
    }
    let lifecycle: Vec<&str> = t
        .get("lifecycle")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if !lifecycle.contains(&"login") || !lifecycle.contains(&"spawn") {
        return Err(format!(
            "rivet transcript lifecycle is {lifecycle:?} (expected to contain login and spawn) \
             — malformed transcript or the client never reached play"
        ));
    }
    if t.get("azalea_revision").and_then(Value::as_str) != Some(PINNED_AZALEA_REVISION) {
        return Err(format!(
            "rivet transcript azalea_revision is {:?} (expected {PINNED_AZALEA_REVISION}) — the \
             client was not built from the pinned unmodified Azalea revision",
            t.get("azalea_revision").and_then(Value::as_str)
        ));
    }
    let chunk_count = t.get("chunk_count").and_then(Value::as_u64).unwrap_or(0);
    if chunk_count != JOIN_CHUNK_COUNT {
        return Err(format!(
            "rivet transcript received {chunk_count} chunks (expected {JOIN_CHUNK_COUNT}): the \
             server did not send the deterministic view-distance-4 send-set"
        ));
    }
    let y = t
        .get("position")
        .and_then(|p| p.get("y"))
        .and_then(Value::as_f64);
    if y != Some(JOIN_SPAWN_Y) {
        return Err(format!(
            "rivet transcript spawn position.y is {y:?} (expected {JOIN_SPAWN_Y}): the server \
             did not spawn the player at Rivet's fixed superflat spawn"
        ));
    }
    Ok("play (login + configuration + spawn; deterministic 117-chunk send-set)")
}

/// Project a client run onto the canonical `join` transcript.
pub fn normalize_join(raw: &str) -> Result<Value, String> {
    let records = parse_records(raw)?;
    check_outcome_terminal(&records, outcome(&records))?;

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

    // The `starting` record carries the client binary's pinned Azalea build
    // revision; surface it so the verdict can prove the client was built from
    // the pinned unmodified revision.
    let azalea_revision = records
        .iter()
        .find(|r| r.get("event") == Some(&json!("starting")))
        .and_then(|r| r.get("azalea_revision").and_then(Value::as_str))
        .map(str::to_owned);

    let mut transcript = json!({
        "protocol": PROTOCOL,
        "scenario": "join",
        "outcome": outcome(&records),
        "lifecycle": lifecycle,
        "azalea_revision": azalea_revision,
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
    let outcome = if moved.is_some() {
        "moved"
    } else {
        outcome(&records)
    };
    check_outcome_terminal(&records, outcome)?;

    let mut transcript = json!({
        "protocol": PROTOCOL,
        "scenario": "move",
        "outcome": outcome,
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
            // The full-precision spawn origin the client subtracted to
            // normalize X/Z, carried verbatim and excluded from parity (see
            // `excluded_move_fields`). The harness needs it to invert the
            // normalization for the Rivet-trace cross-check.
            "spawn_origin": walk.get("spawn_origin").cloned().unwrap_or(Value::Null),
            // The client already normalizes `last_sent` to spawn-relative X/Z
            // (matching the samples), so it is copied verbatim and compared: a
            // differing `last_sent` surfaces as a compared diff and fails
            // parity — it is not absorbed by an exclusion. Paper-vs-Rivet
            // equality on X/Z is not assumed here; the live both-mode
            // differential verifies it each run (its comparator and divergence
            // gate require the compared fields to match).
            "last_sent": walk.get("last_sent").cloned().unwrap_or(Value::Null),
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

/// The server's keepalive kick limit (keepalive.rs `KEEPALIVE_LIMIT_MS` = 30 s).
/// Wall-clock survival requires the client to stay connected in PLAY strictly
/// longer than this window while echoing every live challenge.
pub const DWELL_SURVIVAL_SECONDS: f64 = 30.0;
/// The minimum challenge/echo count a surviving dwell must show. The server
/// transmits one keepalive per second (the 1 s throttle), so surviving past the
/// 30 s kick limit requires at least 30 challenges, each echoed.
pub const DWELL_MIN_CHALLENGES: usize = 30;
/// The minimum wall-clock span (ms) between the first and last challenge
/// receipt — the challenges must actually span the survival window, not cluster
/// at its start.
pub const DWELL_MIN_SPAN_MS: u64 = 30_000;
/// The minimum wall-clock dwell window (s) the `dwell` scenario accepts. The
/// verdict requires the challenge span (last receipt offset minus first) to
/// reach `DWELL_MIN_SPAN_MS`, and the first challenge lands ~1.2 s after spawn
/// (the join burst must settle first), so a window of only 31 s —
/// `DWELL_SURVIVAL_SECONDS + 1` — would span ~29.8 s and fail the verdict on a
/// healthy run. 35 s leaves a comfortable margin above the 30 s span for any
/// realistic first-challenge offset.
pub const DWELL_MIN_DWELL_SECONDS: u64 = 35;

/// Nondeterministic fields of the `dwell` transcript, with justification.
///
/// Rivet's keepalive challenge id is the `TickTime` millis reading at transmit
/// time (keepalive.rs `now_ms` — `Util.getMillis()` semantics), monotonic per
/// boot, so the raw challenge/echo ids differ every boot and are excluded from
/// parity. The wall-clock survival window (`connected_wall_seconds`), the
/// challenge count, the span, and the 1:1 challenge->echo relationship are
/// deterministic-in-outcome and are compared/verdict-checked, not excluded.
fn excluded_dwell_fields() -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert(
        "dwell.challenge_ids".to_owned(),
        json!("Rivet's keepalive challenge id is the TickTime millis reading at transmit time (keepalive.rs now_ms, Util.getMillis() semantics) — monotonic per boot, so the raw ids differ every boot; the 1:1 challenge->echo relationship is compared structurally via echo_relationship"),
    );
    map.insert(
        "dwell.echo_ids".to_owned(),
        json!("echo of the per-boot keepalive ids; relationship compared structurally via echo_relationship"),
    );
    map
}

/// Project a client run onto the canonical `dwell` transcript.
///
/// The client's `dwell` record carries the wall-clock survival window
/// (`connected_wall_seconds`, measured on the client's monotonic clock from
/// spawn), the raw challenge/echo keepalive ids, and the first/last challenge
/// offsets. This normalizer projects that record onto the canonical shape, adds
/// the structural echo-relationship flag (set equality on the raw ids — every
/// challenge must have exactly one matching echo), and attaches the explicit
/// nondeterminism declaration (the per-boot id values). The survival scalars —
/// `connected_wall_seconds`, `challenge_count`, `echo_count`, and the span — are
/// the verdict-checked acceptance invariants.
///
/// `challenge_count`, `echo_count`, and `challenge_span_ms` are *derived*, not
/// copied from the record: the record's duplicated count/span fields are
/// redundant with the `challenge_ids`/`echo_ids` arrays and the first/last
/// offset pair, so a malformed client that declares a self-contradictory count
/// or span (a lying or buggy record) must not shape the transcript. The counts
/// come from the array lengths, and the span from `last_challenge_offset_ms -
/// first_challenge_offset_ms` (computed in the client from the receipt
/// instants). A span whose offsets are absent or inverted (a corrupt/out-of-order
/// stream) normalizes to `null` and fails the verdict.
///
/// A `dwell` run that never completes the window (timeout / connection_failed /
/// disconnect before the `dwell` record — a server that kicked the client, or a
/// client that never spawned) normalizes to the same failure outcomes as the
/// other scenarios, minus the dwell observables.
pub fn normalize_dwell(raw: &str) -> Result<Value, String> {
    let records = parse_records(raw)?;

    let lifecycle: Vec<String> = records
        .iter()
        .filter_map(|r| r.get("event").and_then(Value::as_str))
        .filter(|e| LIFECYCLE_EVENTS.contains(e))
        .map(str::to_owned)
        .collect();

    let dwell = records
        .iter()
        .find(|r| r.get("event") == Some(&json!("dwell")))
        .cloned();
    let outcome = if dwell.is_some() {
        "dwelled"
    } else {
        outcome(&records)
    };
    check_outcome_terminal(&records, outcome)?;

    // The `starting` record carries the client binary's pinned Azalea build
    // revision; surface it so the verdict can prove the client was built from
    // the pinned unmodified revision.
    let azalea_revision = records
        .iter()
        .find(|r| r.get("event") == Some(&json!("starting")))
        .and_then(|r| r.get("azalea_revision").and_then(Value::as_str))
        .map(str::to_owned);

    let mut transcript = json!({
        "protocol": PROTOCOL,
        "scenario": "dwell",
        "outcome": outcome,
        "lifecycle": lifecycle,
        "azalea_revision": azalea_revision,
        "excluded": excluded_dwell_fields(),
    });

    if let Some(dwell) = dwell {
        let challenge_ids = dwell.get("challenge_ids").cloned().unwrap_or(json!([]));
        let echo_ids = dwell.get("echo_ids").cloned().unwrap_or(json!([]));
        // Canonical counts: the actual challenge/echo arrays, not the record's
        // duplicated count fields (a malformed record could declare anything).
        let challenge_count = challenge_ids.as_array().map(|a| a.len()).unwrap_or(0);
        let echo_count = echo_ids.as_array().map(|a| a.len()).unwrap_or(0);
        // Canonical span: recomputed from the first/last receipt offsets the
        // client measured. The record's declared `challenge_span_ms` is a
        // redundant duplicate and is not trusted.
        let first_offset = dwell
            .get("first_challenge_offset_ms")
            .and_then(Value::as_u64);
        let last_offset = dwell
            .get("last_challenge_offset_ms")
            .and_then(Value::as_u64);
        let challenge_span_ms = match (first_offset, last_offset) {
            (Some(first), Some(last)) if last >= first => Some(last - first),
            _ => None,
        };
        transcript["dwell"] = json!({
            "requested_dwell_seconds": dwell.get("requested_dwell_seconds").cloned().unwrap_or(Value::Null),
            "connected_wall_seconds": dwell.get("connected_wall_seconds").cloned().unwrap_or(Value::Null),
            "challenge_count": challenge_count,
            "echo_count": echo_count,
            "challenge_ids": challenge_ids,
            "echo_ids": echo_ids,
            "echo_relationship": set_equality(&challenge_ids, &echo_ids),
            "first_challenge_offset_ms": first_offset.map(Value::from),
            "last_challenge_offset_ms": last_offset.map(Value::from),
            "challenge_span_ms": challenge_span_ms.map(Value::from),
        });
    }

    Ok(transcript)
}

/// Verify a normalized `dwell` transcript proves wall-clock keepalive survival
/// against a genuine Rivet boot: the unmodified pinned Azalea client spawned
/// into PLAY, stayed connected strictly past the server's 30 s keepalive kick
/// limit, and echoed every live keepalive challenge.
///
/// Returns a human-readable description of the survival boundary reached. Errors
/// when the transcript does not prove that:
///
/// - the outcome is anything but `dwelled` — a kicked/disconnected client, or
///   one that never spawned, emits a terminal before the `dwell` record.
/// - the lifecycle does not contain both `login` and `spawn`.
/// - `azalea_revision != PINNED_AZALEA_REVISION` — the client was not built from
///   the pinned unmodified Azalea revision.
/// - `connected_wall_seconds <= DWELL_SURVIVAL_SECONDS` — the client did not
///   survive past the kick limit (a client that stopped echoing would be kicked
///   ~1 s after the first unanswered challenge exceeds 30 s).
/// - `challenge_count < DWELL_MIN_CHALLENGES` — the server did not keep issuing
///   live challenges across the window (its 1 s cadence implies >= 30).
/// - `echo_count != challenge_count` or `echo_relationship == false` — a client
///   that stopped echoing every challenge would be kicked; the transcript must
///   show a 1:1 challenge->echo pairing.
/// - `challenge_span_ms < DWELL_MIN_SPAN_MS` — the challenges did not span the
///   survival window.
/// - `first_challenge_offset_ms` absent or null — no keepalive challenge ever
///   arrived during the window (the normalizer projects an absent raw offset to
///   null, so the guard checks the value, not just the key).
///
/// The companion server-side checks (in the runner) are the `connection
/// established` log (the client genuinely reached the Rivet port) and the
/// absence of the `read timeout` kick log.
pub fn rivet_dwell_verdict(t: &Value) -> Result<&'static str, String> {
    let outcome = t
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if outcome != "dwelled" {
        return Err(format!(
            "dwell transcript outcome is {outcome} (expected dwelled): the client never completed \
             the dwell window — a kicked or disconnected client emits a terminal before the dwell \
             record"
        ));
    }
    let lifecycle: Vec<&str> = t
        .get("lifecycle")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if !lifecycle.contains(&"login") || !lifecycle.contains(&"spawn") {
        return Err(format!(
            "dwell transcript lifecycle is {lifecycle:?} (expected to contain login and spawn) — \
             malformed transcript or the client never reached play"
        ));
    }
    if t.get("azalea_revision").and_then(Value::as_str) != Some(PINNED_AZALEA_REVISION) {
        return Err(format!(
            "dwell transcript azalea_revision is {:?} (expected {PINNED_AZALEA_REVISION}) — the \
             client was not built from the pinned unmodified Azalea revision",
            t.get("azalea_revision").and_then(Value::as_str)
        ));
    }
    let dwell = t.get("dwell").and_then(Value::as_object).ok_or_else(|| {
        "dwell transcript has no dwell record — the client never emitted the dwell outcome"
            .to_owned()
    })?;
    let connected = dwell
        .get("connected_wall_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if connected <= DWELL_SURVIVAL_SECONDS {
        return Err(format!(
            "dwell connected_wall_seconds is {connected} (expected > {DWELL_SURVIVAL_SECONDS}): \
             the client did not survive past the server's {DWELL_SURVIVAL_SECONDS}s keepalive kick \
             limit"
        ));
    }
    let challenge_count = dwell
        .get("challenge_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if (challenge_count as usize) < DWELL_MIN_CHALLENGES {
        return Err(format!(
            "dwell challenge_count is {challenge_count} (expected >= {DWELL_MIN_CHALLENGES}): the \
             server did not keep issuing live keepalives across the window (its 1 s cadence over \
             >{DWELL_SURVIVAL_SECONDS}s implies at least {DWELL_MIN_CHALLENGES})"
        ));
    }
    let echo_count = dwell.get("echo_count").and_then(Value::as_u64).unwrap_or(0);
    if echo_count != challenge_count {
        return Err(format!(
            "dwell echo_count {echo_count} != challenge_count {challenge_count}: a client that \
             stops echoing keepalives would be kicked by the server, so survival requires every \
             challenge echoed"
        ));
    }
    if dwell.get("echo_relationship").and_then(Value::as_bool) != Some(true) {
        return Err(
            "dwell challenge->echo relationship is not 1:1 (echo_relationship is false): every \
             challenge must have exactly one matching echo"
                .to_owned(),
        );
    }
    let span = dwell
        .get("challenge_span_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if span < DWELL_MIN_SPAN_MS {
        return Err(format!(
            "dwell challenge_span_ms is {span} (expected >= {DWELL_MIN_SPAN_MS}): the challenges \
             did not span the full survival window"
        ));
    }
    if dwell
        .get("first_challenge_offset_ms")
        .and_then(Value::as_u64)
        .is_none()
    {
        return Err(
            "dwell has no first_challenge_offset_ms (absent or null): no keepalive challenge \
             arrived during the window"
                .to_owned(),
        );
    }
    Ok(
        "dwell (wall-clock survival past the 30 s keepalive kick limit with a 1:1 challenge->echo cadence)",
    )
}

/// The translation key Rivet's anti-cheat gate answers a NaN movement frame
/// with (`session.rs` `dispatch_move_player` → `disconnect_invalid_movement`:
/// `multiplayer.disconnect.invalid_player_movement`, issues #86/#158). The
/// `kick` scenario requires the real Azalea client to have decoded exactly this
/// key from the `ClientboundDisconnectPacket` Rivet encoded before closing.
pub const KICK_REASON_KEY: &str = "multiplayer.disconnect.invalid_player_movement";

/// Project a client run onto the canonical `kick` transcript.
///
/// The client's `disconnect` record carries the decoded disconnect reason: the
/// raw Debug rendering (`reason`), the translation key of a translatable reason
/// (`reason_key`), and whether the client had reached spawn (`after_spawn`).
/// This normalizer projects that record onto the canonical shape and attaches
/// the lifecycle and pinned Azalea revision, so the verdict can require the real
/// Azalea client decoded Rivet's exact invalid-movement key.
///
/// A `kick` run that never reaches the kick — a `timeout`/`connection_failed`
/// (the NaN frame never reached a server that answered), or a disconnect before
/// spawn — normalizes to those failure outcomes, minus the kick observables.
pub fn normalize_kick(raw: &str) -> Result<Value, String> {
    let records = parse_records(raw)?;

    let lifecycle: Vec<String> = records
        .iter()
        .filter_map(|r| r.get("event").and_then(Value::as_str))
        .filter(|e| LIFECYCLE_EVENTS.contains(e))
        .map(str::to_owned)
        .collect();

    let disconnect = records
        .iter()
        .find(|r| r.get("event") == Some(&json!("disconnect")))
        .cloned();
    let outcome = outcome(&records);
    check_outcome_terminal(&records, outcome)?;

    // The `starting` record carries the client binary's pinned Azalea build
    // revision; surface it so the verdict can prove the client was built from
    // the pinned unmodified revision.
    let azalea_revision = records
        .iter()
        .find(|r| r.get("event") == Some(&json!("starting")))
        .and_then(|r| r.get("azalea_revision").and_then(Value::as_str))
        .map(str::to_owned);

    let mut transcript = json!({
        "protocol": PROTOCOL,
        "scenario": "kick",
        "outcome": outcome,
        "lifecycle": lifecycle,
        "azalea_revision": azalea_revision,
        "excluded": serde_json::Map::new(),
    });

    if let Some(disconnect) = disconnect {
        transcript["kick"] = json!({
            "reason": disconnect.get("reason").cloned().unwrap_or(Value::Null),
            "reason_key": disconnect.get("reason_key").cloned().unwrap_or(Value::Null),
            "after_spawn": disconnect.get("after_spawn").cloned().unwrap_or(Value::Null),
        });
    }

    Ok(transcript)
}

/// Verify a normalized `kick` transcript proves the real Azalea client decoded
/// Rivet's exact disconnect reason after a genuine play session: outcome
/// `disconnected`, lifecycle containing both `login` and `spawn`, the pinned
/// Azalea revision, `after_spawn == true` (the anti-cheat gate answered a NaN
/// movement frame the client sent after it reached spawn), and
/// `reason_key == KICK_REASON_KEY` (the decoded translatable key from the
/// `ClientboundDisconnectPacket`).
///
/// Returns a human-readable description of the decoded-reason boundary reached.
/// Errors when the transcript does not prove that:
///
/// - the outcome is anything but `disconnected` — a `timeout`/`connection_failed`
///   means the NaN frame never reached a server that answered, and `spawned`
///   means the client never got kicked (a regression in the anti-cheat gate).
/// - the lifecycle does not contain both `login` and `spawn`.
/// - `azalea_revision != PINNED_AZALEA_REVISION` — the client was not built from
///   the pinned unmodified Azalea revision.
/// - `after_spawn != true` — the disconnect happened before the client reached
///   spawn, not via the play anti-cheat gate.
/// - `reason_key != KICK_REASON_KEY` — the decoded reason was not Rivet's
///   invalid-player-movement translatable (a literal/plain-text reason, a
///   different key, or no reason at all).
///
/// The companion server-side check (in the runner) is the `connection
/// established` log proving the client genuinely reached the Rivet port.
pub fn rivet_kick_verdict(t: &Value) -> Result<&'static str, String> {
    let outcome = t
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if outcome != "disconnected" {
        return Err(format!(
            "kick transcript outcome is {outcome} (expected disconnected): the client never \
             received the invalid-movement kick — a timeout/connection_failed means the NaN frame \
             never reached a server that answered, and spawned means the server never kicked"
        ));
    }
    let lifecycle: Vec<&str> = t
        .get("lifecycle")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if !lifecycle.contains(&"login") || !lifecycle.contains(&"spawn") {
        return Err(format!(
            "kick transcript lifecycle is {lifecycle:?} (expected to contain login and spawn) — \
             malformed transcript or the client never reached play"
        ));
    }
    if t.get("azalea_revision").and_then(Value::as_str) != Some(PINNED_AZALEA_REVISION) {
        return Err(format!(
            "kick transcript azalea_revision is {:?} (expected {PINNED_AZALEA_REVISION}) — the \
             client was not built from the pinned unmodified Azalea revision",
            t.get("azalea_revision").and_then(Value::as_str)
        ));
    }
    let kick = t.get("kick").and_then(Value::as_object).ok_or_else(|| {
        "kick transcript has no kick record — the client never emitted a disconnect".to_owned()
    })?;
    if kick.get("after_spawn").and_then(Value::as_bool) != Some(true) {
        return Err(format!(
            "kick after_spawn is {:?} (expected true): the disconnect happened before the client \
             reached spawn, not via the play anti-cheat gate",
            kick.get("after_spawn")
        ));
    }
    if kick.get("reason_key").and_then(Value::as_str) != Some(KICK_REASON_KEY) {
        return Err(format!(
            "kick reason_key is {:?} (expected {KICK_REASON_KEY}): the decoded disconnect reason \
             was not Rivet's invalid-player-movement translatable",
            kick.get("reason_key").and_then(Value::as_str)
        ));
    }
    Ok("disconnect (decoded multiplayer.disconnect.invalid_player_movement after spawn)")
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

    /// A genuine Rivet play run at HEAD: the pinned Azalea client completes
    /// login + configuration (registry sync), the play handoff, spawns at
    /// Rivet's fixed superflat spawn y=-63.0, and receives exactly the
    /// deterministic 117-chunk send-set.
    fn rivet_play_records() -> String {
        [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":40,"azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":0.0,"y":-63.0,"z":0.0},"protocol":1}"#,
            r#"{"event":"joined","position":{"x":0.0,"y":-63.0,"z":0.0},"world":"minecraft:overworld","gamemode":"survival","health":{"health":1.0,"food":20,"saturation":5.0},"experience":{"level":0,"progress":0.0,"total":0},"inventory":{"selected_slot":0,"items":[]},"chunk_count":117,"chunks":[[-5,-5],[-5,-4],[0,0],[5,5]],"observation_ms":4123,"protocol":1}"#,
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
                // The full-precision spawn position the client subtracted (the
                // `spawn` record for the fixture is x=9.5 z=-3.5).
                "spawn_origin": {"x": 9.5, "y": -60.0, "z": -3.5},
                // A plausible final sent position: the walk's last moving tick is
                // 19 ticks past the 100th sample (~+25 blocks from spawn).
                "last_sent": {"x": 25.0, "y": -60.0, "z": 0.0},
                "samples": samples,
                "teleports": [1],
                "teleport_acks": [1],
                // A keepalive challengeId from the stored observed boots
                // (client1.stdout.jsonl): Util.getMillis() = System.nanoTime()/1e6,
                // monotonic ms since the Paper JVM started.
                "keepalives": [266783496i64],
                "keepalive_echoes": [266783496i64],
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
        // The final sent position is recorded (copied verbatim) and compared —
        // `differing_last_sent_is_compared` pins that a differing last_sent is
        // surfaced as a compared diff, never an excluded diagnostic; the live
        // both-mode differential verifies Paper-vs-Rivet equality each run.
        assert_eq!(t["walk"]["last_sent"]["x"], json!(25.0));
        assert_eq!(t["walk"]["last_sent"]["y"], json!(-60.0));
        assert_eq!(t["walk"]["last_sent"]["z"], json!(0.0));
        assert!(t["excluded"].get("walk.last_sent").is_none());
        // The full-precision spawn origin is carried (so the harness can
        // reconstruct the absolute last_sent) but excluded from parity — the
        // walk is compared spawn-relative and Paper randomizes the origin.
        assert_eq!(
            t["walk"]["spawn_origin"],
            json!({"x": 9.5, "y": -60.0, "z": -3.5})
        );
        assert!(t["excluded"]["walk.spawn_origin"].is_string());
    }

    #[test]
    fn differing_last_sent_is_compared() {
        // The walk's final sent position is a *compared* field, promoted out of
        // the excluded nondeterministic set by the both-mode differential. This
        // test pins the comparator contract: a differing `last_sent` is surfaced
        // as a compared diff and must FAIL parity — it must never be absorbed by
        // an exclusion. Paper-vs-Rivet equality on X/Z is not proved here; the
        // live both-mode differential verifies it each run.
        let a = normalize_move(&move_records()).expect("normalize");
        // `move_records()` renders the `json!` walk compactly, so the
        // `last_sent` object is an exact substring to tamper.
        let b_raw = move_records().replace(
            r#""last_sent":{"x":25.0,"y":-60.0,"z":0.0}"#,
            r#""last_sent":{"x":26.5,"y":-60.0,"z":0.0}"#,
        );
        let b = normalize_move(&b_raw).expect("normalize");
        let d = super::super::comparator::diff(&a, &b);
        assert!(
            !d.is_identical(),
            "a differing compared last_sent must fail parity: {d:?}"
        );
        assert!(
            d.diffs.iter().any(|f| f.path == "walk.last_sent.x"),
            "the differing last_sent must be a compared diff: {d:?}"
        );
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
            "\"keepalive_echoes\":[266783496]",
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
    fn rivet_play_verdict_accepts_a_genuine_rivet_play_run() {
        // A genuine Rivet play run at HEAD: pinned Azalea completes login +
        // configuration (registry sync) + play handoff + spawn + the exact
        // 117-chunk send-set, at Rivet's fixed superflat spawn y=-63.0.
        let t = normalize_join(&rivet_play_records()).expect("normalize");
        let verdict = rivet_play_verdict(&t).expect("play verdict");
        assert!(
            verdict.contains("play"),
            "verdict must name the play boundary, got {verdict}"
        );
    }

    #[test]
    fn rivet_play_verdict_rejects_a_pre_play_disconnect() {
        // Counterfactual against a stale pre-play Rivet build (or any server
        // that never completes login/configuration): the server closes the
        // client at the login boundary, surfacing as `disconnected` — exactly
        // the outcome the old pre-play verifier accepted. The play verifier
        // must reject it.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":5,"azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"disconnect","reason":"Server closed the connection before login","after_spawn":false,"protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_join(&raw).expect("normalize");
        assert_eq!(t["outcome"], "disconnected");
        let err = rivet_play_verdict(&t).expect_err("disconnected is not play");
        assert!(
            err.contains("spawned"),
            "error must demand the spawned outcome, got {err}"
        );
    }

    #[test]
    fn rivet_play_verdict_rejects_connection_failed() {
        // `connection_failed` is azalea's `ConnectionFailedEvent`, sent only
        // when creating the connection fails (connect refused / first write
        // failed) — the client never reached the server.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":5,"azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"connection_failed","reason":"failed to create connection","protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_join(&raw).expect("normalize");
        assert!(
            rivet_play_verdict(&t).is_err(),
            "connection_failed must not be accepted: the client never completed a session"
        );
    }

    #[test]
    fn rivet_play_verdict_rejects_timeout() {
        // A hung/dead endpoint that never responds is a `timeout`, not proof of
        // play.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":5,"azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"timeout","timeout_seconds":5,"protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_join(&raw).expect("normalize");
        assert!(
            rivet_play_verdict(&t).is_err(),
            "timeout must not be accepted: the client never completed a session"
        );
    }

    #[test]
    fn rivet_play_verdict_rejects_a_fake_server_with_wrong_chunk_count() {
        // Counterfactual against a fake/non-Rivet server: outcome=spawned and
        // the right lifecycle, but not the deterministic 117-chunk send-set.
        let mut t = normalize_join(&rivet_play_records()).expect("normalize");
        t["chunk_count"] = json!(81);
        assert!(
            rivet_play_verdict(&t).is_err(),
            "a spawned transcript without 117 chunks is not a genuine Rivet play run"
        );
    }

    #[test]
    fn rivet_play_verdict_rejects_a_paper_like_spawn_height() {
        // Counterfactual against a non-Rivet (Paper-like) server: spawns the
        // player at y=-60.0, not Rivet's fixed superflat y=-63.0.
        let mut t = normalize_join(&rivet_play_records()).expect("normalize");
        t["position"]["y"] = json!(-60.0);
        assert!(
            rivet_play_verdict(&t).is_err(),
            "a spawn at Paper-like y=-60 is not Rivet's deterministic superflat spawn"
        );
    }

    #[test]
    fn rivet_play_verdict_rejects_a_wrong_azalea_revision() {
        // Counterfactual against a locally-modified or stale client binary:
        // the transcript was produced by a client not built from the pinned
        // unmodified Azalea revision.
        let mut t = normalize_join(&rivet_play_records()).expect("normalize");
        t["azalea_revision"] = json!("deadbeef");
        assert!(
            rivet_play_verdict(&t).is_err(),
            "a client not built from the pinned Azalea revision must not pass"
        );
    }

    #[test]
    fn rivet_play_verdict_rejects_a_spawned_transcript_missing_login() {
        // outcome=spawned without the login lifecycle event is malformed — the
        // lifecycle is the play-progress proof (login -> configuration -> spawn).
        let t = json!({
            "outcome": "spawned",
            "lifecycle": ["init", "spawn"],
            "azalea_revision": PINNED_AZALEA_REVISION,
            "chunk_count": JOIN_CHUNK_COUNT,
            "position": {"x": 0.0, "y": JOIN_SPAWN_Y, "z": 0.0},
        });
        assert!(
            rivet_play_verdict(&t).is_err(),
            "a spawned transcript without login is malformed"
        );
    }

    /// A genuine dwell-mode client run: spawned into play, stayed connected for
    /// 41 wall-clock seconds, echoed all 41 keepalive challenges (a monotonic
    /// per-boot millis id sequence), and emitted the dwell record.
    fn dwell_records() -> String {
        let ids: Vec<i64> = (1000..1041).collect();
        [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":50,"mode":"dwell","dwell_seconds":41,"azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":0.0,"y":-63.0,"z":0.0},"protocol":1}"#,
            &json!({
                "event": "dwell",
                "requested_dwell_seconds": 41,
                "connected_wall_seconds": 41.2,
                "challenge_count": ids.len(),
                "echo_count": ids.len(),
                "challenge_ids": ids,
                "echo_ids": ids,
                "first_challenge_offset_ms": 1200,
                "last_challenge_offset_ms": 41100,
                "challenge_span_ms": 39900,
                "protocol": 1,
            })
            .to_string(),
        ]
        .join("\n")
    }

    #[test]
    fn normalizes_a_successful_dwell() {
        let t = normalize_dwell(&dwell_records()).expect("normalize");
        assert_eq!(t["outcome"], "dwelled");
        assert_eq!(t["scenario"], "dwell");
        assert_eq!(t["lifecycle"], json!(["init", "login", "spawn"]));
        assert_eq!(t["dwell"]["requested_dwell_seconds"], json!(41));
        assert_eq!(t["dwell"]["connected_wall_seconds"], json!(41.2));
        assert_eq!(t["dwell"]["challenge_count"], json!(41));
        assert_eq!(t["dwell"]["echo_count"], json!(41));
        assert_eq!(t["dwell"]["echo_relationship"], json!(true));
        assert_eq!(t["dwell"]["challenge_span_ms"], json!(39900));
        // The raw ids are per-boot and excluded from parity but recorded as
        // diagnostics.
        assert!(t["dwell"]["challenge_ids"].is_array());
        assert!(t["excluded"]["dwell.challenge_ids"].is_string());
        assert!(t["excluded"]["dwell.echo_ids"].is_string());
    }

    #[test]
    fn dwell_echo_relationship_detects_a_missing_echo() {
        // Three challenges, but only two echoes: the multiset differs, so the
        // 1:1 relationship must report false (a client that stops echoing would
        // be kicked by the server).
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":50,"mode":"dwell","dwell_seconds":41,"azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":0.0,"y":-63.0,"z":0.0},"protocol":1}"#,
            r#"{"event":"dwell","requested_dwell_seconds":41,"connected_wall_seconds":41.2,"challenge_count":3,"echo_count":2,"challenge_ids":[1000,1001,1002],"echo_ids":[1000,1001],"first_challenge_offset_ms":1200,"last_challenge_offset_ms":40000,"challenge_span_ms":38800,"protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_dwell(&raw).expect("normalize");
        assert_eq!(t["dwell"]["echo_relationship"], json!(false));
        assert_eq!(t["dwell"]["challenge_count"], json!(3));
        assert_eq!(t["dwell"]["echo_count"], json!(2));
    }

    #[test]
    fn failed_dwell_normalizes_without_dwell_observables() {
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":5,"mode":"dwell","dwell_seconds":41,"azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"disconnect","reason":"read timeout","after_spawn":true,"protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_dwell(&raw).expect("normalize");
        assert_eq!(t["outcome"], "disconnected");
        assert!(t.get("dwell").is_none());
    }

    #[test]
    fn dwell_verdict_accepts_a_genuine_survival_run() {
        let t = normalize_dwell(&dwell_records()).expect("normalize");
        let verdict = rivet_dwell_verdict(&t).expect("dwell verdict");
        assert!(
            verdict.contains("survival"),
            "verdict must name the survival boundary, got {verdict}"
        );
    }

    #[test]
    fn dwell_verdict_rejects_a_kicked_client() {
        // A client kicked by the server (read timeout) emits a terminal before
        // the dwell record — survival never completed.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":5,"mode":"dwell","dwell_seconds":41,"azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":0.0,"y":-63.0,"z":0.0},"protocol":1}"#,
            r#"{"event":"disconnect","reason":"read timeout","after_spawn":true,"protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_dwell(&raw).expect("normalize");
        assert_eq!(t["outcome"], "disconnected");
        let err = rivet_dwell_verdict(&t).expect_err("a kicked client did not survive");
        assert!(
            err.contains("dwelled"),
            "error must demand the dwelled outcome, got {err}"
        );
    }

    #[test]
    fn dwell_verdict_rejects_short_connected_window() {
        // A client that disconnected at exactly the 30 s boundary (or a transcript
        // claiming less) did not survive *past* the kick limit.
        let mut t = normalize_dwell(&dwell_records()).expect("normalize");
        t["dwell"]["connected_wall_seconds"] = json!(30.0);
        let err = rivet_dwell_verdict(&t).expect_err("30s is not survival past the limit");
        assert!(
            err.contains("connected_wall_seconds"),
            "error must name the short window, got {err}"
        );
    }

    #[test]
    fn dwell_verdict_rejects_a_missing_challenge_span() {
        // Challenges clustered at the start of the window (no span) cannot prove
        // the server kept issuing live keepalives across the survival window.
        let mut t = normalize_dwell(&dwell_records()).expect("normalize");
        t["dwell"]["challenge_span_ms"] = json!(5000);
        let err = rivet_dwell_verdict(&t).expect_err("span below the window");
        assert!(
            err.contains("challenge_span_ms"),
            "error must name the span, got {err}"
        );
    }

    #[test]
    fn dwell_verdict_rejects_a_null_first_challenge_offset() {
        // The normalizer emits `first_challenge_offset_ms` as null when the raw
        // record carries no first offset (no keepalive challenge ever arrived),
        // so the key is present-but-null — the verdict guard must reject that
        // value rather than only checking key absence (which the normalizer's
        // explicit null always satisfies). The span stays valid here so it is
        // this offset guard, not the span guard, that must refuse PASS.
        let mut t = normalize_dwell(&dwell_records()).expect("normalize");
        t["dwell"]["first_challenge_offset_ms"] = Value::Null;
        let err = rivet_dwell_verdict(&t).expect_err("null first offset must fail the verdict");
        assert!(
            err.contains("first_challenge_offset_ms"),
            "error must name the missing first offset, got {err}"
        );
    }

    #[test]
    fn normalize_dwell_emits_null_when_the_first_offset_is_absent() {
        // The verdict guards on the *value* of `first_challenge_offset_ms`, so
        // the normalizer's absent→null projection is load-bearing: a raw record
        // with no first offset must normalize to an explicit null (not leave the
        // key out) that the value-aware verdict guard can then reject. (The
        // span also normalizes to null here, since it needs both offsets, so a
        // raw-absent offset can never silently slip past the verdict.)
        let raw = dwell_raw_with(
            r#""requested_dwell_seconds":41,"connected_wall_seconds":41.2,"challenge_count":2,"echo_count":2,"challenge_ids":[1000,1001],"echo_ids":[1000,1001],"last_challenge_offset_ms":41100,"challenge_span_ms":39900"#,
        );
        let t = normalize_dwell(&raw).expect("normalize");
        assert_eq!(t["dwell"]["first_challenge_offset_ms"], Value::Null);
        assert!(
            rivet_dwell_verdict(&t).is_err(),
            "a transcript with no first challenge offset must not pass"
        );
    }

    #[test]
    fn dwell_verdict_rejects_an_unanswered_challenge() {
        // echo_count != challenge_count: a client that stops echoing would be
        // kicked, so a mismatch is a survival violation.
        let mut t = normalize_dwell(&dwell_records()).expect("normalize");
        t["dwell"]["echo_count"] = json!(40);
        t["dwell"]["echo_relationship"] = json!(false);
        let err = rivet_dwell_verdict(&t).expect_err("unanswered challenge");
        assert!(
            err.contains("echo_count"),
            "error must name the echo mismatch, got {err}"
        );
    }

    #[test]
    fn dwell_verdict_rejects_a_wrong_azalea_revision() {
        let mut t = normalize_dwell(&dwell_records()).expect("normalize");
        t["azalea_revision"] = json!("deadbeef");
        assert!(
            rivet_dwell_verdict(&t).is_err(),
            "a client not built from the pinned Azalea revision must not pass"
        );
    }

    /// A dwell-mode raw stream with the given dwell-record body overridden.
    /// Used by the counterfactuals below to mutate the duplicated count/span
    /// fields a malformed client could declare.
    fn dwell_raw_with(body: &str) -> String {
        [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":50,"mode":"dwell","dwell_seconds":41,"azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":0.0,"y":-63.0,"z":0.0},"protocol":1}"#,
            &format!("{{\"event\":\"dwell\",{body},\"protocol\":1}}"),
        ]
        .join("\n")
    }

    #[test]
    fn normalize_dwell_derives_counts_from_the_ids_arrays() {
        // A malformed record declaring counts that contradict its own arrays
        // must not shape the transcript: the canonical counts come from the
        // challenge_ids/echo_ids lengths (41 here), never from the duplicated
        // count fields a buggy or lying client could set.
        let raw = dwell_raw_with(
            r#""requested_dwell_seconds":41,"connected_wall_seconds":41.2,"challenge_count":999,"echo_count":0,"challenge_ids":[1000,1001],"echo_ids":[1000,1001],"first_challenge_offset_ms":1200,"last_challenge_offset_ms":41100,"challenge_span_ms":39900"#,
        );
        let t = normalize_dwell(&raw).expect("normalize");
        assert_eq!(t["dwell"]["challenge_count"], json!(2));
        assert_eq!(t["dwell"]["echo_count"], json!(2));
        assert_eq!(t["dwell"]["echo_relationship"], json!(true));
    }

    #[test]
    fn normalize_dwell_derives_span_from_the_offset_pair() {
        // The record's declared challenge_span_ms is a redundant duplicate of
        // last_offset - first_offset; a record declaring a bogus span must not
        // be trusted — the canonical span is recomputed from the offsets.
        let raw = dwell_raw_with(
            r#""requested_dwell_seconds":41,"connected_wall_seconds":41.2,"challenge_count":2,"echo_count":2,"challenge_ids":[1000,1001],"echo_ids":[1000,1001],"first_challenge_offset_ms":1200,"last_challenge_offset_ms":41100,"challenge_span_ms":99999"#,
        );
        let t = normalize_dwell(&raw).expect("normalize");
        assert_eq!(t["dwell"]["challenge_span_ms"], json!(39900));
    }

    #[test]
    fn normalize_dwell_spans_null_when_offsets_are_inverted() {
        // A corrupt/out-of-order stream whose last offset precedes the first
        // cannot yield a span; the transcript carries null and the verdict
        // refuses PASS rather than trusting a declared span.
        let raw = dwell_raw_with(
            r#""requested_dwell_seconds":41,"connected_wall_seconds":41.2,"challenge_count":2,"echo_count":2,"challenge_ids":[1000,1001],"echo_ids":[1000,1001],"first_challenge_offset_ms":5000,"last_challenge_offset_ms":1000,"challenge_span_ms":99999"#,
        );
        let t = normalize_dwell(&raw).expect("normalize");
        assert_eq!(t["dwell"]["challenge_span_ms"], Value::Null);
        assert!(
            rivet_dwell_verdict(&t).is_err(),
            "an inverted offset pair must fail the span verdict"
        );
    }

    /// A genuine kick-mode client run: the pinned Azalea client spawned into
    /// play, sent a NaN movement frame, and was disconnected by Rivet's
    /// anti-cheat gate. The disconnect record carries the decoded translatable
    /// reason's key.
    fn kick_records() -> String {
        [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":40,"mode":"kick","azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":0.0,"y":-63.0,"z":0.0},"protocol":1}"#,
            r#"{"event":"disconnect","reason":"Some(Translatable(TranslatableComponent { key: \"multiplayer.disconnect.invalid_player_movement\", .. }))","reason_key":"multiplayer.disconnect.invalid_player_movement","after_spawn":true,"protocol":1}"#,
        ]
        .join("\n")
    }

    #[test]
    fn normalizes_a_successful_kick() {
        let t = normalize_kick(&kick_records()).expect("normalize");
        assert_eq!(t["outcome"], "disconnected");
        assert_eq!(t["scenario"], "kick");
        assert_eq!(t["lifecycle"], json!(["init", "login", "spawn"]));
        assert_eq!(
            t["kick"]["reason_key"],
            json!("multiplayer.disconnect.invalid_player_movement")
        );
        assert_eq!(t["kick"]["after_spawn"], json!(true));
        // The raw Debug rendering is carried as a diagnostic but never
        // verdict-checked (only the decoded key is).
        assert!(t["kick"]["reason"].is_string());
    }

    #[test]
    fn kick_verdict_accepts_a_genuine_kick_run() {
        let t = normalize_kick(&kick_records()).expect("normalize");
        let verdict = rivet_kick_verdict(&t).expect("kick verdict");
        assert!(
            verdict.contains("disconnect"),
            "verdict must name the decoded-reason boundary, got {verdict}"
        );
    }

    #[test]
    fn kick_verdict_rejects_a_wrong_reason_key() {
        // The negative control: a decoded reason that is not Rivet's
        // invalid-player-movement key (a literal/plain-text reason, a different
        // key, or no reason) must refuse PASS — this is what proves the verdict
        // actually checks the decoded reason and cannot be waved through by a
        // transcript that never decoded the real key.
        let mut t = normalize_kick(&kick_records()).expect("normalize");
        t["kick"]["reason_key"] = json!("disconnect.genericReason");
        let err = rivet_kick_verdict(&t).expect_err("wrong reason key");
        assert!(
            err.contains("reason_key"),
            "error must name the reason key, got {err}"
        );
    }

    #[test]
    fn kick_verdict_rejects_a_null_reason_key() {
        // A disconnect with no decodable translatable reason (reason_key null:
        // a plain-text/literal reason) must not pass — the client did not decode
        // Rivet's translatable key.
        let mut t = normalize_kick(&kick_records()).expect("normalize");
        t["kick"]["reason_key"] = Value::Null;
        assert!(
            rivet_kick_verdict(&t).is_err(),
            "a null reason_key means the reason was not decoded as translatable"
        );
    }

    #[test]
    fn kick_verdict_rejects_a_pre_spawn_disconnect() {
        // after_spawn=false: the disconnect record claims the client never
        // reached spawn, so it is not a play anti-cheat kick. The transcript
        // still passes the earlier structural checks (outcome disconnected,
        // lifecycle login+spawn, pinned revision) so the after_spawn check is
        // the discriminating one — a client that reached spawn would record
        // after_spawn=true.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":5,"mode":"kick","azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":0.0,"y":-63.0,"z":0.0},"protocol":1}"#,
            r#"{"event":"disconnect","reason":"Server closed the connection before login","reason_key":null,"after_spawn":false,"protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_kick(&raw).expect("normalize");
        assert_eq!(t["outcome"], "disconnected");
        let err = rivet_kick_verdict(&t).expect_err("pre-spawn disconnect is not a kick");
        assert!(
            err.contains("after_spawn"),
            "error must name after_spawn, got {err}"
        );
    }

    #[test]
    fn kick_verdict_rejects_a_timeout() {
        // The server never kicks (a regression in the anti-cheat gate or the
        // disconnect path): the client times out without ever emitting a
        // disconnect terminal.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":5,"mode":"kick","azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":0.0,"y":-63.0,"z":0.0},"protocol":1}"#,
            r#"{"event":"timeout","timeout_seconds":5,"protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_kick(&raw).expect("normalize");
        assert_eq!(t["outcome"], "timeout");
        let err = rivet_kick_verdict(&t).expect_err("timeout is not a kick");
        assert!(
            err.contains("disconnected"),
            "error must demand the disconnected outcome, got {err}"
        );
    }

    #[test]
    fn kick_verdict_rejects_a_wrong_azalea_revision() {
        let mut t = normalize_kick(&kick_records()).expect("normalize");
        t["azalea_revision"] = json!("deadbeef");
        assert!(
            rivet_kick_verdict(&t).is_err(),
            "a client not built from the pinned Azalea revision must not pass"
        );
    }

    #[test]
    fn failed_kick_normalizes_without_kick_observables() {
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25599","username":"RivetProbe","timeout_seconds":5,"mode":"kick","azalea_revision":"x","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"connection_failed","reason":"failed to create connection","protocol":1}"#,
        ]
        .join("\n");
        let t = normalize_kick(&raw).expect("normalize");
        assert_eq!(t["outcome"], "connection_failed");
        assert_eq!(t["lifecycle"], json!(["init"]));
        assert!(t.get("kick").is_none());
    }
}
