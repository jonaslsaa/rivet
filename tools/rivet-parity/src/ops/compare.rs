//! The one comparator every oracle op uses (VERIFICATION §2.4).
//!
//! Both sides are reduced to a canonical JSON line per input,
//! `{"id":<input id>,"ok":true,"result":{...}}` or
//! `{"id":<input id>,"ok":false,"kind":"input_rejected"}`, with result keys in
//! the op's fixed order (the Java op's declared order; the Rust handler emits
//! the same insertion order) and floats as IEEE-754 bit hex. Comparison is byte
//! equality of those lines. The first divergence is reported with the op, the
//! input id and a field-level diff. Fields are excluded only through the
//! committed exclusions table; a Rust panic or a Paper `unavailable` answer is
//! a hard mismatch, never a match.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use serde_json::{Map, Value, json};

/// One side's answer for one input.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The op produced a result (canonical key order).
    Accepted(Map<String, Value>),
    /// An explicit input-rejection verdict.
    Rejected,
    /// No verdict: an infrastructure fault (Java `unavailable`, malformed
    /// response, a missing request field). Never equal to anything.
    Unavailable(String),
}

impl Outcome {
    /// The canonical compared line for input `id`.
    pub fn canonical_line(&self, id: &str) -> String {
        let value = match self {
            Outcome::Accepted(result) => json!({"id": id, "ok": true, "result": result}),
            Outcome::Rejected => json!({"id": id, "ok": false, "kind": "input_rejected"}),
            Outcome::Unavailable(reason) => {
                json!({"id": id, "ok": false, "kind": "unavailable", "reason": reason})
            }
        };
        serde_json::to_string(&value).expect("JSON values always serialize")
    }

    /// Parse a recorded line (`run.sh --record` output) into the outcome it
    /// pins. `dec` (human decimals) and `detail` (Java exception text) are
    /// informational and never compared.
    pub fn from_recorded(line: &str) -> Result<(String, Outcome), String> {
        let value: Value =
            serde_json::from_str(line).map_err(|e| format!("recorded line is not JSON: {e}"))?;
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or("recorded line has no string id")?
            .to_owned();
        match value.get("ok").and_then(Value::as_bool) {
            Some(true) => match value.get("result") {
                Some(Value::Object(result)) => Ok((id, Outcome::Accepted(result.clone()))),
                _ => Err(format!(
                    "recorded line {id} has ok:true but no result object"
                )),
            },
            Some(false) if value.get("kind").and_then(Value::as_str) == Some("input_rejected") => {
                Ok((id, Outcome::Rejected))
            }
            _ => Err(format!(
                "recorded line {id} is neither a result nor an input_rejected verdict"
            )),
        }
    }

    /// Parse a live oracle response (`{"id":n,"protocol":1,"ok":...}`) for `op`.
    /// Only an explicit `input_rejected` naming this op and the Java exception
    /// type is a rejection — the same verdict record mode writes. The exception
    /// message is informational (Java omits a null message), exactly as the
    /// recorded `detail` is never compared.
    pub fn from_live(op: &str, line: &str) -> Outcome {
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(e) => return Outcome::Unavailable(format!("non-JSON oracle line: {e}")),
        };
        match value.get("ok").and_then(Value::as_bool) {
            Some(true) => match value.get("result") {
                Some(Value::Object(result)) => Outcome::Accepted(result.clone()),
                _ => Outcome::Unavailable("oracle success without a result object".into()),
            },
            Some(false) => {
                let error = value.get("error");
                let field = |key: &str| error.and_then(|e| e.get(key)).and_then(Value::as_str);
                if field("kind") == Some("input_rejected")
                    && field("operation") == Some(op)
                    && field("type").is_some_and(|t| !t.is_empty())
                {
                    Outcome::Rejected
                } else {
                    Outcome::Unavailable(format!(
                        "oracle {op} produced no verdict: {}",
                        error.map_or_else(|| "no structured error".to_owned(), Value::to_string)
                    ))
                }
            }
            None => Outcome::Unavailable("oracle response missing boolean `ok`".into()),
        }
    }
}

