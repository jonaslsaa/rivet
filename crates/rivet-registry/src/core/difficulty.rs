//! `net.minecraft.world.Difficulty` — the four difficulty values (#87).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/Difficulty.java`. Ported as the id/name half the change-difficulty
//! wire codec needs: `ClientboundChangeDifficultyPacket.STREAM_CODEC` composes
//! `Difficulty.STREAM_CODEC` (`ByteBufCodecs.idMapper(BY_ID, Difficulty::getId)`)
//! with the locked boolean. The byte-level decode wraps out-of-range ids via
//! `ByIdMap.continuous(..., OutOfBoundsStrategy.WRAP)` — `WRAP` is
//! `Mth.positiveModulo(id, length)` (`Math.floorMod(id, 4)`), so a byte `5`
//! wraps to `EASY` (index 1), `-1` to `HARD` (index 3), and so on (see
//! `by_id`).
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
//! translations), `getInfo`, and `getDisplayName`. The `byName`/`CODEC`
//! half (issue #387 — the `LevelSettings.DifficultySettings.CODEC` field, where
//! an unknown difficulty name fails the whole `difficulty_settings` decode and
//! falls back to `DEFAULT`) is ported here alongside the id↔name wire surface:
//! the four values, `getId`, `getSerializedName`, `byId` (WRAP), and
//! `byName`/`by_name_or` (the `StringRepresentable.EnumCodec` resolver).

use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::string_representable::{self, EnumCodec};

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

/// `Difficulty.values()` — the constants in declaration order (ordinal order),
/// serving the codec's name lookup / `fromInt` (the `by_name` resolver scans
/// it and `from_enum` captures it `'static`).
const VALUES: [Difficulty; 4] = [
    Difficulty::Peaceful,
    Difficulty::Easy,
    Difficulty::Normal,
    Difficulty::Hard,
];

impl Difficulty {
    /// `Difficulty.byName(String)` — `CODEC.byName(name)`, `@Nullable`.
    ///
    /// Java's `StringRepresentable.EnumCodec.byName` runs the codec's name
    /// resolver (`createNameLookup`, a linear scan for `values().length <=
    /// PRE_BUILT_MAP_THRESHOLD` — `Difficulty` has 4 values), so this is that
    /// scan over `VALUES` directly. Unlike `GameType.byName` there is NO
    /// default fallback: an unknown name is `None` (Java returns `null`), which
    /// is exactly the `@Nullable` the `DifficultySettings.CODEC` needs — the
    /// string codec branch fails and the whole field errors.
    pub fn by_name(name: &str) -> Option<Difficulty> {
        VALUES
            .iter()
            .find(|value| value.get_serialized_name() == name)
            .copied()
    }

    /// `byName(name)` then fall back to `default` on an unknown name — the
    /// `Objects.requireNonNullElse` pattern of `GameType.byName(String,
    /// GameType)`. `Difficulty.java` only declares the single-arg nullable
    /// `byName(String)`, so this two-arg helper is the GameType-mirroring
    /// convenience for the `DifficultySettings.CODEC` consumer.
    pub fn by_name_or(name: &str, default: Difficulty) -> Difficulty {
        Self::by_name(name).unwrap_or(default)
    }

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
    /// (`5 -> EASY`, `-1 -> HARD`).
    pub fn by_id(id: i32) -> Difficulty {
        BY_ID[id.rem_euclid(4) as usize]
    }
}

impl rivet_util::string_representable::StringRepresentable for Difficulty {
    /// `Difficulty.getSerializedName()` — the enum's `key` field.
    fn get_serialized_name(&self) -> &str {
        self.get_serialized_name()
    }
}

impl rivet_util::string_representable::EnumOrdinal for Difficulty {
    /// `Enum<Difficulty>.ordinal()` — the declaration position
    /// (`PEACEFUL`..`HARD`), which equals the wire id here.
    fn ordinal(&self) -> usize {
        self.get_id() as usize
    }
}

impl std::fmt::Display for Difficulty {
    /// `Enum.toString()` — the constant name (`PEACEFUL`, ...), not the
    /// serialized key. Only observable through the (unreachable for a real
    /// enum) `id_resolver_codec` encode error `"Element with unknown id: " + e`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Difficulty::Peaceful => "PEACEFUL",
            Difficulty::Easy => "EASY",
            Difficulty::Normal => "NORMAL",
            Difficulty::Hard => "HARD",
        })
    }
}

