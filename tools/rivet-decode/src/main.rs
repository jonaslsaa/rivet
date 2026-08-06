//! The `rivet-decode` binary.
//!
//! Subcommands:
//!   - `decode <capture.bin>`           — split a varint21 capture stream into
//!     frames and print the normalized JSONL transcript to stdout.
//!   - `capture <dir> <frames.hex>...`  — write a capture corpus directory
//!     (`.frame` files + `manifest.json`) from a list of `id:hex` packet
//!     payloads. (Not a Paper-boot capture; the Paper fixture extraction that
//!     produces a real join capture lives in `rivet-oracle`.)
//!   - `verify <dir>`                   — verify a corpus against its manifest
//!     sha256s and re-decode every frame.
//!   - `mutate <dir>`                   — run the hostile-input mutation matrix
//!     over the corpus frames.
//!   - `frag <capture.bin>`             — run the fragmentation / coalescing
//!     checks over a capture stream.
//!
//! Exit codes: 0 = VERIFIED/all checks passed; 1 = FAILED; 3 = UNVERIFIED
//! (a required input is missing or unreadable), mirroring `rivet-oracle`'s
//! gate contract. Any other nonzero exit is a tool failure.

use rivet_decode::corpus;
use rivet_decode::frag;
use rivet_decode::frame;
use rivet_decode::mutate;
use rivet_decode::protocol::{decode_frame, encode_packet, hex, unhex};
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

/// UNVERIFIED: a required input was missing or unreadable, so no real
/// comparison happened. Matches `rivet-oracle`'s gate contract (gate.sh).
const EXIT_UNVERIFIED: u8 = 3;

fn main() -> ExitCode {
    // The decode path deliberately triggers panics (unchecked Java exceptions
    // map to panics in this port) to surface hostile-input errors. Each panic
    // is caught and reported as an `Err`; the default hook's stderr spam would
    // drown the transcript. Errors are still reported by the caller.
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: rivet-decode <decode|capture|verify|mutate|frag> [args...]");
        return ExitCode::from(1);
    }
    match args[1].as_str() {
        "decode" => cmd_decode(&args[2..]),
        "capture" => cmd_capture(&args[2..]),
        "verify" => cmd_verify(&args[2..]),
        "mutate" => cmd_mutate(&args[2..]),
        "frag" => cmd_frag(&args[2..]),
        other => {
            eprintln!("unknown subcommand: {other}");
            ExitCode::from(1)
        }
    }
}

fn eprintln_err(msg: &str) {
    eprintln!("rivet-decode: {msg}");
}

fn stdout_line(line: &str) -> io::Result<()> {
    let mut out = io::BufWriter::new(io::stdout());
    writeln!(out, "{line}")
}

// ---------------------------------------------------------------------------
// decode: capture stream -> normalized JSONL transcript
// ---------------------------------------------------------------------------

fn cmd_decode(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln_err("usage: rivet-decode decode <capture.bin>");
        return ExitCode::from(1);
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln_err(&format!("cannot read {path}: {e}"));
            return ExitCode::from(EXIT_UNVERIFIED);
        }
    };
    let split = match frame::split_stream(&bytes) {
        Ok(s) => s,
        Err(e) => {
            eprintln_err(&format!("cannot split capture: {e}"));
            return ExitCode::from(1);
        }
    };
    if !split.leftover.is_empty() {
        eprintln_err(&format!(
            "capture ends mid-frame: {} trailing bytes",
            split.leftover.len()
        ));
        return ExitCode::from(1);
    }
    let mut ok = true;
    for (seq, frame) in split.frames.iter().enumerate() {
        match decode_frame(frame) {
            Ok(decoded) => {
                let _ = stdout_line(&rivet_decode::protocol::transcript_line(seq, &decoded));
            }
            Err(e) => {
                ok = false;
                eprintln_err(&format!("seq {seq}: {e}"));
            }
        }
    }
    if ok {
        ExitCode::from(0)
    } else {
        ExitCode::from(1)
    }
}

// ---------------------------------------------------------------------------
// capture: id:hex payloads -> corpus directory
// ---------------------------------------------------------------------------

fn cmd_capture(args: &[String]) -> ExitCode {
    if args.len() < 2 {
        eprintln_err("usage: rivet-decode capture <dir> <id:hex> [<id:hex> ...]");
        return ExitCode::from(1);
    }
    let dir = PathBuf::from(&args[0]);
    let mut entries = Vec::new();
    for spec in &args[1..] {
        let (id_s, hex_s) = match spec.split_once(':') {
            Some(pair) => pair,
            None => {
                eprintln_err(&format!("expected id:hex, got {spec}"));
                return ExitCode::from(1);
            }
        };
        let id: i32 = match id_s.parse() {
            Ok(v) => v,
            Err(_) => {
                eprintln_err(&format!("bad packet id {id_s}"));
                return ExitCode::from(1);
            }
        };
        let payload = match unhex(hex_s) {
            Ok(p) => p,
            Err(e) => {
                eprintln_err(&format!("bad hex for {spec}: {e}"));
                return ExitCode::from(1);
            }
        };
        let full = match frame::frame_full(&payload) {
            Ok(f) => f,
            Err(e) => {
                eprintln_err(&format!("cannot frame id {id}: {e}"));
                return ExitCode::from(1);
            }
        };
        let name = rivet_decode::protocol::packet_name(id)
            .map(|s| s.trim_start_matches("minecraft:").to_string())
            .unwrap_or_else(|| format!("packet_{id}"));
        entries.push((id, name, full));
    }
    let stored = match corpus::write_corpus(&dir, &entries) {
        Ok(s) => s,
        Err(e) => {
            eprintln_err(&e);
            return ExitCode::from(1);
        }
    };
    eprintln!("wrote {} frames to {}", stored.len(), dir.display());
    // Re-read to prove the manifest round-trips.
    match corpus::read_corpus(&dir) {
        Ok(re) if re.len() == stored.len() => ExitCode::from(0),
        Ok(re) => {
            eprintln_err(&format!(
                "corpus re-read mismatch: expected {}, got {}",
                stored.len(),
                re.len()
            ));
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln_err(&e);
            ExitCode::from(1)
        }
    }
}

