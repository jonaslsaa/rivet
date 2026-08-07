//! Port of `net.minecraft.world.entity.player.ChatVisiblity` (issue #197).
//!
//! Java: `ChatVisiblity.java` in `working/Paper`. On the wire the value is the
//! enum ordinal via `readEnum`/`writeEnum` (a varint). The `caption` field,
//! `LEGACY_CODEC`, and the `BY_ID` mapper need `Component`/DFU codecs and are
//! deferred; only the ordinal surface the `ClientInformation` codec uses is
//! ported.
//!
//! Placement note: `ChatVisiblity` is a `net.minecraft.world.entity.player`
//! value type, but it lives in `rivet-protocol` (not `rivet-entity`, its
//! package-mirror home) because the only consumer in this slice is the
//! `ClientInformation` packet body and `rivet-entity` is downstream of
//! `rivet-protocol`.

/// `net.minecraft.world.entity.player.ChatVisiblity`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatVisiblity {
    /// `FULL` — ordinal 0.
    Full,
    /// `SYSTEM` — ordinal 1.
    System,
    /// `HIDDEN` — ordinal 2.
    Hidden,
}

impl ChatVisiblity {
    /// The number of constants — the length used by Java's `values().length`
    /// in the out-of-range message.
    pub const COUNT: i32 = 3;

    /// `ChatVisiblity.values()[ordinal]` — declaration order is the wire
    /// ordinal. An id outside the 3 constants is `None` — Java's
    /// `ArrayIndexOutOfBoundsException` — and the codec surfaces it as `Err`.
    pub fn from_id(id: i32) -> Option<ChatVisiblity> {
        match id {
            0 => Some(ChatVisiblity::Full),
            1 => Some(ChatVisiblity::System),
            2 => Some(ChatVisiblity::Hidden),
            _ => None,
        }
    }

    /// `ChatVisiblity.ordinal()` — the wire ordinal.
    pub fn id(&self) -> i32 {
        *self as i32
    }
}
