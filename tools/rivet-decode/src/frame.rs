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

// ---------------------------------------------------------------------------
// Raw byte readers/writers (big-endian Java `DataOutput` + Minecraft VarInt).
// ---------------------------------------------------------------------------

/// Read a signed VarInt from `buf` starting at `*offset`, advancing the offset.
/// Returns `None` when the buffer is exhausted before a complete VarInt.
pub fn read_varint(buf: &[u8], offset: &mut usize) -> Option<i32> {
    let mut value: i32 = 0;
    let mut shift = 0;
    loop {
        let byte = *buf.get(*offset)?;
        *offset += 1;
        value |= i32::from(byte & 0x7F).wrapping_shl(shift);
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift >= 35 {
            return None;
        }
    }
}

/// Append the VarInt encoding of `value` to `out`.
pub fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut v = u32::from_le_bytes(value.to_le_bytes());
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            out.push(byte | 0x80);
        } else {
            out.push(byte);
            return;
        }
    }
}

/// Borrow `n` bytes from `buf` starting at `*offset`, advancing the offset.
pub fn read_bytes<'a>(buf: &'a [u8], offset: &mut usize, n: usize) -> Option<&'a [u8]> {
    let end = offset.checked_add(n)?;
    let slice = buf.get(*offset..end)?;
    *offset = end;
    Some(slice)
}

/// Read a big-endian `i32`, advancing the offset by 4.
pub fn read_i32(buf: &[u8], offset: &mut usize) -> Option<i32> {
    let b = read_bytes(buf, offset, 4)?;
    Some(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read a big-endian `u16`, advancing the offset by 2.
pub fn read_u16(buf: &[u8], offset: &mut usize) -> Option<u16> {
    let b = read_bytes(buf, offset, 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
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