/// `Difficulty.CODEC` — `StringRepresentable.fromEnum(Difficulty::values)`
/// (the `orCompressed` name/ordinal codec with the `byName` resolver).
///
/// Ops-generic in the port, hence the factory shape (same as
/// `GameType::game_type_codec`). The `DifficultySettings.CODEC` (#323) uses
/// `field_of("difficulty", difficulty_codec())` so the string branch errors on
/// an unknown name and the whole record falls back to `DEFAULT`.
pub fn difficulty_codec<Ops: DynamicOps + 'static>() -> EnumCodec<Difficulty, Ops> {
    string_representable::from_enum::<Difficulty, Ops>(&VALUES)
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

    #[test]
    fn by_name_uses_serialized_keys_and_returns_none_for_unknown() {
        // `Difficulty.byName(String)` — `CODEC.byName(name)`, `@Nullable`:
        // unlike GameType there is NO default fallback, so an unknown name is
        // `None` (Java returns null), which is what fails the
        // `DifficultySettings.CODEC` field.
        assert_eq!(Difficulty::by_name("peaceful"), Some(Difficulty::Peaceful));
        assert_eq!(Difficulty::by_name("easy"), Some(Difficulty::Easy));
        assert_eq!(Difficulty::by_name("normal"), Some(Difficulty::Normal));
        assert_eq!(Difficulty::by_name("hard"), Some(Difficulty::Hard));
        assert_eq!(Difficulty::by_name("not_a_difficulty"), None);
        assert_eq!(Difficulty::by_name(""), None);
    }

    #[test]
    fn by_name_or_falls_back_to_default() {
        // `byName(name, _default)` — `requireNonNullElse`.
        assert_eq!(
            Difficulty::by_name_or("hard", Difficulty::Peaceful),
            Difficulty::Hard
        );
        assert_eq!(
            Difficulty::by_name_or("nope", Difficulty::Easy),
            Difficulty::Easy
        );
    }

    #[test]
    fn display_is_the_java_constant_name() {
        assert_eq!(Difficulty::Peaceful.to_string(), "PEACEFUL");
        assert_eq!(Difficulty::Easy.to_string(), "EASY");
        assert_eq!(Difficulty::Normal.to_string(), "NORMAL");
        assert_eq!(Difficulty::Hard.to_string(), "HARD");
    }

    #[test]
    fn codec_roundtrips_via_json() {
        use rivet_serialization::json_ops::JsonOps;
        use rivet_serialization::{Decoder, Encoder};
        let ops = JsonOps::INSTANCE;
        let codec = difficulty_codec::<JsonOps>();
        for value in [
            Difficulty::Peaceful,
            Difficulty::Easy,
            Difficulty::Normal,
            Difficulty::Hard,
        ] {
            let encoded = codec
                .encode_start(&ops, &value)
                .get_or_throw("encode")
                .clone();
            assert_eq!(
                encoded,
                ops.create_string(value.get_serialized_name().to_string())
            );
            let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
            assert_eq!(decoded.0, value);
        }
        // The string branch rejects an unknown name (the DifficultySettings
        // field then errors and the whole record falls back to DEFAULT).
        let unknown = ops.create_string("not_a_difficulty".to_string());
        assert!(codec.decode(&ops, &unknown).result().is_none());
    }

    #[test]
    fn codec_integer_ordinal_branch_only_under_compressed_ops() {
        use rivet_serialization::Decoder;
        use rivet_serialization::json_ops::JsonOps;
        let normal = JsonOps::INSTANCE;
        let compressed = JsonOps::COMPRESSED;
        let codec = difficulty_codec::<JsonOps>();
        assert!(
            codec
                .decode(&normal, &normal.create_int(0))
                .result()
                .is_none()
        );
        assert_eq!(
            codec
                .decode(&compressed, &compressed.create_int(0))
                .get_or_throw("decode")
                .0,
            Difficulty::Peaceful
        );
        assert_eq!(
            codec
                .decode(&compressed, &compressed.create_int(3))
                .get_or_throw("decode")
                .0,
            Difficulty::Hard
        );
        // Out-of-range ids fail the ordinal branch.
        assert!(
            codec
                .decode(&compressed, &compressed.create_int(4))
                .result()
                .is_none()
        );
        assert!(
            codec
                .decode(&compressed, &compressed.create_int(-1))
                .result()
                .is_none()
        );
    }
}
