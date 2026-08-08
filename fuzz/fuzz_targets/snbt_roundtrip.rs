//! Fuzz target: SNBT parse → print → re-parse round-trip.
//!
//! `StringTagVisitor` (the SNBT printer) is not a parser but it is the other
//! half of the untrusted-data surface: its output is fed back into the parser
//! in real servers, so a printer bug that emits invalid SNBT (or a parse bug
//! that rejects its own output) is a real failure mode. This target asserts
//! the round-trip identity `parse(print(tag)) == tag` for every successfully
//! parsed input. The second parse must always succeed.
//!
//! The body lives in `rivet_fuzz::targets::snbt_roundtrip` so the
//! deterministic seed regressions (`cargo test -p rivet-fuzz`) drive the same
//! code.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    rivet_fuzz::targets::snbt_roundtrip(data);
});
