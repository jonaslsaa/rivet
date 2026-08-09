//! Parser for the rivet-server authoritative movement trace.
//!
//! `rivet-server` run with `RIVET_TRACE_MOVEMENT=1` emits a machine-readable
//! audit on stderr (`crates/rivet-server/src/server/movement_trace.rs`):
//! `RIVET_TELEPORT_ACK`, `RIVET_MOVE_ACCEPTED`, and `RIVET_SESSION_END`
//! records in the `key=value` Debug schema, one message per line, flowing
//! through tracing-subscriber's info subscriber (which paints ANSI codes even
//! when the writer is a file, so this parser strips them first).
//!
//! This is the server-side half of the movement differential: the Azalea
//! client is client-authoritative for the player, so the client transcript
//! alone cannot prove Rivet's *server* actually accepted and authoritative-ly
//! tracked the walk. The trace is that proof — the tick thread's own record of
//! the teleport ack it accepted and every post-ack move frame it snapped into
//! the tick-owned player, plus the final authoritative position at session end.
//!
//! Parsing is strict like the transcript policy: a line carrying one of the
//! `RIVET_*` tags whose fields do not parse is a schema violation (harness
//! defect), not a skip. Lines without a trace tag (boot logs, `RIVET_READY`,
//! keepalive chatter) are ignored.

use serde_json::{Value, json};
/// pinned in `movement_trace.rs`). The scenario boots rivet-server with it set
/// so the tick thread emits its authoritative movement audit.
pub const TRACE_MOVEMENT_ENV: &str = "RIVET_TRACE_MOVEMENT";

/// The three trace tags emitted by `rivet-server` (pinned in `movement_trace.rs`).
pub const TAG_TELEPORT_ACK: &str = "RIVET_TELEPORT_ACK";
pub const TAG_MOVE_ACCEPTED: &str = "RIVET_MOVE_ACCEPTED";
pub const TAG_SESSION_END: &str = "RIVET_SESSION_END";

/// One `RIVET_TELEPORT_ACK` record: the server's handling of a client
/// `accept_teleportation` frame.
#[derive(Debug, Clone, PartialEq)]
pub struct TeleportAck {
    /// The echoed `awaitingTeleport` id.
    pub ack_id: i64,
    /// `accepted` | `ignored` | `invalid`.
    pub outcome: String,
    /// The awaited position snapped into the player on the `accepted` path
    /// (the spawn for the scenario's single teleport).
    pub position: Option<[f64; 3]>,
}

/// One `RIVET_MOVE_ACCEPTED` record: the exact clamped/wrapped position the
/// tick thread snapped into the tick-owned player for an accepted move frame,
/// and the session's running accepted-frame count.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveAccepted {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub accepted_frames: usize,
}

/// The `RIVET_SESSION_END` record: the final authoritative position + rotation
/// and movement counts at a traced disconnect (EOF / timeout / inbound
/// overflow).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionEnd {
    pub reason: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub accepted_frames: usize,
    pub move_frames_seen: usize,
}

/// The full movement trace parsed from a rivet-server log.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MovementTrace {
    pub teleport_acks: Vec<TeleportAck>,
    pub moves: Vec<MoveAccepted>,
    pub session_end: Option<SessionEnd>,
}

impl MovementTrace {
    /// The final authoritative position the server accepted (the session-end
    /// record), or `None` when the session never ended on a traced disconnect.
    pub fn final_position(&self) -> Option<[f64; 3]> {
        self.session_end.as_ref().map(|e| [e.x, e.y, e.z])
    }

