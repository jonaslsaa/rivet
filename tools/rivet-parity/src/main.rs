//! Byte-for-byte NBT/SNBT parity diff between rivet-nbt and the Paper Java
//! oracle (`tools/rivet-reference-oracle`).
//!
//! Corpus: the 432 committed M0 chunk-NBT fixtures plus hand-built SNBT inputs
//! exercising every tag type, numeric suffix, array form, string quoting,
//! Unicode, and the pretty printer's `KEY_ORDER` / `no_indentation` paths.
//!
//! Sections:
//! - `snbt.parse`   — SNBT -> canonical + pretty + tag type, oracle vs Rust.
//! - `nbt.decode`   — binary NBT -> canonical + pretty, oracle vs Rust.
//! - `nbt.encode`   — compound SNBT -> binary NBT, oracle vs Rust (byte-for-byte
//!   for single-key-deep compounds; semantic for multi-key, where the binary
//!   field order is the documented HashMap-iteration-order divergence).
//! - `idem`         — Rust-internal read->write->read structural idempotence.
//!
//! Output: JSON Lines on stdout (one object per check + a `stats` summary), a
//! human summary on stderr. Run `cargo run -p rivet-parity -- --help`.

mod corpus;
mod oracle;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::snbt_printer_tag_visitor::SnbtPrinterTagVisitor;
use rivet_nbt::string_tag_visitor::StringTagVisitor;
use rivet_nbt::tag::Tag;
use rivet_nbt::tag_parser::TagParser;
use rivet_util::{DataInputStream, DataOutputStream};
use serde_json::{Value, json};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// One checked comparison between the oracle and Rust.
struct Check {
    kind: &'static str,
    id: String,
    input: Option<String>,
    ok: bool,
    skipped: bool,
    divergences: Vec<String>,
    fields: Vec<Value>,
    note: Option<String>,
}

impl Check {
    fn new(kind: &'static str, id: String, input: Option<String>) -> Self {
        Check {
            kind,
            id,
            input,
            ok: true,
            skipped: false,
            divergences: Vec::new(),
            fields: Vec::new(),
            note: None,
        }
    }

    /// Compare a required field; a mismatch fails the check.
    fn field(&mut self, name: &str, expected: &str, got: &str) {
        let ok = expected == got;
        self.ok &= ok;
        let mut f = json!({ "name": name, "ok": ok });
        if !ok {
            f["expected"] = json!(expected);
            f["got"] = json!(got);
        }
        self.fields.push(f);
    }

    /// Compare a soft field; a mismatch is reported but does not fail the check.
    fn field_soft(&mut self, name: &str, expected: &str, got: &str) {
        let ok = expected == got;
        let mut f = json!({ "name": name, "ok": ok, "soft": true });
        if !ok {
            f["expected"] = json!(expected);
            f["got"] = json!(got);
        }
        self.fields.push(f);
    }

    fn divergence(&mut self, name: &str) {
        self.divergences.push(name.to_string());
    }

    fn note(&mut self, text: &str) {
        self.note = Some(text.to_string());
    }

    fn skip(&mut self, why: &str) {
        self.skipped = true;
        self.note(why);
    }

    fn finish(self) -> Value {
        let mut out = json!({
            "kind": self.kind,
            "id": self.id,
            "ok": self.ok,
            "skipped": self.skipped,
            "divergences": self.divergences,
            "fields": self.fields,
        });
        if let Some(input) = self.input {
            out["input"] = json!(input);
        }
        if let Some(note) = self.note {
            out["note"] = json!(note);
        }
        out
    }
}

/// Aggregated per-kind counts for the human summary.
#[derive(Default)]
struct Summary {
    totals: BTreeMap<String, usize>,
    matched: BTreeMap<String, usize>,
    diverged: BTreeMap<String, usize>,
    mismatched: BTreeMap<String, usize>,
    skipped: BTreeMap<String, usize>,
    hard_ids: Vec<String>,
}

