//! Fixture persistence, provenance, and byte-diff comparison for the join
//! packet-capture fixture.
//!
//! Layout (mirrors `tools/rivet-oracle/fixtures/`):
//!
//! ```text
//! fixtures/join/
//!   manifest.json   provenance (Paper commit, scenario, bot identity, config)
//!                   + one `captured` entry per canonical packet: identity,
//!                   SHA-256, byte length, and the normalization note.
//!   capture.jsonl   one JSON object per canonical packet: the hex body bytes.
//! ```
//!
//! The comparison contract: a fresh capture, normalized through
//! `normalize::canonicalize`, must reproduce the committed `capture.jsonl`
//! byte-for-byte. Any divergence (missing/extra/mismatched packet) is reported
//! with the exact identity and both hashes.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::normalize::NormalizedPacket;
use crate::packet::{Direction, State};

/// Protocol version the fixture is pinned to (MC 26.2).
pub const PROTOCOL: i32 = 776;
/// Fixture manifest format version.
pub const FORMAT: u64 = 1;

/// One captured packet as persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedEntry {
    pub state: String,
    pub direction: String,
    pub id: i32,
    #[serde(default)]
    pub id_name: String,
    pub sha256: String,
    pub bytes: usize,
    #[serde(default)]
    pub note: String,
}

/// The fixture manifest: provenance + the per-packet integrity list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u64,
    pub scenario: String,
    pub protocol: i32,
    /// Pinned Paper provenance, e.g. `26.2-DEV-main@0a99345`.
    pub paper: String,
    /// Offline bot identity used for the join.
    pub bot_identity: String,
    /// The exact server config the capture is defined against.
    pub server_config: String,
    /// Azalea client revision the bot binary was built from.
    pub azalea_revision: String,
    pub captured: Vec<CapturedEntry>,
}

/// A single packet line in `capture.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PacketLine {
    state: String,
    direction: String,
    id: i32,
    body: String,
}

fn state_name(s: State) -> &'static str {
    match s {
        State::Handshake => "handshake",
        State::Status => "status",
        State::Login => "login",
        State::Configuration => "configuration",
        State::Play => "play",
    }
}

fn state_from_name(s: &str) -> State {
    match s {
        "handshake" => State::Handshake,
        "status" => State::Status,
        "login" => State::Login,
        "configuration" => State::Configuration,
        _ => State::Play,
    }
}

fn direction_name(d: Direction) -> &'static str {
    d.flow()
}

fn direction_from_name(d: &str) -> Direction {
    match d {
        "serverbound" => Direction::Serverbound,
        _ => Direction::Clientbound,
    }
}

pub fn hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn unhex(s: &str) -> io::Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "odd-length hex"));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        out.push(byte);
    }
    Ok(out)
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(data);
    let mut s = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Serialize a canonical packet list to the `capture.jsonl` representation.
pub fn capture_lines(packets: &[NormalizedPacket]) -> String {
    let mut out = String::new();
    for p in packets {
        let line = PacketLine {
            state: state_name(p.state).to_owned(),
            direction: direction_name(p.direction).to_owned(),
            id: p.id,
            body: hex(&p.body),
        };
        out.push_str(&serde_json::to_string(&line).expect("packet line serializes"));
        out.push('\n');
    }
    out
}

/// Read `capture.jsonl` back into canonical packets (bodies only; notes are
/// reconstructed as empty because the manifest owns the justification).
pub fn read_capture(dir: &Path) -> io::Result<Vec<NormalizedPacket>> {
    let path = dir.join("capture.jsonl");
    let raw = fs::read_to_string(&path)?;
    let mut packets = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let line: PacketLine = serde_json::from_str(line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        packets.push(NormalizedPacket {
            state: state_from_name(&line.state),
            direction: direction_from_name(&line.direction),
            id: line.id,
            body: unhex(&line.body)?,
            note: String::new(),
        });
    }
    Ok(packets)
}

/// Write the manifest + capture.jsonl for a canonical packet list.
pub fn write_fixture(
    dir: &Path,
    packets: &[NormalizedPacket],
    manifest: &Manifest,
) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let lines = capture_lines(packets);
    fs::write(dir.join("capture.jsonl"), &lines)?;
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(manifest).unwrap(),
    )?;
    Ok(())
}

/// Build a `CapturedEntry` list from canonical packets (used when constructing
/// the manifest).
pub fn build_captured(packets: &[NormalizedPacket]) -> Vec<CapturedEntry> {
    packets
        .iter()
        .map(|p| CapturedEntry {
            state: state_name(p.state).to_owned(),
            direction: direction_name(p.direction).to_owned(),
            id: p.id,
            id_name: crate::packet::packet_name(p.state, p.direction, p.id)
                .unwrap_or("minecraft:unknown")
                .to_owned(),
            sha256: sha256_hex(&p.body),
            bytes: p.body.len(),
            note: p.note.clone(),
        })
        .collect()
}

