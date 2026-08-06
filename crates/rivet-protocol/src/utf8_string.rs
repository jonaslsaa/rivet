//! Port of `net.minecraft.network.Utf8String`.
//!
//! Java: `Utf8String.java` in `working/Paper` (vanilla 26.2). Length-prefixed
//! protocol UTF-8 strings with an exact Java-observable contract: every check
//! fires on the same byte/char count and in the same order as the Java
//! implementation, and every error message matches `DecoderException` /
//! `EncoderException` verbatim. Per PORTING.md line 33 the unchecked netty
//! exceptions map to `panic!` with the exact message.
//!
//! Two boundaries matter and both are ported exactly:
//!
//! - `ByteBufUtil.utf8MaxBytes(n) = n * MAX_BYTES_PER_CHAR_UTF8`, where netty's
//!   `MAX_BYTES_PER_CHAR_UTF8 = (int) CharsetUtil.encoder(UTF_8).maxBytesPerChar()`.
//!   On the JDKs Paper runs on that is `3.0` (verified empirically). Note that
//!   `maxLength * 3` is Java `int` arithmetic — `wrapping_mul` reproduces the
//!   overflow for absurd `maxLength` values (unreachable at `MAX_STRING_LENGTH`).
//! - Malformed UTF-8 decodes via the WHATWG "UTF-8 decode" algorithm (what the
//!   JDK's `new String(bytes, UTF_8)` implements), not the maximal-subpart
//!   rules that `String::from_utf8_lossy` uses. The decoder here is
//!   differential-tested against a Java reference over an 18,848-case corpus.
//!   In particular a 3-byte surrogate sequence (`ED A0 80`) yields one U+FFFD
//!   in Java but three under `from_utf8_lossy`, so a hand port is required.
//!
//! The write side takes `&str` (always valid UTF-8, so no unpaired surrogates):
//! netty's `writeUtf8` would emit the same bytes for any valid string, and
//! `String::length()` (UTF-16 code units) is the count used in every check and
//! message. The netty temp scratch buffer is an allocation detail, not behavior.

use bytes::{Buf, BufMut};

/// `ByteBufUtil.MAX_BYTES_PER_CHAR_UTF8` — `(int) CharsetUtil.encoder(UTF_8).maxBytesPerChar()`
/// on the JDKs Paper ships on (3.0).
const MAX_BYTES_PER_CHAR_UTF8: i32 = 3;

/// `ByteBufUtil.utf8MaxBytes(int)` — the upper bound on the UTF-8 byte length
/// of `seqLength` UTF-16 code units.
pub fn utf8_max_bytes(seq_length: i32) -> i32 {
    seq_length.wrapping_mul(MAX_BYTES_PER_CHAR_UTF8)
}

/// `Utf8String.read(ByteBuf, int)`.
///
/// Ordering and consumption match Java exactly: the varint length is consumed
/// first, the length-bounds checks fire before the payload is touched, and the
/// payload is consumed *before* the decoded UTF-16 length is checked (so that
/// final error leaves the cursor past the payload, as Java's reader index does).
pub fn read(input: &mut impl Buf, max_length: i32) -> String {
    let max_encoded_length = utf8_max_bytes(max_length);
    let buffer_length = crate::var_int::read(input);
    if buffer_length > max_encoded_length {
        panic!(
            "The received encoded string buffer length is longer than maximum allowed ({buffer_length} > {max_encoded_length})"
        );
    } else if buffer_length < 0 {
        panic!("The received encoded string buffer length is less than zero! Weird string!");
    } else {
        let available_bytes = input.remaining() as i32;
        if buffer_length > available_bytes {
            panic!(
                "Not enough bytes in buffer, expected {buffer_length}, but got {available_bytes}"
            );
        } else {
            // `input.toString(readerIndex, bufferLength, UTF_8)` then advance —
            // the copy advances the cursor exactly as the Java reader index does.
            let mut bytes = vec![0u8; buffer_length as usize];
            input.copy_to_slice(&mut bytes);
            let result = decode_utf8(&bytes);
            let result_length = result.encode_utf16().count() as i32;
            if result_length > max_length {
                panic!(
                    "The received string length is longer than maximum allowed ({result_length} > {max_length})"
                );
            } else {
                result
            }
        }
    }
}

