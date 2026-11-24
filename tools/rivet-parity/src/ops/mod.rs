//! The generic oracle-op framework, Rust side (VERIFICATION §2.1, §2.4).
//!
//! An oracle op is a named Java handler in `RivetReferenceOracle`'s registry,
//! a Rust handler here ([`handlers`]), a committed input corpus and one
//! comparator ([`compare`]). The fixtures live in
//! `tools/rivet-reference-oracle/fixtures/ops/`:
//!
//! - `corpus/<op>.jsonl` — one `{"id":...,<request fields>}` per line;
//! - `<op>.jsonl` — Paper's recorded answers (`run.sh --record`);
//! - `manifest.json` — Paper pin + commit, op source SHA-256, and per op the
//!   corpus SHA-256/count and output SHA-256/count;
//! - `exclusions.tsv` — the comparator's committed exclusion table.
//!
//! Modes (`rivet-parity ops <mode>`):
//!
//! - `check` — offline, no JVM: validate the manifest closure, run every Rust
//!   handler over its corpus and compare against the recorded answers. This is
//!   also a cargo test, so it runs in every scoped gate.
//! - `live` — boot the oracle once, stream every corpus through Paper,
//!   compare Paper with Rivet, and require live Paper to still equal the
//!   recorded answers (a stale golden is a FAIL).
//! - `emit-corpus <dir>` — regenerate the corpora from `rivet-parity`'s own
//!   corpus (a test pins the committed files to this output).
//!
//! Every row asserts closure (declared == executed + excluded, an excluded
//! input being one the Rust side never runs; zero executed is
//! UNVERIFIED) and runs its tamper negative; `--break-handler <op>` swaps in a
//! deliberately broken Rust handler and passes only when the comparator
//! detects and names it.

pub mod codec;
pub mod codec_corpus;
pub mod compare;
pub mod handlers;
pub mod packet;
pub mod packet_corpus;
pub mod packet_seeds;

use std::collections::BTreeSet;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use rivet_harness_common::closure::{Closure, Status};
use rivet_harness_common::negative::Verdict;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use compare::{Divergence, Exclusion, Outcome};

/// `tools/rivet-reference-oracle/fixtures/ops`.
pub fn ops_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../rivet-reference-oracle/fixtures/ops")
}

/// The Java file holding every op's source (its SHA-256 is pinned per recording).
pub fn op_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rivet-reference-oracle/src/RivetReferenceOracle.java")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// One corpus input.
#[derive(Debug, Clone)]
pub struct CorpusEntry {
    pub id: String,
    /// The request fields (everything but `id`).
    pub request: Map<String, Value>,
}

impl CorpusEntry {
    /// The live request body: `{"op":<op>,<fields>}`.
    fn body(&self, op: &str) -> String {
        let mut body = Map::new();
        body.insert("op".into(), json!(op));
        body.extend(self.request.clone());
        serde_json::to_string(&Value::Object(body)).expect("JSON values always serialize")
    }
}

/// One recorded op: its corpus and Paper's recorded answers, closure-checked.
#[derive(Debug, Clone)]
pub struct OpFixture {
    pub op: String,
    pub declared: usize,
    pub corpus: Vec<CorpusEntry>,
    pub recorded: Vec<(String, Outcome)>,
}

/// Why the recorded fixtures could not be used.
#[derive(Debug)]
pub enum LoadError {
    /// Nothing recorded yet (UNVERIFIED).
    Absent(String),
    /// Present but inconsistent, tampered or stale (FAIL).
    Invalid(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Absent(m) | LoadError::Invalid(m) => f.write_str(m),
        }
    }
}

fn parse_corpus(name: &str, text: &str) -> Result<Vec<CorpusEntry>, String> {
    let mut ids = BTreeSet::new();
    let mut entries = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let at = format!("{name}:{}", index + 1);
        let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(line) else {
            return Err(format!("{at}: corpus line is not a JSON object"));
        };
        let Some(Value::String(id)) = fields.shift_remove("id") else {
            return Err(format!("{at}: corpus entry has no string id"));
        };
        if fields.contains_key("op") {
            return Err(format!("{at}: corpus entries must not carry `op`"));
        }
        if !ids.insert(id.clone()) {
            return Err(format!("{at}: duplicate input id {id}"));
        }
        entries.push(CorpusEntry {
            id,
            request: fields,
        });
    }
    Ok(entries)
}

fn manifest_str<'a>(entry: &'a Value, key: &str) -> Result<&'a str, LoadError> {
    entry
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| LoadError::Invalid(format!("ops manifest entry lacks string `{key}`")))
}

