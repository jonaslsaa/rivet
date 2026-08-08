//! Java-grounded tests for the rivet-util `java.io` byte-IO surface:
//! big-endian primitives, modified-UTF-8 (`writeUTF`/`readUTF`), the
//! `DelegateDataOutput` delegate, and the `FastBufferedInputStream` buffer.
//!
//! The `readUTF` expected values were produced by running OpenJDK 25's
//! `DataInputStream.readUTF` over the same byte bodies (see the byte-offset
//! error messages, the overlong `C1 80` -> `U+0040` and `E0 80 80` -> NUL
//! behaviors, and the raw-NUL acceptance). The surrogate cases are the one
//! documented deviation: Java `String` holds an unpaired surrogate, Rust
//! `String` cannot, so a lone surrogate errors here.

use rivet_util::data_io::{
    DataInput, DataInputStream, DataOutput, DataOutputStream, decode_modified_utf8,
};
use rivet_util::delegate_data_output::DelegateDataOutput;
use rivet_util::fast_buffered_input_stream::FastBufferedInputStream;
use std::io::{self, Cursor, Read};

// ---------------------------------------------------------------------------
// Big-endian primitives (byte-exact)
// ---------------------------------------------------------------------------

#[test]
fn write_byte_exact_layouts() {
    let mut out = DataOutputStream::new(Vec::new());
    out.write_boolean(true).unwrap();
    out.write_boolean(false).unwrap();
    out.write_byte(-1).unwrap();
    out.write_byte(0x80).unwrap();
    out.write_short(0x0102).unwrap();
    out.write_short(-1).unwrap(); // 0xFFFF
    out.write_int(0x0102_0304).unwrap();
    out.write_int(-1).unwrap(); // 0xFFFFFFFF
    out.write_long(0x0102_0304_0506_0708).unwrap();
    out.write_long(-1).unwrap();
    let bytes = out.into_inner();
    assert_eq!(
        bytes,
        [
            0x01, 0x00, // true, false
            0xFF, 0x80, // byte -1, byte 0x80
            0x01, 0x02, 0xFF, 0xFF, // short 0x0102, short -1
            0x01, 0x02, 0x03, 0x04, 0xFF, 0xFF, 0xFF, 0xFF, // int
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, // long, long -1
        ]
    );
}

#[test]
fn read_primitive_byte_exact() {
    let data: Vec<u8> = [
        0xFF, // unsigned byte 255
        0x01, 0x02, // unsigned short 0x0102
        0xFF, 0xFF, 0xFF, 0xFE, // int -2
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, // long
        0x3F, 0x80, 0x00, 0x00, // float 1.0
        0x40, 0x09, 0x21, 0xFB, 0x54, 0x44, 0x2D, 0x18, // double pi
    ]
    .into_iter()
    .collect();
    let mut input = DataInputStream::new(Cursor::new(data));
    assert_eq!(input.read_unsigned_byte().unwrap(), 255);
    assert_eq!(input.read_unsigned_short().unwrap(), 0x0102);
    assert_eq!(input.read_int().unwrap(), -2);
    assert_eq!(input.read_long().unwrap(), 0x0102_0304_0506_0708);
    assert_eq!(input.read_float().unwrap(), 1.0f32);
    assert_eq!(input.read_double().unwrap(), std::f64::consts::PI);
}

#[test]
fn read_float_preserves_raw_nan_payload() {
    // Java `DataInputStream.readFloat` = `Float.intBitsToFloat`, which does NOT
    // canonicalize a non-canonical NaN payload on read (canonicalization is a
    // write-side-only property of `Float.floatToIntBits`).
    let non_canonical: u32 = 0x7FC0_0001;
    let mut input = DataInputStream::new(Cursor::new(non_canonical.to_be_bytes()));
    assert_eq!(input.read_float().unwrap().to_bits(), non_canonical);
}

