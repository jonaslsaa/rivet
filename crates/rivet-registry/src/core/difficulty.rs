//! `net.minecraft.world.Difficulty` — the four difficulty values (#87).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/Difficulty.java`. Ported as the id/name half the change-difficulty
//! wire codec needs: `ClientboundChangeDifficultyPacket.STREAM_CODEC` composes
//! `Difficulty.STREAM_CODEC` (`ByteBufCodecs.idMapper(BY_ID, Difficulty::getId)`)
//! with the locked boolean. The byte-level decode wraps out-of-range ids via
//! `ByIdMap.continuous(..., OutOfBoundsStrategy.WRAP)` — a byte `5` maps back to
//! `NORMAL`, `-1` to `HARD`, and so on (see `by_id`).
//!
//! Placement follows the documented `GameType` precedent (OWNERSHIP.md
//! §Registries — "pure value types stay in `rivet-registry::core`, with only
//! their `StreamCodec` impls crossing to `rivet-protocol`"): `Difficulty` is a
//! pure value enum the change-difficulty codec needs, `rivet-world →
//! rivet-protocol` already exists, and `rivet-protocol → rivet-world` would be
//! a cycle. The wire `StreamCodec` impl lives in `rivet-protocol`.
//!
//! Deliberately deferred (blocked by later units; no declarations emitted) to
//! the `mc.world` unit: the display `Component`s (`options.difficulty.*`
//! translations), `getInfo`, `byName`/`CODEC` (needs the serialization
//! `StringRepresentable.EnumCodec` surface), and `getDisplayName`. The pure
//! id↔name surface ports here only as far as the wire needs: the four values,
//! `getId`, `getSerializedName`, and `byId` (WRAP).

/// `Difficulty` — the four difficulty values, keyed by their wire ids
/// (`0`..`3`, `peaceful`..`hard`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Difficulty {
    /// `PEACEFUL` — id `0`, key `"peaceful"`.
    Peaceful,
    /// `EASY` — id `1`, key `"easy"`.
    Easy,
    /// `NORMAL` — id `2`, key `"normal"`.
    Normal,
    /// `HARD` — id `3`, key `"hard"`.
    Hard,
}

/// `BY_ID = ByIdMap.continuous(Difficulty::getId, values(), OutOfBoundsStrategy.WRAP)`.
///
/// The WRAP strategy maps any id via `values[Math.floorMod(id - firstId, i)]`
/// — `firstId = 0`, `i = 4`, so `values[floorMod(id, 4)]`. Java's
/// `Math.floorMod` matches Rust's `rem_euclid` for a positive divisor, and the
/// ids are dense from 0, so a `const` array indexed by `id.rem_euclid(4)` is
/// exactly Java's WRAP (no negative-index or modulo-sign edge cases).
const BY_ID: [Difficulty; 4] = [
    Difficulty::Peaceful,
    Difficulty::Easy,
    Difficulty::Normal,
    Difficulty::Hard,
];

impl Difficulty {
    /// `Difficulty.getId()`.
    pub fn get_id(&self) -> i32 {
        match self {
            Difficulty::Peaceful => 0,
            Difficulty::Easy => 1,
            Difficulty::Normal => 2,
            Difficulty::Hard => 3,
        }
    }

    /// `Difficulty.getSerializedName()` — the enum key.
    pub fn get_serialized_name(&self) -> &'static str {
        match self {
            Difficulty::Peaceful => "peaceful",
            Difficulty::Easy => "easy",
            Difficulty::Normal => "normal",
            Difficulty::Hard => "hard",
        }
    }

    /// `Difficulty.byId(int)` — `BY_ID.apply(id)` with the WRAP strategy:
    /// any id (negative or `>= 4`) maps around the four values
    /// (`5 -> NORMAL`, `-1 -> HARD`).
    pub fn by_id(id: i32) -> Difficulty {
        BY_ID[id.rem_euclid(4) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_and_name() {
        let cases = [
            (Difficulty::Peaceful, 0, "peaceful"),
            (Difficulty::Easy, 1, "easy"),
            (Difficulty::Normal, 2, "normal"),
            (Difficulty::Hard, 3, "hard"),
        ];
        for (difficulty, id, name) in cases {
            assert_eq!(difficulty.get_id(), id);
            assert_eq!(difficulty.get_serialized_name(), name);
        }
    }

    #[test]
    fn by_id_maps_ids_and_wraps() {
        assert_eq!(Difficulty::by_id(0), Difficulty::Peaceful);
        assert_eq!(Difficulty::by_id(1), Difficulty::Easy);
        assert_eq!(Difficulty::by_id(2), Difficulty::Normal);
        assert_eq!(Difficulty::by_id(3), Difficulty::Hard);
        // WRAP: `floorMod(id, 4)` for the out-of-range cases.
        assert_eq!(Difficulty::by_id(4), Difficulty::Peaceful);
        assert_eq!(Difficulty::by_id(5), Difficulty::Easy);
        assert_eq!(Difficulty::by_id(-1), Difficulty::Hard);
        assert_eq!(Difficulty::by_id(-4), Difficulty::Peaceful);
        assert_eq!(Difficulty::by_id(i32::MIN), Difficulty::Peaceful);
        assert_eq!(Difficulty::by_id(i32::MAX), Difficulty::Hard);
    }
}