fn manifest_count(entry: &Value, key: &str) -> Result<usize, LoadError> {
    entry
        .get(key)
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .ok_or_else(|| LoadError::Invalid(format!("ops manifest entry lacks integer `{key}`")))
}

fn read_pinned(dir: &Path, relative: &str, sha256: &str) -> Result<Vec<u8>, LoadError> {
    if relative.contains("..") || relative.starts_with('/') {
        return Err(LoadError::Invalid(format!(
            "ops manifest path escapes the fixture dir: {relative}"
        )));
    }
    let path = dir.join(relative);
    let bytes = fs::read(&path)
        .map_err(|e| LoadError::Invalid(format!("cannot read {}: {e}", path.display())))?;
    let actual = sha256_hex(&bytes);
    if actual != sha256 {
        return Err(LoadError::Invalid(format!(
            "{relative}: SHA-256 {actual} does not match the manifest's {sha256} (hand-edited or stale; re-record with run.sh --record)"
        )));
    }
    Ok(bytes)
}

/// Load and closure-check every recorded op under `dir`.
pub fn load_fixtures(dir: &Path, op_source: &Path) -> Result<Vec<OpFixture>, LoadError> {
    let manifest_path = dir.join("manifest.json");
    let text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(LoadError::Absent(format!(
                "no recorded ops: {} is missing",
                manifest_path.display()
            )));
        }
        Err(e) => {
            return Err(LoadError::Invalid(format!(
                "{}: {e}",
                manifest_path.display()
            )));
        }
    };
    let manifest: Value = serde_json::from_str(&text)
        .map_err(|e| LoadError::Invalid(format!("ops manifest is not JSON: {e}")))?;
    if manifest.get("format").and_then(Value::as_u64) != Some(1)
        || manifest.get("kind").and_then(Value::as_str) != Some("oracle-ops")
    {
        return Err(LoadError::Invalid(
            "ops manifest is not format-1 `oracle-ops`".into(),
        ));
    }
    let source = fs::read(op_source).map_err(|e| {
        LoadError::Invalid(format!(
            "cannot read op source {}: {e}",
            op_source.display()
        ))
    })?;
    let pinned_source = manifest_str(&manifest, "op_source_sha256")?;
    if sha256_hex(&source) != pinned_source {
        return Err(LoadError::Invalid(format!(
            "op source {} changed since the ops were recorded (manifest pins {pinned_source}); re-record with `tools/rivet-reference-oracle/run.sh --record`",
            op_source.display()
        )));
    }
    let Some(entries) = manifest.get("ops").and_then(Value::as_array) else {
        return Err(LoadError::Invalid("ops manifest has no `ops` array".into()));
    };
    let mut fixtures = Vec::new();
    let mut listed = BTreeSet::new();
    for entry in entries {
        let op = manifest_str(entry, "op")?.to_owned();
        let corpus_path = manifest_str(entry, "corpus")?;
        listed.insert(corpus_path.to_owned());
        let corpus_bytes = read_pinned(dir, corpus_path, manifest_str(entry, "corpus_sha256")?)?;
        let output_path = manifest_str(entry, "output")?;
        let output_bytes = read_pinned(dir, output_path, manifest_str(entry, "output_sha256")?)?;
        let corpus_n = manifest_count(entry, "corpus_n")?;
        let output_n = manifest_count(entry, "output_n")?;
        let corpus = parse_corpus(
            corpus_path,
            std::str::from_utf8(&corpus_bytes)
                .map_err(|e| LoadError::Invalid(format!("{corpus_path}: {e}")))?,
        )
        .map_err(LoadError::Invalid)?;
        let mut recorded = Vec::new();
        for (index, line) in std::str::from_utf8(&output_bytes)
            .map_err(|e| LoadError::Invalid(format!("{output_path}: {e}")))?
            .lines()
            .enumerate()
        {
            recorded
                .push(Outcome::from_recorded(line).map_err(|e| {
                    LoadError::Invalid(format!("{output_path}:{}: {e}", index + 1))
                })?);
        }
        if corpus.len() != corpus_n || recorded.len() != output_n || corpus_n != output_n {
            return Err(LoadError::Invalid(format!(
                "{op}: closure broken — manifest declares {corpus_n} inputs / {output_n} answers, files hold {} / {}",
                corpus.len(),
                recorded.len()
            )));
        }
        if let Some((index, (entry, _))) = corpus
            .iter()
            .zip(&recorded)
            .enumerate()
            .find(|(_, (c, (rid, _)))| c.id != *rid)
        {
            return Err(LoadError::Invalid(format!(
                "{op}: recorded answer {} is for {:?}, corpus input is {:?}",
                index + 1,
                recorded[index].0,
                entry.id
            )));
        }
        fixtures.push(OpFixture {
            op,
            declared: corpus_n,
            corpus,
            recorded,
        });
    }
    // Every committed corpus must be recorded: an unlisted corpus would be an
    // input set that silently never runs.
    if let Ok(listing) = fs::read_dir(dir.join("corpus")) {
        for file in listing.flatten() {
            let name = format!("corpus/{}", file.file_name().to_string_lossy());
            if !listed.contains(&name) {
                return Err(LoadError::Invalid(format!(
                    "{name} is not in the ops manifest (unrecorded corpus)"
                )));
            }
        }
    }
    if fixtures.is_empty() {
        return Err(LoadError::Absent("ops manifest records zero ops".into()));
    }
    Ok(fixtures)
}

