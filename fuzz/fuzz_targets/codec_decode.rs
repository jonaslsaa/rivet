//! Fuzz target: the DFU codec combinators (`rivet-serialization`) decoding
//! untrusted `Tag` values over `NbtOps`.
//!
//! Input bytes are parsed as SNBT (when valid) to obtain a `Tag`, which is then
//! fed through a battery of codec combinators: primitives, list, pair, either,
//! unbounded/simple map, compound list, and a `RecordCodecBuilder` over a
//! mixed-type record. `parse_fully` rejects trailing input, so on failure the
//! target falls back to `parse_as_argument` (which leaves trailing input
//! unconsumed) — a trailing-garbage document still feeds the battery with its
//! leading value. Decoding must never panic — error paths surface as
//! `DataResult::error`, so a panic here is a real bug.
//!
//! The body lives in `rivet_fuzz::targets::codec_decode` so the deterministic
//! seed regressions (`cargo test -p rivet-fuzz`) drive the same code.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    rivet_fuzz::targets::codec_decode(data);
});
