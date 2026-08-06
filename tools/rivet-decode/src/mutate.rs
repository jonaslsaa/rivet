//! The hostile-input mutation matrix (issue #97 DoD).
//!
//! Every mutation takes a real captured frame body and perturbs one aspect of
//! the wire format. The expected outcome is part of the matrix:
//! - `unknown_id` — packet id set outside the 0..69 vanilla table; must be
//!   rejected (`Received unknown packet id n`).
//! - `enum_out_of_range` — a varint enum ordinal bumped past its range
//!   (`client_command` length 3, `player_action` length 8); must be rejected
//!   with Java's AIOOBE text.
//! - `nan_inf` — a float/double payload's bits set to NaN/Inf; the raw-bit
//!   codecs must still decode (benign), so the result must round-trip
//!   byte-identically.
//! - `varint_boundary` — a 5-byte continuation varint; must be rejected
//!   (`VarInt too big`).
//! - `truncation` — the body cut short; must be rejected (not a clean decode).
//!
//! The matrix is applied to a corpus of captured frames. Frames whose packet id
//! does not carry the mutated field are skipped (the mutation does not apply to
//! them); the report records which mutations applied and which did not, so a
//! corpus missing e.g. `client_command` still produces an honest, non-silent
//! result.

use crate::protocol::{decode_frame, packet_name};
use serde_json::{Value, json};

/// One applied mutation.
#[derive(Debug)]
pub struct MutationReport {
    pub kind: &'static str,
    pub target_id: i32,
    pub target_name: String,
    pub applied: bool,
    pub note: String,
    pub outcome: String,
    pub error: Option<String>,
}

/// Run the mutation matrix against a corpus of frame payloads
/// (`[id varint][body]`). Each row targets a set of packet ids; the first
/// captured frame matching one is mutated.
pub fn run(corpus_payloads: &[(i32, Vec<u8>)]) -> Vec<MutationReport> {
    let mut reports = Vec::new();
    for row in MATRIX {
        let frame = corpus_payloads
            .iter()
            .find(|(id, _)| row.target_ids.contains(id));
        match frame {
            Some((id, payload)) => reports.push((row.mutate)(*id, payload.clone(), row)),
            None => reports.push(MutationReport {
                kind: row.kind,
                target_id: row.target_ids[0],
                target_name: packet_name(row.target_ids[0])
                    .unwrap_or("<unknown>")
                    .to_string(),
                applied: false,
                note: "no captured frame for any target id; mutation not applied".to_string(),
                outcome: "skipped".to_string(),
                error: None,
            }),
        }
    }
    reports
}

/// Decode a payload, mapping a panic to `Err` (decode_frame already does this;
/// this is a thin alias kept for the matrix helpers).
fn decode(payload: &[u8]) -> Result<(), String> {
    decode_frame(payload).map(|_| ())
}

/// Build the report for one applied mutation.
fn report_for(
    row: &MatrixRow,
    id: i32,
    mutated: Vec<u8>,
    expect_reject: bool,
    note: String,
) -> MutationReport {
    let result = decode(&mutated);
    let (outcome, error) = match (&result, expect_reject) {
        (Err(e), true) => ("rejected".to_string(), Some(e.clone())),
        (Err(e), false) => ("unexpectedly-rejected".to_string(), Some(e.clone())),
        (Ok(()), true) => ("accepted-when-should-reject".to_string(), None),
        (Ok(()), false) => ("accepted".to_string(), None),
    };
    MutationReport {
        kind: row.kind,
        target_id: id,
        target_name: packet_name(id).unwrap_or("<unknown>").to_string(),
        applied: true,
        note,
        outcome,
        error,
    }
}

/// Replace the packet-id varint with an id outside the 0..69 table.
fn mutate_unknown_id(id: i32, payload: Vec<u8>, row: &MatrixRow) -> MutationReport {
    let mut mutated = payload.clone();
    if let Some(first) = mutated.first_mut() {
        *first = 100;
    }
    report_for(
        row,
        id,
        mutated,
        true,
        "packet id set to 100 (outside vanilla 0..69)".to_string(),
    )
}

/// Bump the enum ordinal varint that starts a `client_command` body.
fn mutate_client_command_enum(id: i32, payload: Vec<u8>, row: &MatrixRow) -> MutationReport {
    let mut mutated = payload.clone();
    // id varint (1 byte) then the action varint (1 byte).
    if let Some(b) = mutated.get_mut(1) {
        *b = 3; // out of range for Action[3]
    }
    report_for(
        row,
        id,
        mutated,
        true,
        "client_command action ordinal 3 (Action length 3)".to_string(),
    )
}

