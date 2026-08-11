//! xxh3_64 digest helpers for the #54 chunk-hash engine.
//!
//! The gate hashes the **raw serialized Level-compound payload** (region
//! framing excluded — the fixtures are the decompressed chunk NBT, so a
//! payload is exactly the bytes Paper wrote for that chunk). The digest is
//! lower-case 16 hex over the raw bytes; there is deliberately no word
//! reinterpretation (no `from_le_bytes`/`to_le_bytes` dance that a future
//! maintainer could get wrong on a different-endian host — xxhash-rust
//! consumes the byte slice directly).
//!
//! `self_check` pins known-answer vectors against the canonical xxHash
//! reference (verified against `xxhash-rust` at wiring time). The gate always
//! runs it, so a wrong xxh variant or an endianness slip fails loudly instead
//! of silently corrupting every digest (false-green threat 3).

use std::fmt::Write;

/// `xxh3_64` of `data`, as lower-case 16 hex digits.
pub fn xxh3_64_hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(16);
    let _ = write!(s, "{:016x}", xxhash_rust::xxh3::xxh3_64(data));
    s
}

/// Pinned known-answer vectors. The empty-input anchor `2d06800538d394c2` is
/// the canonical xxHash reference value; the rest were computed from
/// `xxhash-rust 0.8.18` at wiring time and are re-checked on every gate run.
const KNOWN_ANSWERS: &[(&[u8], &str)] = &[
    (b"", "2d06800538d394c2"),
    (b"a", "e6c632b61e964e1f"),
    (b"abc", "78af5f94892f3950"),
    (b"hello world", "d447b1ea40e6988b"),
    (b"rivet-m2-chunk-hash", "b64135c66e2798cb"),
];

/// Seeded known-answer vectors (the `xxh3_64_with_seed` path is not used by
/// the gate, but pinning it guards against a future variant mix-up).
const SEEDED_ANSWERS: &[(u64, &[u8], &str)] =
    &[(1, b"", "4dc5b0cc826f6703"), (42, b"a", "4c437dd47f0716f4")];

/// Verify the `xxh3_64` implementation against the pinned vectors. `Err` on
/// any mismatch, naming the input and expected/actual digests.
pub fn self_check() -> Result<(), String> {
    for (data, expected) in KNOWN_ANSWERS {
        let actual = xxh3_64_hex(data);
        if actual != *expected {
            return Err(format!(
                "xxh3_64({data:?}) = {actual}, expected {expected} — wrong xxh variant or \
                 endianness; the whole #54 digest table would be wrong"
            ));
        }
    }
    for (seed, data, expected) in SEEDED_ANSWERS {
        let actual = xxhash_rust::xxh3::xxh3_64_with_seed(data, *seed);
        if format!("{actual:016x}") != *expected {
            return Err(format!(
                "xxh3_64_with_seed({data:?}, {seed}) = {actual:016x}, expected {expected}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_answer_vectors_match() {
        self_check().expect("pinned xxh3_64 vectors must match");
    }

    #[test]
    fn output_is_lowercase_16_hex() {
        for data in [&b""[..], b"a", b"rivet-m2-chunk-hash"] {
            let h = xxh3_64_hex(data);
            assert_eq!(h.len(), 16, "digest length for {data:?}");
            assert!(
                h.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "digest {h} is not lower-case hex"
            );
        }
    }

    #[test]
    fn different_inputs_differ() {
        let a = xxh3_64_hex(b"minecraft:full");
        let b = xxh3_64_hex(b"minecraft:structure_starts");
        assert_ne!(a, b);
    }
}