/// A Rust handler, possibly deliberately broken for the negative control.
#[derive(Clone, Copy)]
struct RustSide {
    handler: handlers::Handler,
    broken: bool,
}

fn rust_side(op: &str, broken_op: Option<&str>) -> Option<RustSide> {
    handlers::handler(op).map(|handler| RustSide {
        handler,
        broken: broken_op == Some(op),
    })
}

fn run_rust(
    op: &str,
    side: RustSide,
    corpus: &[CorpusEntry],
    exclusions: &[Exclusion],
) -> Vec<(String, Outcome)> {
    let trace = std::env::var_os("RIVET_OPS_TRACE").is_some();
    corpus
        .iter()
        .map(|entry| {
            // An excluded input is never handed to the Rust handler: some
            // exclusions exist because the Rust decoder cannot survive it.
            if compare::input_excluded(op, &entry.id, exclusions) {
                return (
                    entry.id.clone(),
                    Outcome::Unavailable("excluded input not executed".into()),
                );
            }
            if trace {
                eprintln!("[ops trace] {op} {}", entry.id);
            }
            let outcome = catch_unwind(AssertUnwindSafe(|| (side.handler)(&entry.request)))
                .unwrap_or_else(|_| Outcome::Unavailable("Rust handler panicked".into()));
            // The deliberately broken handler: every answer is corrupted, the
            // way a wrong port would be.
            let outcome = if side.broken {
                compare::tamper(&outcome)
            } else {
                outcome
            };
            (entry.id.clone(), outcome)
        })
        .collect()
}

/// The verdict of one op row.
#[derive(Debug)]
pub struct OpRow {
    pub op: String,
    pub mode: &'static str,
    pub closure: Closure,
    pub negative: Option<Verdict>,
    pub status: Status,
    pub first: Option<Divergence>,
    pub mismatched_ids: Vec<String>,
    pub note: Option<String>,
    /// Per ported Rust type (packet struct, codec): how its inputs compared.
    pub subjects: Vec<SubjectTally>,
}

/// One scoreboard subject's tally within an op row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubjectTally {
    pub subject: String,
    pub executed: usize,
    pub matched: usize,
    pub mismatched: usize,
    pub excluded: usize,
}

fn tally_subjects(
    op: &str,
    corpus: &[CorpusEntry],
    comparison: &compare::Comparison,
) -> Vec<SubjectTally> {
    let mut tallies: std::collections::BTreeMap<String, SubjectTally> = Default::default();
    for (entry, (_, verdict)) in corpus.iter().zip(&comparison.per_input) {
        let Some(subject) = handlers::subject(op, &entry.request) else {
            continue;
        };
        let tally = tallies
            .entry(subject.clone())
            .or_insert_with(|| SubjectTally {
                subject,
                ..SubjectTally::default()
            });
        match verdict {
            compare::InputVerdict::Matched => {
                tally.executed += 1;
                tally.matched += 1;
            }
            compare::InputVerdict::Mismatched => {
                tally.executed += 1;
                tally.mismatched += 1;
            }
            // Never run on the Rust side: declared, not executed.
            compare::InputVerdict::Excluded => tally.excluded += 1,
        }
    }
    tallies.into_values().collect()
}

impl OpRow {
    fn to_json(&self) -> Value {
        json!({
            "kind": "op",
            "mode": self.mode,
            "op": self.op,
            "status": self.status.to_string(),
            "declared": self.closure.declared,
            "executed": self.closure.executed,
            "matched": self.closure.matched,
            "mismatched": self.closure.mismatched,
            "negative": self.negative.as_ref().map(|v| v.to_string()),
            "negative_detected": self.negative.as_ref().is_some_and(Verdict::passed),
            "first_divergence": self.first.as_ref().map(|d| json!({
                "op": d.op,
                "id": d.id,
                "fields": d.fields.iter().map(|f| json!({"path": f.path, "expected": f.expected, "actual": f.actual})).collect::<Vec<_>>(),
            })),
            "mismatched_ids": self.mismatched_ids,
            "note": self.note,
            "subjects": self.subjects.iter().map(|t| json!({
                "subject": t.subject,
                "executed": t.executed,
                "matched": t.matched,
                "mismatched": t.mismatched,
                "excluded": t.excluded,
            })).collect::<Vec<_>>(),
        })
    }
}

