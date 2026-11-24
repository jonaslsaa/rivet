//! `rivet-arena-trace` — the L4 arena tick-trace differential
//! (docs/VERIFICATION.md section 2.2 L4, item 7; `arenas/README.md`).
//!
//! ```text
//! rivet-arena-trace check  [--arena NAME]... [--ledger]   offline: Rivet vs the committed Paper traces
//! rivet-arena-trace record [--arena NAME]...              twin Paper boots -> admission -> commit traces
//! rivet-arena-trace live   [--arena NAME]... [--ledger]   twin Paper boots, then Rivet vs the fresh traces
//! rivet-arena-trace compare EXPECTED ACTUAL               first divergence between two trace files
//! rivet-arena-trace rivet --arena NAME [--out FILE]       the Rivet trace of one committed arena
//! ```
//!
//! Exit status: 0 when the harness is healthy (closure, admission and the
//! tamper negative all hold) and no arena that the ledger records as PASS has
//! regressed; 1 otherwise; 2 (UNVERIFIED) when a Paper prerequisite is
//! missing. An arena that diverges is a FAIL result, not a harness failure.

mod compare;

use compare::{Comparison, compare as compare_traces, tamper};
use rivet_server::server::level::arena_trace::{self, ArenaHeader, Exclusions};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PAPER_COMMIT: &str = "0a99345";

fn oracle_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn repo_dir() -> PathBuf {
    oracle_dir().join("../..")
}
fn arenas_dir() -> PathBuf {
    oracle_dir().join("arenas")
}
fn paper_dir() -> PathBuf {
    arenas_dir().join("paper")
}
fn work_dir() -> PathBuf {
    std::env::var_os("RIVET_ARENA_WORK")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_dir().join("work/arena-trace"))
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The declared corpus: `arenas/arenas.tsv`, which must list exactly the
/// `arenas/*.json` files (closure: declared == present).
fn corpus() -> Result<Vec<String>, String> {
    let mut declared = Vec::new();
    for line in read(&arenas_dir().join("arenas.tsv"))?.lines().skip(1) {
        if let Some(name) = line.split('\t').next().filter(|n| !n.is_empty()) {
            declared.push(name.to_owned());
        }
    }
    let mut present: Vec<String> = std::fs::read_dir(arenas_dir())
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_suffix(".json")
                .map(str::to_owned)
        })
        .collect();
    present.sort();
    let mut sorted = declared.clone();
    sorted.sort();
    if sorted != present {
        return Err(format!(
            "arenas.tsv declares {sorted:?} but arenas/*.json holds {present:?}"
        ));
    }
    if declared.is_empty() {
        return Err("no arenas declared".into());
    }
    Ok(declared)
}

fn select(all: Vec<String>, wanted: &[String]) -> Result<Vec<String>, String> {
    if wanted.is_empty() {
        return Ok(all);
    }
    for name in wanted {
        if !all.contains(name) {
            return Err(format!("unknown arena {name}"));
        }
    }
    Ok(all.into_iter().filter(|n| wanted.contains(n)).collect())
}

fn exclusions() -> Result<Exclusions, String> {
    Exclusions::parse(&read(&arenas_dir().join("exclusions.tsv"))?)
}

fn probe_sha256() -> Result<String, String> {
    let java = std::fs::read(oracle_dir().join("src/java/arena/ArenaTraceProbe.java"))
        .map_err(|e| e.to_string())?;
    let yml =
        std::fs::read(oracle_dir().join("src/java/arena/plugin.yml")).map_err(|e| e.to_string())?;
    Ok(sha256(&[java, yml].concat()))
}