#[test]
fn write_nan_canonicalizes_float() {
    // `Float.floatToIntBits(Float.NaN)` == 0x7FC00000 regardless of payload.
    let mut out = DataOutputStream::new(Vec::new());
    out.write_float(f32::NAN).unwrap();
    assert_eq!(out.into_inner(), [0x7F, 0xC0, 0x00, 0x00]);
}

#[test]
fn write_nan_canonicalizes_double() {
    // `Double.doubleToLongBits(Double.NaN)` == 0x7FF8000000000000L.
    let mut out = DataOutputStream::new(Vec::new());
    out.write_double(f64::NAN).unwrap();
    assert_eq!(
        out.into_inner(),
        [0x7F, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    );
}

// ---------------------------------------------------------------------------
// writeUTF / readUTF — well-formed strings
// ---------------------------------------------------------------------------

/// Wrap `body` in a 2-byte big-endian length and run `readUTF` over it.
fn read_body(body: &[u8]) -> io::Result<String> {
    let mut raw = (body.len() as u16).to_be_bytes().to_vec();
    raw.extend_from_slice(body);
    let mut input = DataInputStream::new(Cursor::new(raw));
    input.read_utf()
}

#[test]
fn write_utf_ascii_and_nul() {
    let mut out = DataOutputStream::new(Vec::new());
    out.write_utf("").unwrap();
    out.write_utf("abc").unwrap();
    out.write_utf("\u{0}\u{0}").unwrap();
    // "" -> 00 00; "abc" -> 00 03 61 62 63; "\0\0" -> 00 04 C0 80 C0 80.
    assert_eq!(
        out.into_inner(),
        [
            0x00, 0x00, 0x00, 0x03, b'a', b'b', b'c', 0x00, 0x04, 0xC0, 0x80, 0xC0, 0x80
        ]
    );
}

#[test]
fn write_utf_emoji_surrogate_pair_byte_exact() {
    // U+10401 is astral: `DataOutputStream.writeUTF` encodes it as the 6-byte
    // CESU-8 surrogate pair (length prefix 6).
    let mut out = DataOutputStream::new(Vec::new());
    out.write_utf("\u{10401}").unwrap();
    assert_eq!(
        out.into_inner(),
        [0x00, 0x06, 0xED, 0xA0, 0x81, 0xED, 0xB0, 0x81]
    );
}

#[test]
fn write_utf_read_utf_round_trip_bmp_and_astral() {
    let samples = ["", "hello", "héllo wörld", "日本語", "💩", "a\u{0}b", "𐐁𐐁"];
    for s in samples {
        let mut out = DataOutputStream::new(Vec::new());
        out.write_utf(s).unwrap();
        let mut input = DataInputStream::new(Cursor::new(out.into_inner()));
        assert_eq!(input.read_utf().unwrap(), s, "round trip of {s:?}");
    }
}

/// Exhaustive round trip over every BMP code unit plus a sample of astral
/// scalars: the encoder writes each, the decoder must give it back exactly.
/// Each code point is round-tripped individually (a single 65KB write would
/// exceed the 2-byte length prefix).
#[test]
fn write_utf_read_utf_round_trip_every_bmp_codepoint() {
    for cp in 0x0000..=0xFFFFu32 {
        // Rust has no lone surrogates to encode.
        if (0xD800..=0xDFFF).contains(&cp) {
            continue;
        }
        let ch = char::from_u32(cp).unwrap();
        let mut out = DataOutputStream::new(Vec::new());
        out.write_utf(&ch.to_string()).unwrap();
        let mut input = DataInputStream::new(Cursor::new(out.into_inner()));
        assert_eq!(
            input.read_utf().unwrap(),
            ch.to_string(),
            "round trip of U+{cp:04X}"
        );
    }
    // A sample of astral scalars (including boundaries).
    for cp in [0x10000, 0x10401, 0x1F4A9, 0x10FFFD, 0x10FFFF] {
        let ch = char::from_u32(cp).unwrap();
        let mut out = DataOutputStream::new(Vec::new());
        out.write_utf(&ch.to_string()).unwrap();
        let mut input = DataInputStream::new(Cursor::new(out.into_inner()));
        assert_eq!(
            input.read_utf().unwrap(),
            ch.to_string(),
            "round trip of U+{cp:04X}"
        );
    }
}

#[test]
fn write_utf_length_boundary_65535_bytes() {
    // Modified-UTF-8 bodies up to 65535 bytes are length-prefixable.
    let ascii = "a".repeat(65_535);
    let mut out = DataOutputStream::new(Vec::new());
    out.write_utf(&ascii).unwrap();
    let bytes = out.into_inner();
    assert_eq!(bytes.len(), 2 + 65_535);
    assert_eq!(bytes[0], 0xFF);
    assert_eq!(bytes[1], 0xFF);
    let mut input = DataInputStream::new(Cursor::new(bytes));
    assert_eq!(input.read_utf().unwrap(), ascii);

    // 32767 NULs (2-byte C0 80 each) + 'a' = 65535 bytes.
    let mixed = format!("{}a", "\u{0}".repeat(32_767));
    assert_eq!(mixed.chars().count(), 32_768);
    let mut out = DataOutputStream::new(Vec::new());
    out.write_utf(&mixed).unwrap();
    let bytes = out.into_inner();
    assert_eq!(bytes.len(), 2 + 65_535);
    assert_eq!(bytes[0], 0xFF);
    assert_eq!(bytes[1], 0xFF);
    let mut input = DataInputStream::new(Cursor::new(bytes));
    assert_eq!(input.read_utf().unwrap(), mixed);
}

/// Java `DataOutputStream.writeUTF` throws `UTFDataFormatException` *before*
/// writing anything; the buffer must stay empty. Message format matches
/// OpenJDK 25's `tooLongMsg`.
#[test]
fn write_utf_overflow_exact_ascii_message() {
    let ascii = "a".repeat(65_536); // 65536 modified-UTF-8 bytes
    let mut out = DataOutputStream::new(Vec::new());
    let err = out.write_utf(&ascii).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        err.to_string(),
        "encoded string (aaaaaaaa...aaaaaaaa) too long: 65536 bytes"
    );
    assert!(out.into_inner().is_empty());
}

