//! `net.minecraft.util.Mth` — only the `floor` pair used by `FloatTag`/`DoubleTag`.
//!
//! Owned by rivet-util; the parity-tested `floor`/`floor_d` are re-exported
//! here (see `rivet_util::mth`).

pub fn floor(v: f32) -> i32 {
    rivet_util::floor(v)
}

pub fn floor_d(v: f64) -> i32 {
    rivet_util::floor_d(v)
}