impl Summary {
    fn record(&mut self, check: &Value) {
        let kind = check["kind"].as_str().unwrap_or("?").to_string();
        let ok = check["ok"].as_bool().unwrap_or(false);
        let skipped = check["skipped"].as_bool().unwrap_or(false);
        let divergences = check["divergences"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        let id = check["id"].as_str().unwrap_or("?").to_string();

        *self.totals.entry(kind.clone()).or_insert(0) += 1;
        if skipped {
            *self.skipped.entry(kind).or_insert(0) += 1;
        } else if !ok {
            *self.mismatched.entry(kind).or_insert(0) += 1;
            self.hard_ids.push(id);
        } else if divergences > 0 {
            *self.diverged.entry(kind).or_insert(0) += 1;
        } else {
            *self.matched.entry(kind).or_insert(0) += 1;
        }
    }
}

// ---- Rust-side NBT/SNBT helpers ----

struct RustDescribe {
    canonical: String,
    pretty: String,
    type_name: String,
    id: i8,
}

fn describe_tag(tag: &Tag) -> RustDescribe {
    RustDescribe {
        canonical: StringTagVisitor::to_string(tag),
        pretty: SnbtPrinterTagVisitor::new().visit(tag),
        type_name: tag.get_type().name(),
        id: tag.id(),
    }
}

fn describe_compound(compound: &CompoundTag) -> RustDescribe {
    describe_tag(&Tag::Compound(compound.clone()))
}

fn rust_read_compound(bytes: &[u8]) -> Result<CompoundTag, String> {
    let mut input = DataInputStream::new(std::io::Cursor::new(bytes));
    nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).map_err(|e| e.to_string())
}

fn rust_parse_snbt(input: &str) -> Result<Tag, String> {
    TagParser::create(NbtOps::instance())
        .parse_fully(input)
        .map_err(|e| e.to_string())
}

