//! Fuzz target: the SNBT parser (`TagParser`), the reader for untrusted
//! server-side `net.minecraft.nbt` input.
//!
//! The parser takes UTF-8 strings; a raw byte slice is interpreted as UTF-8
//! with lossy replacement so the fuzzer explores the full grammar surface
//! (numbers, quoted/unquoted strings, escapes, maps, lists, typed arrays,
//! builtins) without being blocked on invalid UTF-8. Error paths
//! (`NbtFormatException`) are expected and must not panic.
//!
//! The body lives in `rivet_fuzz::targets::snbt` so the deterministic seed
//! regressions (`cargo test -p rivet-fuzz`) drive the same code.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    rivet_fuzz::targets::snbt(data);
});