fn evaluate(
    mode: &'static str,
    fixture: &OpFixture,
    expected: &[(String, Outcome)],
    actual: &[(String, Outcome)],
    negative: Option<Verdict>,
    exclusions: &[Exclusion],
) -> OpRow {
    let comparison = compare::compare(&fixture.op, expected, actual, exclusions);
    let subjects = tally_subjects(&fixture.op, &fixture.corpus, &comparison);
    let closure = Closure {
        declared: fixture.declared,
        executed: comparison.executed,
        matched: comparison.matched,
        mismatched: comparison.mismatched_ids.len(),
        excluded: comparison.excluded,
    };
    let status = closure.status(negative.as_ref().is_some_and(Verdict::passed));
    OpRow {
        op: fixture.op.clone(),
        mode,
        closure,
        negative,
        status,
        first: comparison.first,
        mismatched_ids: comparison.mismatched_ids,
        note: None,
        subjects,
    }
}

fn no_handler_row(mode: &'static str, fixture: &OpFixture) -> OpRow {
    OpRow {
        op: fixture.op.clone(),
        mode,
        closure: Closure {
            declared: fixture.declared,
            ..Closure::default()
        },
        negative: None,
        status: Status::Unverified,
        first: None,
        mismatched_ids: Vec::new(),
        note: Some("no Rust handler for this op yet (recorded golden only)".into()),
        subjects: Vec::new(),
    }
}

/// Offline mode: recorded Paper answers vs the Rust handlers, no JVM.
pub fn check(
    fixtures: &[OpFixture],
    exclusions: &[Exclusion],
    broken_op: Option<&str>,
) -> Vec<OpRow> {
    fixtures
        .iter()
        .map(|fixture| {
            let Some(side) = rust_side(&fixture.op, broken_op) else {
                return no_handler_row("check", fixture);
            };
            let actual = run_rust(&fixture.op, side, &fixture.corpus, exclusions);
            let negative =
                compare::tamper_negative(&fixture.op, &fixture.recorded, &actual, exclusions);
            evaluate(
                "check",
                fixture,
                &fixture.recorded,
                &actual,
                negative,
                exclusions,
            )
        })
        .collect()
}

/// Live mode: one oracle boot streams every op corpus; each row compares live
/// Paper with Rivet, and a second row per op requires live Paper to still equal
/// the recorded golden.
pub fn live(
    fixtures: &[OpFixture],
    exclusions: &[Exclusion],
    broken_op: Option<&str>,
) -> Result<Vec<OpRow>, String> {
    let mut oracle = crate::oracle::Oracle::spawn()?;
    let mut rows = Vec::new();
    for fixture in fixtures {
        let bodies: Vec<String> = fixture.corpus.iter().map(|e| e.body(&fixture.op)).collect();
        let lines = oracle
            .client()
            .pipeline(&bodies)
            .map_err(|e| format!("{}: oracle pipeline failed: {e}", fixture.op))?;
        let paper: Vec<(String, Outcome)> = fixture
            .corpus
            .iter()
            .zip(&lines)
            .map(|(entry, line)| (entry.id.clone(), Outcome::from_live(&fixture.op, line)))
            .collect();

        // Freshness: the committed golden must still be what Paper answers.
        let freshness_negative =
            compare::tamper_negative(&fixture.op, &fixture.recorded, &paper, exclusions);
        let mut fresh = evaluate(
            "recorded-vs-live",
            fixture,
            &fixture.recorded,
            &paper,
            freshness_negative,
            exclusions,
        );
        if fresh.status == Status::Fail {
            fresh.note = Some("live Paper no longer reproduces the recorded golden".into());
        }
        rows.push(fresh);

        let Some(side) = rust_side(&fixture.op, broken_op) else {
            rows.push(no_handler_row("live", fixture));
            continue;
        };
        let actual = run_rust(&fixture.op, side, &fixture.corpus, exclusions);
        // The negative goes through the client's tamper hook: re-ask Paper for
        // the first input that matched, have the hook corrupt that response
        // line, and require the comparator to detect and name the input.
        let clean = compare::compare(&fixture.op, &paper, &actual, exclusions);
        let negative = match fixture
            .corpus
            .iter()
            .position(|entry| !clean.mismatched_ids.contains(&entry.id))
        {
            Some(index) => {
                oracle
                    .client()
                    .arm_tamper(rivet_harness_common::oracle_client::Tamper {
                        index: 0,
                        mutate: Box::new(tamper_response_line),
                    });
                let line = oracle
                    .client()
                    .call(&bodies[index])
                    .map_err(|e| format!("{}: tamper re-ask failed: {e}", fixture.op))?;
                let mut tampered = paper.clone();
                tampered[index].1 = Outcome::from_live(&fixture.op, &line);
                let result = compare::compare(&fixture.op, &tampered, &actual, exclusions);
                let named: Vec<String> = result
                    .mismatched_ids
                    .into_iter()
                    .filter(|id| !clean.mismatched_ids.contains(id))
                    .collect();
                Some(rivet_harness_common::negative::verdict(
                    &fixture.corpus[index].id,
                    &named,
                ))
            }
            None => None,
        };
        rows.push(evaluate(
            "live", fixture, &paper, &actual, negative, exclusions,
        ));
    }
    Ok(rows)
}