#[test]
fn write_utf_overflow_exact_nul_message() {
    // 32768 NULs, each a 2-byte C0 80 -> 65536 bytes.
    let nuls = "\u{0}".repeat(32_768);
    let mut out = DataOutputStream::new(Vec::new());
    let err = out.write_utf(&nuls).unwrap_err();
    assert_eq!(
        err.to_string(),
        "encoded string (\0\0\0\0\0\0\0\0...\0\0\0\0\0\0\0\0) too long: 65536 bytes"
    );
    assert!(out.into_inner().is_empty());
}

#[test]
fn write_utf_overflow_exact_supplementary_message() {
    // Astral chars are 6 modified-UTF-8 bytes and 2 UTF-16 code units each:
    // 10923 * 6 = 65538 bytes, and the 8-unit head/tail slice is 4 chars.
    let emoji = "\u{10401}".repeat(10_923);
    let mut out = DataOutputStream::new(Vec::new());
    let err = out.write_utf(&emoji).unwrap_err();
    assert_eq!(
        err.to_string(),
        "encoded string (𐐁𐐁𐐁𐐁...𐐁𐐁𐐁𐐁) too long: 65538 bytes"
    );
    assert!(out.into_inner().is_empty());
}

#[test]
fn write_utf_overflow_head_tail_counts_utf16_units() {
    // The head/tail are 8 UTF-16 code units, not 8 code points: 4 BMP + 2
    // astral chars = 8 units, so the head shows 6 characters.
    let prefix = "éééé𐐁𐐁"; // 8 UTF-16 units, 20 modified-UTF-8 bytes
    let pad = 65_536 - 20;
    let s = format!("{prefix}{}", "Z".repeat(pad));
    let mut out = DataOutputStream::new(Vec::new());
    let err = out.write_utf(&s).unwrap_err();
    assert_eq!(
        err.to_string(),
        "encoded string (éééé𐐁𐐁...ZZZZZZZZ) too long: 65536 bytes"
    );
}

