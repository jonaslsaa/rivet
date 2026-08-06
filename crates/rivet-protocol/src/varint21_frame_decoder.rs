//! Port of `net.minecraft.network.Varint21FrameDecoder` — `extends ByteToMessageDecoder`.
//!
//! Java: `Varint21FrameDecoder.java` in `working/Paper` (vanilla 26.2; the Paper
//! `!ctx.channel().isActive()` skip-and-discard is a connection-level optimization and
//! belongs to the caller — on disconnect the caller drains `buf` — not to frame
//! decoding, so it is not ported here).
//!
//! The surface is raw `bytes::BytesMut` (no netty / tokio `Codec`): the scaffold wires
//! only `bytes`. `decode` peels one frame from the front of the buffer and never
//! consumes partial input, mirroring the `ByteToMessageDecoder` accumulate/reset
//! contract.
//!
//! The Java `helperBuf` scratch buffer is a netty implementation detail; the header is
//! read by peeking at the buffer, so no copy is needed. `BandwidthDebugMonitor` is
//! modeled as an optional callback — only the `onReceive(int)` side is ported, the
//! sample-logger tick subsystem is not.

use bytes::{Buf, Bytes, BytesMut};

/// `Varint21FrameDecoder.MAX_VARINT21_BYTES` (Java `private static final int = 3`).
const MAX_VARINT21_BYTES: usize = 3;

/// Outcome of [`copy_varint`]: input ran out before the varint terminated, the varint
/// is wider than 21 bits, or how many header bytes were read.
enum CopyVarint {
    Pending,
    TooWide,
    Done(usize),
}

/// `CorruptedFrameException` — thrown by [`Varint21FrameDecoder::decode`] for a
/// zero-length frame or a header wider than 21 bits. Mirrors the unchecked
/// `io.netty.handler.codec.CorruptedFrameException`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorruptedFrameException {
    pub message: String,
}

