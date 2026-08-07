//! `Vec3` — the wire-only value slice of the JOML `org.joml.Vector3d` the
//! position-move codecs carry (#87).
//!
//! Java: `net.minecraft.world.phys.Vec3` wraps a `Vector3d`; its `STREAM_CODEC`
//! is three big-endian doubles (`Vec3.STREAM_CODEC`). `PositionMoveRotation`/
//! `ClientboundPlayerPositionPacket` need exactly that wire surface — nothing
//! from the JOML math (`add`/`scale`/`cross`/… is deferred with the JOML unit,
//! see the existing `RivetTodo(#212)` note in `core/mod.rs`).
//!
//! Placement follows the `GameType`/`Difficulty` precedent (OWNERSHIP.md
//! §Registries): the pure value type lives in `rivet-registry::core` (so
//! `rivet-protocol` can hold it below itself), with the `StreamCodec` impl in
//! `rivet-protocol`. This is a deliberate, documented wire-only slice of a
//! deferred JOML type; the JOML unit replaces it with the real vector math.

/// `Vec3` — a 3-component double vector (`Vector3d`), as the position-move
/// wire reads/writes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    /// `x`.
    pub x: f64,
    /// `y`.
    pub y: f64,
    /// `z`.
    pub z: f64,
}

impl Vec3 {
    /// `new Vec3(double x, double y, double z)`.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_three_doubles() {
        let v = Vec3::new(1.0, -63.0, 2.5);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, -63.0);
        assert_eq!(v.z, 2.5);
    }
}