/// Structural checks on one Paper trace: header names this arena at the
/// current spec bytes, one tick record per declared tick, an end record.
fn validate_paper_trace(name: &str, trace: &str) -> Result<i32, String> {
    let spec =
        std::fs::read(arenas_dir().join(format!("{name}.json"))).map_err(|e| e.to_string())?;
    let mut lines = trace.lines();
    let header: serde_json::Value =
        serde_json::from_str(lines.next().ok_or("empty trace")?).map_err(|e| e.to_string())?;
    let parsed = ArenaHeader::parse(&header)?;
    if parsed.name != name {
        return Err(format!("{name}: header names arena {}", parsed.name));
    }
    if header["arena_sha256"] != sha256(&spec) {
        return Err(format!(
            "{name}: trace was recorded from different arena bytes; re-record"
        ));
    }
    let mut ticks = 0;
    let mut end = None;
    for line in lines {
        let v: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        match v["kind"].as_str() {
            Some("tick") if v["t"] == ticks => ticks += 1,
            Some("end") if end.is_none() => end = v["ticks"].as_i64(),
            _ => {
                return Err(format!(
                    "{name}: unexpected record {}",
                    compare::truncate(line, 80)
                ));
            }
        }
    }
    if end != Some(i64::from(parsed.ticks)) || ticks != i64::from(parsed.ticks) + 1 {
        return Err(format!(
            "{name}: closure: {ticks} tick records, end {end:?}, declared {}",
            parsed.ticks
        ));
    }
    Ok(parsed.ticks)
}

/// The Rivet trace for a Paper trace's header.
fn rivet_trace(paper: &str, exclusions: &Exclusions) -> Result<String, String> {
    let header: serde_json::Value =
        serde_json::from_str(paper.lines().next().ok_or("empty trace")?)
            .map_err(|e| e.to_string())?;
    let header = ArenaHeader::parse(&header)?;
    Ok(arena_trace::run(&header, exclusions)?.join("\n") + "\n")
}

struct Outcome {
    name: String,
    comparison: Comparison,
}

/// Rivet vs Paper for each arena, plus the tamper negative on each trace.
fn differential(
    names: &[String],
    traces: &BTreeMap<String, String>,
) -> Result<Vec<Outcome>, String> {
    let exclusions = exclusions()?;
    let mut outcomes = Vec::new();
    for name in names {
        let paper = &traces[name];
        let (tampered, t) = tamper(paper)?;
        let negative = compare_traces(paper, &tampered)?;
        match &negative.divergence {
            Some(d) if d.tick == Some(t) && d.path.starts_with(".blocks") => {}
            other => return Err(format!("{name}: tamper negative not detected ({other:?})")),
        }
        let rivet = rivet_trace(paper, &exclusions).map_err(|e| format!("{name}: harness: {e}"))?;
        let out = work_dir().join("rivet");
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        std::fs::write(out.join(format!("{name}.trace.jsonl")), &rivet)
            .map_err(|e| e.to_string())?;
        outcomes.push(Outcome {
            name: name.clone(),
            comparison: compare_traces(paper, &rivet)?,
        });
    }
    Ok(outcomes)
}

fn report(outcomes: &[Outcome]) {
    println!(
        "{:<26} {:<6} {:>9}  first divergence",
        "arena", "status", "matched"
    );
    for o in outcomes {
        let c = &o.comparison;
        println!(
            "{:<26} {:<6} {:>4}/{:<4}  {}",
            o.name,
            if c.passed() { "PASS" } else { "FAIL" },
            c.matched,
            c.declared,
            c.divergence
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default()
        );
    }
    let passed = outcomes.iter().filter(|o| o.comparison.passed()).count();
    println!(
        "arena-trace: {passed} PASS, {} FAIL of {} arenas (tamper negative detected on every trace)",
        outcomes.len() - passed,
        outcomes.len()
    );
}

fn git_head() -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_dir())
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn ledger_record(
    id: &str,
    status: &str,
    matched: &str,
    mismatched: &str,
    note: &str,
) -> Result<(), String> {
    let status_ok = Command::new("python3")
        .arg(repo_dir().join("scripts/verify_ledger.py"))
        .args([
            "record",
            id,
            "--status",
            status,
            "--matched",
            matched,
            "--mismatched",
            mismatched,
        ])
        .args([
            "--commit",
            &git_head()?,
            "--note",
            &note.replace(['\t', '\n'], " "),
        ])
        .status()
        .map_err(|e| e.to_string())?;
    status_ok
        .success()
        .then_some(())
        .ok_or_else(|| format!("verify_ledger.py record {id} failed"))
}