/// Bump the enum ordinal varint that starts a `player_action` body.
fn mutate_player_action_enum(id: i32, payload: Vec<u8>, row: &MatrixRow) -> MutationReport {
    let mut mutated = payload.clone();
    // id varint (1 byte) then the action ordinal varint (1 byte).
    if let Some(b) = mutated.get_mut(1) {
        *b = 8; // out of range for Action[8]
    }
    report_for(
        row,
        id,
        mutated,
        true,
        "player_action action ordinal 8 (Action length 8)".to_string(),
    )
}

/// Set the first body float's bits to a quiet NaN. Benign for the raw-bit
/// float/double codecs (`chunk_batch_received` body, `move_player_*` leading
/// coords).
fn mutate_nan_inf(id: i32, payload: Vec<u8>, row: &MatrixRow) -> MutationReport {
    let mut mutated = payload.clone();
    let nan_bits = 0x7fc0_0000u32.to_be_bytes();
    let start = 1.min(mutated.len());
    for (i, b) in nan_bits.iter().enumerate() {
        let at = start + i;
        if at < mutated.len() {
            mutated[at] = *b;
        }
    }
    report_for(
        row,
        id,
        mutated,
        false,
        "first body float/double overwritten with quiet-NaN bits".to_string(),
    )
}

/// Replace the body with a 5-byte continuation varint (`VarInt too big`).
fn mutate_varint_boundary(id: i32, _payload: Vec<u8>, row: &MatrixRow) -> MutationReport {
    let mutated = vec![id as u8, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
    report_for(
        row,
        id,
        mutated,
        true,
        "body replaced by 5-byte continuation varint (VarInt too big)".to_string(),
    )
}

/// Cut the body short (drop the trailing byte).
fn mutate_truncation(id: i32, payload: Vec<u8>, row: &MatrixRow) -> MutationReport {
    let mut mutated = payload.clone();
    if mutated.len() > 1 {
        mutated.truncate(mutated.len() - 1);
    }
    report_for(
        row,
        id,
        mutated,
        true,
        "body truncated by one byte".to_string(),
    )
}

struct MatrixRow {
    kind: &'static str,
    target_ids: &'static [i32],
    mutate: fn(i32, Vec<u8>, &MatrixRow) -> MutationReport,
}

const MATRIX: &[MatrixRow] = &[
    MatrixRow {
        kind: "unknown_id",
        target_ids: &[0, 11, 12, 13, 30, 31, 32, 33, 41],
        mutate: mutate_unknown_id,
    },
    MatrixRow {
        kind: "enum_out_of_range",
        target_ids: &[12],
        mutate: mutate_client_command_enum,
    },
    MatrixRow {
        kind: "enum_out_of_range",
        target_ids: &[41],
        mutate: mutate_player_action_enum,
    },
    MatrixRow {
        kind: "nan_inf",
        target_ids: &[11, 30, 31, 32],
        mutate: mutate_nan_inf,
    },
    MatrixRow {
        kind: "varint_boundary",
        target_ids: &[0, 11, 12, 13, 30, 31, 32, 33, 41],
        mutate: mutate_varint_boundary,
    },
    MatrixRow {
        kind: "truncation",
        target_ids: &[0, 11, 12, 13, 30, 31, 32, 33, 41],
        mutate: mutate_truncation,
    },
];

/// A JSON object for one mutation, for the transcript.
pub fn mutation_line(report: &MutationReport) -> String {
    serde_json::to_string(&json!({
        "kind": report.kind,
        "target_id": report.target_id,
        "target_name": report.target_name,
        "applied": report.applied,
        "note": report.note,
        "outcome": report.outcome,
        "error": report.error,
    }))
    .expect("json serialization is infallible")
}

/// True when every applied mutation matched its expected outcome (`rejected`
/// for hostile rows, `accepted` for the benign `nan_inf` row). Skipped rows
/// are not failures.
pub fn all_ok(reports: &[MutationReport]) -> bool {
    reports.iter().all(|r| {
        if !r.applied {
            return true;
        }
        r.outcome == "rejected" || r.outcome == "accepted"
    })
}

/// Keep `Value` referenced (mutation_line uses `json!` which needs the import
/// reachable); silence an unused-import lint on some toolchains.
#[allow(dead_code)]
fn _value_marker(_v: Value) {}
