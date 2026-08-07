//! Protocol framing: VarInts and the MC 26.2 packet frame (length prefix +
//! optional compression), independent of any particular codec.
//!
//! The wire frame is:
//!
//! ```text
//! [VarInt length][payload]
//! ```
//!
//! where `length` is the number of bytes of `payload`. When compression is
//! enabled (a `login_compression` packet with a non-negative threshold has been
//! seen), `payload` is itself:
//!
//! ```text
//! [VarInt dataLength][data]
//! ```
//!
//! - `dataLength == 0` — `data` is the uncompressed packet (`[VarInt id][body]`).
//! - `dataLength > 0`  — `data` is a zlib-compressed block that decompresses to
//!   exactly `dataLength` bytes (the packet `[VarInt id][body]`).
//!
//! Packets whose uncompressed size is at most the threshold are always sent
//! uncompressed (`dataLength == 0`); only larger packets are compressed.
//! Both cases appear on the real join path (server.properties sets
//! `network-compression-threshold=256`, so most join packets are small and
//! uncompressed while the chunk and registry payloads are compressed).

use std::io::Read;

/// A packet parsed out of one direction of the connection.
#[derive(Debug, Clone)]
pub struct PacketFrame {
    /// The decoded packet id (VarInt at the head of the uncompressed payload).
    pub id: i32,
    /// The decoded packet body (everything after the packet id), still
    /// compressed/uncompressed as it was on the wire but never re-encoded.
    pub body: Vec<u8>,
}

/// Read a VarInt from `buf` starting at `*offset`, advancing the offset.
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

/// Read a signed VarLong from `buf` starting at `*offset`, advancing the offset.
/// Java's `writeVarLong` uses the same base-128 scheme as `writeVarInt` but over
/// 64 bits (up to 10 bytes). `set_time` totalTicks is the only VarLong on the
/// join path; the plain VarInt reader must not be used on it.
pub fn read_varlong(buf: &[u8], offset: &mut usize) -> Option<i64> {
    let mut value: i64 = 0;
    let mut shift = 0;
    loop {
        let byte = *buf.get(*offset)?;
        *offset += 1;
        value |= i64::from(byte & 0x7F).wrapping_shl(shift);
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift >= 70 {
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

/// Whether the wire payload begins with a compression `dataLength` field. This
/// is determined by the negotiated threshold (from `login_compression`), NOT by
/// the payload itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// No `login_compression` seen yet (or threshold -1): payload is the raw
    /// packet.
    Off,
    /// `login_compression` seen: payload starts with a VarInt `dataLength`.
    On,
}

/// The largest uncompressed packet the join capture will legitimately carry. The
/// `dataLength` field is a VarInt capable of encoding up to 2^31, so a corrupted
/// stream could otherwise demand a 2 GiB allocation before decompression.
const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024 * 1024;

/// Parse one framed packet from a complete `raw` frame (`[VarInt length][payload]`),
/// given the negotiated compression state.
///
/// Returns the packet id and the raw body bytes (the packet payload minus its
/// id VarInt; decompressed if the frame was compressed).
pub fn parse_frame(raw: &[u8], compression: Compression) -> Option<PacketFrame> {
    let mut offset = 0usize;
    let length = read_varint(raw, &mut offset)? as usize;
    let payload = raw.get(offset..offset + length)?;
    let (decompressed, _) = match compression {
        Compression::Off => (payload.to_vec(), 0),
        Compression::On => {
            let mut inner = 0usize;
            let data_length = read_varint(payload, &mut inner)?;
            let data = payload.get(inner..)?;
            if data_length == 0 {
                (data.to_vec(), inner)
            } else {
                let data_length = data_length as usize;
                if data_length > MAX_DECOMPRESSED_BYTES {
                    return None;
                }
                let mut decoder = flate2::read::ZlibDecoder::new(data);
                let mut out = Vec::with_capacity(data_length);
                decoder.read_to_end(&mut out).ok()?;
                (out, inner)
            }
        }
    };
    let mut id_off = 0usize;
    let id = read_varint(&decompressed, &mut id_off)?;
    let body = decompressed[id_off..].to_vec();
    Some(PacketFrame { id, body })
}

/// Frame a packet (id + body) into raw wire bytes for the given compression
/// state. Used by tests to synthesize wire frames.
#[cfg(test)]
pub fn frame_packet(id: i32, body: &[u8], compression: Compression) -> Vec<u8> {
    let mut packet = Vec::new();
    write_varint(&mut packet, id);
    packet.extend_from_slice(body);
    match compression {
        Compression::Off => {
            let mut frame = Vec::new();
            write_varint(&mut frame, packet.len() as i32);
            frame.extend_from_slice(&packet);
            frame
        }
        Compression::On => {
            // Packets no larger than the threshold are framed with dataLength 0;
            // the proxy captures the threshold as `Compression::On` regardless,
            // so emit the uncompressed form (dataLength 0).
            let mut frame = Vec::new();
            let mut payload = Vec::new();
            write_varint(&mut payload, 0);
            payload.extend_from_slice(&packet);
            write_varint(&mut frame, payload.len() as i32);
            frame.extend_from_slice(&payload);
            frame
        }
    }
}

/// Read exactly `n` bytes from `buf` at `*offset`, advancing the offset.
pub fn read_bytes<'a>(buf: &'a [u8], offset: &mut usize, n: usize) -> Option<&'a [u8]> {
    let end = offset.checked_add(n)?;
    let slice = buf.get(*offset..end)?;
    *offset = end;
    Some(slice)
}

