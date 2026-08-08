//! Shared panic-filter helper for the binary-NBT fuzz targets.
//!
//! libfuzzer-sys aborts the process on *every* panic (even one caught by
//! `catch_unwind`), so the binary targets could never tolerate the faithful
//! panics that a ported `NbtIo` raises on hostile input. This module swaps the
//! panic hook for one that aborts only on panics outside the known-faithful
//! set — a genuine bug still aborts and writes an artifact. The regression
//! tests reuse the same `FAITHFUL_PANIC_FRAGMENTS` table to classify panics
//! without installing the hook.

use std::any::Any;
use std::panic::{self, AssertUnwindSafe, PanicHookInfo};

/// Panic-message substrings of the faithful Java crash paths in rivet-nbt's
/// read path. Keep in sync with the `panic!` sites in `crates/rivet-nbt`:
/// - `nbt_io.rs`: "Missing type on ListTag", "ListTag length cannot be
///   negative", "Array tag length must be < 1 << 24" (`check_array_length`)
/// - `nbt_accounter.rs`: the `NbtAccounterException` quota/depth panics
///
/// The DFU compressed-map decode path (`codec_compressed_decode`) also runs
/// under this filter: an out-of-range packed-list index is Java's
/// `IndexOutOfBoundsException` from `CompressedMapLike.get` on hostile input,
/// so "out of bounds for compressed-map list" is swallowed too.
const FAITHFUL_PANIC_FRAGMENTS: &[&str] = &[
    "Missing type on ListTag",
    "ListTag length cannot be negative",
    "Array tag length must be < 1 << 24",
    "Tried to read NBT tag that was too big",
    "Tried to account NBT tag with negative size",
    "Tried to read NBT tag with too high complexity",
    "NBT-Accounter tried to pop stack-depth",
    "out of bounds for compressed-map list", // Java IndexOutOfBoundsException
];

/// The panic message text from a `catch_unwind` payload, `""` if not a string.
pub fn message_of(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    String::new()
}

fn panic_message(info: &PanicHookInfo<'_>) -> String {
    message_of(info.payload())
}

/// Whether `message` matches one of the faithful Java crash sites.
pub fn is_faithful_message(message: &str) -> bool {
    FAITHFUL_PANIC_FRAGMENTS.iter().any(|f| message.contains(f))
}

fn is_faithful_panic(info: &PanicHookInfo<'_>) -> bool {
    is_faithful_message(&panic_message(info))
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
