//! Capture corpus directory + provenance manifest.
//!
//! A corpus is a directory holding one `.frame` file per captured packet and a
//! `manifest.json` recording provenance and a SHA-256 per file. The frame file
//! is the full varint21 frame (`[varint(len)][packet id varint][body]`); the
//! manifest pins the packet id, name, byte count, and hash so [`verify`]
//! (crate root) can prove the corpus is intact and byte-stable.
//!
//! Layout:
//! ```text
//! corpus/
//!   manifest.json
//!   0000_accept_teleportation.frame
//!   0001_chunk_batch_received.frame
//!   ...
//! ```

use crate::frame::{frame_full, frame_payload};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The manifest file name.
pub const MANIFEST: &str = "manifest.json";

/// One captured packet.
#[derive(Debug, Clone)]
pub struct CorpusEntry {
    /// Zero-based capture order.
    pub seq: usize,
    /// Canonical `minecraft:` name.
    pub name: String,
    /// Vanilla protocol id.
    pub id: i32,
    /// The full varint21 frame bytes on disk.
    pub full_frame: Vec<u8>,
    /// SHA-256 of `full_frame`.
    pub sha256: String,
}

/// SHA-256 hex of a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn frame_file_name(seq: usize, name: &str) -> String {
    format!("{seq:04}_{name}.frame")
}

/// Write a corpus directory (overwriting it). Returns the entries written.
pub fn write_corpus(
    dir: &Path,
    entries: &[(i32, String, Vec<u8>)],
) -> Result<Vec<CorpusEntry>, String> {
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let mut stored = Vec::new();
    let mut files: BTreeMap<String, Value> = BTreeMap::new();
    for (seq, (id, name, full_frame)) in entries.iter().enumerate() {
        let file = frame_file_name(seq, name);
        let path = dir.join(&file);
        fs::write(&path, full_frame).map_err(|e| format!("write {}: {e}", path.display()))?;
        let hash = sha256_hex(full_frame);
        files.insert(
            file.clone(),
            json!({
                "sha256": hash,
                "bytes": full_frame.len(),
                "packet_id": id,
                "packet_name": name,
            }),
        );
        stored.push(CorpusEntry {
            seq,
            name: name.clone(),
            id: *id,
            full_frame: full_frame.clone(),
            sha256: hash,
        });
    }
    let manifest = json!({
        "tool": "rivet-decode",
        "format": "serverbound_play_capture",
        "state": "play",
        "flow": "serverbound",
        "protocol": 776,
        "packet_count": stored.len(),
        "files": files,
    });
    fs::write(
        dir.join(MANIFEST),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .map_err(|e| format!("write {MANIFEST}: {e}"))?;
    Ok(stored)
}

/// Read and validate a corpus directory against its manifest. Returns
/// `Err` for a missing/corrupt manifest or a SHA-256/bytes mismatch.
pub fn read_corpus(dir: &Path) -> Result<Vec<CorpusEntry>, String> {
    let manifest_path = dir.join(MANIFEST);
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|_| format!("missing manifest: {}", manifest_path.display()))?;
    let manifest: Value =
        serde_json::from_str(&manifest_text).map_err(|e| format!("bad manifest JSON: {e}"))?;
    let files = manifest
        .get("files")
        .and_then(|f| f.as_object())
        .ok_or_else(|| "manifest has no files object".to_string())?;

    let mut entries = Vec::new();
    for (file, meta) in files {
        let meta = meta.as_object().ok_or("bad file metadata")?;
        let id = meta
            .get("packet_id")
            .and_then(|v| v.as_i64())
            .ok_or("missing packet_id")? as i32;
        let name = meta
            .get("packet_name")
            .and_then(|v| v.as_str())
            .ok_or("missing packet_name")?
            .to_string();
        let expected_bytes = meta.get("bytes").and_then(|v| v.as_i64()) as Option<i64>;
        let expected_hash = meta
            .get("sha256")
            .and_then(|v| v.as_str())
            .ok_or("missing sha256")?;

        let path = dir.join(file);
        let bytes =
            fs::read(&path).map_err(|_| format!("missing corpus file: {}", path.display()))?;
        let actual_hash = sha256_hex(&bytes);
        if actual_hash != expected_hash {
            return Err(format!(
                "{}: sha256 mismatch (expected {expected_hash}, got {actual_hash})",
                path.display()
            ));
        }
        if let Some(expected_bytes) = expected_bytes
            && bytes.len() as i64 != expected_bytes
        {
            return Err(format!(
                "{}: byte count mismatch (expected {expected_bytes}, got {})",
                path.display(),
                bytes.len()
            ));
        }
        entries.push(CorpusEntry {
            seq: entries.len(),
            name,
            id,
            full_frame: bytes,
            sha256: actual_hash,
        });
    }
    entries.sort_by_key(|e| e.seq);
    Ok(entries)
}

/// Read a single frame payload from a full-frame entry.
pub fn payload_of(entry: &CorpusEntry) -> Result<Vec<u8>, String> {
    frame_payload(&entry.full_frame)
}

/// Build a full frame from a packet payload (`[id varint][body]`).
pub fn full_from_payload(id: i32, name: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let _ = id;
    let _ = name;
    frame_full(payload)
}
