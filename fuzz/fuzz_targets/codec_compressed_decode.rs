//! Fuzz target: the DFU compressed-map decode path (`compressMaps() == true`).
//!
//! The `codec_decode` target exercises the codec combinators over `NbtOps`,
//! whose `compress_maps()` defaults to `false` — so it never reaches the
//! packed-list decode through a `KeyCompressor`-backed `CompressedMapLike`.
//! This target feeds hostile `serde_json::Value`s through the same codec
//! battery over `JsonOps::COMPRESSED` (the compressed path) and
//! `JsonOps::INSTANCE` (the object path), covering `compressedDecode`,
//! `compressed_map_like`, `KeyCompressor` construction/string fallback, null
//! slots, unknown-key slot-0 reads, and list-length bounds.
//!
//! An out-of-range compressed-map index is a *faithful* Java crash
//! (`IndexOutOfBoundsException` from the packed-list `MapLike`), so the
//! compressed path runs under `common`'s panic filter (see
//! `FAITHFUL_PANIC_FRAGMENTS`); every other panic is a genuine bug and aborts
//! the fuzzer. The input length is capped so a pathological `-max_len` cannot
//! force a large allocation before any codec limit applies.
#![no_main]
use libfuzzer_sys::fuzz_target;

mod common;
use common::guarded;
use rivet_fuzz::targets::{CODEC_COMPRESSED_STEPS, codec_compressed_decode_step};

fuzz_target!(|data: &[u8]| {
    for step in 0..CODEC_COMPRESSED_STEPS {
        guarded(|| codec_compressed_decode_step(data, step));
    }
});