fn rust_encode_compound(input: &str) -> Result<Vec<u8>, String> {
    let compound = tag_parser_parse_compound(input)?;
    let mut buf: Vec<u8> = Vec::new();
    let mut out = DataOutputStream::new(&mut buf);
    nbt_io::write(&compound, &mut out).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn tag_parser_parse_compound(input: &str) -> Result<CompoundTag, String> {
    rivet_nbt::tag_parser::parse_compound_fully(input).map_err(|e| e.to_string())
}

/// Every compound at every nesting level has at most one key -> binary field
/// order is trivially deterministic on both sides.
fn is_single_key_deep(tag: &Tag) -> bool {
    match tag {
        Tag::Compound(c) => {
            if c.size() > 1 {
                return false;
            }
            c.values().all(is_single_key_deep)
        }
        Tag::List(l) => l.iter().all(is_single_key_deep),
        _ => true,
    }
}

// ---- section runners ----

fn check_snbt_parse(
    oracle: Option<&mut oracle::Oracle>,
    id: &str,
    input: &str,
    expected_accept: bool,
) -> Value {
    let mut check = Check::new("snbt.parse", id.to_string(), Some(input.to_string()));
    let rust = rust_parse_snbt(input);

    let Some(handle) = oracle else {
        check.skip("oracle unavailable");
        return check.finish();
    };
    match handle.call("snbt.parse", &[("input", input)]) {
        Ok(result) => {
            if !expected_accept {
                // The oracle accepted an input we classified as invalid: flag it.
                check.field("reject", "rejected", "accepted");
                return check.finish();
            }
            match &rust {
                Ok(tag) => {
                    let rd = describe_tag(tag);
                    let oc = result["snbt"].as_str().unwrap_or("");
                    let op = result["pretty_snbt"].as_str().unwrap_or("");
                    let ot = result["tag_type"].as_str().unwrap_or("");
                    let oi = result["tag_id"].as_i64().unwrap_or(-1);
                    check.field("tag_type", ot, &rd.type_name);
                    check.field("tag_id", &oi.to_string(), &rd.id.to_string());
                    check.field("canonical", oc, &rd.canonical);
                    check.field("pretty", op, &rd.pretty);
                }
                Err(rust_err) => {
                    check.field("accept", "true", "false");
                    check.note(&format!("rust parse error: {rust_err}"));
                }
            }
        }
        Err(oracle_err) => match &rust {
            Ok(_) => {
                check.field("accept", "true", "false");
                check.note(&format!("oracle error: {oracle_err}"));
            }
            Err(rust_err) => {
                // Both rejected. Accept/reject parity holds; error text is soft.
                check.field_soft("error_text", "rejected", "rejected");
                let _ = rust_err;
                check.note(&format!("both rejected; oracle error: {oracle_err}"));
            }
        },
    }
    check.finish()
}

fn check_nbt_decode(
    oracle: Option<&mut oracle::Oracle>,
    id: &str,
    label: &str,
    bytes: &[u8],
) -> Value {
    let mut check = Check::new("nbt.decode", id.to_string(), Some(label.to_string()));
    let rust = rust_read_compound(bytes);

    let Some(handle) = oracle else {
        match &rust {
            Ok(c) => {
                let _ = describe_compound(c);
            }
            Err(_) => check.field("rust_read", "ok", "err"),
        }
        check.skip("oracle unavailable");
        return check.finish();
    };

    let base64 = B64.encode(bytes);
    match handle.call("nbt.decode", &[("input_base64", &base64)]) {
        Ok(result) => match &rust {
            Ok(compound) => {
                let rd = describe_compound(compound);
                let oc = result["snbt"].as_str().unwrap_or("");
                let op = result["pretty_snbt"].as_str().unwrap_or("");
                let ot = result["tag_type"].as_str().unwrap_or("");
                let oi = result["tag_id"].as_i64().unwrap_or(-1);
                check.field("tag_type", ot, &rd.type_name);
                check.field("tag_id", &oi.to_string(), &rd.id.to_string());
                check.field("canonical", oc, &rd.canonical);
                check.field("pretty", op, &rd.pretty);
            }
            Err(rust_err) => {
                check.field("rust_read", "ok", "err");
                check.note(&format!("rust read error: {rust_err}"));
            }
        },
        Err(oracle_err) => match &rust {
            Ok(_) => {
                check.field("oracle_read", "ok", "err");
                check.note(&format!("oracle error: {oracle_err}"));
            }
            Err(_) => check.note(&format!("both rejected; oracle error: {oracle_err}")),
        },
    }
    check.finish()
}

fn check_nbt_encode(
    oracle: Option<&mut oracle::Oracle>,
    id: &str,
    input: &str,
    single_key: bool,
) -> Value {
    let mut check = Check::new("nbt.encode", id.to_string(), Some(input.to_string()));
    let rust_bytes = rust_encode_compound(input);

    let Some(handle) = oracle else {
        match &rust_bytes {
            Ok(_) => {}
            Err(_) => check.field("rust_encode", "ok", "err"),
        }
        check.skip("oracle unavailable");
        return check.finish();
    };

    match handle.call("nbt.encode", &[("input", input)]) {
        Ok(result) => match &rust_bytes {
            Ok(rbytes) => {
                let oc = result["snbt"].as_str().unwrap_or("");
                let op = result["pretty_snbt"].as_str().unwrap_or("");
                let ot = result["tag_type"].as_str().unwrap_or("");
                let oi = result["tag_id"].as_i64().unwrap_or(-1);
                let oracle_len = result["bytes"].as_i64().unwrap_or(-1);

                let rust_tag = match rust_parse_snbt(input) {
                    Ok(tag) => tag,
                    Err(err) => {
                        // `rust_bytes` succeeded above, so this is genuinely
                        // unreachable — but a panic here would truncate the
                        // transcript, so fail this check instead.
                        check.field("rust_parse", "ok", &err);
                        return check.finish();
                    }
                };
                let rd = describe_tag(&rust_tag);
                check.field("tag_type", ot, &rd.type_name);
                check.field("tag_id", &oi.to_string(), &rd.id.to_string());
                check.field("canonical", oc, &rd.canonical);
                check.field("pretty", op, &rd.pretty);
                check.field(
                    "bytes_len",
                    &oracle_len.to_string(),
                    &rbytes.len().to_string(),
                );

                // Binary field order.
                let oracle_bytes = match B64.decode(result["output_base64"].as_str().unwrap_or(""))
                {
                    Ok(b) => b,
                    Err(e) => {
                        check.field("oracle_base64", "valid", "invalid");
                        check.note(&format!("oracle base64 decode: {e}"));
                        return check.finish();
                    }
                };
                if single_key {
                    // Single-key-deep: byte order is deterministic on both sides,
                    // so differing bytes are a real encoding bug (byte_for_byte is
                    // binding).
                    if rbytes == &oracle_bytes {
                        check.field("byte_for_byte", "match", "match");
                    } else {
                        check.field(
                            "byte_for_byte",
                            &B64.encode(&oracle_bytes),
                            &B64.encode(rbytes),
                        );
                        check.note("single-key-deep compound: binary bytes must match");
                    }
                } else {
                    // Multi-key compound: binary field order is the documented
                    // HashMap-iteration-order divergence (Java fastutil order vs
                    // Rust's randomized HashMap seed), so byte_for_byte is not
                    // binding. It is reported as a soft field (this run may
                    // coincidentally match), and `semantic` below — both binaries
                    // re-read to the same canonical SNBT — is the binding check.
                    check.divergence("compound_key_order");
                    if rbytes == &oracle_bytes {
                        check.field_soft("byte_for_byte", "match", "match");
                    } else {
                        check.field_soft(
                            "byte_for_byte",
                            &B64.encode(&oracle_bytes),
                            &B64.encode(rbytes),
                        );
                    }
                }

                // Semantic: both binaries must read back to the same canonical SNBT.
                match (
                    rust_read_compound(&oracle_bytes),
                    rust_read_compound(rbytes),
                ) {
                    (Ok(o), Ok(r)) => {
                        let o_canon = describe_compound(&o).canonical;
                        let r_canon = describe_compound(&r).canonical;
                        check.field("semantic", &o_canon, &r_canon);
                    }
                    (Err(e1), _) => {
                        check.field("semantic_read_oracle", "ok", "err");
                        check.note(&format!("oracle bytes unreadable in rust: {e1}"));
                    }
                    (_, Err(e2)) => {
                        check.field("semantic_read_rust", "ok", "err");
                        check.note(&format!("rust bytes unreadable in rust: {e2}"));
                    }
                }
            }
            Err(rust_err) => {
                check.field("rust_encode", "ok", "err");
                check.note(&format!("rust encode error: {rust_err}"));
            }
        },
        Err(oracle_err) => match &rust_bytes {
            Ok(_) => {
                check.field("oracle_encode", "ok", "err");
                check.note(&format!("oracle error: {oracle_err}"));
            }
            Err(_) => check.note(&format!("both rejected; oracle error: {oracle_err}")),
        },
    }
    check.finish()
}

/// Rust-internal read->write->read structural idempotence.
fn check_idem(id: &str, label: &str, bytes: &[u8]) -> Value {
    let mut check = Check::new("idem", id.to_string(), Some(label.to_string()));
    match rust_read_compound(bytes) {
        Err(e) => {
            check.field("read", "ok", "err");
            check.note(&format!("rust read error: {e}"));
            return check.finish();
        }
        Ok(first) => {
            let mut buf: Vec<u8> = Vec::new();
            {
                let mut out = DataOutputStream::new(&mut buf);
                if let Err(e) = nbt_io::write(&first, &mut out) {
                    check.field("write", "ok", "err");
                    check.note(&format!("rust write error: {e}"));
                    return check.finish();
                }
            }
            match rust_read_compound(&buf) {
                Err(e) => {
                    check.field("read_after_write", "ok", "err");
                    check.note(&format!("rust read-after-write error: {e}"));
                }
                Ok(second) => {
                    let ok = first == second;
                    check.field("structural", "equal", if ok { "equal" } else { "differ" });
                    if !ok {
                        check.note("read->write->read changed the tag tree");
                    }
                }
            }
        }
    }
    check.finish()
}

// ---- main ----

/// Workspace-root `PARITY.md` scoreboard path. Points at the workspace root
/// (this crate is `tools/rivet-parity`, two levels down), so the file is
/// written in-place even when run from a worktree.
fn scoreboard_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("rivet-parity is exactly two levels under the workspace root")
        .join("PARITY.md")
}

