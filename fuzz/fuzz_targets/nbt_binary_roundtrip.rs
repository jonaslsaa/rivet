//! Fuzz target: binary NBT write-path canonicalization idempotence.
//!
//! The `nbt_binary` target covers the decode side; this target adds the write
//! side (`NbtIo.writeUnnamedTag` through NbtIo's `StringFallbackDataOutput`),
//! asserting the canonicalization property Java gives the format: writing a
//! parsed tag, re-reading it, and writing again must produce byte-identical
//! output on the second write.
//!
//! Java justifies this as an invariant of `NbtIo.writeUnnamedTag`: the only
//! non-canonical values a parse can produce are non-canonical NaN payloads
//! (re-canonicalized by `writeFloat`/`writeDouble`) and overlong
//! modified-UTF-8 encodings (decoded and re-encoded in canonical form). A
//! hostile input that violates this is a writer or encoder bug, and fails the
//! assertion. `StringFallbackDataOutput` absorbs strings whose canonical
//! re-encoding exceeds 65535 bytes (writing `""`, exactly Java's catch of
//! `UTFDataFormatException`), so the write path cannot fail on a parsed tag.
//!
//! Faithful parse panics (negative list length, missing list element type,
//! oversized array, accounter quota/depth) are swallowed by `common`'s panic
//! filter; anything else aborts the fuzzer. The accounter is bounded to the
//! server's default 2 MiB quota so a hostile input cannot force a huge
//! allocation.
#![no_main]
use libfuzzer_sys::fuzz_target;

mod common;
use common::guarded;

fuzz_target!(|data: &[u8]| {
    guarded(|| rivet_fuzz::targets::nbt_binary_roundtrip(data));
});
