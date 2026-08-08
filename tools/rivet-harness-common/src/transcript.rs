//! Strict JSONL transcript policies for the harness client protocols.
//!
//! The harness clients (`rivet-client`) emit a JSON-lines stream on stdout,
//! and the capture fixtures are JSON-lines files. Every consumer must apply
//! the same strict policy, which this module splits into two std-only,
//! caller-generic steps (the crate stays dependency-free — the caller already
//! has its JSON parser):
//!
//! 1. **malformed-line rejection** — [`parse_lines`]: non-blank lines must all
//!    parse; a truncated or mangled record fails the whole parse instead of
//!    being silently skipped;
//! 2. **terminal policy** — [`check_terminal`]: a stream reporting its
//!    terminal record (`joined`, `moved`) twice is corrupt (the client exits
//!    after its single terminal), and a stream that never reports it did not
//!    complete the observable outcome. Both are hard failures.
//!
//! The capture fixture reader uses only step 1 (its format has no terminal
//! record); the join/move client transcripts use both steps plus the
//! consumer's own protocol-version check.

/// Split a raw stream into candidate lines, applying the fixture line policy:
/// blank lines are skipped, everything else must parse.
pub fn lines(raw: &str) -> impl Iterator<Item = &str> {
    raw.lines().filter(|line| !line.trim().is_empty())
}

/// Strictly parse each non-blank line with `parse_line`; any error fails the
/// whole parse (a malformed record is a harness/protocol bug, never a skip).
pub fn parse_lines<V>(
    raw: &str,
    parse_line: impl Fn(&str) -> Result<V, String>,
) -> Result<Vec<V>, String> {
    lines(raw).map(parse_line).collect()
}

/// Strict terminal policy: exactly one record must carry `terminal` as its
/// event name. More than one is a corrupt stream (duplicate terminal); zero
/// means the observable outcome did not complete (missing terminal). `event_of`
/// extracts the event name from a record.
pub fn check_terminal<V>(
    records: &[V],
    terminal: &str,
    event_of: impl Fn(&V) -> Option<&str>,
) -> Result<(), String> {
    let count = records
        .iter()
        .filter(|r| event_of(r) == Some(terminal))
        .count();
    if count > 1 {
        return Err(format!(
            "duplicate terminal: {count} {terminal} records in one transcript (the client \
             emits its terminal once, then exits)"
        ));
    }
    if count == 0 {
        return Err(format!(
            "missing terminal: no {terminal} record in the transcript — the observable \
             outcome did not complete"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_value(line: &str) -> Result<serde_json::Value, String> {
        serde_json::from_str(line).map_err(|e| format!("invalid JSON line: {e}: {line}"))
    }

    fn event_of(v: &serde_json::Value) -> Option<&str> {
        v.get("event").and_then(serde_json::Value::as_str)
    }

    #[test]
    fn parses_a_clean_stream() {
        let raw = [
            r#"{"event":"starting","protocol":1}"#,
            r#"{"event":"joined","protocol":1}"#,
        ]
        .join("\n");
        let records = parse_lines(&raw, parse_value).expect("parse");
        assert_eq!(records.len(), 2);
        assert_eq!(records[1]["event"], "joined");
        check_terminal(&records, "joined", event_of).expect("terminal present");
    }

    #[test]
    fn rejects_a_malformed_line() {
        let raw = ["{\"event\":\"init\"}", "not json", "{\"event\":\"joined\"}"].join("\n");
        let err = parse_lines(&raw, parse_value).expect_err("malformed line must fail");
        assert!(
            err.contains("invalid JSON line"),
            "error must name the line: {err}"
        );
    }

    #[test]
    fn rejects_a_duplicate_terminal() {
        let raw = [
            r#"{"event":"joined","protocol":1}"#,
            r#"{"event":"joined","protocol":1}"#,
        ]
        .join("\n");
        let records = parse_lines(&raw, parse_value).expect("parse");
        let err = check_terminal(&records, "joined", event_of).expect_err("duplicate terminal");
        assert!(
            err.contains("duplicate terminal"),
            "error must name the duplicate: {err}"
        );
    }

    #[test]
    fn rejects_a_missing_terminal() {
        let raw = [
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"spawn","protocol":1}"#,
        ]
        .join("\n");
        let records = parse_lines(&raw, parse_value).expect("parse");
        let err = check_terminal(&records, "joined", event_of).expect_err("missing terminal");
        assert!(
            err.contains("missing terminal"),
            "error must name the absence: {err}"
        );
    }

    #[test]
    fn skip_blank_lines_before_parse() {
        let raw = "\n{\"event\":\"joined\"}\n\n";
        let records = parse_lines(raw, parse_value).expect("parse");
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn terminal_is_scoped_to_a_specific_event() {
        // A `joined` record is not the `moved` terminal, so a moved-only
        // transcript must be reported as missing-`moved` (and vice versa).
        let records =
            parse_lines(r#"{"event":"joined","protocol":1}"#, parse_value).expect("parse");
        let err = check_terminal(&records, "moved", event_of).expect_err("no moved record");
        assert!(err.contains("missing terminal"));
        check_terminal(&records, "joined", event_of).expect("joined terminal present");
    }
}