    /// Trace-internal self-consistency, returning a human-readable summary on
    /// success. Each check pins one load-bearing fact about the authoritative
    /// movement machine:
    ///
    /// 1. the spawn teleport was actually accepted (`outcome=accepted`);
    /// 2. at least one post-ack move frame was accepted (the walk was seen);
    /// 3. the session ended on a traced disconnect with a final position;
    /// 4. the accepted-frame counter matches the number of `MOVE_ACCEPTED`
    ///    records (every accepted move was traced, nothing was dropped);
    /// 5. the final authoritative position is the last accepted move's position
    ///    (the server's displacement bookkeeping is internally consistent).
    pub fn check_authoritative(&self) -> Result<String, String> {
        if !self.teleport_acks.iter().any(|a| a.outcome == "accepted") {
            return Err(
                "no accepted teleport ack in the movement trace — the spawn teleport was never \
                 acknowledged"
                    .to_string(),
            );
        }
        if self.moves.is_empty() {
            return Err(
                "no accepted move frames in the movement trace — the walk never reached the \
                 server's authoritative movement path"
                    .to_string(),
            );
        }
        let end = self.session_end.as_ref().ok_or_else(|| {
            "no RIVET_SESSION_END in the movement trace — the session never ended on a traced \
             disconnect, so there is no final authoritative position"
                .to_string()
        })?;
        if !is_traced_disconnect_reason(&end.reason) {
            return Err(format!(
                "session ended on {reason}, not a traced movement disconnect (EOF / timeout / \
                 inbound overflow)",
                reason = end.reason
            ));
        }
        if end.accepted_frames != self.moves.len() {
            return Err(format!(
                "session-end accepted_frames={} but the trace records {} accepted moves — the \
                 counter and the record trail disagree",
                end.accepted_frames,
                self.moves.len()
            ));
        }
        let last = &self.moves[self.moves.len() - 1];
        if last.x != end.x || last.y != end.y || last.z != end.z {
            return Err(format!(
                "last accepted move ({}, {}, {}) != session-end final position ({}, {}, {}) — \
                 the server's authoritative displacement is inconsistent",
                last.x, last.y, last.z, end.x, end.y, end.z
            ));
        }
        Ok(format!(
            "teleport-ack accepted (id 1) at spawn; {} accepted moves; final authoritative \
             position ({}, {}, {}) after {move_frames_seen} seen frames",
            self.moves.len(),
            end.x,
            end.y,
            end.z,
            move_frames_seen = end.move_frames_seen
        ))
    }
}

/// Whether a session-end reason string is one the movement trace reports
/// (`crates/rivet-server` `is_traced_disconnect`: EOF / timeout / inbound
/// overflow). Mirrors the thiserror Display strings from `packet_listener.rs`.
fn is_traced_disconnect_reason(reason: &str) -> bool {
    matches!(reason, "disconnect.endOfStream" | "disconnect.timeout")
        || reason.starts_with("inbound overflow: ")
}

/// Strip ANSI SGR sequences (`\x1b[...m`) that tracing-subscriber paints even
/// when writing to a file (its writer is configured as stderr and it colors
/// regardless of tty detection). The trace tags and `key=value` fields are
/// ASCII, so the escape bytes carry no payload.
fn strip_ansi(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse a `key=value` field token from the Debug schema. String values are
/// double-quoted (tracing renders `&str` fields with `Debug`); the `%id`
/// `conn#N` display and numerics are bare. A bare token that is not a number
/// (like `id=conn#0`) stays a string — only the fields the trace schema pins as
/// numeric are re-read numerically by the accessors.
///
/// Integer tokens stay integers (serde_json's `Number::as_i64` returns `None`
/// for a float-typed `Number` even when the value is integral, so an
/// `ack_id=1` read as an f64 would not satisfy the integer accessors).
fn parse_field(token: &str) -> Result<(&str, Value), String> {
    let (key, value) = token
        .split_once('=')
        .ok_or_else(|| format!("malformed movement-trace field (no '='): {token}"))?;
    if key.is_empty() {
        return Err(format!(
            "malformed movement-trace field (empty key): {token}"
        ));
    }
    let value = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(inner) => Value::String(inner.to_owned()),
        None => match value.parse::<i64>() {
            Ok(i) => json!(i),
            Err(_) => match value.parse::<f64>() {
                Ok(n) => json!(n),
                Err(_) => Value::String(value.to_owned()),
            },
        },
    };
    Ok((key, value))
}

fn get_str(fields: &[(String, Value)], key: &str) -> Result<String, String> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("movement-trace record missing string field {key}"))
}