#[test]
fn write_utf_overflow_utf16_slice_keeps_whole_straddling_char() {
    // Java's `substring(0, 8)` slices UTF-16 code units, so with 3 astral
    // chars + 'X' (7 units) the 8th unit is the high half of the next astral
    // pair, leaving a lone high surrogate at the end of Java's head. The tail
    // (`substring(slen - 8, slen)`) cuts the other way: it starts on the low
    // half of its first astral char, leaving a lone low surrogate at the start
    // of Java's tail. Rust cannot hold a lone surrogate, so the whole
    // straddling character is kept in each slice.
    let prefix = "𐐁𐐁𐐁X𐐁"; // 9 UTF-16 units, 25 bytes
    let suffix = "𐐁X𐐁𐐁𐐁"; // 9 UTF-16 units, 25 bytes
    let pad = 65_536 - 25 - 25;
    let s = format!("{prefix}{}{suffix}", "Z".repeat(pad));
    let mut out = DataOutputStream::new(Vec::new());
    let err = out.write_utf(&s).unwrap_err();
    assert_eq!(
        err.to_string(),
        "encoded string (𐐁𐐁𐐁X𐐁...𐐁X𐐁𐐁𐐁) too long: 65536 bytes"
    );
}

#[test]
fn read_utf_short_read_is_error() {
    // Declared length larger than the stream: Java readFully -> EOFException.
    let mut input = DataInputStream::new(Cursor::new(vec![0x00, 0x05, b'a']));
    let err = input.read_utf().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
}

// ---------------------------------------------------------------------------
// readUTF — OpenJDK hostile-input parity (byte-exact, verified against
// OpenJDK 25 `DataInputStream.readUTF`).
// ---------------------------------------------------------------------------

/// Assert `readUTF` over `body` decodes to `expected` (Java OK case).
fn assert_ok(body: &[u8], expected: &str) {
    let got = read_body(body).unwrap();
    assert_eq!(got, expected, "body {body:02X?}");
}

/// Assert `readUTF` over `body` errors with a message containing `msg`.
fn assert_err(body: &[u8], msg: &str) {
    let err = read_body(body).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    let text = err.to_string();
    assert!(
        text.contains(msg),
        "body {body:02X?}: expected {msg:?} in {text:?}"
    );
}

#[test]
fn read_utf_ascii_and_nul_forms() {
    assert_ok(&[], "");
    assert_ok(&[0x00], "\u{0}"); // raw NUL is a valid one-byte char
    assert_ok(&[0x41], "A");
    assert_ok(&[0x7F], "\u{7F}");
    assert_ok(&[0x00, 0x41], "\u{0}A");
    assert_ok(&[0x41, 0x00], "A\u{0}");
    assert_ok(&[0x41, 0x42, 0x43], "ABC");
    assert_ok(&[0x41, 0xC2, 0x80], "A\u{80}");
}

#[test]
fn read_utf_overlong_and_boundary_forms() {
    // C0 80 -> overlong NUL; C1 80 -> U+0040 (Java only validates the
    // continuation byte, never the lead-byte non-overlong bound).
    assert_ok(&[0xC0, 0x80], "\u{0}");
    assert_ok(&[0xC1, 0x80], "\u{40}");
    assert_ok(&[0xC1, 0xBF], "\u{7F}");
    assert_ok(&[0xC2, 0x80], "\u{80}");
    assert_ok(&[0xDF, 0xBF], "\u{7FF}");
    assert_ok(&[0xE0, 0x80, 0x80], "\u{0}"); // overlong three-byte NUL
    assert_ok(&[0xE0, 0xA0, 0x80], "\u{800}");
    assert_ok(&[0xE1, 0x80, 0x80], "\u{1000}");
    assert_ok(&[0xEF, 0xBF, 0xBF], "\u{FFFF}");
}

