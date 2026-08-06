//! Varint21 framing helpers over the ported `Varint21FrameDecoder` /
//! `Varint21LengthFieldPrepender` (protocol 776).
//!
//! A capture stream is a concatenation of varint21 frames, each
//! `[varint(length)][packet id varint][body]`. The frame decoder peels the
//! length header and yields the frame payload `[packet id varint][body]`, which
//! [`crate::protocol::decode_frame`] consumes.

use bytes::{Bytes, BytesMut};
use rivet_protocol::varint21_frame_decoder::Varint21FrameDecoder;
use rivet_protocol::varint21_length_field_prepender::encode_frame;

/// A stream split into frames plus the unconsumed trailing bytes (a partial
/// frame at the end of the stream).
#[derive(Debug)]
pub struct Split {
    /// Frame payloads: `[packet id varint][body]` (the length header removed).
    pub frames: Vec<Bytes>,
    /// Trailing bytes that did not form a complete frame.
    pub leftover: BytesMut,
}

/// Split a byte stream into varint21 frames. A nonzero `leftover` means the
/// stream ends mid-frame — an incomplete capture.
pub fn split_stream(stream: &[u8]) -> Result<Split, String> {
    let decoder = Varint21FrameDecoder::new(None);
    let mut buf = BytesMut::from(stream);
    let mut frames = Vec::new();
    while let Some(frame) = decoder
        .decode(&mut buf)
        .map_err(|e| format!("corrupted frame: {e}"))?
    {
        frames.push(frame);
    }
    Ok(Split {
        frames,
        leftover: buf,
    })
}

/// The full varint21 frame for a payload: `[varint(len)][payload]`.
pub fn frame_full(payload: &[u8]) -> Result<Vec<u8>, String> {
    encode_frame(payload)
        .map(|b| b.to_vec())
        .map_err(|e| e.message)
}

/// The payload of a full frame (the length header removed), for re-decode.
pub fn frame_payload(full_frame: &[u8]) -> Result<Vec<u8>, String> {
    let mut buf = BytesMut::from(full_frame);
    let decoder = Varint21FrameDecoder::new(None);
    match decoder.decode(&mut buf).map_err(|e| e.message)? {
        Some(payload) => Ok(payload.to_vec()),
        None => Err("incomplete frame".to_string()),
    }
}