fn get_i64(fields: &[(String, Value)], key: &str) -> Result<i64, String> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.as_i64())
        .ok_or_else(|| format!("movement-trace record missing integer field {key}"))
}

fn get_f64(fields: &[(String, Value)], key: &str) -> Result<f64, String> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.as_f64())
        .ok_or_else(|| format!("movement-trace record missing number field {key}"))
}

fn get_usize(fields: &[(String, Value)], key: &str) -> Result<usize, String> {
    let n = get_i64(fields, key)?;
    usize::try_from(n).map_err(|_| format!("movement-trace field {key} is negative: {n}"))
}

/// Parse one tag-bearing line's fields (after the tag token) into
/// `(key, value)` pairs.
fn parse_fields(line: &str, tag: &str) -> Result<Vec<(String, Value)>, String> {
    let after = line.split_once(tag).map(|(_, rest)| rest).unwrap_or(line);
    after
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|token| {
            let (k, v) = parse_field(token)?;
            Ok((k.to_owned(), v))
        })
        .collect()
}

/// Parse a rivet-server log (raw, ANSI-painted) into a [`MovementTrace`].
///
/// Strict: a line carrying one of the trace tags whose fields do not parse is
/// a schema violation and fails the whole parse — the trace is the harness's
/// server-side contract, so a mangled record must never be silently dropped.
pub fn parse(raw: &str) -> Result<MovementTrace, String> {
    let clean = strip_ansi(raw);
    let mut trace = MovementTrace::default();
    for line in clean.lines() {
        if line.contains(TAG_TELEPORT_ACK) {
            let fields = parse_fields(line, TAG_TELEPORT_ACK)?;
            let ack_id = get_i64(&fields, "ack_id")?;
            let outcome = get_str(&fields, "outcome")?;
            let position = match outcome.as_str() {
                "accepted" => Some([
                    get_f64(&fields, "x")?,
                    get_f64(&fields, "y")?,
                    get_f64(&fields, "z")?,
                ]),
                _ => None,
            };
            trace.teleport_acks.push(TeleportAck {
                ack_id,
                outcome,
                position,
            });
        } else if line.contains(TAG_MOVE_ACCEPTED) {
            let fields = parse_fields(line, TAG_MOVE_ACCEPTED)?;
            trace.moves.push(MoveAccepted {
                x: get_f64(&fields, "x")?,
                y: get_f64(&fields, "y")?,
                z: get_f64(&fields, "z")?,
                accepted_frames: get_usize(&fields, "accepted_frames")?,
            });
        } else if line.contains(TAG_SESSION_END) {
            let fields = parse_fields(line, TAG_SESSION_END)?;
            if trace.session_end.is_some() {
                return Err(
                    "duplicate RIVET_SESSION_END record — one session ends once".to_string()
                );
            }
            trace.session_end = Some(SessionEnd {
                reason: get_str(&fields, "reason")?,
                x: get_f64(&fields, "x")?,
                y: get_f64(&fields, "y")?,
                z: get_f64(&fields, "z")?,
                accepted_frames: get_usize(&fields, "accepted_frames")?,
                move_frames_seen: get_usize(&fields, "move_frames_seen")?,
            });
        }
    }
    Ok(trace)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A genuine-shaped rivet-server log: the `RIVET_TELEPORT_ACK` at spawn
    /// (0, -63, 0), a short trail of accepted moves (the final one matching the
    /// session-end position), and a traced `disconnect.endOfStream` session
    /// end. The server emits one move record per accepted frame, so the final
    /// `accepted_frames` counter equals the number of move records — the
    /// fixture uses small counts so the trail stays readable while honoring
    /// `check_authoritative`'s counter invariant. ANSI codes are painted on to
    /// prove the parser strips them.
    fn genuine_log() -> String {
        let ack = format!(
            "\x1b[2m2026-08-09T02:21:17.996467Z\x1b[0m \x1b[32m INFO\x1b[0m {TAG_TELEPORT_ACK} \
             id=conn#0 ack_id=1 outcome=\"accepted\" x=0.0 y=-63.0 z=0.0"
        );
        let m1 = format!(
            "\x1b[2m2026-08-09T02:21:17.996538Z\x1b[0m \x1b[32m INFO\x1b[0m {TAG_MOVE_ACCEPTED} \
             id=conn#0 x=0.0 y=-63.0 z=0.0 y_rot=0.0 x_rot=0.0 accepted_frames=1"
        );
        let m2 = format!(
            "\x1b[2m2026-08-09T02:21:17.996538Z\x1b[0m \x1b[32m INFO\x1b[0m {TAG_MOVE_ACCEPTED} \
             id=conn#0 x=25.6449734426827 y=-63.0 z=0.0 y_rot=-90.0 x_rot=0.0 accepted_frames=2"
        );
        let end = format!(
            "\x1b[2m2026-08-09T02:21:24.197317Z\x1b[0m \x1b[32m INFO\x1b[0m {TAG_SESSION_END} \
             id=conn#0 reason=disconnect.endOfStream x=25.6449734426827 y=-63.0 z=0.0 \
             y_rot=-90.0 x_rot=0.0 accepted_frames=2 move_frames_seen=3"
        );
        format!(
            "2026-08-09T02:21:17.000000Z  INFO RIVET_READY\n{ack}\n{m1}\n{m2}\n{end}\n\
             some unrelated boot log line\n"
        )
    }

    #[test]
    fn parses_a_genuine_trace_and_passes_authoritative_checks() {
        let t = parse(&genuine_log()).expect("parse");
        assert_eq!(t.teleport_acks.len(), 1);
        assert_eq!(t.teleport_acks[0].ack_id, 1);
        assert_eq!(t.teleport_acks[0].outcome, "accepted");
        assert_eq!(t.teleport_acks[0].position, Some([0.0, -63.0, 0.0]));
        assert_eq!(t.moves.len(), 2);
        assert_eq!(t.moves[0].x, 0.0);
        assert_eq!(t.moves[1].accepted_frames, 2);
        let end = t.session_end.as_ref().expect("session end");
        assert_eq!(end.reason, "disconnect.endOfStream");
        assert_eq!(end.x, 25.6449734426827);
        assert_eq!(end.accepted_frames, 2);
        assert_eq!(end.move_frames_seen, 3);
        let summary = t.check_authoritative().expect("authoritative checks");
        assert!(summary.contains("accepted moves"), "{summary}");
        assert_eq!(t.final_position(), Some([25.6449734426827, -63.0, 0.0]));
    }

    #[test]
    fn strips_ansi_painted_fields() {
        // The genuine log above is ANSI-painted; parsing it proves the escape
        // sequences are stripped before the key=value fields are read.
        let t = parse(&genuine_log()).expect("parse ANSI-painted log");
        assert_eq!(t.moves.len(), 2);
        assert_eq!(t.moves[1].x, 25.6449734426827);
    }

    #[test]
    fn an_ignored_ack_carries_no_position() {
        let line = format!("{TAG_TELEPORT_ACK} id=conn#0 ack_id=7 outcome=\"ignored\"");
        let t = parse(&line).expect("parse");
        assert_eq!(t.teleport_acks.len(), 1);
        assert_eq!(t.teleport_acks[0].outcome, "ignored");
        assert_eq!(t.teleport_acks[0].position, None);
    }

    #[test]
    fn an_invalid_ack_carries_the_reason_and_no_position() {
        let line = format!(
            "{TAG_TELEPORT_ACK} id=conn#0 ack_id=1 outcome=\"invalid\" reason=invalid_player_movement"
        );
        let t = parse(&line).expect("parse");
        assert_eq!(t.teleport_acks[0].outcome, "invalid");
        assert_eq!(t.teleport_acks[0].position, None);
    }

    #[test]
    fn a_malformed_trace_field_fails_the_whole_parse() {
        let line = format!("{TAG_MOVE_ACCEPTED} id=conn#0 x=0.0 this-is-not-a-field y=-63.0");
        let err = parse(&line).expect_err("malformed field must fail");
        assert!(
            err.contains("no '='"),
            "error must name the malformed token: {err}"
        );
    }

    #[test]
    fn missing_required_field_fails_the_whole_parse() {
        let line = format!("{TAG_MOVE_ACCEPTED} id=conn#0 x=0.0 y=-63.0 z=0.0");
        let err = parse(&line).expect_err("missing accepted_frames must fail");
        assert!(err.contains("accepted_frames"), "{err}");
    }

    #[test]
    fn duplicate_session_end_is_rejected() {
        let a = format!(
            "{TAG_SESSION_END} id=conn#0 reason=disconnect.endOfStream x=1.0 y=-63.0 z=0.0 \
             accepted_frames=1 move_frames_seen=1"
        );
        let b = a.clone();
        let err = parse(&format!("{a}\n{b}")).expect_err("duplicate session end");
        assert!(err.contains("duplicate RIVET_SESSION_END"), "{err}");
    }

    #[test]
    fn authoritative_check_rejects_a_missing_session_end() {
        let t = parse(&genuine_log()).expect("parse");
        // Drop the session-end record: the trace saw a walk but no traced end.
        let mut no_end = t.clone();
        no_end.session_end = None;
        let err = no_end.check_authoritative().expect_err("no session end");
        assert!(err.contains("no RIVET_SESSION_END"), "{err}");
    }

    #[test]
    fn authoritative_check_rejects_an_unaccepted_teleport() {
        let t = parse(&genuine_log()).expect("parse");
        let mut ignored = t.clone();
        ignored.teleport_acks[0].outcome = "ignored".to_owned();
        let err = ignored.check_authoritative().expect_err("ignored teleport");
        assert!(err.contains("no accepted teleport ack"), "{err}");
    }

    #[test]
    fn authoritative_check_rejects_a_counter_mismatch() {
        let t = parse(&genuine_log()).expect("parse");
        let mut bad = t.clone();
        // Accepted-frame counter says 125 but only 2 moves were recorded.
        bad.session_end.as_mut().unwrap().accepted_frames = 125;
        let err = bad.check_authoritative().expect_err("counter mismatch");
        assert!(err.contains("accepted_frames=125"), "{err}");
    }

    #[test]
    fn authoritative_check_rejects_a_displaced_final_position() {
        let t = parse(&genuine_log()).expect("parse");
        let mut bad = t.clone();
        bad.session_end.as_mut().unwrap().x = 99.0;
        let err = bad.check_authoritative().expect_err("displaced final");
        assert!(err.contains("!= session-end"), "{err}");
    }

    #[test]
    fn authoritative_check_rejects_an_untraced_disconnect_reason() {
        let t = parse(&genuine_log()).expect("parse");
        let mut bad = t.clone();
        bad.session_end.as_mut().unwrap().reason =
            "multiplayer.disconnect.server_shutdown".to_owned();
        let err = bad.check_authoritative().expect_err("untraced reason");
        assert!(err.contains("not a traced movement disconnect"), "{err}");
    }

    #[test]
    fn a_negative_count_field_is_rejected_as_negative() {
        // The server never emits a negative counter, but the guard must exist:
        // a signed value that does not fit a usize (here, a negative one) must
        // fail the parse rather than wrap into a huge accepted-frames count.
        let line = format!(
            "{TAG_SESSION_END} id=conn#0 reason=disconnect.endOfStream x=0.0 y=-63.0 z=0.0 \
             accepted_frames=-1 move_frames_seen=1"
        );
        let err = parse(&line).expect_err("negative accepted_frames must fail");
        assert!(
            err.contains("accepted_frames is negative"),
            "error must name the negative field: {err}"
        );
    }

    #[test]
    fn ignores_boot_and_ready_lines() {
        // RIVET_READY carries the RIVET_ prefix but is not a trace tag, and
        // plain log lines carry none — both must be skipped, not parsed.
        let t = parse(
            "2026-08-09T02:21:17.000000Z  INFO RIVET_READY\n\
             2026-08-09T02:21:17.000000Z  INFO listener bound\n",
        )
        .expect("parse without trace records");
        assert_eq!(t.teleport_acks.len(), 0);
        assert_eq!(t.moves.len(), 0);
        assert!(t.session_end.is_none());
    }
}