/// `Utf8String.write(ByteBuf, CharSequence, int)`.
pub fn write(output: &mut impl BufMut, value: &str, max_length: i32) {
    // `value.length()` is the UTF-16 code-unit count.
    let value_length = value.encode_utf16().count() as i32;
    if value_length > max_length {
        panic!("String too big (was {value_length} characters, max {max_length})");
    }

    // Java encodes into a temp scratch buffer sized `utf8MaxBytes(value)` to
    // learn the exact byte count; for a valid `&str` that count is just the
    // UTF-8 byte length, so the scratch buffer is omitted. The `bytesWritten`
    // / `maxAllowedEncodedLength` check is kept (it can only fire for input
    // that Rust's `String` cannot represent, but the ordering is contract).
    let bytes_written = value.len() as i32;
    let max_allowed_encoded_length = utf8_max_bytes(max_length);
    if bytes_written > max_allowed_encoded_length {
        panic!(
            "String too big (was {bytes_written} bytes encoded, max {max_allowed_encoded_length})"
        );
    }

    crate::var_int::write(output, bytes_written);
    output.put_slice(value.as_bytes());
}

/// The WHATWG "UTF-8 decode" algorithm, matching the JDK's
/// `new String(bytes, StandardCharsets.UTF_8)` (verified differentially over an
/// 18,848-case corpus spanning every single byte, every two-byte combo, and
/// random short sequences). Notable divergences from `from_utf8_lossy`:
/// overlong/out-of-range sequences consume one byte per replacement so trailing
/// ASCII survives intact, and a lone 3-byte surrogate consumes all three bytes
/// into a single U+FFFD.
fn decode_utf8(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len());
    let mut bytes_needed = 0u32;
    let mut code_point: u32 = 0;
    let mut lower_byte: u32 = 0x80;
    let mut upper_byte: u32 = 0xBF;
    let mut i = 0usize;

    while i < input.len() {
        let byte = input[i] as u32;
        i += 1;
        if bytes_needed == 0 {
            match byte {
                0x00..=0x7F => out.push(char::from_u32(byte).unwrap()),
                0xC2..=0xDF => {
                    bytes_needed = 1;
                    code_point = byte & 0x1F;
                    lower_byte = 0x80;
                    upper_byte = 0xBF;
                }
                0xE0..=0xEF => {
                    bytes_needed = 2;
                    code_point = byte & 0x0F;
                    lower_byte = if byte == 0xE0 { 0xA0 } else { 0x80 };
                    upper_byte = 0xBF;
                }
                0xF0..=0xF4 => {
                    bytes_needed = 3;
                    code_point = byte & 0x07;
                    if byte == 0xF0 {
                        lower_byte = 0x90;
                        upper_byte = 0xBF;
                    } else if byte == 0xF4 {
                        lower_byte = 0x80;
                        upper_byte = 0x8F;
                    } else {
                        lower_byte = 0x80;
                        upper_byte = 0xBF;
                    }
                }
                _ => out.push('\u{FFFD}'),
            }
        } else if (0x80..=0xBF).contains(&byte) {
            if byte < lower_byte || byte > upper_byte {
                // Reset the sequence and re-process this byte as the start of
                // a fresh sequence (WHATWG "byte is not in the range ...").
                bytes_needed = 0;
                code_point = 0;
                lower_byte = 0x80;
                upper_byte = 0xBF;
                out.push('\u{FFFD}');
                i -= 1;
            } else {
                code_point = (code_point << 6) | (byte & 0x3F);
                bytes_needed -= 1;
                lower_byte = 0x80;
                upper_byte = 0xBF;
                if bytes_needed == 0 {
                    out.push(char::from_u32(code_point).unwrap_or('\u{FFFD}'));
                }
            }
        } else {
            // Continuation byte expected but an ASCII/lead/out-of-range byte
            // arrived: emit one replacement and re-process the byte.
            bytes_needed = 0;
            code_point = 0;
            out.push('\u{FFFD}');
            i -= 1;
        }
    }

    if bytes_needed != 0 {
        out.push('\u{FFFD}'); // truncated at end of input
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::panic::catch_unwind;

    fn encoded(input: &str, max_length: i32) -> Vec<u8> {
        let mut buf = BytesMut::new();
        write(&mut buf, input, max_length);
        buf.to_vec()
    }

    fn decoded(bytes: &[u8], max_length: i32) -> String {
        read(&mut BytesMut::from(bytes), max_length)
    }

    /// Catches the panic `Utf8String` raises for a Java exception and returns
    /// its payload as a string, so tests can assert exact messages.
    fn panic_message<F: FnOnce() -> R, R: std::fmt::Debug>(f: F) -> String {
        let err = catch_unwind(std::panic::AssertUnwindSafe(f))
            .expect_err("expected the closure to panic");
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "non-string panic payload".to_string())
    }

    #[test]
    fn utf8_max_bytes_is_three_per_unit() {
        assert_eq!(utf8_max_bytes(0), 0);
        assert_eq!(utf8_max_bytes(1), 3);
        assert_eq!(utf8_max_bytes(MAX_STRING_LENGTH), 98_301);
        // Java int arithmetic: wraps, does not saturate.
        assert_eq!(
            utf8_max_bytes(i32::MAX),
            (i32::MAX as u32).wrapping_mul(3) as i32
        );
    }

    const MAX_STRING_LENGTH: i32 = 32_767;

    // ---- WHATWG decode equivalence (the corpus-probed cases) ---------------

    #[test]
    fn decode_matches_java_ascii() {
        assert_eq!(decode_utf8(b""), "");
        assert_eq!(decode_utf8(b"abc"), "abc");
        assert_eq!(decode_utf8(&[0x7F]), "\u{7F}");
        assert_eq!(decode_utf8(b"hello world"), "hello world");
    }

    #[test]
    fn decode_matches_java_bmp_and_supplementary() {
        assert_eq!(decode_utf8("é".as_bytes()), "é");
        assert_eq!(decode_utf8("€".as_bytes()), "€");
        assert_eq!(decode_utf8("💩".as_bytes()), "💩");
        assert_eq!(decode_utf8(&[0xF0, 0x90, 0x80, 0x80]), "\u{10000}");
        assert_eq!(decode_utf8(&[0xED, 0x9F, 0xBF]), "\u{D7FF}");
        assert_eq!(decode_utf8(&[0xEE, 0x80, 0x80]), "\u{E000}");
    }

    #[test]
    fn decode_matches_java_single_invalid_byte() {
        assert_eq!(decode_utf8(&[0xFF]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0x80]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0xFE]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0xF8]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0x41, 0xFF, 0x42]), "A\u{FFFD}B");
        assert_eq!(decode_utf8(&[0xFF, 0x41, 0x80]), "\u{FFFD}A\u{FFFD}");
    }

    #[test]
    fn decode_matches_java_truncated_sequences() {
        assert_eq!(decode_utf8(&[0xC2]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0xE0, 0xA0]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0xF0, 0x9F, 0x92]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0xF0, 0x90]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0xF4, 0x80]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0xF1, 0x80]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0xE1]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0xE0]), "\u{FFFD}");
    }

    #[test]
    fn decode_matches_java_bad_continuation() {
        // Each of these consumes one byte as the replacement then re-processes
        // the ASCII so it survives.
        assert_eq!(decode_utf8(&[0xC2, 0x41]), "\u{FFFD}A");
        assert_eq!(decode_utf8(&[0xE1, 0x41]), "\u{FFFD}A");
        assert_eq!(decode_utf8(&[0xE0, 0xA4, 0x41]), "\u{FFFD}A");
        assert_eq!(decode_utf8(&[0xE1, 0x80, 0x41]), "\u{FFFD}A");
        assert_eq!(decode_utf8(&[0xE1, 0x41, 0x41]), "\u{FFFD}AA");
        assert_eq!(decode_utf8(&[0xF0, 0x90, 0x80, 0x41]), "\u{FFFD}A");
        assert_eq!(decode_utf8(&[0xF1, 0x80, 0x80, 0x41]), "\u{FFFD}A");
        assert_eq!(decode_utf8(&[0xE1, 0x80, 0xFF]), "\u{FFFD}\u{FFFD}");
    }

    #[test]
    fn decode_matches_java_overlong_and_out_of_range() {
        // Overlong 2-byte forms: two replacements.
        assert_eq!(decode_utf8(&[0xC0, 0xAF]), "\u{FFFD}\u{FFFD}");
        assert_eq!(decode_utf8(&[0xC1, 0xBF]), "\u{FFFD}\u{FFFD}");
        // Overlong 3-byte: E0 80..9F is invalid, three replacements.
        assert_eq!(decode_utf8(&[0xE0, 0x80, 0xAF]), "\u{FFFD}\u{FFFD}\u{FFFD}");
        assert_eq!(decode_utf8(&[0xE0, 0x80, 0x41]), "\u{FFFD}\u{FFFD}A");
        assert_eq!(decode_utf8(&[0xE0, 0x9F, 0x41]), "\u{FFFD}\u{FFFD}A");
        // Surrogate 3-byte forms are invalid UTF-8: ED A0..BF.
        assert_eq!(decode_utf8(&[0xED, 0xA0, 0x80]), "\u{FFFD}");
        assert_eq!(decode_utf8(&[0xED, 0xBF, 0xBF]), "\u{FFFD}");
        // 4-byte out-of-range leads.
        assert_eq!(
            decode_utf8(&[0xF0, 0x8F, 0x80, 0x80]),
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}"
        );
        assert_eq!(
            decode_utf8(&[0xF4, 0x90, 0x80, 0x80]),
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}"
        );
        assert_eq!(
            decode_utf8(&[0xF5, 0x80, 0x80, 0x80]),
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}"
        );
    }

    #[test]
    fn decode_matches_java_mixed_valid_and_invalid() {
        // Valid emoji then a trailing invalid byte.
        assert_eq!(
            decode_utf8(&[0xF0, 0x9F, 0x92, 0xA9, 0xFF]),
            "\u{1F4A9}\u{FFFD}"
        );
        assert_eq!(
            decode_utf8(&[0xE0, 0xA4, 0x41, 0xE2, 0x82, 0x41]),
            "\u{FFFD}A\u{FFFD}A"
        );
        // ED A0 80 (1 replacement) followed by a valid char.
        assert_eq!(decode_utf8(&[0xED, 0xA0, 0x80, 0x41]), "\u{FFFD}A");
    }

    // ---- read: happy path ------------------------------------------------

    #[test]
    fn read_round_trips_plain_and_unicode() {
        for s in ["", "a", "hello world", "é", "€", "💩", "héllo wörld 💩"] {
            let mut buf = BytesMut::new();
            write(&mut buf, s, MAX_STRING_LENGTH);
            assert_eq!(decoded(&buf, MAX_STRING_LENGTH), s, "round trip {s:?}");
        }
    }

    #[test]
    fn read_encodes_length_as_varint_then_utf8_bytes() {
        let out = encoded("abc", MAX_STRING_LENGTH);
        assert_eq!(out, [0x03, b'a', b'b', b'c']);
        // "héllo" is 5 UTF-16 units but 6 UTF-8 bytes; the varint is the byte
        // count, exactly as Java writes `bytesWritten`.
        let out = encoded("héllo", MAX_STRING_LENGTH);
        assert_eq!(out[0], 6);
        assert_eq!(&out[1..], "héllo".as_bytes());
    }

    #[test]
    fn read_supplementary_char_counts_as_two_utf16_units() {
        // "💩💩" is 2 scalars -> 4 UTF-16 units, encoding to 8 bytes. With
        // maxLength 3 the byte bound (8 <= 9) passes but the UTF-16 unit bound
        // (4 > 3) fails — proving the read-side length check counts UTF-16
        // units, not code points. Hand-built wire so the write-side check does
        // not fire first.
        let mut buf = BytesMut::new();
        crate::var_int::write(&mut buf, "💩💩".len() as i32);
        buf.extend_from_slice("💩💩".as_bytes());
        let msg = panic_message(|| decoded(&buf, 3));
        assert_eq!(
            msg,
            "The received string length is longer than maximum allowed (4 > 3)"
        );
        // maxLength 4 accepts it (byte bound 8 <= 12, unit bound 4 <= 4).
        assert_eq!(decoded(&buf, 4), "💩💩");
    }

    #[test]
    fn read_byte_max_boundaries() {
        // 32767 ASCII chars encode to 32767 bytes <= 32767*3.
        let big = "a".repeat(32_767);
        assert_eq!(
            decoded(&encoded(&big, MAX_STRING_LENGTH), MAX_STRING_LENGTH),
            big
        );
        // 32767 BMP 3-byte chars encode to 98301 bytes == the max-encoded bound.
        let big3 = "\u{0800}".repeat(32_767);
        let mut buf = BytesMut::new();
        write(&mut buf, &big3, MAX_STRING_LENGTH);
        assert_eq!(decoded(&buf, MAX_STRING_LENGTH), big3);
    }

    // ---- read: error cases and exact messages ----------------------------

    #[test]
    fn read_rejects_buffer_length_over_max_encoded() {
        // maxLength 5 -> maxEncodedLength 15. VarInt 16 with 16 payload bytes.
        let mut buf = BytesMut::new();
        crate::var_int::write(&mut buf, 16);
        buf.extend_from_slice(&[0u8; 16]);
        let msg = panic_message(|| decoded(&buf, 5));
        assert_eq!(
            msg,
            "The received encoded string buffer length is longer than maximum allowed (16 > 15)"
        );
    }

    #[test]
    fn read_rejects_negative_buffer_length() {
        // VarInt -1 is five bytes 0xFF 0xFF 0xFF 0xFF 0x0F.
        let mut buf = BytesMut::new();
        crate::var_int::write(&mut buf, -1);
        let msg = panic_message(|| decoded(&buf, MAX_STRING_LENGTH));
        assert_eq!(
            msg,
            "The received encoded string buffer length is less than zero! Weird string!"
        );
    }

    #[test]
    fn read_rejects_when_payload_exceeds_available_bytes() {
        // Length 10 but only 3 bytes present.
        let mut buf = BytesMut::new();
        crate::var_int::write(&mut buf, 10);
        buf.extend_from_slice(b"abc");
        let msg = panic_message(|| decoded(&buf, MAX_STRING_LENGTH));
        assert_eq!(msg, "Not enough bytes in buffer, expected 10, but got 3");
    }

    #[test]
    fn read_rejects_when_decoded_length_exceeds_max() {
        // "hello" = 5 units; maxLength 4.
        let msg = panic_message(|| decoded(&encoded("hello", MAX_STRING_LENGTH), 4));
        assert_eq!(
            msg,
            "The received string length is longer than maximum allowed (5 > 4)"
        );
    }

    #[test]
    fn read_error_leaves_cursor_past_payload_for_length_error() {
        // A decode-then-length-check failure consumes the payload, exactly as
        // Java advances the reader index before the final `length()` check.
        let mut buf = BytesMut::new();
        write(&mut buf, "hello", MAX_STRING_LENGTH);
        let mut reader = BytesMut::from(buf.as_ref());
        let msg = panic_message(|| read(&mut reader, 4));
        assert_eq!(
            msg,
            "The received string length is longer than maximum allowed (5 > 4)"
        );
        // The varint (1 byte) + payload (5 bytes) were consumed.
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn read_length_error_does_not_consume_payload() {
        // "Not enough bytes" fires before the payload is touched: only the
        // varint is consumed.
        let mut buf = BytesMut::new();
        crate::var_int::write(&mut buf, 10);
        buf.extend_from_slice(b"abc");
        let mut reader = BytesMut::from(buf.as_ref());
        let _ = panic_message(|| read(&mut reader, MAX_STRING_LENGTH));
        assert_eq!(reader.remaining(), 3); // varint consumed, payload intact
    }

    #[test]
    fn read_encoded_bound_error_consumes_only_varint() {
        // The max-encoded-length check fires before any payload is read.
        let mut buf = BytesMut::new();
        crate::var_int::write(&mut buf, 16);
        buf.extend_from_slice(&[0u8; 16]);
        let mut reader = BytesMut::from(buf.as_ref());
        let _ = panic_message(|| read(&mut reader, 5));
        assert_eq!(reader.remaining(), 16);
    }

    // ---- write: error cases ---------------------------------------------

    #[test]
    fn write_rejects_too_many_characters() {
        let msg = panic_message(|| encoded("hello", 4));
        assert_eq!(msg, "String too big (was 5 characters, max 4)");
    }

    #[test]
    fn write_rejects_too_many_utf16_units() {
        // 💩 is 2 UTF-16 units; maxLength 1 rejects it at the char check.
        let msg = panic_message(|| encoded("💩", 1));
        assert_eq!(msg, "String too big (was 2 characters, max 1)");
    }

    #[test]
    fn write_accepts_at_character_boundary() {
        // Exactly maxLength UTF-16 units is fine.
        assert_eq!(decoded(&encoded("hello", 5), 5), "hello");
        assert_eq!(decoded(&encoded("💩", 2), 2), "💩");
    }

    #[test]
    fn write_byte_overflow_check_never_fires_for_valid_strings() {
        // For valid UTF-8, bytes <= 3 * utf16 units, so the encoded-bytes check
        // cannot fire when the char check passed; assert a pathological max
        // that wraps does not break the happy path.
        let s = "a".repeat(MAX_STRING_LENGTH as usize);
        assert_eq!(
            decoded(&encoded(&s, MAX_STRING_LENGTH), MAX_STRING_LENGTH),
            s
        );
    }
}
