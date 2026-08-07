//! `net.minecraft.world.entity.Relative` — the position-move rotation flags
//! (#87).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/entity/Relative.java`. A pure enum whose wire form is a packed `int`
//! bitmask: `SET_STREAM_CODEC = ByteBufCodecs.INT.map(Relative::unpack,
//! Relative::pack)`. `ClientboundPlayerPositionPacket` composes it, and
//! `ServerboundMovePlayerPacket` re-reads it on the serverbound side.
//!
//! Placement follows the documented `GameType` precedent (OWNERSHIP.md
//! §Registries): pure value types stay in `rivet-registry::core`, with only
//! their `StreamCodec` impls crossing to `rivet-protocol`. `Relative` is a pure
//! value enum; the entity unit (M3) takes over the full class (the
//! `ALL`/`ROTATION`/`DELTA` set constants and the `union`/`rotation`/
//! `position`/`direction` factories), which only need `Set` and are deferred.
//!
//! Wire behavior preserved: `pack` ORs each present flag's mask (`1 << bit`),
//! `unpack` iterates the enum constants in declaration order and includes the
//! value whenever its bit is set — so decode -> encode is identity for any
//! `int` (mask bits beyond the declared constants are dropped on decode, and a
//! re-encode only sets the bits Java would).

/// `Relative` — the nine position/delta/rotation flags, keyed by their bit
/// indices (`0`..`8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relative {
    /// `X` — bit `0`.
    X,
    /// `Y` — bit `1`.
    Y,
    /// `Z` — bit `2`.
    Z,
    /// `Y_ROT` — bit `3`.
    YRot,
    /// `X_ROT` — bit `4`.
    XRot,
    /// `DELTA_X` — bit `5`.
    DeltaX,
    /// `DELTA_Y` — bit `6`.
    DeltaY,
    /// `DELTA_Z` — bit `7`.
    DeltaZ,
    /// `ROTATE_DELTA` — bit `8`.
    RotateDelta,
}

impl Relative {
    /// All constants in Java `values()` declaration order.
    const VALUES: [Relative; 9] = [
        Relative::X,
        Relative::Y,
        Relative::Z,
        Relative::YRot,
        Relative::XRot,
        Relative::DeltaX,
        Relative::DeltaY,
        Relative::DeltaZ,
        Relative::RotateDelta,
    ];

    /// `Relative.getMask()` — `1 << this.bit`.
    fn get_mask(self) -> i32 {
        1i32 << (self as i32)
    }

    /// `Relative.isSet(int)` — `(value & this.getMask()) == this.getMask()`.
    fn is_set(self, value: i32) -> bool {
        (value & self.get_mask()) == self.get_mask()
    }

    /// `Relative.unpack(int)` — iterates `values()` in declaration order and
    /// includes each constant whose bit is set.
    pub fn unpack(value: i32) -> Vec<Relative> {
        let mut result = Vec::new();
        for argument in Self::VALUES {
            if argument.is_set(value) {
                result.push(argument);
            }
        }
        result
    }

    /// `Relative.pack(Set<Relative>)` — ORs each present flag's mask.
    pub fn pack(set: &[Relative]) -> i32 {
        let mut result = 0i32;
        for argument in set {
            result |= argument.get_mask();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_and_unpack_round_trip_in_order() {
        // `EnumSet.noneOf` + add in declaration order -> `values()` order.
        let set = vec![
            Relative::X,
            Relative::Z,
            Relative::XRot,
            Relative::RotateDelta,
        ];
        let packed = Relative::pack(&set);
        assert_eq!(packed, (1 << 0) | (1 << 2) | (1 << 4) | (1 << 8));
        assert_eq!(Relative::unpack(packed), set);
    }

    #[test]
    fn empty_set_packs_to_zero() {
        assert_eq!(Relative::pack(&[]), 0);
        assert_eq!(Relative::unpack(0), Vec::<Relative>::new());
    }

    #[test]
    fn unknown_high_bits_are_dropped() {
        // A hostile/mutated wire sets bit 12 (not a declared flag): `unpack`
        // ignores it, and a re-encode drops it (only the declared masks).
        let mut set = Relative::unpack((1 << 12) | (1 << 1));
        set.sort_by_key(|r| *r as i32);
        assert_eq!(set, vec![Relative::Y]);
        assert_eq!(Relative::pack(&set), 1 << 1);
    }

    #[test]
    fn all_nine_flags_round_trip() {
        let all = Relative::VALUES.to_vec();
        assert_eq!(all.len(), 9);
        assert_eq!(Relative::unpack(Relative::pack(&all)), all);
    }
}