/// A single differing packet between a fresh capture and the committed fixture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PacketDiff {
    /// Packets present in the fixture but absent from (or mismatched in) the
    /// fresh capture: (index, identity, expected sha, actual sha).
    pub mismatched: Vec<(usize, String, String, String)>,
    /// Packets in the fixture with no counterpart in the fresh capture.
    pub missing: Vec<(usize, String)>,
    /// Packets in the fresh capture with no counterpart in the fixture.
    pub extra: Vec<(usize, String)>,
}

impl PacketDiff {
    pub fn is_clean(&self) -> bool {
        self.mismatched.is_empty() && self.missing.is_empty() && self.extra.is_empty()
    }
}

fn identity(p: &NormalizedPacket) -> String {
    format!(
        "{}/{} id {} ({})",
        state_name(p.state),
        direction_name(p.direction),
        p.id,
        crate::packet::packet_name(p.state, p.direction, p.id).unwrap_or("minecraft:unknown")
    )
}

/// Byte-diff a fresh canonical capture against the committed one.
pub fn diff_packets(expected: &[NormalizedPacket], actual: &[NormalizedPacket]) -> PacketDiff {
    let mut d = PacketDiff::default();
    for (i, (e, a)) in expected.iter().zip(actual.iter()).enumerate() {
        if e.state != a.state || e.direction != a.direction || e.id != a.id || e.body != a.body {
            d.mismatched
                .push((i, identity(e), sha256_hex(&e.body), sha256_hex(&a.body)));
        }
    }
    for (i, p) in actual.iter().enumerate().skip(expected.len()) {
        d.extra.push((i, identity(p)));
    }
    for (i, p) in expected.iter().enumerate().skip(actual.len()) {
        d.missing.push((i, identity(p)));
    }
    d
}

impl fmt::Display for PacketDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.mismatched.is_empty() {
            writeln!(f, "  {} packet(s) mismatched:", self.mismatched.len())?;
            for (i, id, want, got) in &self.mismatched {
                writeln!(
                    f,
                    "    [{i}] {id}\n      expected {want}\n      actual   {got}"
                )?;
            }
        }
        if !self.missing.is_empty() {
            writeln!(
                f,
                "  {} packet(s) missing from fresh capture:",
                self.missing.len()
            )?;
            for (i, id) in &self.missing {
                writeln!(f, "    [{i}] {id}")?;
            }
        }
        if !self.extra.is_empty() {
            writeln!(
                f,
                "  {} packet(s) extra in fresh capture:",
                self.extra.len()
            )?;
            for (i, id) in &self.extra {
                writeln!(f, "    [{i}] {id}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::NormalizedPacket;

    fn sample() -> Vec<NormalizedPacket> {
        vec![
            NormalizedPacket {
                state: State::Play,
                direction: Direction::Clientbound,
                id: 49,
                body: vec![0x00, 0x00, 0x00, 0x01],
                note: String::new(),
            },
            NormalizedPacket {
                state: State::Play,
                direction: Direction::Clientbound,
                id: 45,
                body: vec![0x01, 0x02, 0x03],
                note: String::new(),
            },
        ]
    }

    #[test]
    fn capture_lines_round_trip() {
        let lines = capture_lines(&sample());
        let dir = std::env::temp_dir().join(format!("rivet-capture-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("capture.jsonl"), &lines).unwrap();
        let read = read_capture(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(read, sample());
    }

    #[test]
    fn diff_clean_when_identical() {
        let d = diff_packets(&sample(), &sample());
        assert!(d.is_clean());
    }

    #[test]
    fn diff_detects_tampered_body() {
        let mut tampered = sample();
        tampered[1].body[0] ^= 0xFF;
        let d = diff_packets(&sample(), &tampered);
        assert!(!d.is_clean());
        assert_eq!(d.mismatched.len(), 1);
        assert_eq!(d.mismatched[0].0, 1);
    }

    #[test]
    fn diff_detects_missing_and_extra() {
        let fewer = vec![sample()[0].clone()];
        let more = {
            let mut v = sample();
            v.push(NormalizedPacket {
                state: State::Play,
                direction: Direction::Clientbound,
                id: 1,
                body: vec![0x00],
                note: String::new(),
            });
            v
        };
        let d = diff_packets(&sample(), &fewer);
        assert!(!d.is_clean());
        assert_eq!(d.missing.len(), 1);
        let d2 = diff_packets(&sample(), &more);
        assert_eq!(d2.extra.len(), 1);
    }
}