/// One committed exclusion of `op`. `field` is either a dot path starting at
/// `result.` (as the field diff names it), dropped from both sides before
/// comparison, or `input:<id>`, which takes that whole input out of the
/// comparison: a known divergence (or an input Rivet cannot survive) filed as
/// `issue` and left unfixed by the verification PR. Excluded inputs are
/// counted separately from matched and mismatched, never silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exclusion {
    pub op: String,
    pub field: String,
    pub reason: String,
    pub issue: String,
}

/// Parse the committed exclusions table (columns `op`, `field`, `reason`, `issue`, tab-separated; header
/// first). Every row needs a reason and a GitHub issue number. Two kinds of
/// row exist: `result.<path>` (a field Paper itself varies across twin runs,
/// VERIFICATION §2.4.3) and `input:<id>` (a known Rivet divergence found by the
/// op, filed as an issue and not fixed in the PR that records it; the input is
/// reported as excluded, never as matched).
pub fn parse_exclusions(text: &str) -> Result<Vec<Exclusion>, String> {
    let mut lines = text.lines();
    if lines.next() != Some("op\tfield\treason\tissue") {
        return Err(
            "exclusions table must start with the header `op\\tfield\\treason\\tissue`".into(),
        );
    }
    let mut rows = Vec::new();
    for (index, line) in lines.enumerate() {
        let columns: Vec<&str> = line.split('\t').collect();
        let [op, field, reason, issue] = columns.as_slice() else {
            return Err(format!(
                "exclusions row {}: expected 4 tab-separated columns",
                index + 2
            ));
        };
        if [op, field, reason, issue]
            .iter()
            .any(|c| c.trim().is_empty())
        {
            return Err(format!(
                "exclusions row {}: every column is required",
                index + 2
            ));
        }
        if !issue.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!(
                "exclusions row {}: issue must be a GitHub issue number, got `{issue}`",
                index + 2
            ));
        }
        if !field.starts_with("result.") && !field.starts_with("input:") {
            return Err(format!(
                "exclusions row {}: field must be a path under `result.` or `input:<id>` (the envelope is never excluded)",
                index + 2
            ));
        }
        rows.push(Exclusion {
            op: (*op).to_owned(),
            field: (*field).to_owned(),
            reason: (*reason).to_owned(),
            issue: (*issue).to_owned(),
        });
    }
    Ok(rows)
}

/// Load the committed exclusions table.
pub fn load_exclusions(path: &Path) -> Result<Vec<Exclusion>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read exclusions table {}: {e}", path.display()))?;
    parse_exclusions(&text)
}

/// Whether `id` of `op` is excluded as a whole input.
pub fn input_excluded(op: &str, id: &str, exclusions: &[Exclusion]) -> bool {
    exclusions.iter().any(|e| {
        e.op == op
            && e.field
                .strip_prefix("input:")
                .is_some_and(|excluded| excluded == id)
    })
}

fn remove_path(map: &mut Map<String, Value>, path: &str) {
    match path.split_once('.') {
        None => {
            map.shift_remove(path);
        }
        Some((head, rest)) => {
            if let Some(Value::Object(inner)) = map.get_mut(head) {
                remove_path(inner, rest);
            }
        }
    }
}

fn apply_exclusions(op: &str, outcome: &Outcome, exclusions: &[Exclusion]) -> Outcome {
    match outcome {
        Outcome::Accepted(result) => {
            let mut result = result.clone();
            for exclusion in exclusions.iter().filter(|e| e.op == op) {
                if let Some(path) = exclusion.field.strip_prefix("result.") {
                    remove_path(&mut result, path);
                }
            }
            Outcome::Accepted(result)
        }
        other => other.clone(),
    }
}

/// One differing field between expected (Paper) and actual (Rivet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDiff {
    pub path: String,
    pub expected: String,
    pub actual: String,
}

impl fmt::Display for FieldDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: expected {} got {}",
            self.path,
            clip(&self.expected),
            clip(&self.actual)
        )
    }
}

fn clip(text: &str) -> String {
    const MAX: usize = 160;
    if text.chars().count() <= MAX {
        text.to_owned()
    } else {
        let head: String = text.chars().take(MAX).collect();
        format!("{head}…")
    }
}

fn render(value: Option<&Value>) -> String {
    value.map_or_else(|| "<absent>".to_owned(), Value::to_string)
}