/// Ledger status of every `arena.*` row, to detect a PASS regression.
fn ledger_status() -> BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(repo_dir().join("VERIFY.tsv")) else {
        return BTreeMap::new();
    };
    text.lines()
        .filter_map(|l| {
            let cells: Vec<_> = l.split('\t').collect();
            (cells.len() > 9 && cells[0].starts_with("arena."))
                .then(|| (cells[0].to_owned(), cells[9].to_owned()))
        })
        .collect()
}

fn write_ledger(outcomes: &[Outcome], how: &str) -> Result<(), String> {
    for o in outcomes {
        let c = &o.comparison;
        let note = match &c.divergence {
            None => format!(
                "{how}: Rivet trace identical to Paper for all {} tick records",
                c.declared
            ),
            Some(d) => format!("{how}: first divergence {d}"),
        };
        ledger_record(
            &format!("arena.{}", o.name),
            if c.passed() { "PASS" } else { "FAIL" },
            &c.matched.to_string(),
            &c.mismatched().to_string(),
            &compare::truncate(&note, 400),
        )?;
    }
    Ok(())
}

fn regressions(outcomes: &[Outcome]) -> Vec<String> {
    let ledger = ledger_status();
    outcomes
        .iter()
        .filter(|o| !o.comparison.passed())
        .filter(|o| ledger.get(&format!("arena.{}", o.name)).map(String::as_str) == Some("PASS"))
        .map(|o| o.name.clone())
        .collect()
}

fn committed_traces(names: &[String]) -> Result<BTreeMap<String, String>, String> {
    let manifest: serde_json::Value =
        serde_json::from_str(&read(&paper_dir().join("manifest.json"))?)
            .map_err(|e| e.to_string())?;
    if manifest["paper_commit"] != PAPER_COMMIT || manifest["twin_identical"] != true {
        return Err("paper/manifest.json is not a twin-admitted Paper 0a99345 recording".into());
    }
    if manifest["probe_sha256"] != probe_sha256()? {
        return Err("ArenaTraceProbe changed since the traces were recorded; re-record".into());
    }
    let mut traces = BTreeMap::new();
    for name in names {
        let text = read(&paper_dir().join(format!("{name}.trace.jsonl")))?;
        if manifest["arenas"][name]["trace_sha256"] != sha256(text.as_bytes()) {
            return Err(format!(
                "{name}: committed trace does not match paper/manifest.json"
            ));
        }
        validate_paper_trace(name, &text)?;
        traces.insert(name.clone(), text);
    }
    Ok(traces)
}

/// One fresh Paper boot over `names`; returns the trace texts.
fn paper_run(names: &[String], label: &str) -> Result<BTreeMap<String, String>, (u8, String)> {
    let out = work_dir().join(label);
    let status = Command::new("bash")
        .arg(oracle_dir().join("scripts/arena_trace_paper.sh"))
        .arg(&out)
        .arg(names.join(","))
        .status()
        .map_err(|e| (1, e.to_string()))?;
    match status.code() {
        Some(0) => {}
        Some(2) => return Err((2, "Paper runtime prerequisites missing".into())),
        _ => return Err((1, format!("Paper arena boot {label} failed"))),
    }
    let mut traces = BTreeMap::new();
    for name in names {
        let text =
            read(&out.join("traces").join(format!("{name}.trace.jsonl"))).map_err(|e| (1, e))?;
        validate_paper_trace(name, &text).map_err(|e| (1, e))?;
        traces.insert(name.clone(), text);
    }
    Ok(traces)
}

/// Admission (VERIFICATION 2.5): two fresh Paper boots must agree byte for
/// byte on every arena before any Paper-vs-Rivet comparison counts.
fn twin(names: &[String]) -> Result<BTreeMap<String, String>, (u8, String)> {
    let a = paper_run(names, "paper-a")?;
    let b = paper_run(names, "paper-b")?;
    for name in names {
        if a[name] != b[name] {
            let c = compare_traces(&a[name], &b[name]).map_err(|e| (1, e))?;
            return Err((
                1,
                format!("{name}: Paper twin runs differ: {:?}", c.divergence),
            ));
        }
    }
    Ok(a)
}

