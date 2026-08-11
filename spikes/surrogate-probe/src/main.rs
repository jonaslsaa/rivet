//! Rust ground-truth counter-probe for the surrogate-string boundary (GitHub
//! #264). Prints JSON Lines on stdout, one line per probe. Each line is valid
//! JSON; the Java `SurrogateProbe` is the independent ground truth, while this
//! half mirrors the crate code (the `whatwg` module is a byte-identical copy of
//! `rivet-protocol::utf8_string::decode_utf8`, not an independent check). The
//! two halves use different key prefixes and value shapes, so they are compared
//! row-by-row by hand rather than diffed wholesale. Run via `./run.sh`.

use rivet_util::data_io::{decode_modified_utf8, write_utf_body};

fn out(key: &str, value: String) {
    println!("{{\"probe\":{},\"value\":{}}}", q(key), q(&value));
}

fn q(s: &str) -> String {
    // Minimal JSON string escaping for probe output.
    serde_json::to_string(s).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    // Lowercase, no separators — the same shape as Java `HexFormat.of()` so the
    // two halves' byte fields are directly comparable.
    let mut s = String::new();
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn main() {
    // 1. decode_modified_utf8 on lone surrogates (Java readUTF preserves them).
    for (key, bytes) in [
        ("rust_mutf8_decode_high", &[0xEDu8, 0xA0, 0x80][..]),
        ("rust_mutf8_decode_low", &[0xED, 0xB0, 0x80][..]),
        (
            "rust_mutf8_decode_pair",
            &[0xED, 0xA0, 0x80, 0xED, 0xB0, 0x80][..],
        ),
    ] {
        match decode_modified_utf8(bytes) {
            Ok(s) => out(
                key,
                format!("ok units={} chars={}", s.encode_utf16().count(), s),
            ),
            Err(e) => out(key, format!("ERR kind={:?} msg={}", e.kind(), e)),
        }
    }

    // 2. write_utf_body round-trip of what Rust CAN hold (U+FFFD etc.).
    for (key, s) in [
        ("rust_mutf8_encode_fffd", "\u{FFFD}"),
        ("rust_mutf8_encode_question", "?"),
        ("rust_mutf8_encode_pair", "\u{1F4A9}"),
    ] {
        let enc = match write_utf_body(s) {
            Ok(b) => b,
            Err(e) => {
                out(
                    "rust_mutf8_encode_err",
                    format!("ERR {}", q(&e.to_string())),
                );
                Vec::new()
            }
        };
        out(
            key,
            format!("units={} bytes={}", s.encode_utf16().count(), hex(&enc)),
        );
    }

    // 3. WHATWG decode (utf8_string::decode_utf8) — must match Java new String.
    for (key, bytes) in [
        ("rust_whatwg_ed_a0_80", &[0xEDu8, 0xA0, 0x80][..]),
        ("rust_whatwg_ed_a0_80_41", &[0xED, 0xA0, 0x80, 0x41][..]),
        ("rust_whatwg_pair", &[0xF0, 0x9F, 0x92, 0xA9][..]),
    ] {
        let s = crate::whatwg::decode(bytes);
        out(
            key,
            format!("units={} chars={}", s.encode_utf16().count(), s),
        );
    }

    // 4. serde_json parse of a lone-surrogate escape (Gson parses to lone
    //    surrogate then re-serializes to "?").
    for (key, json) in [
        ("rust_serde_json_parse_ud800", "\"\\ud800\""),
        ("rust_serde_json_parse_udc00", "\"\\udc00\""),
        ("rust_serde_json_parse_pair", "\"\\ud83d\\udca9\""),
    ] {
        match serde_json::from_str::<serde_json::Value>(json) {
            Ok(v) => {
                let s = v.as_str().unwrap_or("<non-string>");
                out(
                    key,
                    format!(
                        "units={} chars={} reserialized={}",
                        s.encode_utf16().count(),
                        s,
                        v
                    ),
                );
            }
            Err(e) => out(key, format!("ERR {}", e)),
        }
    }
}

// Mirror of `crates/rivet-protocol/src/utf8_string.rs::decode_utf8`, kept
// byte-identical so the probe crate need not depend on rivet-protocol. This is
// NOT an independent implementation — the JDK decoder behind the Java probe's
// `jdk_decode_*` rows is the independent check for the protocol boundary.
mod whatwg {
    pub fn decode(input: &[u8]) -> String {
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
                bytes_needed = 0;
                code_point = 0;
                out.push('\u{FFFD}');
                i -= 1;
            }
        }
        if bytes_needed != 0 {
            out.push('\u{FFFD}');
        }
        out
    }
}