fn diff_values(path: &str, expected: &Value, actual: &Value, out: &mut Vec<FieldDiff>) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            let keys: BTreeSet<&String> = e.keys().chain(a.keys()).collect();
            let before = out.len();
            for key in keys {
                let child = format!("{path}.{key}");
                match (e.get(key), a.get(key)) {
                    (Some(ev), Some(av)) => diff_values(&child, ev, av, out),
                    (ev, av) => out.push(FieldDiff {
                        path: child,
                        expected: render(ev),
                        actual: render(av),
                    }),
                }
            }
            let order_e: Vec<&String> = e.keys().collect();
            let order_a: Vec<&String> = a.keys().collect();
            if out.len() == before && order_e != order_a {
                out.push(FieldDiff {
                    path: format!("{path} (key order)"),
                    expected: format!("{order_e:?}"),
                    actual: format!("{order_a:?}"),
                });
            }
        }
        (Value::Array(e), Value::Array(a)) if e.len() == a.len() => {
            for (index, (ev, av)) in e.iter().zip(a).enumerate() {
                diff_values(&format!("{path}[{index}]"), ev, av, out);
            }
        }
        _ if expected != actual => out.push(FieldDiff {
            path: path.to_owned(),
            expected: expected.to_string(),
            actual: actual.to_string(),
        }),
        _ => {}
    }
}

/// The field-level diff of two canonical lines (empty when the bytes match).
pub fn field_diff(expected_line: &str, actual_line: &str) -> Vec<FieldDiff> {
    if expected_line == actual_line {
        return Vec::new();
    }
    let parse = |line: &str| serde_json::from_str::<Value>(line).unwrap_or(Value::Null);
    let (expected, actual) = (parse(expected_line), parse(actual_line));
    let mut out = Vec::new();
    if let (Value::Object(e), Value::Object(a)) = (&expected, &actual) {
        for key in ["id", "ok", "kind", "reason", "result"] {
            match (e.get(key), a.get(key)) {
                (Some(ev), Some(av)) => diff_values(key, ev, av, &mut out),
                (None, None) => {}
                (ev, av) => out.push(FieldDiff {
                    path: key.to_owned(),
                    expected: render(ev),
                    actual: render(av),
                }),
            }
        }
    }
    if out.is_empty() {
        // Same values, different bytes (e.g. escaping): still a divergence.
        out.push(FieldDiff {
            path: "<line bytes>".to_owned(),
            expected: expected_line.to_owned(),
            actual: actual_line.to_owned(),
        });
    }
    out
}

/// The first divergence of a comparison run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    pub op: String,
    pub id: String,
    pub fields: Vec<FieldDiff>,
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} input {}:", self.op, self.id)?;
        for field in self.fields.iter().take(4) {
            write!(f, " [{field}]")?;
        }
        if self.fields.len() > 4 {
            write!(f, " (+{} more fields)", self.fields.len() - 4)?;
        }
        Ok(())
    }
}

/// How one input compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputVerdict {
    Matched,
    Mismatched,
    Excluded,
}

/// Counts and divergences of one op comparison.
#[derive(Debug, Clone, Default)]
pub struct Comparison {
    pub executed: usize,
    /// The per-input verdict, in corpus order (for per-subject tallies).
    pub per_input: Vec<(String, InputVerdict)>,
    pub matched: usize,
    /// Inputs taken out of the comparison by an `input:<id>` exclusion.
    pub excluded: usize,
    pub mismatched_ids: Vec<String>,
    pub first: Option<Divergence>,
}

