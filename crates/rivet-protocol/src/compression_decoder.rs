//! Port of `net.minecraft.network.CompressionDecoder` — `extends
//! ByteToMessageDecoder`.
//!
//! Java: `CompressionDecoder.java` in `working/Paper` (vanilla 26.2). The Paper
//! Velocity compressor path is an optimization with identical wire semantics;
//! M1 is zlib-only, so this is the vanilla `inflater` path (`compressor == null`).
//!
//! In the netty pipeline the decoder sits between the SPLITTER
//! (`Varint21FrameDecoder`) and the packet `DECODER`, so its input is always one
//! *complete* VarInt21 frame payload:
//! `varint(declaredLength) ++ payload`. `declaredLength == 0` means `payload`
//! is the raw uncompressed packet; otherwise `payload` is a zlib stream and
//! `declaredLength` is the packet's uncompressed size.
//!
//! Because the outer frame decoder has already bound the frame, this port is a
//! per-frame transform over `&[u8]` — no accumulation state, mirroring how
//! fragmentation/coalescing is the SPLITTER's job in netty. The zlib inflater is
//! held and reset per frame exactly like Java's `Inflater`.

use bytes::{Buf, Bytes, BytesMut};
use flate2::{Decompress, FlushDecompress};

/// `CompressionDecoder.MAXIMUM_COMPRESSED_LENGTH` (Java `public static final int`).
///
/// Not consulted by the vanilla decode path (the outer VarInt21 frame decoder
/// already caps the compressed frame at 3 header bytes / ~2 MiB); ported for
/// surface fidelity.
pub const MAXIMUM_COMPRESSED_LENGTH: i32 = 2_097_152;

/// `CompressionDecoder.MAXIMUM_UNCOMPRESSED_LENGTH` (Java `public static final int`).
pub const MAXIMUM_UNCOMPRESSED_LENGTH: i32 = 8_388_608;

/// `DecoderException` — thrown by [`CompressionDecoder::decode`] for a
/// badly-compressed, oversized, or corrupt frame. Mirrors the unchecked
/// `io.netty.handler.codec.DecoderException` as a `Result` error so the
/// connection closes deterministically instead of panicking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressionDecodeError {
    pub message: String,
}

impl CompressionDecodeError {
    pub fn new(message: impl Into<String>) -> Self {
        CompressionDecodeError {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CompressionDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CompressionDecodeError {}

/// `CompressionDecoder`.
pub struct CompressionDecoder {
    threshold: i32,
    validate_decompressed: bool,
    /// Reused zlib inflater — Java `Inflater`, `inflater.reset()` per frame.
    inflater: Decompress,
}

impl std::fmt::Debug for CompressionDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompressionDecoder")
            .field("threshold", &self.threshold)
            .field("validate_decompressed", &self.validate_decompressed)
            .finish()
    }
}

impl CompressionDecoder {
    /// `CompressionDecoder(int threshold, boolean validateDecompressed)`.
    pub fn new(threshold: i32, validate_decompressed: bool) -> Self {
        CompressionDecoder {
            threshold,
            validate_decompressed,
            inflater: Decompress::new(true),
        }
    }

    /// `setThreshold(int, boolean)` — reconfigures in place (idempotent, so
    /// `Connection.setupCompression` can be re-run after the login packet).
    pub fn set_threshold(&mut self, threshold: i32, validate_decompressed: bool) {
        self.threshold = threshold;
        self.validate_decompressed = validate_decompressed;
    }