/// The live-mode tamper: corrupt the response's result the same way
/// [`compare::tamper`] corrupts an outcome, keeping the line well-formed.
fn tamper_response_line(line: &str) -> String {
    let Ok(Value::Object(mut response)) = serde_json::from_str::<Value>(line) else {
        return format!("{line}tampered");
    };
    let ok = response.get("ok").and_then(Value::as_bool) == Some(true);
    if ok {
        if let Some(Value::Object(result)) = response.get("result")
            && let Outcome::Accepted(tampered) = compare::tamper(&Outcome::Accepted(result.clone()))
        {
            response.insert("result".into(), Value::Object(tampered));
        }
    } else {
        response.insert("ok".into(), json!(true));
        response.insert("result".into(), json!({"tampered": true}));
        response.shift_remove("error");
    }
    serde_json::to_string(&Value::Object(response)).expect("JSON values always serialize")
}

/// The corpora, derived from `rivet-parity`'s own corpus so the recorded ops
/// and the classic parity rows exercise the same inputs.
pub fn emit_corpus() -> Result<Vec<(&'static str, String)>, String> {
    fn line(id: String, fields: &[(&str, &str)]) -> String {
        let mut object = Map::new();
        object.insert("id".into(), json!(id));
        for (key, value) in fields {
            object.insert((*key).to_owned(), json!(value));
        }
        serde_json::to_string(&Value::Object(object)).expect("JSON values always serialize") + "\n"
    }
    let b64 = base64::engine::general_purpose::STANDARD;

    let mut snbt = String::new();
    for (label, input) in crate::corpus::parse_corpus() {
        snbt += &line(format!("parse.{label}"), &[("input", input)]);
    }
    for (label, input) in crate::corpus::invalid_corpus() {
        snbt += &line(format!("parse-invalid.{label}"), &[("input", input)]);
    }

    let mut decode = String::new();
    let mut first_encoded: Option<Vec<u8>> = None;
    for (label, input) in crate::corpus::encode_corpus() {
        let bytes = crate::rust_encode_compound(&input)
            .map_err(|e| format!("encode corpus entry {label} does not encode: {e}"))?;
        decode += &line(
            format!("decode.{label}"),
            &[("input_base64", &b64.encode(&bytes))],
        );
        first_encoded.get_or_insert(bytes);
    }
    let first = first_encoded.ok_or("encode corpus is empty")?;
    let mut trailing = first.clone();
    trailing.push(0);
    let invalid: [(&str, String); 5] = [
        ("empty", String::new()),
        ("bad-base64", "!!!".into()),
        ("truncated", b64.encode(&first[..first.len() - 1])),
        ("trailing-byte", b64.encode(&trailing)),
        // A named TAG_Byte root: NbtIo.read requires a compound root.
        ("non-compound-root", b64.encode([1u8, 0, 0, 5])),
    ];
    for (label, input) in &invalid {
        decode += &line(
            format!("decode-invalid.{label}"),
            &[("input_base64", input)],
        );
    }

    let closure = crate::corpus::load_fixture_closure().map_err(|e| match e {
        crate::corpus::ClosureError::Absent(m) | crate::corpus::ClosureError::Invalid(m) => m,
    })?;
    let mut component = String::new();
    for entry in &closure.text_entries {
        component += &line(
            format!("component.{}", entry.id),
            &[("input", &entry.input)],
        );
    }

    Ok(vec![
        ("codec.reencode", codec_corpus::emit()?),
        ("component.json", component),
        ("nbt.decode", decode),
        ("packet.reencode", packet_corpus::emit()?),
        ("snbt.parse", snbt),
    ])
}