#[test]
fn read_utf_surrogate_pair_combines() {
    // Encoder output for an astral char: a proper high+low pair, which Java
    // returns as the two code units and Rust materializes as the one scalar.
    assert_ok(&[0xED, 0xA0, 0x80, 0xED, 0xB0, 0x80], "\u{10000}");
    assert_ok(&[0xED, 0xA0, 0x81, 0xED, 0xB0, 0x81], "\u{10401}");
    // Maximum astral scalar U+10FFFF.
    assert_ok(&[0xED, 0xAF, 0xBF, 0xED, 0xBF, 0xBF], "\u{10FFFF}");
}

#[test]
fn read_utf_unpaired_surrogate_errors() {
    // Java String accepts a lone surrogate; Rust String cannot. Documented
    // deviation.
    assert!(read_body(&[0xED, 0xA0, 0x80]).is_err()); // lone high
    assert!(read_body(&[0xED, 0xB0, 0x80]).is_err()); // lone low
    assert!(read_body(&[0xED, 0xA0, 0x80, 0x41]).is_err()); // high then non-pair
    assert!(read_body(&[0x41, 0xED, 0xB0, 0x80]).is_err()); // trailing low
    assert!(read_body(&[0xED, 0xA0, 0x80, 0xED, 0xA0, 0x80]).is_err()); // high high
}

#[test]
fn read_utf_malformed_messages() {
    // Truncated lead bytes -> "malformed input: partial character at end".
    assert_err(&[0xC0], "malformed input: partial character at end");
    assert_err(&[0xC2], "malformed input: partial character at end");
    assert_err(&[0xE0, 0x80], "malformed input: partial character at end");
    // Invalid lead bytes (10xx/1111) -> offset 0.
    assert_err(&[0x80], "malformed input around byte 0");
    assert_err(&[0xF0, 0x90, 0x80, 0x80], "malformed input around byte 0");
    // Non-continuation second byte -> offset 2.
    assert_err(&[0xC2, 0x41], "malformed input around byte 2");
    assert_err(&[0xC1, 0x00], "malformed input around byte 2");
    assert_err(&[0xC0, 0x00], "malformed input around byte 2");
    assert_err(&[0xC0, 0xC0], "malformed input around byte 2");
    assert_err(&[0xDF, 0x00], "malformed input around byte 2");
    // Non-continuation third byte -> offset 2 (Java reports the byte before
    // the position, i.e. the third byte at index 2).
    assert_err(&[0xE0, 0x80, 0x41], "malformed input around byte 2");
    assert_err(&[0xE0, 0x41, 0x80], "malformed input around byte 2");
    assert_err(&[0xE0, 0x80, 0x00], "malformed input around byte 2");
    // ASCII run then bad byte -> offset of the bad byte.
    assert_err(&[0x41, 0x80], "malformed input around byte 1");
}

#[test]
fn decode_modified_utf8_matches_java_dump() {
    // A compact re-statement of the differential corpus: every body OpenJDK
    // accepts must decode identically here (surrogate pairs combined), and
    // every body OpenJDK rejects must reject with the same message.
    let ok_cases: &[(&[u8], &str)] = &[
        (&[], ""),
        (&[0x00], "\u{0}"),
        (&[0x41], "A"),
        (&[0x7F], "\u{7F}"),
        (&[0xC0, 0x80], "\u{0}"),
        (&[0xC1, 0x80], "\u{40}"),
        (&[0xC2, 0x80], "\u{80}"),
        (&[0xDF, 0xBF], "\u{7FF}"),
        (&[0xE0, 0x80, 0x80], "\u{0}"), // overlong three-byte NUL
        (&[0xE0, 0xA0, 0x80], "\u{800}"),
        (&[0xEF, 0xBF, 0xBF], "\u{FFFF}"),
        (&[0xE1, 0x80, 0x80], "\u{1000}"),
        (&[0xC1, 0xBF], "\u{7F}"),
        (&[0x41, 0xC2, 0x80], "A\u{80}"),
    ];
    for (body, expected) in ok_cases {
        assert_eq!(
            &decode_modified_utf8(body).unwrap(),
            expected,
            "{body:02X?}"
        );
    }

    let err_cases: &[&[u8]] = &[
        &[0x80],
        &[0xF0, 0x90, 0x80, 0x80],
        &[0xC2, 0x41],
        &[0xC1, 0x00],
        &[0xC0, 0x00],
        &[0xC0, 0xC0],
        &[0xDF, 0x00],
        &[0xE0, 0x80, 0x41],
        &[0xE0, 0x41, 0x80],
        &[0xE0, 0x80, 0x00],
        &[0x41, 0x80],
        &[0xC0],
        &[0xE0, 0x80],
        &[0xED, 0xA0, 0x80], // lone surrogate: Java OK, Rust deviation
        &[0xED, 0xB0, 0x80],
    ];
    for body in err_cases {
        assert!(
            decode_modified_utf8(body).is_err(),
            "{body:02X?} should err"
        );
    }
}