/// Compare `expected` (Paper) with `actual` (Rivet) outcomes, input by input.
/// Inputs are paired by position and must carry the same id; a missing or
/// misaligned answer is a mismatch on that input.
pub fn compare(
    op: &str,
    expected: &[(String, Outcome)],
    actual: &[(String, Outcome)],
    exclusions: &[Exclusion],
) -> Comparison {
    let mut comparison = Comparison::default();
    let count = expected.len().max(actual.len());
    for index in 0..count {
        let (id, expected_line, actual_line, verdicts) =
            match (expected.get(index), actual.get(index)) {
                (Some((eid, e)), Some((aid, a))) => {
                    // An infrastructure fault is no verdict: it never matches,
                    // not even an identical fault on the other side.
                    let verdicts = !matches!(e, Outcome::Unavailable(_))
                        && !matches!(a, Outcome::Unavailable(_));
                    let e = apply_exclusions(op, e, exclusions);
                    let a = apply_exclusions(op, a, exclusions);
                    (
                        eid.clone(),
                        e.canonical_line(eid),
                        a.canonical_line(aid),
                        verdicts,
                    )
                }
                (Some((eid, e)), None) => (
                    eid.clone(),
                    e.canonical_line(eid),
                    "<missing>".into(),
                    false,
                ),
                (None, Some((aid, a))) => (
                    aid.clone(),
                    "<missing>".into(),
                    a.canonical_line(aid),
                    false,
                ),
                (None, None) => unreachable!("index < max(len)"),
            };
        comparison.executed += 1;
        if input_excluded(op, &id, exclusions) {
            comparison.excluded += 1;
            comparison.per_input.push((id, InputVerdict::Excluded));
            continue;
        }
        if verdicts && expected_line == actual_line {
            comparison.matched += 1;
            comparison.per_input.push((id, InputVerdict::Matched));
            continue;
        }
        comparison
            .per_input
            .push((id.clone(), InputVerdict::Mismatched));
        if comparison.first.is_none() {
            comparison.first = Some(Divergence {
                op: op.to_owned(),
                id: id.clone(),
                fields: field_diff(&expected_line, &actual_line),
            });
        }
        comparison.mismatched_ids.push(id);
    }
    comparison
}

/// Deterministically corrupt one outcome so it can never equal the original:
/// the first result field is changed (string suffixed, bool flipped, number
/// incremented), a rejection becomes an acceptance and vice versa.
pub fn tamper(outcome: &Outcome) -> Outcome {
    match outcome {
        Outcome::Accepted(result) => {
            let mut result = result.clone();
            match result.iter_mut().next() {
                Some((_, value)) => {
                    let replacement = match &*value {
                        Value::String(s) => Value::String(format!("{s}\u{fffd}tampered")),
                        Value::Bool(b) => Value::Bool(!*b),
                        Value::Number(n) => match n.as_i64() {
                            Some(i) => json!(i.wrapping_add(1)),
                            None => Value::String(format!("tampered {n}")),
                        },
                        other => Value::String(format!("tampered {other}")),
                    };
                    *value = replacement;
                }
                None => {
                    result.insert("tampered".into(), Value::Bool(true));
                }
            }
            Outcome::Accepted(result)
        }
        Outcome::Rejected => {
            let mut result = Map::new();
            result.insert("tampered".into(), Value::Bool(true));
            Outcome::Accepted(result)
        }
        Outcome::Unavailable(_) => Outcome::Rejected,
    }
}