fn record(names: &[String]) -> Result<(), (u8, String)> {
    let traces = twin(names)?;
    let io = |e: std::io::Error| (1, e.to_string());
    std::fs::create_dir_all(paper_dir()).map_err(io)?;
    let manifest_path = paper_dir().join("manifest.json");
    let mut manifest: serde_json::Value = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let probe = probe_sha256().map_err(|e| (1, e))?;
    if manifest["probe_sha256"] != probe {
        // A new probe invalidates every older recording: re-record them all.
        let all = corpus().map_err(|e| (1, e))?;
        if names.len() != all.len() {
            return Err((
                1,
                "the probe changed: record every arena (no --arena)".into(),
            ));
        }
        manifest["arenas"] = serde_json::json!({});
    }
    manifest["schema"] = arena_trace::SCHEMA.into();
    manifest["paper_commit"] = PAPER_COMMIT.into();
    manifest["probe_sha256"] = probe.into();
    manifest["twin_identical"] = true.into();
    for (name, text) in &traces {
        std::fs::write(paper_dir().join(format!("{name}.trace.jsonl")), text).map_err(io)?;
        let spec = std::fs::read(arenas_dir().join(format!("{name}.json"))).map_err(io)?;
        manifest["arenas"][name] = serde_json::json!({
            "arena_sha256": sha256(&spec),
            "trace_sha256": sha256(text.as_bytes()),
            "trace_lines": text.lines().count(),
        });
    }
    let arenas = manifest["arenas"].as_object_mut().expect("object");
    arenas.sort_keys();
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("json") + "\n",
    )
    .map_err(io)?;
    println!(
        "recorded {} twin-identical Paper arena traces",
        traces.len()
    );
    Ok(())
}