impl CorruptedFrameException {
    pub fn new(message: impl Into<String>) -> Self {
        CorruptedFrameException {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CorruptedFrameException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CorruptedFrameException {}

/// `BandwidthDebugMonitor` — the `onReceive(int)` side of the Java debug monitor,
/// modeled as an optional callback. The sample-logger tick subsystem is not ported.
pub type BandwidthDebugMonitor = fn(usize);

/// `Varint21FrameDecoder`.
///
/// Stateless in this port: the only state Java keeps per handler is the netty
/// `helperBuf` scratch buffer, which `decode` does not need.
#[derive(Debug, Clone, Copy)]
pub struct Varint21FrameDecoder {
    /// `@Nullable BandwidthDebugMonitor monitor`.
    monitor: Option<BandwidthDebugMonitor>,
}

impl Varint21FrameDecoder {
    /// `Varint21FrameDecoder(@Nullable BandwidthDebugMonitor)`.
    pub fn new(monitor: Option<BandwidthDebugMonitor>) -> Self {
        Varint21FrameDecoder { monitor }
    }

    /// `decode(...)` — peels one frame off the front of `buf`.
    ///
    /// Returns `None` (consuming nothing) when the header or payload is incomplete, so
    /// the caller can append more bytes and call again. Returns `Err` for a zero-length
    /// frame or a header wider than 21 bits; like Java, the header bytes are consumed on
    /// those error paths (netty `copyVarint` advances the reader index before throwing
    /// and `decode` does not reset it there).
    pub fn decode(&self, buf: &mut BytesMut) -> Result<Option<Bytes>, CorruptedFrameException> {
        let mut helper = [0u8; MAX_VARINT21_BYTES];
        let header_len = match copy_varint(buf.as_ref(), &mut helper) {
            CopyVarint::Pending => return Ok(None),
            CopyVarint::TooWide => {
                // Java: `copyVarint` read all three header bytes from `in` before
                // throwing, and `decode` does not `resetReaderIndex` on this path, so
                // the three header bytes stay consumed.
                buf.advance(MAX_VARINT21_BYTES);
                return Err(CorruptedFrameException::new("length wider than 21-bit"));
            }
            CopyVarint::Done(len) => len,
        };

        let length = read_varint(&helper[..header_len]);
        if length == 0 {
            // Java: the header bytes were consumed from `in` by `copyVarint` and
            // `decode` throws without resetting the reader index, so they stay consumed.
            buf.advance(header_len);
            return Err(CorruptedFrameException::new("Frame length cannot be zero"));
        }
        let length = length as usize;

        if buf.len() < header_len + length {
            // `in.resetReaderIndex()` — the payload has not fully arrived yet.
            return Ok(None);
        }

        if let Some(monitor) = self.monitor {
            monitor(length + get_byte_size(length));
        }

        let mut frame = buf.split_to(header_len + length);
        drop(frame.split_to(header_len));
        Ok(Some(frame.freeze()))
    }
}

/// `copyVarint(ByteBuf, ByteBuf)` — copies up to three header bytes into `out`, stopping
/// at the first byte without a continuation bit.
fn copy_varint(input: &[u8], out: &mut [u8; MAX_VARINT21_BYTES]) -> CopyVarint {
    for i in 0..MAX_VARINT21_BYTES {
        if i >= input.len() {
            // `!in.isReadable()` — the header has not fully arrived.
            return CopyVarint::Pending;
        }
        let byte = input[i];
        out[i] = byte;
        if !has_continuation_bit(byte) {
            return CopyVarint::Done(i + 1);
        }
    }
    // All three bytes had continuation bits.
    CopyVarint::TooWide
}

/// `VarInt.hasContinuationBit(byte)`.
fn has_continuation_bit(byte: u8) -> bool {
    byte & 0x80 == 0x80
}

/// `VarInt.read` on the collected header bytes (always a terminated `<=3`-byte varint).
fn read_varint(header: &[u8]) -> i32 {
    let mut out = 0;
    for (i, &byte) in header.iter().enumerate() {
        out |= ((byte & 0x7F) as i32) << (i * 7);
    }
    out
}

/// `VarInt.getByteSize(int)` — the `VARINT_EXACT_BYTE_LENGTHS[Integer.numberOfLeadingZeros]`
/// table, i.e. `ceil(bit_length(v) / 7)` for `v >= 1` and `1` for `v == 0`.
///
/// Stays local (duplicated in `varint21_length_field_prepender.rs`) because
/// `crate::var_int::get_byte_size` is `i32`-scoped while frame lengths are `usize`.
fn get_byte_size(value: usize) -> usize {
    if value == 0 {
        return 1;
    }
    let bits = usize::BITS as usize - value.leading_zeros() as usize;
    bits.div_ceil(7)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static RECEIVED: AtomicUsize = AtomicUsize::new(0);

    fn count_bytes(n: usize) {
        RECEIVED.fetch_add(n, Ordering::SeqCst);
    }

    fn decode_all(decoder: &Varint21FrameDecoder, buf: &mut BytesMut) -> Vec<Bytes> {
        let mut out = Vec::new();
        while let Some(frame) = decoder.decode(buf).unwrap() {
            out.push(frame);
        }
        out
    }

    #[test]
    fn empty_buffer_is_pending() {
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::new();
        assert_eq!(decoder.decode(&mut buf).unwrap(), None);
    }

    #[test]
    fn single_frame() {
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&b"\x05Hello"[..]);
        assert_eq!(
            decode_all(&decoder, &mut buf),
            vec![Bytes::from_static(b"Hello")]
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn multiple_frames_in_one_buffer() {
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&b"\x01A\x02BC"[..]);
        assert_eq!(
            decode_all(&decoder, &mut buf),
            vec![Bytes::from_static(b"A"), Bytes::from_static(b"BC")]
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn pending_header_only_consumes_nothing() {
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&b"\x05"[..]);
        assert_eq!(decoder.decode(&mut buf).unwrap(), None);
        assert_eq!(&buf[..], b"\x05");
        buf.extend_from_slice(b"Hello");
        assert_eq!(
            decoder.decode(&mut buf).unwrap().unwrap(),
            Bytes::from_static(b"Hello")
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn pending_continuation_header_then_complete() {
        // 0x80 (one continuation byte) is pending; append 0x02 to make length 256.
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&b"\x80"[..]);
        assert_eq!(decoder.decode(&mut buf).unwrap(), None);
        assert_eq!(&buf[..], b"\x80");
        buf.extend_from_slice(b"\x02");
        // Header complete, payload not yet.
        assert_eq!(decoder.decode(&mut buf).unwrap(), None);
        assert_eq!(&buf[..], b"\x80\x02");
        let payload = vec![0xABu8; 256];
        buf.extend_from_slice(&payload);
        assert_eq!(
            decoder.decode(&mut buf).unwrap().unwrap(),
            Bytes::from(payload)
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn pending_partial_payload_consumes_nothing() {
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&b"\x05Hel"[..]);
        assert_eq!(decoder.decode(&mut buf).unwrap(), None);
        assert_eq!(&buf[..], b"\x05Hel");
        buf.extend_from_slice(b"lo");
        assert_eq!(
            decoder.decode(&mut buf).unwrap().unwrap(),
            Bytes::from_static(b"Hello")
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn incremental_byte_by_byte_feed() {
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::new();
        let frame: &[u8] = b"\x05Hello";
        for (i, b) in frame.iter().copied().enumerate() {
            buf.extend_from_slice(&[b]);
            let complete = i + 1 == frame.len();
            let decoded = decoder.decode(&mut buf).unwrap();
            assert_eq!(decoded.is_some(), complete, "at byte {i}");
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn pending_then_three_continuation_bytes_is_too_wide() {
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&b"\x80\x80"[..]);
        assert_eq!(decoder.decode(&mut buf).unwrap(), None);
        buf.extend_from_slice(b"\x80");
        let err = decoder.decode(&mut buf).unwrap_err();
        assert_eq!(err.message, "length wider than 21-bit");
        // Java consumes the three header bytes before throwing (copyVarint advances the
        // reader index; decode does not resetReaderIndex on this path).
        assert_eq!(&buf[..], b"");
    }

    #[test]
    fn zero_length_frame_is_corrupted() {
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&b"\x00"[..]);
        let err = decoder.decode(&mut buf).unwrap_err();
        assert_eq!(err.message, "Frame length cannot be zero");
        // Java consumes the header byte before throwing.
        assert_eq!(&buf[..], b"");
    }

    #[test]
    fn two_byte_varint_of_zero_is_corrupted() {
        // 0x80 0x00 encodes zero across two bytes — still a zero-length frame.
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&b"\x80\x00"[..]);
        let err = decoder.decode(&mut buf).unwrap_err();
        assert_eq!(err.message, "Frame length cannot be zero");
        // Both header bytes stay consumed, exactly as Java leaves the reader index.
        assert_eq!(&buf[..], b"");
    }

    #[test]
    fn header_wider_than_21_bits_is_corrupted() {
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&b"\x80\x80\x80"[..]);
        let err = decoder.decode(&mut buf).unwrap_err();
        assert_eq!(err.message, "length wider than 21-bit");
        // All three header bytes consumed (Java's copyVarint read them before throwing).
        assert_eq!(&buf[..], b"");
    }

    #[test]
    fn four_byte_header_is_rejected_at_three_bytes() {
        // 2^21 needs a four-byte varint; the first three bytes all carry continuation bits.
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&b"\x80\x80\x80\x01"[..]);
        let err = decoder.decode(&mut buf).unwrap_err();
        assert_eq!(err.message, "length wider than 21-bit");
        // Exactly the three scanned header bytes are consumed; the fourth is untouched.
        assert_eq!(&buf[..], b"\x01");
    }

    #[test]
    fn three_byte_header_decodes() {
        // 0x80 0x80 0x01 => length 16384 (2^14), the smallest three-byte varint.
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&b"\x80\x80\x01"[..]);
        let payload = vec![0xCDu8; 16384];
        buf.extend_from_slice(&payload);
        assert_eq!(
            decoder.decode(&mut buf).unwrap().unwrap(),
            Bytes::from(payload)
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn max_three_byte_length_decodes() {
        // 0xFF 0xFF 0x7F => length 2097151 (2^21 - 1), the largest three-byte varint.
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&b"\xFF\xFF\x7F"[..]);
        let payload = vec![0x42u8; 2097151];
        buf.extend_from_slice(&payload);
        assert_eq!(
            decoder.decode(&mut buf).unwrap().unwrap(),
            Bytes::from(payload)
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn monitor_reports_header_plus_payload() {
        RECEIVED.store(0, Ordering::SeqCst);
        let decoder = Varint21FrameDecoder::new(Some(count_bytes));
        let mut buf = BytesMut::from(&b"\x05Hello"[..]);
        assert_eq!(
            decoder.decode(&mut buf).unwrap().unwrap(),
            Bytes::from_static(b"Hello")
        );
        assert_eq!(RECEIVED.load(Ordering::SeqCst), 6);

        // Two-byte header: length 256 reports 256 + 2.
        RECEIVED.store(0, Ordering::SeqCst);
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&b"\x80\x02"[..]);
        buf.extend_from_slice(&vec![0u8; 256]);
        let frame = decoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame.len(), 256);
        assert_eq!(RECEIVED.load(Ordering::SeqCst), 258);
    }
}
