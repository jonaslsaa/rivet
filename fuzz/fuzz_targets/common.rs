//! Shared helpers for the rivet fuzz targets.
//!
//! libfuzzer-sys aborts the process on *every* panic (even one caught by
//! `catch_unwind`), so the binary targets could never tolerate the faithful
//! panics that a ported `NbtIo` raises on hostile input. This module swaps the
//! panic hook for one that aborts only on panics outside the known-faithful
//! set — a genuine bug still aborts and writes an artifact.

use std::panic::{self, AssertUnwindSafe, PanicHookInfo};

/// Panic-message substrings of the faithful Java crash paths in rivet-nbt's
/// read path. Keep in sync with the `panic!` sites in `crates/rivet-nbt`:
/// - `nbt_io.rs`: "Missing type on ListTag", "ListTag length cannot be
///   negative", "Array tag length must be < 1 << 24" (`check_array_length`)
/// - `nbt_accounter.rs`: the `NbtAccounterException` quota/depth panics
const FAITHFUL_PANIC_FRAGMENTS: &[&str] = &[
    "Missing type on ListTag",
    "ListTag length cannot be negative",
    "Array tag length must be < 1 << 24",
    "Tried to read NBT tag that was too big",
    "Tried to account NBT tag with negative size",
    "Tried to read NBT tag with too high complexity",
    "NBT-Accounter tried to pop stack-depth",
];

fn panic_message(info: &PanicHookInfo<'_>) -> String {
    if let Some(s) = info.payload().downcast_ref::<&str>() {
        return (*s).to_string();
    }
    if let Some(s) = info.payload().downcast_ref::<String>() {
        return s.clone();
    }
    String::new()
}

fn is_faithful_panic(info: &PanicHookInfo<'_>) -> bool {
    let message = panic_message(info);
    FAITHFUL_PANIC_FRAGMENTS.iter().any(|f| message.contains(f))
}

pub fn install_panic_filter() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let default_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if is_faithful_panic(info) {
                // Swallowed by the `catch_unwind` in `guarded`.
            } else {
                default_hook(info);
                std::process::abort();
            }
        }));
    });
}

pub fn guarded(f: impl FnOnce()) {
    install_panic_filter();
    let _ = panic::catch_unwind(AssertUnwindSafe(f));
}
