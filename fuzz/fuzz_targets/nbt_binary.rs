//! Fuzz target: the binary NBT readers (`NbtIo.read` / `read_any_tag` /
//! `read_unnamed_tag` / `read_compressed`) over raw untrusted bytes.
//!
//! `read_any_tag` reads the raw tag value; `read` wraps the same path and
//! additionally requires the root to be a compound; `read_compressed` exercises
//! the gzip-decompressor + read pipeline. All entry points are exercised on the
//! same input.
//!
//! The read path panics on inputs that crash Java's parse — see the `nbt_io`
//! module doc: missing list element type, negative list length, oversized array
//! (`check_array_length`), and `NbtAccounter` quota/depth. Those are faithful,
//! expected outcomes on untrusted input, so each call runs under the panic
//! filter in `rivet_fuzz::common` (which swallows faithful panics and aborts on
//! genuine bugs). The accounter is bounded to the server's default 2 MiB quota
//! so a hostile input cannot force a huge allocation before the quota panic
//! fires.
#![no_main]
use libfuzzer_sys::fuzz_target;

mod common;
use common::guarded;
use rivet_fuzz::targets::{NBT_BINARY_STEPS, nbt_binary_step};

fuzz_target!(|data: &[u8]| {
    for step in 0..NBT_BINARY_STEPS {
        guarded(|| nbt_binary_step(data, step));
    }
});