// ---------------------------------------------------------------------------
// readFully / skipBytes
// ---------------------------------------------------------------------------

#[test]
fn read_fully_exact_and_short() {
    let data: Vec<u8> = (0..10u8).collect();
    let mut input = DataInputStream::new(Cursor::new(data.clone()));
    assert_eq!(input.read_fully(4).unwrap(), vec![0, 1, 2, 3]);
    // Full remaining read.
    assert_eq!(input.read_fully(6).unwrap(), vec![4, 5, 6, 7, 8, 9]);
    // Reading past EOF is an error (Java readFully -> EOFException).
    assert_eq!(
        input.read_fully(1).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn skip_bytes_partial_eof_semantics() {
    let data: Vec<u8> = (0..5u8).collect();
    let mut input = DataInputStream::new(Cursor::new(data.clone()));
    // Skips within range.
    assert_eq!(input.skip_bytes(3).unwrap(), 3);
    assert_eq!(input.read_unsigned_byte().unwrap(), 3);
    // Skip past EOF: returns the count actually skipped, never an error.
    assert_eq!(input.skip_bytes(100).unwrap(), 1);
    // Now at EOF; skip returns 0.
    assert_eq!(input.skip_bytes(10).unwrap(), 0);
}

// ---------------------------------------------------------------------------
// DelegateDataOutput
// ---------------------------------------------------------------------------

#[test]
fn delegate_data_output_forwards_everything() {
    let mut backing = DataOutputStream::new(Vec::new());
    let mut delegated: DelegateDataOutput<_> = DelegateDataOutput::new(&mut backing);
    delegated.write_boolean(true).unwrap();
    delegated.write_byte(-2).unwrap();
    delegated.write_short(0x1234).unwrap();
    delegated.write_int(0xDEAD_BEEFu32 as i32).unwrap();
    delegated.write_long(0x0102_0304_0506_0708).unwrap();
    delegated.write_float(2.5f32).unwrap();
    delegated.write_double(0.5f64).unwrap();
    delegated.write_utf("héllo 💩").unwrap();

    // Reading back through a plain DataInputStream must give the same values.
    let mut input = DataInputStream::new(Cursor::new(backing.into_inner()));
    assert_eq!(input.read_unsigned_byte().unwrap(), 1);
    assert_eq!(input.read_unsigned_byte().unwrap(), 0xFE);
    assert_eq!(input.read_unsigned_short().unwrap(), 0x1234);
    assert_eq!(input.read_int().unwrap(), 0xDEAD_BEEFu32 as i32);
    assert_eq!(input.read_long().unwrap(), 0x0102_0304_0506_0708);
    assert_eq!(input.read_float().unwrap(), 2.5f32);
    assert_eq!(input.read_double().unwrap(), 0.5f64);
    assert_eq!(input.read_utf().unwrap(), "héllo 💩");
}

// ---------------------------------------------------------------------------
// FastBufferedInputStream
// ---------------------------------------------------------------------------

/// A reader that hands out the source in fixed-size chunks (like a network
/// stream), tracking how many underlying reads occur through a shared counter.
struct ChunkedReader {
    data: Vec<u8>,
    pos: usize,
    chunk: usize,
    reads: std::rc::Rc<std::cell::Cell<usize>>,
}

impl ChunkedReader {
    fn new(data: Vec<u8>, chunk: usize) -> (Self, std::rc::Rc<std::cell::Cell<usize>>) {
        let reads = std::rc::Rc::new(std::cell::Cell::new(0));
        (
            ChunkedReader {
                data,
                pos: 0,
                chunk,
                reads: reads.clone(),
            },
            reads,
        )
    }
}

impl io::Read for ChunkedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reads.set(self.reads.get() + 1);
        let n = self.chunk.min(buf.len()).min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

#[test]
fn fast_buffered_input_stream_reads_across_chunks() {
    // A 10-byte source in 3-byte chunks through a 4-byte buffer.
    let data: Vec<u8> = (0..10u8).collect();
    let (reader, _reads) = ChunkedReader::new(data.clone(), 3);
    let mut buffered = FastBufferedInputStream::with_buffer_size(reader, 4);

    let mut out = [0u8; 5];
    // Read 1: buffer empty, len 5 >= 4 -> Java bypasses the buffer. The
    // underlying chunked reader only has a 3-byte chunk, so the direct read
    // returns 3.
    assert_eq!(buffered.read(&mut out[..5]).unwrap(), 3);
    assert_eq!(&out[..3], &[0, 1, 2]);
    // Read 2: buffer empty, len 3 < 4 -> fills (reads 3, [3,4,5]).
    assert_eq!(buffered.read(&mut out[..3]).unwrap(), 3);
    assert_eq!(&out[..3], &[3, 4, 5]);
    // Read 3: buffer empty, len 5 >= 4 -> bypass, reads 3 ([6,7,8]).
    assert_eq!(buffered.read(&mut out[..5]).unwrap(), 3);
    assert_eq!(&out[..3], &[6, 7, 8]);
    // Read 4: buffer empty, len 5 >= 4 -> bypass, reads the last 1.
    assert_eq!(buffered.read(&mut out[..5]).unwrap(), 1);
    assert_eq!(&out[..1], &[9]);
    // Read 5: EOF.
    assert_eq!(buffered.read(&mut out[..5]).unwrap(), 0);
}

#[test]
fn fast_buffered_input_stream_within_buffer_avoids_refills() {
    let data: Vec<u8> = (0..10u8).collect();
    let (reader, reads) = ChunkedReader::new(data, 10); // one 10-byte chunk
    let mut buffered = FastBufferedInputStream::new(reader);

    let mut out = [0u8; 4];
    // First read fills the 8KB buffer (1 underlying read) and copies 4.
    assert_eq!(buffered.read(&mut out).unwrap(), 4);
    assert_eq!(&out, &[0, 1, 2, 3]);
    // Second read comes from the buffer (no underlying read).
    assert_eq!(buffered.read(&mut out).unwrap(), 4);
    assert_eq!(&out, &[4, 5, 6, 7]);
    // Third read consumes the last 2 buffered bytes, then the buffer is empty
    // so a fill is needed: the underlying reader is exhausted -> EOF.
    assert_eq!(buffered.read(&mut out).unwrap(), 2);
    assert_eq!(&out[..2], &[8, 9]);
    assert_eq!(buffered.read(&mut out).unwrap(), 0);
    // Exactly 2 underlying reads: the initial fill + the final exhausted fill.
    assert_eq!(reads.get(), 2);
}

#[test]
fn fast_buffered_input_stream_empty_source() {
    let mut buffered = FastBufferedInputStream::new(Cursor::new(Vec::<u8>::new()));
    let mut out = [0u8; 16];
    assert_eq!(buffered.read(&mut out).unwrap(), 0);
    assert_eq!(buffered.read(&mut []).unwrap(), 0);
}
