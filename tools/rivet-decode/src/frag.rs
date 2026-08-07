//! Fragmentation / coalescing checks (issue #97 DoD).
//!
//! A real TCP stream is a byte sequence; the varint21 frame decoder must yield
//! the same packet sequence no matter how the bytes are chunked. This module
//! verifies three deterministic splits of a capture stream:
//!   - **byte-at-a-time**  — feed one byte per `decode` call (maximal
//!     fragmentation).
//!   - **coalesced**       — feed the whole stream at once (maximal
//!     coalescing; multiple frames per call).
//!   - **chunked**         — feed in fixed-size chunks (mixed).
//!
//! Every split must decode the identical frame sequence, byte-for-byte.

use bytes::BytesMut;
use rivet_protocol::varint21_frame_decoder::Varint21FrameDecoder;

/// One frame decoded under a split, as its payload bytes (`[id varint][body]`).
#[derive(Debug, Clone)]
pub struct FragFrame {
    pub payload: Vec<u8>,
}

/// Decode a full capture stream with a per-call feed of `chunk_size` bytes
/// (`chunk_size = 1` is byte-at-a-time; `>= stream.len()` is coalesced).
fn feed(stream: &[u8], chunk_size: usize) -> Result<Vec<FragFrame>, String> {
    let decoder = Varint21FrameDecoder::new(None);
    let mut buf = BytesMut::new();
    let mut out = Vec::new();
    let mut at = 0;
    while at < stream.len() {
        let end = (at + chunk_size).min(stream.len());
        buf.extend_from_slice(&stream[at..end]);
        at = end;
        while let Some(frame) = decoder.decode(&mut buf).map_err(|e| e.message)? {
            out.push(FragFrame {
                payload: frame.to_vec(),
            });
        }
    }
    // Drain any frame that completed exactly at the end.
    while let Some(frame) = decoder.decode(&mut buf).map_err(|e| e.message)? {
        out.push(FragFrame {
            payload: frame.to_vec(),
        });
    }
    if !buf.is_empty() {
        return Err(format!(
            "stream ends mid-frame ({} trailing bytes)",
            buf.len()
        ));
    }
    Ok(out)
}

/// The result of running all three splits over one capture stream.
#[derive(Debug)]
pub struct FragReport {
    /// The reference frame sequence (byte-at-a-time).
    pub reference: Vec<FragFrame>,
    /// `(split_name, frames, matched)`.
    pub splits: Vec<(&'static str, Vec<FragFrame>, bool)>,
}

/// Run the three fragmentation splits. `reference` is the byte-at-a-time feed,
/// which every other split must reproduce exactly.
pub fn run(stream: &[u8]) -> Result<FragReport, String> {
    let reference = feed(stream, 1)?;
    let mut splits = Vec::new();
    for (name, chunk) in [
        ("byte_at_a_time", 1usize),
        ("coalesced", stream.len()),
        ("chunked", 7),
    ] {
        let frames = feed(stream, chunk)?;
        let matched = frames
            .iter()
            .zip(reference.iter())
            .all(|(a, b)| a.payload == b.payload)
            && frames.len() == reference.len();
        splits.push((name, frames, matched));
    }
    Ok(FragReport { reference, splits })
}

/// True when every split reproduced the byte-at-a-time reference sequence.
pub fn all_ok(report: &FragReport) -> bool {
    report.splits.iter().all(|(_, _, matched)| *matched)
}

/// A JSON object for one split, for the transcript.
pub fn split_line(split_name: &str, frames: &[FragFrame], matched: bool) -> String {
    serde_json::json!({
        "split": split_name,
        "frames": frames.len(),
        "matched_reference": matched,
        "frame_payload_hex": frames.iter().map(|f| crate::protocol::hex(&f.payload)).collect::<Vec<_>>(),
    })
    .to_string()
}