// ---------------------------------------------------------------------------
// verify: corpus -> manifest + decode
// ---------------------------------------------------------------------------

fn cmd_verify(args: &[String]) -> ExitCode {
    let Some(dir) = args.first() else {
        eprintln_err("usage: rivet-decode verify <dir>");
        return ExitCode::from(1);
    };
    let dir = PathBuf::from(dir);
    let entries = match corpus::read_corpus(&dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln_err(&format!("verify {}: {e}", dir.display()));
            return ExitCode::from(EXIT_UNVERIFIED);
        }
    };
    let mut ok = true;
    for entry in &entries {
        let payload = match corpus::payload_of(entry) {
            Ok(p) => p,
            Err(e) => {
                ok = false;
                eprintln_err(&format!("seq {}: {e}", entry.seq));
                continue;
            }
        };
        match decode_frame(&payload) {
            Ok(decoded) => {
                let _ = stdout_line(&rivet_decode::protocol::transcript_line(
                    entry.seq, &decoded,
                ));
            }
            Err(e) => {
                ok = false;
                eprintln_err(&format!("seq {} (id {}): {e}", entry.seq, entry.id));
            }
        }
    }
    if ok {
        eprintln!(
            "OK: {} frames verified against manifest sha256s",
            entries.len()
        );
        ExitCode::from(0)
    } else {
        ExitCode::from(1)
    }
}

// ---------------------------------------------------------------------------
// mutate: corpus -> mutation matrix
// ---------------------------------------------------------------------------

fn cmd_mutate(args: &[String]) -> ExitCode {
    let Some(dir) = args.first() else {
        eprintln_err("usage: rivet-decode mutate <dir>");
        return ExitCode::from(1);
    };
    let dir = PathBuf::from(dir);
    let entries = match corpus::read_corpus(&dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln_err(&format!("mutate {}: {e}", dir.display()));
            return ExitCode::from(EXIT_UNVERIFIED);
        }
    };
    let mut payloads = Vec::new();
    for entry in &entries {
        let payload = match corpus::payload_of(entry) {
            Ok(p) => p,
            Err(e) => {
                eprintln_err(&format!("seq {}: {e}", entry.seq));
                return ExitCode::from(1);
            }
        };
        payloads.push((entry.id, payload));
    }
    let reports = mutate::run(&payloads);
    for report in &reports {
        let _ = stdout_line(&mutate::mutation_line(report));
    }
    let ok = mutate::all_ok(&reports);
    eprintln!(
        "mutation matrix: {} rows, {} applied, {} skipped, {} expected outcomes",
        reports.len(),
        reports.iter().filter(|r| r.applied).count(),
        reports.iter().filter(|r| !r.applied).count(),
        reports
            .iter()
            .filter(|r| r.applied && (r.outcome == "rejected" || r.outcome == "accepted"))
            .count()
    );
    if ok {
        ExitCode::from(0)
    } else {
        ExitCode::from(1)
    }
}

// ---------------------------------------------------------------------------
// frag: capture stream -> fragmentation/coalescing checks
// ---------------------------------------------------------------------------

fn cmd_frag(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln_err("usage: rivet-decode frag <capture.bin>");
        return ExitCode::from(1);
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln_err(&format!("cannot read {path}: {e}"));
            return ExitCode::from(EXIT_UNVERIFIED);
        }
    };
    let report = match frag::run(&bytes) {
        Ok(r) => r,
        Err(e) => {
            eprintln_err(&format!("fragmentation check failed: {e}"));
            return ExitCode::from(1);
        }
    };
    for (name, frames, matched) in &report.splits {
        let _ = stdout_line(&frag::split_line(name, frames, *matched));
    }
    let ok = frag::all_ok(&report);
    eprintln!(
        "fragmentation: {} reference frames; {} splits, all matched = {ok}",
        report.reference.len(),
        report.splits.len()
    );
    if ok {
        ExitCode::from(0)
    } else {
        ExitCode::from(1)
    }
}

// Keep the encode path referenced from the crate surface (used by re-encode
// checks in tests); silence dead-code if the binary never calls it directly.
#[allow(dead_code)]
fn _encode_marker() {
    let _ = encode_packet(0, &rivet_decode::protocol::PlayPacket::ClientTickEnd);
    let _ = hex(&[]);
}
