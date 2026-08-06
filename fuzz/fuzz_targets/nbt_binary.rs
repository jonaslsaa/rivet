//! Fuzz target: the binary NBT readers (`NbtIo.read` / `read_any_tag` /
//! `read_unnamed_tag`) over raw untrusted bytes.
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
//! filter in `common` (which swallows faithful panics and aborts on genuine
//! bugs). The accounter is bounded to the server's default 2 MiB quota so a
//! hostile input cannot force a huge allocation before the quota panic fires.
#![no_main]
use libfuzzer_sys::fuzz_target;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io::{read, read_any_tag, read_compressed, read_unnamed_tag};
use rivet_util::data_io::DataInputStream;
use std::io::Cursor;

mod common;
use common::guarded;

fuzz_target!(|data: &[u8]| {
    // Binary NBT, no gzip — `read_any_tag` (raw tag value).
    guarded(|| {
        let mut dis = DataInputStream::new(Cursor::new(data));
        let mut acc = NbtAccounter::default_quota();
        let _ = read_any_tag(&mut dis, &mut acc);
    });
    // `read_unnamed_tag` (named root path).
    guarded(|| {
        let mut dis = DataInputStream::new(Cursor::new(data));
        let mut acc = NbtAccounter::default_quota();
        let _ = read_unnamed_tag(&mut dis, &mut acc);
    });
    // Root-must-be-compound entry point.
    guarded(|| {
        let mut dis = DataInputStream::new(Cursor::new(data));
        let mut acc = NbtAccounter::default_quota();
        let _ = read(&mut dis, &mut acc);
    });
    // Compressed (gzip) input — the server-disk format.
    guarded(|| {
        let mut acc = NbtAccounter::default_quota();
        let _ = read_compressed(Cursor::new(data), &mut acc);
    });
});
