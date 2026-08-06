//! Port of `net.minecraft.network.CompressionEncoder` — `extends
//! MessageToByteEncoder<ByteBuf>`.
//!
//! Java: `CompressionEncoder.java` in `working/Paper` (vanilla 26.2). The Paper
//! Velocity compressor path is an optimization that produces the same wire
//! framing as the vanilla `Deflater` path; M1 is zlib-only, so this is the
//! vanilla `compressor == null` branch (`this.deflater`). The surface is raw
//! `bytes::BytesMut` (no netty / tokio `Codec`), matching
//! [`crate::varint21_length_field_prepender`] and
//! [`crate::varint21_frame_decoder`].
//!
//! Wire layout (the payload carried inside one VarInt21 frame):
//! `varint(declaredLength) ++ payload`, where `payload` is the raw packet
//! bytes when `len < threshold` (declaredLength `0`) or a zlib stream of the
//! packet bytes when `len >= threshold` (declaredLength `len`). `threshold` is
//! reconfigurable at runtime (`Connection.setupCompression`), mirroring
//! `setThreshold`.

use bytes::BytesMut;
use flate2::{Compress, Compression, FlushCompress, Status};

/// `CompressionDecoder.MAXIMUM_UNCOMPRESSED_LENGTH` — the Java `8388608` literal
/// the encoder hard-caps against.
pub const MAXIMUM_UNCOMPRESSED_LENGTH: i32 = 8_388_608;

/// The vanilla `encodeBuf` scratch size (Java `new byte[8192]`).
const ENCODE_BUF_LEN: usize = 8192;

/// `IllegalArgumentException` — thrown by [`CompressionEncoder::encode`] when the
/// uncompressed packet exceeds [`MAXIMUM_UNCOMPRESSED_LENGTH`]. Mirrors the
/// unchecked Java exception as a `Result` error so the connection closes
/// deterministically instead of panicking across the task boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressionEncodeError {
    pub message: String,
}

