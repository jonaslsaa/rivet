//! Fuzz target: the modified-UTF-8 wire codec (`rivet-util::data_io`).
//!
//! This is the string codec underneath both `NbtIo` (binary NBT keys and
//! strings) and the protocol `FriendlyByteBuf` — a hostile input exercises its
//! `decode_modified_utf8` body directly, including the error paths Java raises
//! as `UTFDataFormatException` (malformed continuation bytes, truncated lead
//! bytes, unpaired surrogates) and the encoder's overlong-form writing.
//!
//! The asserted property is Java's canonicalization idempotence:
//! `decode(encode(decode(x))) == decode(x)`. The encoder can *legitimately*
//! fail when a decoded string's canonical re-encoding exceeds 65535 bytes (a
//! long raw-NUL run re-encodes 2x) — a faithful `UTFDataFormatException`. No
//! panic here is faithful — the decoder and encoder surface every
//! malformed-input condition as `Err` — so any panic aborts the fuzzer.
//!
//! The body lives in `rivet_fuzz::targets::data_io_modified_utf8` so the
//! deterministic seed regressions (`cargo test -p rivet-fuzz`) drive the same
//! code.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    rivet_fuzz::targets::data_io_modified_utf8(data);
});