fn print_rows(rows: &[OpRow]) {
    for row in rows {
        println!("{}", row.to_json());
        eprintln!(
            "  {:<16} {:<17} {:<10} {} negative={}",
            row.op,
            row.mode,
            row.status.to_string(),
            row.closure,
            row.negative
                .as_ref()
                .map_or_else(|| "not run".to_owned(), ToString::to_string)
        );
        if !row.subjects.is_empty() {
            let clean = row
                .subjects
                .iter()
                .filter(|t| t.mismatched == 0 && t.excluded == 0)
                .count();
            eprintln!(
                "      subjects: {} ({} with every input matched)",
                row.subjects.len(),
                clean
            );
            let all = std::env::var_os("RIVET_OPS_SUBJECTS").is_some();
            for t in &row.subjects {
                if all || t.mismatched > 0 || t.excluded > 0 {
                    eprintln!(
                        "        {:<48} executed={:<4} matched={:<4} mismatched={:<3} excluded={}",
                        t.subject, t.executed, t.matched, t.mismatched, t.excluded
                    );
                }
            }
        }
        if let Some(first) = &row.first {
            eprintln!("      first divergence: {first}");
        }
        if let Some(note) = &row.note {
            eprintln!("      note: {note}");
        }
    }
}

/// `--break-handler` verdict: the broken op's row must FAIL with its first
/// divergence naming that op.
fn broken_handler_verdict(rows: &[OpRow], broken: &str) -> i32 {
    let detected = rows.iter().find(|row| {
        row.op == broken
            && matches!(row.mode, "check" | "live")
            && row.status == Status::Fail
            && row.first.as_ref().is_some_and(|d| d.op == broken)
    });
    match detected {
        Some(row) => {
            eprintln!(
                "  NEGATIVE PASS — deliberately broken Rust handler for {broken} detected and named: {}",
                row.first.as_ref().expect("checked above")
            );
            0
        }
        None => {
            eprintln!("  NEGATIVE FAIL — the broken {broken} handler was not detected and named");
            1
        }
    }
}