fn run() -> Result<(), (u8, String)> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fail = |e: String| (1u8, e);
    let command = args.first().map(String::as_str).unwrap_or("");
    let mut arenas = Vec::new();
    let mut ledger = false;
    let mut out = None;
    let mut positional = Vec::new();
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--arena" => arenas.push(
                rest.next()
                    .ok_or((1, "--arena needs a name".into()))?
                    .clone(),
            ),
            "--ledger" => ledger = true,
            "--out" => {
                out = Some(PathBuf::from(
                    rest.next().ok_or((1, "--out needs a path".into()))?,
                ))
            }
            other if other.starts_with("--") => return Err((1, format!("unknown flag {other}"))),
            other => positional.push(other.to_owned()),
        }
    }
    match command {
        "check" | "live" => {
            let names = select(corpus().map_err(fail)?, &arenas).map_err(fail)?;
            let traces = if command == "check" {
                committed_traces(&names).map_err(fail)?
            } else {
                let fresh = twin(&names)?;
                // Record mode re-run must reproduce the committed recordings
                // byte for byte (VERIFICATION 2.1): drift is a harness failure.
                for name in &names {
                    let committed = paper_dir().join(format!("{name}.trace.jsonl"));
                    if committed.exists() && read(&committed).map_err(fail)? != fresh[name] {
                        return Err((
                            1,
                            format!(
                                "{name}: fresh Paper trace differs from the committed recording; re-record"
                            ),
                        ));
                    }
                }
                fresh
            };
            let outcomes = differential(&names, &traces).map_err(fail)?;
            report(&outcomes);
            if ledger {
                let how = if command == "check" {
                    "vs committed twin-admitted Paper traces"
                } else {
                    "live vs fresh twin-admitted Paper boots"
                };
                write_ledger(&outcomes, how).map_err(fail)?;
                if arenas.is_empty() {
                    ledger_record(
                        "arena.harness",
                        "PASS",
                        &outcomes.len().to_string(),
                        "0",
                        &format!(
                            "{command}: {} arenas, Paper twin admission byte-identical, closure held, tamper negative detected on every trace",
                            outcomes.len()
                        ),
                    )
                    .map_err(fail)?;
                }
            }
            let regressed = regressions(&outcomes);
            if !regressed.is_empty() {
                return Err((1, format!("ledger PASS arenas regressed: {regressed:?}")));
            }
            Ok(())
        }
        "record" => record(&select(corpus().map_err(fail)?, &arenas).map_err(fail)?),
        "compare" => {
            let [expected, actual] = positional.as_slice() else {
                return Err((1, "compare EXPECTED ACTUAL".into()));
            };
            let c = compare_traces(
                &read(Path::new(expected)).map_err(fail)?,
                &read(Path::new(actual)).map_err(fail)?,
            )
            .map_err(fail)?;
            match &c.divergence {
                None => println!("identical: {}/{} tick records", c.matched, c.declared),
                Some(d) => {
                    println!(
                        "diverged after {}/{} tick records: {d}",
                        c.matched, c.declared
                    );
                    return Err((1, "traces differ".into()));
                }
            }
            Ok(())
        }
        "rivet" => {
            let [name] = arenas.as_slice() else {
                return Err((1, "rivet --arena NAME".into()));
            };
            let paper = committed_traces(std::slice::from_ref(name)).map_err(fail)?;
            let trace = rivet_trace(&paper[name], &exclusions().map_err(fail)?).map_err(fail)?;
            match out {
                Some(path) => std::fs::write(&path, trace).map_err(|e| (1, e.to_string()))?,
                None => print!("{trace}"),
            }
            Ok(())
        }
        _ => Err((
            1,
            "usage: rivet-arena-trace check|record|live|compare|rivet (see the module docs)".into(),
        )),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            if code == 2 {
                eprintln!("UNVERIFIED: {message}");
            } else {
                eprintln!("rivet-arena-trace: {message}");
            }
            ExitCode::from(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Closure + provenance of the committed corpus: every declared arena has
    /// a twin-admitted Paper trace matching the manifest, the arena bytes and
    /// the current probe.
    #[test]
    fn committed_paper_traces_are_closed_and_match_their_manifest() {
        let names = corpus().unwrap();
        let traces = committed_traces(&names).unwrap();
        assert_eq!(traces.len(), names.len());
        assert!(
            names.len() >= 10,
            "VERIFICATION item 7 needs at least 10 arenas"
        );
    }

    /// The tamper negative fires on every committed trace at the tampered
    /// tick, so a comparator that passes vacuously cannot go unnoticed.
    #[test]
    fn tamper_negative_is_detected_on_every_committed_trace() {
        let names = corpus().unwrap();
        let traces = committed_traces(&names).unwrap();
        for (name, paper) in &traces {
            let (tampered, t) = tamper(paper).unwrap();
            let d = compare_traces(paper, &tampered)
                .unwrap()
                .divergence
                .expect(name);
            assert_eq!(d.tick, Some(t), "{name}");
            assert!(compare_traces(paper, paper).unwrap().passed(), "{name}");
        }
    }

    /// The Rivet runner sets up every committed arena and produces a trace
    /// that either runs to its end record or stops with a named refusal. This
    /// asserts the harness, not parity: PASS/FAIL per arena is the ledger's.
    #[test]
    fn rivet_runner_replays_every_committed_arena() {
        let names = corpus().unwrap();
        let traces = committed_traces(&names).unwrap();
        let exclusions = exclusions().unwrap();
        for (name, paper) in &traces {
            let rivet = rivet_trace(paper, &exclusions).unwrap_or_else(|e| panic!("{name}: {e}"));
            let last: serde_json::Value =
                serde_json::from_str(rivet.lines().last().unwrap()).unwrap();
            assert!(
                matches!(last["kind"].as_str(), Some("end" | "error")),
                "{name}: {last}"
            );
            let header = |t: &str| compare::canonical(t.lines().next().unwrap()).unwrap();
            assert_eq!(
                header(&rivet),
                header(paper),
                "{name}: the runner echoes the Paper header"
            );
        }
    }
}