impl CompressionEncodeError {
    pub fn new(message: impl Into<String>) -> Self {
        CompressionEncodeError {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CompressionEncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CompressionEncodeError {}

/// `CompressionEncoder`.
///
/// Holds the reusable zlib compressor (Java `Deflater`, reset per frame) and
/// scratch buffer (Java `encodeBuf`), so encoding is an in-place transform on
/// the re-usable compressor — no per-frame allocation of the compressor.
#[derive(Debug)]
pub struct CompressionEncoder {
    threshold: i32,
    /// Reused zlib compressor — Java `Deflater`, `deflater.reset()` per frame.
    deflater: Compress,
    /// Reused deflate scratch buffer — Java `encodeBuf`.
    encode_buf: [u8; ENCODE_BUF_LEN],
}

impl CompressionEncoder {
    /// `CompressionEncoder(int threshold)` — a fresh zlib compressor at the
    /// default level (Java `new Deflater()`, level 6; Paper's
    /// `misc.compression-level` default of `-1` resolves to the same).
    pub fn new(threshold: i32) -> Self {
        CompressionEncoder {
            threshold,
            deflater: Compress::new(Compression::new(6), true),
            encode_buf: [0u8; ENCODE_BUF_LEN],
        }
    }

    /// `getThreshold()`.
    pub fn get_threshold(&self) -> i32 {
        self.threshold
    }

    /// `setThreshold(int)` — reconfigures the threshold without replacing the
    /// compressor (idempotent, so `setupCompression` can be re-run).
    pub fn set_threshold(&mut self, threshold: i32) {
        self.threshold = threshold;
    }

    /// `encode(...)` — one packet frame in, the compression-frame payload out.
    ///
    /// Mirrors the Java `encode` order:
    /// 1. reject `len > 8388608` ("Packet too big");
    /// 2. `threshold < 0` disables compression — a negative threshold would
    ///    otherwise wrap the signed comparison and take the wrong branch (Java
    ///    always compresses for `threshold < 0`; Rust would never compress);
    ///    `setup_compression` never passes one, this guards the public surface;
    /// 3. below `threshold` → `varint(0) ++ raw`;
    /// 4. else → `varint(len) ++ zlib` streamed through `encodeBuf` until the
    ///    deflater is `finished()`, then `deflater.reset()`.
    ///
    /// The zlib output is a standard RFC-1950 stream (`flate2` zlib header) —
    /// Java's `Inflater` accepts it. DEFLATE is not canonical: the exact
    /// compressed bytes differ from a given zlib build's output (miniz_oxide
    /// vs JDK zlib), but the framing and the decompressed content are
    /// byte-identical.
    ///
    /// Note: at `threshold == 0`, an empty payload compresses (Java has the
    /// same `0 < 0 == false`), emitting `varint(0) ++ zlib-of-empty` — a frame a
    /// decoder reads as a raw packet, not an empty one. This mirrors Paper
    /// byte-for-byte, and the wire protocol is unchanged; `setup_compression`
    /// only ever uses the default 256 or a positive threshold.
    pub fn encode(&mut self, uncompressed: &[u8]) -> Result<BytesMut, CompressionEncodeError> {
        let uncompressed_length = uncompressed.len();
        if uncompressed_length > MAXIMUM_UNCOMPRESSED_LENGTH as usize {
            return Err(CompressionEncodeError::new(format!(
                "Packet too big (is {uncompressed_length}, should be less than {MAXIMUM_UNCOMPRESSED_LENGTH})"
            )));
        }

        let mut out = BytesMut::new();
        if self.threshold < 0 {
            // Disabled compression: raw frame. `setup_compression` never
            // constructs an encoder with a negative threshold, but the public
            // constructor accepts one.
            crate::var_int::write(&mut out, 0);
            out.extend_from_slice(uncompressed);
            return Ok(out);
        }
        if uncompressed_length < self.threshold as usize {
            crate::var_int::write(&mut out, 0);
            out.extend_from_slice(uncompressed);
            return Ok(out);
        }

        crate::var_int::write(&mut out, uncompressed_length as i32);
        let mut input_pos = 0;
        loop {
            let before_in = self.deflater.total_in();
            let before_out = self.deflater.total_out();
            let status = self
                .deflater
                .compress(
                    &uncompressed[input_pos..],
                    &mut self.encode_buf,
                    FlushCompress::Finish,
                )
                .map_err(|e| {
                    CompressionEncodeError::new(format!(
                        "deflate failed: {}",
                        e.message().unwrap_or("zlib error")
                    ))
                })?;
            input_pos += (self.deflater.total_in() - before_in) as usize;
            let written = (self.deflater.total_out() - before_out) as usize;
            out.extend_from_slice(&self.encode_buf[..written]);
            if status == Status::StreamEnd {
                break;
            }
        }
        self.deflater.reset();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression_decoder::CompressionDecoder;
    use bytes::Bytes;
    use flate2::FlushDecompress;

    /// zlib-decompress `data` with a throwaway flate2 inflater. Panics if the
    /// input is not a complete, valid RFC-1950 stream (tests only feed complete
    /// streams), instead of looping forever on a truncated one.
    ///
    /// Uses `FlushDecompress::None` (not `Finish`) for chunked streaming: `Finish`
    /// tells the inflater to expect the end, which is invalid mid-stream when the
    /// output buffer fills before the stream ends.
    fn zlib_inflate(data: &[u8]) -> Vec<u8> {
        let mut inflater = flate2::Decompress::new(true);
        let mut out = Vec::new();
        let mut scratch = [0u8; 8192];
        let mut input_pos = 0;
        loop {
            let before_in = inflater.total_in();
            let before_out = inflater.total_out();
            let status = inflater
                .decompress(&data[input_pos..], &mut scratch, FlushDecompress::None)
                .expect("valid zlib stream");
            input_pos += (inflater.total_in() - before_in) as usize;
            let written = (inflater.total_out() - before_out) as usize;
            out.extend_from_slice(&scratch[..written]);
            match status {
                Status::StreamEnd => break,
                // `Ok` with progress means more output is pending; `BufError` or
                // `Ok` without progress means the stream is truncated.
                Status::Ok if written > 0 || input_pos < data.len() => continue,
                Status::Ok | Status::BufError => panic!("zlib stream did not terminate"),
            }
        }
        out
    }

    #[test]
    fn below_threshold_is_verbatim_with_zero_header() {
        let mut encoder = CompressionEncoder::new(256);
        let payload = [0x42u8; 255];
        let out = encoder.encode(&payload).unwrap();
        // varint(0) ++ raw bytes
        assert_eq!(&out[..1], &[0x00]);
        assert_eq!(&out[1..], &payload[..]);
    }

    #[test]
    fn empty_payload_below_threshold_is_single_zero() {
        let mut encoder = CompressionEncoder::new(256);
        let out = encoder.encode(b"").unwrap();
        assert_eq!(&out[..], &[0x00]);
    }

    #[test]
    fn at_threshold_compresses() {
        let mut encoder = CompressionEncoder::new(256);
        let payload = [0x42u8; 256];
        let out = encoder.encode(&payload).unwrap();
        // Compressed: header is varint(len), payload starts with zlib header.
        assert_eq!(&out[..2], &[0x80, 0x02]);
        assert_eq!(out[2], 0x78); // zlib CMF byte
        let inflated = zlib_inflate(&out[2..]);
        assert_eq!(inflated, payload);
    }

    #[test]
    fn above_threshold_compresses() {
        let mut encoder = CompressionEncoder::new(4);
        let payload = b"Hello, world! Hello, world! Hello, world!";
        let out = encoder.encode(payload).unwrap();
        assert_ne!(&out[..1], &[0x00]);
        let inflated = zlib_inflate(&out[1..]);
        assert_eq!(inflated, payload);
    }

    #[test]
    fn zero_threshold_compresses_everything() {
        let mut encoder = CompressionEncoder::new(0);
        // `0 < 0` is false, so even the empty payload takes the compress path.
        let out = encoder.encode(b"").unwrap();
        assert_eq!(
            &out[..],
            &[0x00, 0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]
        );
        let out = encoder.encode(b"a").unwrap();
        assert_eq!(&out[..1], &[0x01]);
        assert_eq!(zlib_inflate(&out[1..]), b"a");
    }

    #[test]
    fn too_large_payload_is_rejected_before_threshold() {
        let mut encoder = CompressionEncoder::new(256);
        let payload = vec![0u8; (MAXIMUM_UNCOMPRESSED_LENGTH + 1) as usize];
        let err = encoder.encode(&payload).unwrap_err();
        assert_eq!(
            err.message,
            "Packet too big (is 8388609, should be less than 8388608)"
        );
    }

    #[test]
    fn exactly_8mib_is_accepted() {
        // Java's bound is `> 8388608`, so exactly 8 MiB passes the encoder cap
        // (a real client's packet of exactly the maximum is not "too big").
        let mut encoder = CompressionEncoder::new(256);
        let payload = vec![0u8; MAXIMUM_UNCOMPRESSED_LENGTH as usize];
        let out = encoder.encode(&payload).unwrap();
        // 8 MiB's varint header is 4 bytes (`0x80 0x80 0x80 0x04`).
        let header_len = crate::var_int::get_byte_size(MAXIMUM_UNCOMPRESSED_LENGTH) as usize;
        assert_eq!(zlib_inflate(&out[header_len..]), payload);
    }

    #[test]
    fn reuses_compressor_across_frames() {
        // Threshold 0 compresses every payload (0 < 0 is false), exercising the
        // compressor reset across frames of varying sizes, including empty.
        let mut encoder = CompressionEncoder::new(0);
        let payloads: [&[u8]; 4] = [b"aaaa", b"bbbbbbbb", b"cccc", b""];
        for payload in payloads {
            let out = encoder.encode(payload).unwrap();
            assert_eq!(zlib_inflate(&out[1..]), payload);
        }
    }

    #[test]
    fn set_threshold_reconfigures() {
        let mut encoder = CompressionEncoder::new(256);
        let payload = [0x42u8; 10];
        assert_eq!(&encoder.encode(&payload).unwrap()[..1], &[0x00]); // raw
        encoder.set_threshold(5);
        let out = encoder.encode(&payload).unwrap();
        assert_ne!(&out[..1], &[0x00]); // compressed now
        assert_eq!(encoder.get_threshold(), 5);
    }

    #[test]
    fn negative_threshold_disables_compression() {
        // A negative threshold means "disabled": a signed comparison on the
        // `as usize` wrap would otherwise take the wrong branch forever. The
        // guard takes the raw path for every payload.
        let mut encoder = CompressionEncoder::new(-1);
        for payload in [&b""[..], b"a", &[0x42u8; 256], &vec![0x11u8; 10_000][..]] {
            let out = encoder.encode(payload).unwrap();
            assert_eq!(
                &out[..1],
                &[0x00],
                "must stay raw for {} bytes",
                payload.len()
            );
            assert_eq!(&out[1..], payload);
        }
        assert_eq!(encoder.get_threshold(), -1);
    }

    #[test]
    fn negative_threshold_via_setter_disables_compression() {
        // `set_threshold(-1)` is the encoder half of `setup_compression(-1)`,
        // which removes the handlers in Java. The encoder keeps the guard.
        let mut encoder = CompressionEncoder::new(0);
        assert_ne!(&encoder.encode(b"a").unwrap()[..1], &[0x00]); // compressed
        encoder.set_threshold(-1);
        assert_eq!(&encoder.encode(b"a").unwrap()[..], &[0x00, b'a']); // raw
    }

    #[test]
    fn round_trip_via_decoder() {
        let mut encoder = CompressionEncoder::new(256);
        let mut decoder = CompressionDecoder::new(256, true);
        for payload in [
            &b"tiny"[..],
            &vec![0xABu8; 300][..],
            &vec![0x00u8; 10_000][..],
        ] {
            let frame = encoder.encode(payload).unwrap();
            let decoded = decoder.decode(&frame).unwrap();
            assert_eq!(decoded, Bytes::from(payload.to_vec()));
        }
    }
}
