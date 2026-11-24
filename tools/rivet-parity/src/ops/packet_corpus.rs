//! The `packet.reencode` corpus: every ported packet codec fed from three
//! sources, all committed as `corpus/packet.reencode.jsonl`.
//!
//! 1. The Paper join capture (`tools/rivet-capture/fixtures/join/capture.jsonl`):
//!    every captured body whose (phase, direction, id) Rivet has ported.
//! 2. Seeded valid instances: per packet, bodies the Rust codecs themselves
//!    encode from deterministic pseudo-random values ([`typed_seeds`]).
//! 3. Seeded hostile inputs per packet: random bodies of assorted lengths, and
//!    mutations (truncated, extended, bit-flipped) of every valid body above.
//!
//! Paper answers every input; nothing here hand-types an expected value. The
//! generator is a pure function of the committed capture and [`CORPUS_SEED`],
//! so a test pins the committed corpus to it.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::packet::{self, Entry, encode_hex};
use super::packet_seeds;

/// The committed seed of every pseudo-random choice in this corpus.
pub const CORPUS_SEED: u64 = 0x5249_5645_5430_0004;

/// SplitMix64: a tiny, well-known, dependency-free deterministic generator.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator for one named stream, so adding a stream never perturbs the others.
    pub fn for_stream(name: &str) -> Rng {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for byte in name.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Rng(hash ^ CORPUS_SEED)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound.max(1)
    }

    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next_u64() as u8).collect()
    }

    /// 0..`max` random bytes.
    pub fn some_bytes(&mut self, max: u64) -> Vec<u8> {
        let len = self.below(max) as usize;
        self.bytes(len)
    }
}

fn capture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rivet-capture/fixtures/join/capture.jsonl")
}

fn line(id: &str, entry: &Entry, packet_id: usize, body: &[u8]) -> String {
    let value = json!({
        "id": id,
        "state": entry.state,
        "direction": entry.direction,
        "packet_id": packet_id,
        "body": encode_hex(body),
    });
    serde_json::to_string(&value).expect("JSON values always serialize") + "\n"
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// Build the corpus text.
pub fn emit() -> Result<String, String> {
    let entries = packet::entries();
    let mut out = String::new();

    // 1. The captured join bodies for ported packets.
    let capture = std::fs::read_to_string(capture_path())
        .map_err(|e| format!("cannot read the join capture: {e}"))?;
    let mut captured: Vec<(usize, Vec<u8>)> = Vec::new();
    for (index, raw) in capture.lines().enumerate() {
        let value: Value =
            serde_json::from_str(raw).map_err(|e| format!("capture line {}: {e}", index + 1))?;
        let field = |key: &str| value.get(key).and_then(Value::as_str);
        let (Some(state), Some(direction), Some(body)) =
            (field("state"), field("direction"), field("body"))
        else {
            return Err(format!(
                "capture line {} lacks state/direction/body",
                index + 1
            ));
        };
        let id = value
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("capture line {} lacks an id", index + 1))?
            as usize;
        let body = packet::decode_hex(body)
            .ok_or_else(|| format!("capture line {} body is not hex", index + 1))?;
        let Some(name) = packet::names(state, direction).and_then(|t| t.get(id)) else {
            continue;
        };
        let Some(position) = entries
            .iter()
            .position(|e| e.state == state && e.direction == direction && e.name == *name)
        else {
            continue; // a packet Rivet has not ported: nothing to verify
        };
        out += &line(
            &format!("capture.{:03}.{state}.{direction}.{}", index, short(name)),
            &entries[position],
            id,
            &body,
        );
        captured.push((position, body));
    }

    // 2 + 3. Per packet: valid seeds, then hostile inputs.
    for (position, entry) in entries.iter().enumerate() {
        let id = packet::packet_id(entry.state, entry.direction, entry.name)
            .ok_or_else(|| format!("{} {} has no network id", entry.state, entry.name))?;
        let stem = format!("{}.{}.{}", entry.state, entry.direction, short(entry.name));
        let mut rng = Rng::for_stream(&stem);

        let mut valid: Vec<Vec<u8>> = packet_seeds::seeds(entry, &mut rng);
        valid.extend(
            captured
                .iter()
                .filter(|(p, _)| *p == position)
                .map(|(_, body)| body.clone()),
        );
        for (n, body) in valid.iter().enumerate().take(6) {
            out += &line(&format!("seed.{stem}.{n}"), entry, id, body);
        }

        // Hostile: random bodies of assorted lengths.
        for (n, len) in [0usize, 1, 2, 3, 4, 8, 13, 24].iter().enumerate() {
            let body = rng.bytes(*len);
            out += &line(&format!("random.{stem}.{n}"), entry, id, &body);
        }
        // Hostile: mutations of the valid bodies.
        for (n, body) in valid.iter().enumerate().take(4) {
            if body.is_empty() {
                // an empty valid body can only be extended
                out += &line(&format!("mutate.{stem}.{n}.extend"), entry, id, &[0]);
                continue;
            }
            out += &line(
                &format!("mutate.{stem}.{n}.truncate"),
                entry,
                id,
                &body[..body.len() - 1],
            );
            let mut extended = body.clone();
            extended.push(rng.next_u64() as u8);
            out += &line(&format!("mutate.{stem}.{n}.extend"), entry, id, &extended);
            for flip in 0..2 {
                let mut flipped = body.clone();
                let at = rng.below(flipped.len() as u64) as usize;
                flipped[at] ^= 1 << rng.below(8);
                out += &line(
                    &format!("mutate.{stem}.{n}.flip{flip}"),
                    entry,
                    id,
                    &flipped,
                );
            }
            let mut replaced = body.clone();
            let at = rng.below(replaced.len() as u64) as usize;
            replaced[at] = rng.next_u64() as u8;
            out += &line(&format!("mutate.{stem}.{n}.replace"), entry, id, &replaced);
        }
    }

    // Unknown ids: Paper answers "unknown packet id" for each.
    let mut seen = std::collections::BTreeSet::new();
    for entry in &entries {
        if !seen.insert((entry.state, entry.direction)) {
            continue;
        }
        let len = packet::names(entry.state, entry.direction).map_or(0, <[&str]>::len);
        for (n, id) in [len, len + 1, 1 << 20].iter().enumerate() {
            out += &line(
                &format!("unknown-id.{}.{}.{n}", entry.state, entry.direction),
                entry,
                *id,
                &[],
            );
        }
    }
    Ok(out)
}