/// Emit or refresh the workspace-root `PARITY.md` scoreboard.
///
/// Sections are driven purely by the live run's stats, so a check that stops
/// being exercised disappears from the scoreboard and a red gate (hard
/// mismatches) leaves the `mismatched` column visibly nonzero instead of
/// writing a green-washed table. When `fixture_cap` is set, a provenance note
/// is appended so a capped snapshot is not mistaken for full-corpus coverage.
fn write_scoreboard(summary: &Summary, fixture_cap: Option<usize>) {
    let mut rows: BTreeMap<String, (usize, usize, usize, usize)> = BTreeMap::new();
    for kind in ["snbt.parse", "nbt.decode", "nbt.encode", "idem"] {
        let total = summary.totals.get(kind).copied().unwrap_or(0);
        let skipped = summary.skipped.get(kind).copied().unwrap_or(0);
        // Skipped checks (e.g. oracle unavailable) were not measured; a row of
        // zeros would read as a total parity failure. Render only what ran.
        if total == 0 || total == skipped {
            continue;
        }
        rows.insert(
            format!("rivet-nbt:{kind}"),
            (
                total - skipped,
                summary.matched.get(kind).copied().unwrap_or(0),
                summary.diverged.get(kind).copied().unwrap_or(0),
                summary.mismatched.get(kind).copied().unwrap_or(0),
            ),
        );
    }

    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut md = String::new();
    md.push_str("# PARITY scoreboard\n\n");
    md.push_str(&format!(
        "Differential parity vs the pinned Paper Java oracle. Refreshed by the \
         `rivet-parity` tool (`cargo run -p rivet-parity -- --scoreboard`). \
         _Run date: {date}_\n\n"
    ));
    md.push_str("| crate/check | inputs | matched | diverged | mismatched | date |\n");
    md.push_str("|---|---|---|---|---|---|\n");
    for (check, (total, matched, diverged, mismatched)) in &rows {
        md.push_str(&format!(
            "| {check} | {total} | {matched} | {diverged} | {mismatched} | {date} |\n"
        ));
    }
    if let Some(cap) = fixture_cap {
        md.push_str(&format!(
            "\n_Snapshot: fixture-backed rows above were generated with `--limit-fixtures={cap}` \
             (a deliberate capped run); the full corpus is the 432 committed M0 chunk-NBT fixtures._\n"
        ));
    }
    md.push_str("\n### Divergences\n\n");
    md.push_str("`compound_key_order` is the documented HashMap-iteration-order divergence; all such checks remain `ok` and are counted under `diverged`, never under `mismatched`.\n");

    let path = scoreboard_path();
    match std::fs::write(&path, md) {
        Ok(()) => eprintln!("[rivet-parity] scoreboard written to {}", path.display()),
        Err(e) => eprintln!("[rivet-parity] scoreboard write failed: {e}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut limit_fixtures: Option<usize> = None;
    let mut no_oracle = false;
    let mut write_scoreboard_flag = false;
    for arg in &args[1..] {
        match arg.as_str() {
            "--no-oracle" => no_oracle = true,
            "--scoreboard" => write_scoreboard_flag = true,
            "--help" | "-h" => {
                eprintln!(
                    "rivet-parity: byte-for-byte NBT/SNBT parity diff vs the Paper Java oracle\n\
                     \n\
                     usage: cargo run -p rivet-parity [--no-oracle] [--scoreboard] [--limit-fixtures N]\n\
                     \n\
                     JSON-Lines transcript on stdout, human summary on stderr.\n\
                     --scoreboard also emits/refreshes PARITY.md at the workspace root.\n\
                     The oracle needs the M0 Paper runtime; point RIVET_PAPER_JAR,\n\
                     RIVET_PAPER_LIBRARIES, RIVET_PAPER_RUNTIME_JAR at it."
                );
                return;
            }
            other => {
                if let Some(n) = other.strip_prefix("--limit-fixtures=") {
                    limit_fixtures = n.parse().ok();
                } else {
                    eprintln!("unknown argument: {other}");
                    std::process::exit(2);
                }
            }
        }
    }

    let mut summary = Summary::default();
    let mut transcript: Vec<Value> = Vec::new();

    let mut oracle_handle = if no_oracle {
        None
    } else {
        match oracle::Oracle::spawn() {
            Ok(mut o) => {
                match o.provenance() {
                    Ok(p) => eprintln!(
                        "[rivet-parity] oracle: Paper {} ({}) sha256 {}",
                        p["paper_implementation"].as_str().unwrap_or("?"),
                        p["paper_commit"].as_str().unwrap_or("?"),
                        p["paper_sha256"]
                            .as_str()
                            .unwrap_or("?")
                            .get(..12)
                            .unwrap_or("?"),
                    ),
                    Err(e) => eprintln!("[rivet-parity] oracle ping warning: {e}"),
                }
                Some(o)
            }
            Err(e) => {
                eprintln!("[rivet-parity] ORACLE BLOCKER: {e}");
                eprintln!(
                    "[rivet-parity] continuing with Rust-only checks (idem + internal round-trips)"
                );
                None
            }
        }
    };
    let mut oracle = oracle_handle.as_mut();

    // ---- snbt.parse: hand-built corpus ----
    for (label, input) in corpus::parse_corpus() {
        let check = check_snbt_parse(
            oracle.as_deref_mut(),
            &format!("parse.{label}"),
            input,
            true,
        );
        summary.record(&check);
        transcript.push(check);
    }

    // ---- snbt.parse: deliberately-invalid corpus (accept/reject parity) ----
    for (label, input) in corpus::invalid_corpus() {
        let check = check_snbt_parse(
            oracle.as_deref_mut(),
            &format!("parse-invalid.{label}"),
            input,
            false,
        );
        summary.record(&check);
        transcript.push(check);
    }

    // ---- fixtures ----
    let fixtures = match corpus::fixtures_dir() {
        Some(dir) => {
            let chunk_root = dir.clone();
            let mut all = corpus::collect_fixtures(&dir);
            if let Some(n) = limit_fixtures {
                all.truncate(n);
            }
            eprintln!(
                "[rivet-parity] fixtures: {} chunk-NBT files under {}",
                all.len(),
                dir.display()
            );
            Some((chunk_root, all))
        }
        None => {
            eprintln!("[rivet-parity] M0 fixtures not present; skipping fixture sections");
            None
        }
    };

    if let Some((chunk_root, all)) = &fixtures {
        for path in all {
            let label = corpus::fixture_label(chunk_root, path);
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("[rivet-parity] cannot read fixture {}: {e}", path.display());
                    continue;
                }
            };

            // Binary -> canonical (oracle nbt.decode vs rust read+print).
            let check = check_nbt_decode(
                oracle.as_deref_mut(),
                &format!("decode.{label}"),
                &label,
                &bytes,
            );
            summary.record(&check);
            transcript.push(check);

            // Rust internal idempotence (always runs).
            let check = check_idem(&format!("idem.{label}"), &label, &bytes);
            summary.record(&check);
            transcript.push(check);
        }
    }

    // ---- snbt.parse of fixture canonical SNBT + nbt.encode of it ----
    // (run after decode so the corpus is the decoded canonical; needs the
    // canonical which we compute in Rust and feed to both sides).
    if let Some((chunk_root, all)) = &fixtures {
        for path in all {
            let label = corpus::fixture_label(chunk_root, path);
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let Ok(compound) = rust_read_compound(&bytes) else {
                continue;
            };
            let canonical = describe_compound(&compound).canonical;

            let check = check_snbt_parse(
                oracle.as_deref_mut(),
                &format!("parse-fixture.{label}"),
                &canonical,
                true,
            );
            summary.record(&check);
            transcript.push(check);

            let single_key = is_single_key_deep(&Tag::Compound(compound.clone()));
            let check = check_nbt_encode(
                oracle.as_deref_mut(),
                &format!("encode-fixture.{label}"),
                &canonical,
                single_key,
            );
            summary.record(&check);
            transcript.push(check);
        }
    }

    // ---- nbt.encode: hand-built corpus ----
    for (label, input) in corpus::encode_corpus() {
        let single_key = rust_parse_snbt(&input)
            .map(|t| is_single_key_deep(&t))
            .unwrap_or(false);
        let check = check_nbt_encode(
            oracle.as_deref_mut(),
            &format!("encode.{label}"),
            &input,
            single_key,
        );
        summary.record(&check);
        transcript.push(check);
    }

    // ---- emit transcript + summary ----
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for check in &transcript {
        let line = serde_json::to_string(check).expect("serialize check");
        let _ = writeln!(out, "{line}");
    }

    let stats = json!({
        "total": transcript.len(),
        "matched": summary.matched.values().sum::<usize>(),
        "diverged": summary.diverged.values().sum::<usize>(),
        "mismatched": summary.mismatched.values().sum::<usize>(),
        "skipped": summary.skipped.values().sum::<usize>(),
    });
    let _ = writeln!(
        out,
        "{}",
        serde_json::to_string(&json!({"kind": "stats", "stats": stats})).expect("stats")
    );
    let _ = out.flush();

    // Refresh the checked-in PARITY.md scoreboard (in place, at the workspace
    // root) when asked. Runs regardless of hard mismatches so a red gate is
    // reflected in the committed numbers.
    if write_scoreboard_flag {
        write_scoreboard(&summary, limit_fixtures);
    }

    // Human summary on stderr.
    eprintln!();
    eprintln!("=== rivet-nbt vs Paper Java oracle — parity summary ===");
    // Invalid-corpus checks carry kind "snbt.parse" too; their ids are
    // prefixed `parse-invalid.` in the transcript.
    let kinds = ["snbt.parse", "nbt.decode", "nbt.encode", "idem"];
    for kind in kinds {
        let total = summary.totals.get(kind).copied().unwrap_or(0);
        if total == 0 {
            continue;
        }
        let matched = summary.matched.get(kind).copied().unwrap_or(0);
        let diverged = summary.diverged.get(kind).copied().unwrap_or(0);
        let mismatched = summary.mismatched.get(kind).copied().unwrap_or(0);
        let skipped = summary.skipped.get(kind).copied().unwrap_or(0);
        eprintln!(
            "  {kind:<18} total={total:<5} matched={matched:<5} diverged={diverged:<5} mismatched={mismatched:<4} skipped={skipped}"
        );
    }
    eprintln!();
    eprintln!(
        "  TOTAL matched={} diverged={} mismatched={} skipped={}",
        stats["matched"], stats["diverged"], stats["mismatched"], stats["skipped"]
    );
    if !summary.hard_ids.is_empty() {
        eprintln!();
        eprintln!("  HARD MISMATCHES ({}):", summary.hard_ids.len());
        for id in &summary.hard_ids {
            eprintln!("    - {id}");
        }
        std::process::exit(1);
    }
    if summary.mismatched.values().sum::<usize>() == 0 {
        eprintln!("  RESULT: byte-for-byte parity holds (within documented divergences)");
    }
}
