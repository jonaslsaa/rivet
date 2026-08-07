//! Port of `net.minecraft.world.entity.HumanoidArm` (issue #197).
//!
//! Java: `HumanoidArm.java` in `working/Paper`. On the wire the value is the
//! enum ordinal via `readEnum`/`writeEnum` (a varint); for this enum the ordinal
//! equals `HumanoidArm.STREAM_CODEC`'s `id` (declaration order == id order), so
//! both paths agree. The `name`/`caption` fields, `CODEC`, and `STREAM_CODEC`
//! need `Component`/DFU codecs and are deferred; only the ordinal surface the
//! `ClientInformation` codec uses plus `getOpposite` are ported.
//!
//! Placement note: `HumanoidArm` is a `net.minecraft.world.entity` value type,
//! but it lives in `rivet-protocol` (not `rivet-entity`, its package-mirror
//! home) because the only consumer in this slice is the `ClientInformation`
//! packet body and `rivet-entity` is downstream of `rivet-protocol`.

/// `net.minecraft.world.entity.HumanoidArm`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HumanoidArm {
    /// `LEFT` — ordinal 0.
    Left,
    /// `RIGHT` — ordinal 1.
    Right,
}

impl HumanoidArm {
    /// The number of constants — the length used by Java's `values().length`
    /// in the out-of-range message.
    pub const COUNT: i32 = 2;

    /// `HumanoidArm.values()[ordinal]` — declaration order is the wire ordinal.
    /// An id outside the 2 constants is `None` — Java's
    /// `ArrayIndexOutOfBoundsException` — and the codec surfaces it as `Err`.
    pub fn from_id(id: i32) -> Option<HumanoidArm> {
        match id {
            0 => Some(HumanoidArm::Left),
            1 => Some(HumanoidArm::Right),
            _ => None,
        }
    }

    /// `HumanoidArm.ordinal()` — the wire ordinal.
    pub fn id(&self) -> i32 {
        *self as i32
    }

    /// `HumanoidArm.getOpposite()`.
    pub fn get_opposite(&self) -> HumanoidArm {
        match self {
            HumanoidArm::Left => HumanoidArm::Right,
            HumanoidArm::Right => HumanoidArm::Left,
        }
    }
}