/// Read a big-endian f64 (Java `writeDouble`).
pub fn read_f64(buf: &[u8], offset: &mut usize) -> Option<f64> {
    let b = read_bytes(buf, offset, 8)?;
    Some(f64::from_be_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

/// Read a big-endian i32 (Java `writeInt`).
pub fn read_i32(buf: &[u8], offset: &mut usize) -> Option<i32> {
    let b = read_bytes(buf, offset, 4)?;
    Some(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read a big-endian f32 (Java `writeFloat`).
pub fn read_f32(buf: &[u8], offset: &mut usize) -> Option<f32> {
    let b = read_bytes(buf, offset, 4)?;
    Some(f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read a big-endian i64 (Java `writeLong`).
pub fn read_i64(buf: &[u8], offset: &mut usize) -> Option<i64> {
    let b = read_bytes(buf, offset, 8)?;
    Some(i64::from_be_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

#[cfg(test)]
pub mod test_helpers {
    /// Size in bytes of the VarInt encoding of `value`.
    pub fn varint_len(value: i32) -> usize {
        let mut size = 1;
        let mut v = u32::from_le_bytes(value.to_le_bytes());
        while v >= 0x80 {
            v >>= 7;
            size += 1;
        }
        size
    }

    /// Append the VarLong (base-128, up to 10 bytes) encoding of `value`.
    pub fn write_varlong(out: &mut Vec<u8>, value: i64) {
        let mut v = u64::from_le_bytes(value.to_le_bytes());
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
}

#[cfg(test)]
mod tests {
    use super::test_helpers::{varint_len, write_varlong};
    use super::*;

    #[test]
    fn varint_round_trip() {
        for value in [
            0i32,
            1,
            127,
            128,
            255,
            256,
            300,
            32_767,
            32_768,
            65_535,
            21_4748_3647,
        ] {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            assert_eq!(buf.len(), varint_len(value), "len for {value}");
            let mut off = 0;
            assert_eq!(read_varint(&buf, &mut off), Some(value));
            assert_eq!(off, buf.len());
        }
    }

    #[test]
    fn float_long_helpers() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&1.5f32.to_be_bytes());
        buf.extend_from_slice(&(-7i64).to_be_bytes());
        let mut off = 0;
        assert_eq!(read_f32(&buf, &mut off), Some(1.5));
        assert_eq!(read_i64(&buf, &mut off), Some(-7));
    }

    #[test]
    fn varlong_round_trip() {
        for value in [0i64, 1, 127, 128, 16_777_215, i32::MAX as i64, -1, -300] {
            let mut buf = Vec::new();
            write_varlong(&mut buf, value);
            let mut off = 0;
            assert_eq!(super::read_varlong(&buf, &mut off), Some(value));
            assert_eq!(off, buf.len());
        }
    }

    #[test]
    fn frame_uncompressed_round_trip() {
        let body = vec![0x01, 0x02, 0x03];
        let frame = frame_packet(0x2A, &body, Compression::Off);
        let parsed = parse_frame(&frame, Compression::Off).expect("parse");
        assert_eq!(parsed.id, 0x2A);
        assert_eq!(parsed.body, body);
    }

    #[test]
    fn frame_compressed_uncompressed_payload_round_trip() {
        // With compression on but a small packet, the payload is dataLength 0 + packet.
        let body = vec![0xAA, 0xBB];
        let frame = frame_packet(7, &body, Compression::On);
        let parsed = parse_frame(&frame, Compression::On).expect("parse");
        assert_eq!(parsed.id, 7);
        assert_eq!(parsed.body, body);
    }

    #[test]
    fn parse_frame_rejects_truncated() {
        let frame = frame_packet(1, &[0x00], Compression::Off);
        assert!(parse_frame(&frame[..frame.len() - 1], Compression::Off).is_none());
    }

    #[test]
    fn compressed_payload_decompresses() {
        // Build a packet > threshold, compress with zlib, frame with dataLength.
        let mut packet = Vec::new();
        write_varint(&mut packet, 42);
        packet.extend(std::iter::repeat_n(0x5A, 1000));
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write;
        encoder.write_all(&packet).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut payload = Vec::new();
        write_varint(&mut payload, packet.len() as i32);
        payload.extend_from_slice(&compressed);
        let mut frame = Vec::new();
        write_varint(&mut frame, payload.len() as i32);
        frame.extend_from_slice(&payload);

        let parsed = parse_frame(&frame, Compression::On).expect("parse compressed");
        assert_eq!(parsed.id, 42);
        assert_eq!(parsed.body.len(), 1000);
        assert!(parsed.body.iter().all(|&b| b == 0x5A));
    }
}
