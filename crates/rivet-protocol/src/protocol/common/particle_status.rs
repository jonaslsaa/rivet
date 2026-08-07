//! Port of `net.minecraft.server.level.ParticleStatus` (issue #197).
//!
//! Java: `ParticleStatus.java` in `working/Paper`. On the wire the value is the
//! enum ordinal via `readEnum`/`writeEnum` (a varint). The `caption` field,
//! `LEGACY_CODEC`, and the `BY_ID` mapper need `Component`/DFU codecs and are
//! deferred; only the ordinal surface the `ClientInformation` codec uses is
//! ported.
//!
//! Placement note: `ParticleStatus` is a `net.minecraft.server.level` value
//! type, but it lives in `rivet-protocol` (not `rivet-server`, its
//! package-mirror home) because the only consumer in this slice is the
//! `ClientInformation` packet body and `rivet-server` is downstream of
//! `rivet-protocol`.

/// `net.minecraft.server.level.ParticleStatus`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleStatus {
    /// `ALL` — ordinal 0.
    All,
    /// `DECREASED` — ordinal 1.
    Decreased,
    /// `MINIMAL` — ordinal 2.
    Minimal,
}

impl ParticleStatus {
    /// The number of constants — the length used by Java's `values().length`
    /// in the out-of-range message.
    pub const COUNT: i32 = 3;

    /// `ParticleStatus.values()[ordinal]` — declaration order is the wire
    /// ordinal. An id outside the 3 constants is `None` — Java's
    /// `ArrayIndexOutOfBoundsException` — and the codec surfaces it as `Err`.
    pub fn from_id(id: i32) -> Option<ParticleStatus> {
        match id {
            0 => Some(ParticleStatus::All),
            1 => Some(ParticleStatus::Decreased),
            2 => Some(ParticleStatus::Minimal),
            _ => None,
        }
    }

    /// `ParticleStatus.ordinal()` — the wire ordinal.
    pub fn id(&self) -> i32 {
        *self as i32
    }
}
