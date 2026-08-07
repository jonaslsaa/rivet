//! Shared panic filter for the rivet-protocol packet-decode fuzz targets.
//!
//! Decoding a packet body reaches raw `FriendlyByteBuf` reads that faithfully
//! panic on hostile input the way Paper's raw netty layer does:
//!   - a short read panics with the `bytes` crate's
//!     `"advance out of bounds"` (netty `IndexOutOfBoundsException`);
//!   - an over-length varint panics with `"VarInt too big"` /
//!     `"VarLong too big"` (`VarInt.read`'s `RuntimeException`);
//!   - a negative collection count panics with `"Illegal Capacity: -n"`
//!     (Java `IllegalArgumentException` from the `ArrayList(int)` constructor);
//!   - an out-of-range enum ordinal panics with
//!     `"Index n out of bounds for length m"` (Java `ArrayIndexOutOfBoundsException`
//!     from `readEnum`);
//!   - `FriendlyByteBuf::read_slice` panics with `"read_slice: ..."` on a
//!     negative or short request (the raw-buffer defensive guard).
//!
//! `libfuzzer-sys` installs a panic hook that aborts on *every* panic, so
//! `catch_unwind` alone cannot tolerate faithful panics. This module swaps in a
//! filtered hook that swallows exactly the faithful sites above and aborts on
//! anything else — a genuine bug still crashes the fuzzer and writes an
//! artifact.
//!
//! Keep `FAITHFUL_PANIC_FRAGMENTS` in sync with the `panic!` sites reachable
//! from the covered serverbound decode paths. The codec-boundary paths that
//! report an error without panicking (`string_utf8`'s length cases, an
//! over-limit `list_max` count, the `from_id` enum reads) return `Err` and are
//! deliberately *not* listed — a negative `list_max` count is the one
//! collection-path panic, covered by the `"Illegal Capacity"` entry above. If a
//! body regresses to a raw helper (`read_utf`, `read_byte_array`, ...) its
//! panic message is not recognized and correctly crashes the fuzzer.

use std::panic::{self, AssertUnwindSafe, PanicHookInfo};

/// Panic-message substrings of the faithful Java crash paths reachable from
/// the packet-decode fuzz targets. See the module doc for the Java mapping.
const FAITHFUL_PANIC_FRAGMENTS: &[&str] = &[
    "advance out of bounds", // bytes crate EOF (netty IndexOutOfBounds)
    "VarInt too big",
    "VarLong too big",
    "Illegal Capacity: ",       // Java IllegalArgumentException (ArrayList(int))
    "out of bounds for length", // Java ArrayIndexOutOfBoundsException (readEnum)
    "read_slice:",              // FriendlyByteBuf::read_slice guard
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

/// Run one decode under the filtered hook; faithful panics are returned as
/// `Err` from `catch_unwind` and ignored.
pub fn guarded(f: impl FnOnce()) {
    install_panic_filter();
    let _ = panic::catch_unwind(AssertUnwindSafe(f));
}

/// Upper bound on the bytes handed to one decode. The decode paths are already
/// bounded by the codec limits (a 16-unit string, a `list(64)`, fixed scalars);
/// this caps the only input-proportional allocations (`decode_utf8`'s scratch
/// `String`) so a pathological `-max_len` cannot force a large allocation.
pub const MAX_INPUT_LEN: usize = 1 << 16;