/// `ops diff <op> [<id-substring>]`: print Paper's recorded answer next to
/// Rivet's for every input that does not match (the triage view for filing
/// issues; never part of a gate).
fn run_diff(op: &str, filter: &str) -> i32 {
    let dir = ops_dir();
    let (fixtures, exclusions) = match (
        load_fixtures(&dir, &op_source()),
        compare::load_exclusions(&dir.join("exclusions.tsv")),
    ) {
        (Ok(f), Ok(e)) => (f, e),
        (Err(e), _) => {
            eprintln!("{e}");
            return 1;
        }
        (_, Err(e)) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let Some(fixture) = fixtures.iter().find(|f| f.op == op) else {
        eprintln!("no recorded op {op}");
        return 2;
    };
    let Some(side) = rust_side(op, None) else {
        eprintln!("no Rust handler for {op}");
        return 2;
    };
    let actual = run_rust(op, side, &fixture.corpus, &exclusions);
    let mut shown = 0;
    for ((id, expected), (_, got)) in fixture.recorded.iter().zip(&actual) {
        if !id.contains(filter) || compare::input_excluded(op, id, &exclusions) {
            continue;
        }
        let (e, g) = (expected.canonical_line(id), got.canonical_line(id));
        if e != g || matches!(got, Outcome::Unavailable(_)) {
            println!("{id}\n  paper: {e}\n  rivet: {g}");
            shown += 1;
        }
    }
    eprintln!("{shown} differing inputs");
    0
}

fn usage() -> i32 {
    eprintln!(
        "usage: rivet-parity ops check [--break-handler <op>]\n\
         \x20      rivet-parity ops live [--break-handler <op>]\n\
         \x20      rivet-parity ops emit-corpus <dir>\n\
         \x20      rivet-parity ops diff <op> [<id-substring>]\n\
         Exit: 0 PASS, 1 FAIL, 3 UNVERIFIED (with --break-handler: 0 only when the\n\
         broken handler is detected and named)."
    );
    2
}

/// `rivet-parity ops ...`; returns the process exit code.
pub fn run_cli(args: &[String]) -> i32 {
    let Some(mode) = args.first().map(String::as_str) else {
        return usage();
    };
    if mode == "diff" {
        return match args {
            [_, op] => run_diff(op, ""),
            [_, op, filter] => run_diff(op, filter),
            _ => usage(),
        };
    }
    if mode == "emit-corpus" {
        let [_, dir] = args else { return usage() };
        return match emit_corpus() {
            Ok(files) => {
                let dir = PathBuf::from(dir);
                if let Err(e) = fs::create_dir_all(&dir) {
                    eprintln!("cannot create {}: {e}", dir.display());
                    return 1;
                }
                for (op, text) in files {
                    let path = dir.join(format!("{op}.jsonl"));
                    if let Err(e) = fs::write(&path, text) {
                        eprintln!("cannot write {}: {e}", path.display());
                        return 1;
                    }
                    eprintln!("wrote {}", path.display());
                }
                0
            }
            Err(e) => {
                eprintln!("emit-corpus failed: {e}");
                1
            }
        };
    }
    let broken = match &args[1..] {
        [] => None,
        [flag, op] if flag == "--break-handler" => {
            if handlers::handler(op).is_none() {
                eprintln!("--break-handler: no Rust handler named {op}");
                return 2;
            }
            Some(op.as_str())
        }
        _ => return usage(),
    };
    let dir = ops_dir();
    let fixtures = match load_fixtures(&dir, &op_source()) {
        Ok(fixtures) => fixtures,
        Err(LoadError::Absent(m)) => {
            eprintln!("[rivet-parity ops] UNVERIFIED: {m}");
            return Status::Unverified.exit_code().into();
        }
        Err(LoadError::Invalid(m)) => {
            eprintln!("[rivet-parity ops] FAIL: {m}");
            return Status::Fail.exit_code().into();
        }
    };
    let exclusions = match compare::load_exclusions(&dir.join("exclusions.tsv")) {
        Ok(exclusions) => exclusions,
        Err(m) => {
            eprintln!("[rivet-parity ops] FAIL: {m}");
            return Status::Fail.exit_code().into();
        }
    };
    let rows = match mode {
        "check" => check(&fixtures, &exclusions, broken),
        "live" => match live(&fixtures, &exclusions, broken) {
            Ok(rows) => rows,
            Err(m) => {
                eprintln!("[rivet-parity ops] UNVERIFIED: {m}");
                return Status::Unverified.exit_code().into();
            }
        },
        _ => return usage(),
    };
    eprintln!("=== oracle ops ({mode}) ===");
    print_rows(&rows);
    if let Some(op) = broken {
        return broken_handler_verdict(&rows, op);
    }
    let status = Status::combine(rows.iter().map(|row| row.status));
    eprintln!("  STATUS: {status}");
    status.exit_code().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> Vec<OpFixture> {
        load_fixtures(&ops_dir(), &op_source()).expect("committed recorded ops load and close")
    }

    /// The offline tier of the op framework: every recorded op's Rust handler
    /// reproduces Paper's recorded answers byte-for-byte, closure holds, and
    /// each comparator's tamper negative is detected and named.
    #[test]
    fn recorded_ops_match_rust_handlers_offline() {
        let exclusions = compare::load_exclusions(&ops_dir().join("exclusions.tsv")).unwrap();
        let rows = check(&fixtures(), &exclusions, None);
        assert!(!rows.is_empty());
        for row in &rows {
            assert_eq!(
                row.status,
                Status::Pass,
                "{} {}: {:?} first={:?} note={:?}",
                row.op,
                row.closure,
                row.negative,
                row.first.as_ref().map(ToString::to_string),
                row.note
            );
            assert!(
                row.closure.executed > 0
                    && row.closure.executed + row.closure.excluded == row.closure.declared
            );
        }
    }

    /// The automated negative: a deliberately broken Rust handler must be
    /// detected and named (op, input id, field) by the same comparator.
    #[test]
    fn deliberately_broken_rust_handler_is_detected_and_named() {
        let exclusions = compare::load_exclusions(&ops_dir().join("exclusions.tsv")).unwrap();
        let fixtures = fixtures();
        for fixture in &fixtures {
            if handlers::handler(&fixture.op).is_none() {
                continue;
            }
            let rows = check(&fixtures, &exclusions, Some(&fixture.op));
            assert_eq!(
                broken_handler_verdict(&rows, &fixture.op),
                0,
                "{}",
                fixture.op
            );
            let row = rows.iter().find(|r| r.op == fixture.op).unwrap();
            let first = row.first.as_ref().unwrap();
            assert_eq!(first.id, fixture.corpus[0].id, "names the first input");
            assert!(!first.fields.is_empty(), "names the diverging field");
            // Only the broken op fails.
            for other in rows.iter().filter(|r| r.op != fixture.op) {
                assert_eq!(other.status, Status::Pass, "{}", other.op);
            }
        }
    }

    /// The committed corpora are exactly what `emit-corpus` derives from the
    /// parity corpus: neither side can drift silently.
    #[test]
    fn committed_corpus_matches_emitted_corpus() {
        for (op, text) in emit_corpus().unwrap() {
            let path = ops_dir().join("corpus").join(format!("{op}.jsonl"));
            let committed = fs::read_to_string(&path).unwrap();
            assert!(
                committed == text,
                "{} drifted from `rivet-parity ops emit-corpus`",
                path.display()
            );
        }
    }

    /// Every ported packet struct and every codec in the Rust table is exercised
    /// by at least one recorded input (the per-subject rows of the exit criterion).
    #[test]
    fn every_ported_subject_has_recorded_inputs() {
        let fixtures = fixtures();
        let subjects_of = |op: &str| -> std::collections::BTreeSet<String> {
            fixtures
                .iter()
                .find(|f| f.op == op)
                .unwrap_or_else(|| panic!("{op} is not recorded"))
                .corpus
                .iter()
                .filter_map(|entry| handlers::subject(op, &entry.request))
                .collect()
        };
        let packets = subjects_of("packet.reencode");
        for entry in packet::entries() {
            assert!(
                packets.contains(entry.subject),
                "packet {} has no recorded input",
                entry.subject
            );
        }
        let codecs = subjects_of("codec.reencode");
        for entry in codec::entries() {
            assert!(
                codecs.contains(entry.name),
                "codec {} has no recorded input",
                entry.name
            );
        }
    }

    /// Every excluded input names a recorded input of its op, so a stale row
    /// cannot hide in the table.
    #[test]
    fn every_excluded_input_is_a_recorded_input() {
        let exclusions = compare::load_exclusions(&ops_dir().join("exclusions.tsv")).unwrap();
        let fixtures = fixtures();
        for exclusion in &exclusions {
            let Some(id) = exclusion.field.strip_prefix("input:") else {
                continue;
            };
            let fixture = fixtures
                .iter()
                .find(|f| f.op == exclusion.op)
                .unwrap_or_else(|| panic!("exclusion for unrecorded op {}", exclusion.op));
            assert!(
                fixture.corpus.iter().any(|entry| entry.id == id),
                "exclusion names {} which is not in the {} corpus",
                id,
                exclusion.op
            );
        }
    }

    #[test]
    fn manifest_tamper_is_detected() {
        let scratch = tempfile::tempdir().unwrap();
        rivet_harness_common::negative::copy_dir_recursive(&ops_dir(), scratch.path()).unwrap();
        // Change one recorded answer without re-recording.
        let target = scratch.path().join("snbt.parse.jsonl");
        let text = fs::read_to_string(&target)
            .unwrap()
            .replacen("\"ok\":true", "\"ok\":false", 1);
        fs::write(&target, text).unwrap();
        match load_fixtures(scratch.path(), &op_source()) {
            Err(LoadError::Invalid(m)) => assert!(m.contains("snbt.parse.jsonl"), "{m}"),
            other => panic!(
                "expected an invalid-fixture error, got {:?}",
                other.map(|f| f.len())
            ),
        }
    }

    #[test]
    fn unrecorded_corpus_is_rejected() {
        let scratch = tempfile::tempdir().unwrap();
        rivet_harness_common::negative::copy_dir_recursive(&ops_dir(), scratch.path()).unwrap();
        fs::write(
            scratch.path().join("corpus/extra.op.jsonl"),
            "{\"id\":\"x\"}\n",
        )
        .unwrap();
        match load_fixtures(scratch.path(), &op_source()) {
            Err(LoadError::Invalid(m)) => assert!(m.contains("unrecorded corpus"), "{m}"),
            other => panic!(
                "expected an invalid-fixture error, got {:?}",
                other.map(|f| f.len())
            ),
        }
    }

    #[test]
    fn changed_op_source_is_stale() {
        let scratch = tempfile::tempdir().unwrap();
        let source = scratch.path().join("RivetReferenceOracle.java");
        fs::write(&source, "// edited without re-recording\n").unwrap();
        match load_fixtures(&ops_dir(), &source) {
            Err(LoadError::Invalid(m)) => {
                assert!(m.contains("changed since the ops were recorded"), "{m}")
            }
            other => panic!(
                "expected a stale-source error, got {:?}",
                other.map(|f| f.len())
            ),
        }
    }

    #[test]
    fn live_tamper_line_never_equals_the_original() {
        for line in [
            r#"{"id":1,"protocol":1,"ok":true,"result":{"accept":true,"canonical":"{}"}}"#,
            r#"{"id":2,"protocol":1,"ok":false,"error":{"kind":"input_rejected","operation":"snbt.parse","type":"X","message":"m"}}"#,
        ] {
            let tampered = tamper_response_line(line);
            assert_ne!(
                Outcome::from_live("snbt.parse", &tampered),
                Outcome::from_live("snbt.parse", line)
            );
        }
    }
}