    /// `decode(...)` — one complete VarInt21 frame payload in, the decompressed
    /// packet frame out (`varint(packetId) ++ body`).
    ///
    /// Mirrors the Java `decode` order:
    /// 1. read `uncompressedLength`; `0` → the rest of the frame is the packet;
    /// 2. reject a negative declared length — Java's `directBuffer(negative)`
    ///    throws `IllegalArgumentException` (a clean netty disconnect); here it
    ///    would panic on the `usize` wrap;
    /// 3. when `validateDecompressed`, reject `len < threshold` (first, as in
    ///    Java);
    /// 4. reject `len > 8388608` with validation on and off (Java's `directBuffer`
    ///    is the same allocation surface);
    /// 5. feed the *whole* remaining frame to the inflater (Java consumes all of
    ///    `in`), inflate into a buffer of exactly the declared size — Java's
    ///    `directBuffer(uncompressedLength)` and single `inflater.inflate` call —
    ///    and reject when the actual decompressed length does not match the
    ///    declared size.
    ///
    /// A stream that would decompress *beyond* the declared length fills the
    /// declared-size buffer and is accepted (Java never reads past the buffer).
    /// A truncated stream ends with `actual < declared` and is rejected; trailing
    /// bytes after the zlib stream are consumed and ignored, exactly as Java
    /// skips all of `in`.
    pub fn decode(&mut self, frame: &[u8]) -> Result<Bytes, CompressionDecodeError> {
        let mut buf = BytesMut::from(frame);
        let uncompressed_length = read_varint(&mut buf)?;

        if uncompressed_length == 0 {
            // `in.readBytes(in.readableBytes())` — everything left is the raw
            // uncompressed packet.
            return Ok(buf.freeze());
        }

        // First, as in Java (`len < threshold` fires before the size cap): with
        // `validate=true` a negative declared length is caught here too, so the
        // message matches Java's. With `validate=false` it is skipped, exactly
        // like Java's `if (this.validateDecompressed)` block.
        if self.validate_decompressed && uncompressed_length < self.threshold {
            return Err(CompressionDecodeError::new(format!(
                "Badly compressed packet - size of {uncompressed_length} is below server threshold of {}",
                self.threshold
            )));
        }

        // Bound the declared-size allocation even with validation off: Java's
        // `directBuffer` has no such cap, so this is a Rust-specific safety bound
        // that keeps the `validate=false` path a clean close instead of a
        // multi-gigabyte allocation.
        if uncompressed_length > MAXIMUM_UNCOMPRESSED_LENGTH {
            return Err(CompressionDecodeError::new(format!(
                "Badly compressed packet - size of {uncompressed_length} is larger than protocol maximum of {MAXIMUM_UNCOMPRESSED_LENGTH}"
            )));
        }

        // Safety net for `validate=false`, where the threshold check above is
        // skipped: a negative declared length would panic on the `usize` wrap
        // here. Java's `directBuffer(negative)` throws instead (a clean netty
        // disconnect), so reject deterministically. `read_varint` can produce one
        // only for a legal 5-byte varint whose 5th byte carries a high bit
        // (`0xFF 0xFF 0xFF 0xFF 0x7F` → -1) — a hostile-but-well-formed frame.
        if uncompressed_length < 0 {
            return Err(CompressionDecodeError::new(format!(
                "Badly compressed packet - negative declared length {uncompressed_length}"
            )));
        }

        // Java: `setupInflaterInput` feeds the whole remaining frame and skips it
        // from `in`; `inflate` writes into a fixed buffer of `uncompressedLength`.
        let declared = uncompressed_length as usize;
        let compressed = &buf[..];
        // One inflate into a declared-size buffer — Java's `inflater.inflate` is
        // called exactly once against `directBuffer(uncompressedLength)`.
        let mut output = vec![0u8; declared];
        let before_out = self.inflater.total_out();
        let status = self
            .inflater
            .decompress(compressed, &mut output, FlushDecompress::Finish);
        let actual = (self.inflater.total_out() - before_out) as usize;
        self.inflater.reset(true);

        if let Err(e) = status {
            // Java wraps the `DataFormatException` in a `DecoderException` whose
            // message is the native zlib reason; not portable here.
            return Err(CompressionDecodeError::new(format!(
                "Badly compressed packet - invalid zlib stream: {}",
                e.message().unwrap_or("inflate failed")
            )));
        }

        if actual != declared {
            return Err(CompressionDecodeError::new(format!(
                "Badly compressed packet - actual length of uncompressed payload {actual} is does not match declared size {uncompressed_length}"
            )));
        }
        output.truncate(actual);
        Ok(Bytes::from(output))
    }
}

/// `VarInt.read` on the frame front — a bounded reader that returns `Result`
/// instead of [`crate::var_int::read`]'s panic on "VarInt too big", and treats a
/// varint running past the end of the frame as malformed (a complete frame must
/// carry its compression header).
fn read_varint(buf: &mut BytesMut) -> Result<i32, CompressionDecodeError> {
    let mut out: u32 = 0;
    for i in 0..5u32 {
        if !buf.has_remaining() {
            return Err(CompressionDecodeError::new(
                "compression length varint runs past end of frame",
            ));
        }
        let byte = buf.get_u8();
        out |= ((byte & 0x7F) as u32) << (i * 7);
        if byte & 0x80 == 0 {
            return Ok(out as i32);
        }
    }
    Err(CompressionDecodeError::new("VarInt too big"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression_encoder::CompressionEncoder;
    use crate::varint21_frame_decoder::Varint21FrameDecoder;
    use crate::varint21_length_field_prepender::encode_frame;

    /// Encode `packet` with a threshold-`256` encoder, wrapped in a VarInt21
    /// frame (the exact bytes `send_packet` puts on the wire).
    fn framed(encoder: &mut CompressionEncoder, packet: &[u8]) -> Bytes {
        let compressed = encoder.encode(packet).unwrap();
        Bytes::from(encode_frame(&compressed).unwrap().to_vec())
    }

    /// The full inbound path: feed raw wire bytes, run the VarInt21 frame
    /// decoder, then decompress each frame. Returns the decoded packet frames.
    fn decode_wire(
        frame_decoder: &Varint21FrameDecoder,
        compression_decoder: &mut CompressionDecoder,
        wire: &[u8],
    ) -> Vec<Bytes> {
        let mut buf = BytesMut::from(wire);
        let mut packets = Vec::new();
        while let Some(frame) = frame_decoder.decode(&mut buf).unwrap() {
            packets.push(compression_decoder.decode(&frame).unwrap());
        }
        packets
    }

    /// A known-good Java `Deflater` zlib stream for `"hello"` (RFC-1950:
    /// CMF/FLG `78 9C`, deflate payload, Adler-32 `06 2C 02 15`).
    const JAVA_HELLO_ZLIB: &[u8] = &[
        0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x07, 0x00, 0x06, 0x2C, 0x02, 0x15,
    ];

    #[test]
    fn declared_zero_returns_rest_verbatim() {
        let mut decoder = CompressionDecoder::new(256, true);
        let frame = [0x00, 0x01, 0x02, 0x03];
        assert_eq!(
            decoder.decode(&frame).unwrap(),
            Bytes::from_static(&[1, 2, 3])
        );
    }

    #[test]
    fn declared_zero_with_empty_payload() {
        let mut decoder = CompressionDecoder::new(256, true);
        let frame = [0x00];
        assert_eq!(decoder.decode(&frame).unwrap(), Bytes::new());
    }

    #[test]
    fn decodes_java_zlib_stream() {
        // A frame produced by Java's CompressionEncoder: varint(5) ++ zlib("hello").
        // Threshold 0 so the declared 5 is not rejected as below-threshold.
        let mut frame = Vec::new();
        frame.push(0x05);
        frame.extend_from_slice(JAVA_HELLO_ZLIB);
        let mut decoder = CompressionDecoder::new(0, true);
        assert_eq!(
            decoder.decode(&frame).unwrap(),
            Bytes::from_static(b"hello")
        );
    }

    #[test]
    fn declared_below_threshold_is_rejected() {
        let mut decoder = CompressionDecoder::new(256, true);
        // varint(10) + zlib of "hello" (5 bytes): the declared size (10) is below
        // the 256 threshold, so validation fires before inflation.
        let mut frame = Vec::new();
        frame.push(0x0A);
        frame.extend_from_slice(JAVA_HELLO_ZLIB);
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(
            err.message,
            "Badly compressed packet - size of 10 is below server threshold of 256"
        );
    }

    #[test]
    fn validate_decompressed_false_skips_threshold_check() {
        let mut decoder = CompressionDecoder::new(256, false);
        let mut frame = Vec::new();
        frame.push(0x0A);
        frame.extend_from_slice(JAVA_HELLO_ZLIB);
        // No threshold check: the stream inflates to 5 bytes, which does not
        // match the declared 10 → the size mismatch fires instead.
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(
            err.message,
            "Badly compressed packet - actual length of uncompressed payload 5 is does not match declared size 10"
        );
    }

    #[test]
    fn declared_above_protocol_maximum_is_rejected() {
        let mut decoder = CompressionDecoder::new(256, true);
        // varint(8388609) — the validation check fires before any data is read.
        let frame = [0x81, 0x80, 0x80, 0x04];
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(
            err.message,
            "Badly compressed packet - size of 8388609 is larger than protocol maximum of 8388608"
        );
    }

    #[test]
    fn negative_declared_length_is_rejected_with_validation_on() {
        // `0xFF 0xFF 0xFF 0xFF 0x7F` decodes to -1 (a legal 5-byte varint whose
        // 5th byte is within range but has a high bit set). With validation on,
        // the threshold check fires first — the same message Java produces
        // (`-1 < threshold`), before the `usize` wrap could panic.
        let mut decoder = CompressionDecoder::new(256, true);
        let frame = [0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 0x01, 0x02];
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(
            err.message,
            "Badly compressed packet - size of -1 is below server threshold of 256"
        );
    }

    #[test]
    fn negative_declared_length_is_rejected_with_validation_off() {
        // With validation off the threshold check is skipped (as in Java), so the
        // negative guard fires before the declared-size allocation — otherwise
        // the `as usize` wrap would be a capacity-overflow panic.
        let mut decoder = CompressionDecoder::new(256, false);
        let frame = [0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 0x01, 0x02];
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(
            err.message,
            "Badly compressed packet - negative declared length -1"
        );
    }

    #[test]
    fn huge_positive_declared_length_is_rejected_before_allocation() {
        let mut decoder = CompressionDecoder::new(256, true);
        // `0xFF 0xFF 0xFF 0xFF 0x07` decodes to 2^31-1. With validation on this
        // was already caught by the protocol-maximum check; it must be caught
        // before allocating a 2 GiB buffer.
        let frame = [0xFF, 0xFF, 0xFF, 0xFF, 0x07, 0x01];
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(
            err.message,
            "Badly compressed packet - size of 2147483647 is larger than protocol maximum of 8388608"
        );
    }

    #[test]
    fn huge_positive_declared_length_is_rejected_with_validation_off() {
        // Same frame with validation off: the 8 MiB cap must still fire before
        // the `vec![0u8; declared]` allocation.
        let mut decoder = CompressionDecoder::new(256, false);
        let frame = [0xFF, 0xFF, 0xFF, 0xFF, 0x07, 0x01];
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(
            err.message,
            "Badly compressed packet - size of 2147483647 is larger than protocol maximum of 8388608"
        );
    }

    #[test]
    fn declared_size_mismatch_is_rejected() {
        let mut decoder = CompressionDecoder::new(4, true);
        let mut frame = Vec::new();
        frame.push(0x0A); // declared 10 (>= threshold 4, so validation passes)
        frame.extend_from_slice(JAVA_HELLO_ZLIB); // inflates to 5
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(
            err.message,
            "Badly compressed packet - actual length of uncompressed payload 5 is does not match declared size 10"
        );
    }

    #[test]
    fn oversized_decompression_truncates_at_declared() {
        // declared 5 but the stream inflates to 11 ("hello world"); Java fills
        // the fixed 5-byte buffer and accepts — decompression is capped, not
        // rejected.
        let mut encoder = CompressionEncoder::new(0);
        let zlib = encoder.encode(b"hello world").unwrap();
        let mut frame = Vec::new();
        frame.push(0x05);
        frame.extend_from_slice(&zlib[1..]); // drop the varint(11) header
        let mut decoder = CompressionDecoder::new(0, true);
        assert_eq!(
            decoder.decode(&frame).unwrap(),
            Bytes::from_static(b"hello")
        );
    }

    #[test]
    fn trailing_data_after_zlib_stream_is_consumed_and_ignored() {
        let mut decoder = CompressionDecoder::new(0, true);
        let mut frame = Vec::new();
        frame.push(0x05);
        frame.extend_from_slice(JAVA_HELLO_ZLIB);
        frame.extend_from_slice(b"trailing-garbage");
        // Java `setupInflaterInput` skips the whole frame; the inflater stops at
        // stream end and the trailing bytes are ignored.
        assert_eq!(
            decoder.decode(&frame).unwrap(),
            Bytes::from_static(b"hello")
        );
    }

    #[test]
    fn truncated_stream_is_rejected() {
        let mut decoder = CompressionDecoder::new(0, true);
        // declared 100, but only the first half of the zlib stream is present.
        // The inflater writes what it can (< 100) and never reaches stream end,
        // so the declared size is never matched.
        let mut frame = Vec::new();
        frame.push(0x64);
        frame.extend_from_slice(&JAVA_HELLO_ZLIB[..6]);
        let err = decoder.decode(&frame).unwrap_err();
        assert!(
            err.message
                .ends_with(" is does not match declared size 100"),
            "unexpected message: {}",
            err.message
        );
        assert!(
            err.message
                .starts_with("Badly compressed packet - actual length")
        );
    }

    #[test]
    fn truncated_stream_mid_data_is_accepted() {
        // declared 5, and the zlib stream is cut right after the deflate payload
        // (before the adler32 trailer). Java's fixed 5-byte buffer fills with
        // "hello" before the inflater ever reaches the missing trailer, so the
        // frame is accepted — decompression is bounded by the declared size, and
        // a missing checksum on the truncated tail is never observed.
        let mut decoder = CompressionDecoder::new(0, true);
        let mut frame = Vec::new();
        frame.push(0x05);
        frame.extend_from_slice(&JAVA_HELLO_ZLIB[..8]); // through the deflate data
        assert_eq!(
            decoder.decode(&frame).unwrap(),
            Bytes::from_static(b"hello")
        );
    }

    #[test]
    fn corrupt_stream_is_rejected() {
        let mut decoder = CompressionDecoder::new(256, true);
        // declared 5, payload is not a zlib stream at all.
        let frame = [0x05, 0xFF, 0x00, 0x11, 0x22, 0x33];
        assert!(decoder.decode(&frame).is_err());
    }

    #[test]
    fn varint_runs_past_end_of_frame() {
        let mut decoder = CompressionDecoder::new(256, true);
        // A single continuation byte cannot terminate the compression varint.
        let frame = [0x80];
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(
            err.message,
            "compression length varint runs past end of frame"
        );
    }

    #[test]
    fn varint_too_big_is_rejected() {
        let mut decoder = CompressionDecoder::new(256, true);
        let frame = [0x80, 0x80, 0x80, 0x80, 0x80];
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(err.message, "VarInt too big");
    }

    #[test]
    fn set_threshold_reconfigures_validation() {
        let mut decoder = CompressionDecoder::new(256, true);
        let mut frame = Vec::new();
        frame.push(0x0A);
        frame.extend_from_slice(JAVA_HELLO_ZLIB);
        assert!(decoder.decode(&frame).is_err()); // 10 < 256
        decoder.set_threshold(4, true);
        // 10 >= 4 now passes the threshold check; size mismatch still fires.
        let err = decoder.decode(&frame).unwrap_err();
        assert_eq!(
            err.message,
            "Badly compressed packet - actual length of uncompressed payload 5 is does not match declared size 10"
        );
    }

    #[test]
    fn fragmented_wire_bytes_round_trip() {
        // The full inbound pipeline: VarInt21 framing + compression. Feed the
        // wire bytes one byte at a time; frames decode only when complete.
        let mut encoder = CompressionEncoder::new(256);
        let packets: Vec<Vec<u8>> = vec![
            b"first-packet".to_vec(),
            vec![0xABu8; 500],
            b"last".to_vec(),
        ];
        let mut wire = Vec::new();
        for p in &packets {
            wire.extend_from_slice(&framed(&mut encoder, p));
        }

        let frame_decoder = Varint21FrameDecoder::new(None);
        let mut compression_decoder = CompressionDecoder::new(256, true);
        let mut buf = BytesMut::new();
        let mut decoded = Vec::new();
        for (i, b) in wire.iter().enumerate() {
            buf.extend_from_slice(&[*b]);
            loop {
                match frame_decoder.decode(&mut buf) {
                    Ok(Some(frame)) => decoded.push(compression_decoder.decode(&frame).unwrap()),
                    Ok(None) => break,
                    Err(e) => panic!("corrupted at byte {i}: {e:?}"),
                }
            }
        }
        let decoded_bytes: Vec<Bytes> = decoded.into_iter().collect();
        assert_eq!(decoded_bytes.len(), packets.len());
        for (got, want) in decoded_bytes.iter().zip(&packets) {
            assert_eq!(&got[..], &want[..]);
        }
    }

    #[test]
    fn coalesced_frames_in_one_buffer_round_trip() {
        let mut encoder = CompressionEncoder::new(256);
        let packets: Vec<Vec<u8>> = vec![
            b"one".to_vec(),
            vec![0xCDu8; 300],
            b"three".to_vec(),
            b"".to_vec(),
        ];
        let mut wire = Vec::new();
        for p in &packets {
            wire.extend_from_slice(&framed(&mut encoder, p));
        }

        let frame_decoder = Varint21FrameDecoder::new(None);
        let mut compression_decoder = CompressionDecoder::new(256, true);
        let decoded = decode_wire(&frame_decoder, &mut compression_decoder, &wire);
        assert_eq!(decoded.len(), packets.len());
        for (got, want) in decoded.iter().zip(&packets) {
            assert_eq!(&got[..], &want[..]);
        }
    }

    #[test]
    fn mixed_below_and_at_threshold_frames_round_trip() {
        // Threshold boundaries across a real wire stream: a below-threshold frame
        // is verbatim, an at-threshold frame is compressed, a large frame is
        // compressed — all in one coalesced buffer.
        let mut encoder = CompressionEncoder::new(256);
        let packets: Vec<Vec<u8>> = vec![
            vec![0x11u8; 255], // below
            vec![0x22u8; 256], // at threshold
            vec![0x33u8; 257], // above
            vec![0x44u8; 10_000],
        ];
        let mut wire = Vec::new();
        for p in &packets {
            wire.extend_from_slice(&framed(&mut encoder, p));
        }

        let frame_decoder = Varint21FrameDecoder::new(None);
        let mut compression_decoder = CompressionDecoder::new(256, true);
        let decoded = decode_wire(&frame_decoder, &mut compression_decoder, &wire);
        assert_eq!(decoded.len(), packets.len());
        for (got, want) in decoded.iter().zip(&packets) {
            assert_eq!(&got[..], &want[..]);
        }
    }
}