/// The mandatory negative control for one comparator: tamper the expected
/// answer of the first input the real comparison matched and require the
/// comparator to detect it and name exactly that input. `None` when no input
/// matched (the row already fails; there is nothing clean to tamper).
pub fn tamper_negative(
    op: &str,
    expected: &[(String, Outcome)],
    actual: &[(String, Outcome)],
    exclusions: &[Exclusion],
) -> Option<rivet_harness_common::negative::Verdict> {
    let clean = compare(op, expected, actual, exclusions);
    let index = (0..expected.len().min(actual.len())).find(|&i| {
        !clean.mismatched_ids.contains(&expected[i].0)
            && !input_excluded(op, &expected[i].0, exclusions)
    })?;
    let mut tampered = expected.to_vec();
    tampered[index].1 = tamper(&tampered[index].1);
    let result = compare(op, &tampered, actual, exclusions);
    let named: Vec<String> = result
        .mismatched_ids
        .into_iter()
        .filter(|id| !clean.mismatched_ids.contains(id))
        .collect();
    Some(rivet_harness_common::negative::verdict(
        &expected[index].0,
        &named,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepted(pairs: &[(&str, Value)]) -> Outcome {
        Outcome::Accepted(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn canonical_lines_have_fixed_envelope_order() {
        let a = accepted(&[("tag_id", json!(10)), ("snbt", json!("{}"))]);
        assert_eq!(
            a.canonical_line("x"),
            r#"{"id":"x","ok":true,"result":{"tag_id":10,"snbt":"{}"}}"#
        );
        assert_eq!(
            Outcome::Rejected.canonical_line("y"),
            r#"{"id":"y","ok":false,"kind":"input_rejected"}"#
        );
    }

    #[test]
    fn recorded_lines_drop_informational_fields() {
        let (id, outcome) = Outcome::from_recorded(
            r#"{"id":"a","ok":true,"result":{"v":"0x3f800000"},"dec":{"v":"1.0"}}"#,
        )
        .unwrap();
        assert_eq!(id, "a");
        assert_eq!(outcome, accepted(&[("v", json!("0x3f800000"))]));
        let (_, rejected) = Outcome::from_recorded(
            r#"{"id":"b","ok":false,"kind":"input_rejected","detail":{"type":"X","message":"m"}}"#,
        )
        .unwrap();
        assert_eq!(rejected, Outcome::Rejected);
        assert!(Outcome::from_recorded(r#"{"id":"c","ok":false,"kind":"unavailable"}"#).is_err());
    }

    #[test]
    fn live_rejection_needs_kind_operation_and_type_but_not_a_message() {
        let eof = r#"{"id":1,"protocol":1,"ok":false,"error":{"kind":"input_rejected","operation":"nbt.decode","type":"java.io.EOFException"}}"#;
        assert_eq!(Outcome::from_live("nbt.decode", eof), Outcome::Rejected);
        // Wrong op, unavailable kind or no type: never a verdict.
        assert!(matches!(
            Outcome::from_live("snbt.parse", eof),
            Outcome::Unavailable(_)
        ));
        let crash = r#"{"id":1,"protocol":1,"ok":false,"error":{"kind":"unavailable","operation":"nbt.decode","type":"java.lang.NullPointerException"}}"#;
        assert!(matches!(
            Outcome::from_live("nbt.decode", crash),
            Outcome::Unavailable(_)
        ));
        let untyped = r#"{"id":1,"protocol":1,"ok":false,"error":{"kind":"input_rejected","operation":"nbt.decode"}}"#;
        assert!(matches!(
            Outcome::from_live("nbt.decode", untyped),
            Outcome::Unavailable(_)
        ));
        assert!(matches!(
            Outcome::from_live("nbt.decode", "not json"),
            Outcome::Unavailable(_)
        ));
    }

    #[test]
    fn unavailable_never_matches_even_itself() {
        let paper = vec![("a".to_owned(), Outcome::Unavailable("crash".into()))];
        let result = compare("op", &paper, &paper.clone(), &[]);
        assert_eq!(result.mismatched_ids, vec!["a".to_owned()]);
        assert_eq!(result.matched, 0);
        let rust = vec![("a".to_owned(), Outcome::Rejected)];
        let result = compare("op", &paper, &rust, &[]);
        assert_eq!(result.mismatched_ids, vec!["a".to_owned()]);
    }

    #[test]
    fn first_divergence_names_op_id_and_field() {
        let paper = vec![
            ("a".to_owned(), accepted(&[("snbt", json!("{x:1b}"))])),
            ("b".to_owned(), accepted(&[("snbt", json!("{y:2b}"))])),
        ];
        let rust = vec![
            ("a".to_owned(), accepted(&[("snbt", json!("{x:1b}"))])),
            ("b".to_owned(), accepted(&[("snbt", json!("{y:3b}"))])),
        ];
        let result = compare("snbt.parse", &paper, &rust, &[]);
        assert_eq!((result.executed, result.matched), (2, 1));
        let first = result.first.unwrap();
        assert_eq!((first.op.as_str(), first.id.as_str()), ("snbt.parse", "b"));
        assert_eq!(first.fields[0].path, "result.snbt");
        assert_eq!(first.fields[0].expected, r#""{y:2b}""#);
    }

    #[test]
    fn key_order_alone_is_a_divergence() {
        let paper = vec![(
            "a".to_owned(),
            accepted(&[("x", json!(1)), ("y", json!(2))]),
        )];
        let rust = vec![(
            "a".to_owned(),
            accepted(&[("y", json!(2)), ("x", json!(1))]),
        )];
        let result = compare("op", &paper, &rust, &[]);
        assert_eq!(result.mismatched_ids, vec!["a".to_owned()]);
        assert_eq!(result.first.unwrap().fields[0].path, "result (key order)");
    }

    #[test]
    fn accept_reject_divergence_is_reported_on_ok() {
        let paper = vec![("a".to_owned(), Outcome::Rejected)];
        let rust = vec![("a".to_owned(), accepted(&[("x", json!(1))]))];
        let first = compare("op", &paper, &rust, &[]).first.unwrap();
        assert!(first.fields.iter().any(|f| f.path == "ok"), "{first}");
    }

    #[test]
    fn missing_answers_are_mismatches() {
        let paper = vec![
            ("a".to_owned(), Outcome::Rejected),
            ("b".to_owned(), Outcome::Rejected),
        ];
        let rust = vec![("a".to_owned(), Outcome::Rejected)];
        let result = compare("op", &paper, &rust, &[]);
        assert_eq!((result.executed, result.matched), (2, 1));
        assert_eq!(result.mismatched_ids, vec!["b".to_owned()]);
    }

    #[test]
    fn exclusions_are_data_and_drop_only_the_named_field() {
        let table = "op\tfield\treason\tissue\nop\tresult.nested.t\tPaper twin runs differ\t1\n";
        let exclusions = parse_exclusions(table).unwrap();
        let paper = vec![(
            "a".to_owned(),
            accepted(&[("nested", json!({"t": 1, "u": 2}))]),
        )];
        let same_except_t = vec![(
            "a".to_owned(),
            accepted(&[("nested", json!({"t": 9, "u": 2}))]),
        )];
        assert_eq!(
            compare("op", &paper, &same_except_t, &exclusions).matched,
            1
        );
        // The exclusion is per-op: another op still compares the field.
        assert_eq!(
            compare("other", &paper, &same_except_t, &exclusions).matched,
            0
        );
        let differs_on_u = vec![(
            "a".to_owned(),
            accepted(&[("nested", json!({"t": 1, "u": 3}))]),
        )];
        assert_eq!(compare("op", &paper, &differs_on_u, &exclusions).matched, 0);
    }

    #[test]
    fn exclusion_rows_need_a_reason_and_an_issue() {
        assert!(parse_exclusions("op\tfield\treason\tissue\nop\tresult.x\t\t7\n").is_err());
        // The issue column is a GitHub issue number, not a URL or a placeholder.
        assert!(
            parse_exclusions("op\tfield\treason\tissue\nop\tresult.x\twhy\thttps://x\n").is_err()
        );
        assert!(parse_exclusions("op\tfield\treason\tissue\nop\tresult.x\twhy\tTBD\n").is_err());
        assert!(parse_exclusions("op\tfield\treason\tissue\nop\tresult.x\twhy\n").is_err());
        assert!(parse_exclusions("wrong header\n").is_err());
        assert!(parse_exclusions("op\tfield\treason\tissue\nop\tok\twhy\t7\n").is_err());
        assert!(
            parse_exclusions("op\tfield\treason\tissue\n")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn tamper_negative_detects_and_names_the_tampered_input() {
        let answers = vec![
            ("a".to_owned(), accepted(&[("accept", json!(false))])),
            ("b".to_owned(), Outcome::Rejected),
        ];
        let verdict = tamper_negative("op", &answers, &answers, &[]).unwrap();
        assert!(verdict.passed(), "{verdict}");
        assert_eq!(verdict.tampered(), Some("a"));
        for outcome in [
            accepted(&[("s", json!("x"))]),
            accepted(&[("n", json!(7))]),
            accepted(&[]),
            Outcome::Rejected,
        ] {
            assert_ne!(tamper(&outcome), outcome);
        }
    }

    #[test]
    fn tamper_negative_skips_already_mismatched_inputs() {
        let paper = vec![
            ("a".to_owned(), Outcome::Rejected),
            ("b".to_owned(), Outcome::Rejected),
        ];
        let rust = vec![
            ("a".to_owned(), accepted(&[("x", json!(1))])),
            ("b".to_owned(), Outcome::Rejected),
        ];
        let verdict = tamper_negative("op", &paper, &rust, &[]).unwrap();
        assert_eq!(verdict.tampered(), Some("b"));
        assert!(verdict.passed());
        let all_bad = vec![("a".to_owned(), accepted(&[("x", json!(1))]))];
        assert!(tamper_negative("op", &paper[..1], &all_bad, &[]).is_none());
    }
}
