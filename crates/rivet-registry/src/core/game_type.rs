//! `net.minecraft.world.level.GameType` — the pure id↔name value enum (#108).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/level/GameType.java`. Ported as the id/name half the spawn-info wire
//! codecs need: `CommonPlayerSpawnInfo` stores and re-encodes the game mode as
//! **signed bytes** (`byId`/`getNullableId`), so this enum's id surface is
//! reachable from `rivet-protocol`.
//!
//! Placement (documented cycle-break, OWNERSHIP.md §Registries — the
//! "pure value types stay in `rivet-registry::core`, with only their
//! `StreamCodec` impls crossing to `rivet-protocol`" rule): `GameType` is a
//! pure value enum, but it must be reachable from `rivet-protocol`
//! (CommonPlayerSpawnInfo stores and re-encodes it) while
//! `rivet-world → rivet-protocol` already exists, so `rivet-protocol →
//! rivet-world` would be a cycle. Same rationale as `ChunkPos`'s documented
//! one-line move into `core`. The wire `StreamCodec` impls live in
//! `rivet-protocol` per the ownership rule.
//!
//! Deliberately deferred (blocked by later work; no declarations emitted) to
//! the `mc.world.level` unit in `rivet-world`: the display `Component`s
//! (`selectWorld.gameMode.*` / `gameMode.*` translations), `getLongDisplayName`/
//! `getShortDisplayName`, and `updatePlayerAbilities` (`Abilities`). The pure
//! id↔name surface ports here only as far as the wire and the level.dat value
//! slice need: `SURVIVAL(0)/CREATIVE(1)/ADVENTURE(2)/SPECTATOR(3)`,
//! `getId`/`getName`/`getSerializedName`, `byId` (ZERO fallback),
//! `byNullableId`/`getNullableId` (`-1` ⇄ null), the
//! `isCreative`/`isSurvival`/`isBlockPlacingRestricted` predicates that are
//! id-pure, and — added by the level.dat codec slice (#373) — `CODEC`
//! (`StringRepresentable.fromEnum`), `LEGACY_ID_CODEC` (`Codec.INT.xmap`), and
//! the `byName`/`byName(name, default)` resolvers. `isValidId` is ported for
//! completeness but is dead surface until the command/handshake paths that call
//! Java's `isValidId` land. The Java enum's `id`/`name` fields are `private
//! final` with no setters; the Rust fields are private and immutable.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::string_representable::{self, EnumCodec};
use std::sync::Arc;

/// `GameType` — the four game-mode values, keyed by their wire ids
/// (`0`..`3`, `survival`..`spectator`).
///
/// `VALUES` is `GameType.values()` — the constants in declaration order
/// (ordinal order). Kept private: nothing outside the module needs to iterate
/// the values (`from_enum` and the codecs capture it via `'static`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameType {
    /// `SURVIVAL` — id `0`, name `"survival"`.
    Survival,
    /// `CREATIVE` — id `1`, name `"creative"`.
    Creative,
    /// `ADVENTURE` — id `2`, name `"adventure"`.
    Adventure,
    /// `SPECTATOR` — id `3`, name `"spectator"`.
    Spectator,
}

/// `GameType.values()` — the constants in declaration order (ordinal order).
///
/// This single array serves both Java roles: `values()` for the codec's
/// name lookup / `fromInt`, and `ByIdMap.continuous(GameType::getId, values(),
/// OutOfBoundsStrategy.ZERO)` for `by_id`. For this enum every value's wire id
/// equals its declaration position (`getId` returns `0`..`3` in declaration
/// order), so the dense id→value array Java builds is identical to `values()`;
/// `by_id` indexes `VALUES` directly instead of declaring a second, drift-prone
/// array. The ZERO strategy is exactly `VALUES.get(id as usize).copied()
/// .unwrap_or(SURVIVAL)` — a negative id casts to a huge `usize`, misses, and
/// falls back, matching Java's `BY_ID.apply(id)` for every `int`. `'static`
/// because `StringRepresentable.fromEnum`/`from_enum_with_mapping` capture the
/// array for their `fromInt`/name-lookup closures.
const VALUES: [GameType; 4] = [
    GameType::Survival,
    GameType::Creative,
    GameType::Adventure,
    GameType::Spectator,
];

impl GameType {
    /// `GameType.DEFAULT_MODE` — `SURVIVAL`.
    pub const DEFAULT_MODE: GameType = GameType::Survival;

    /// `GameType.getId()`.
    pub fn get_id(&self) -> i32 {
        match self {
            GameType::Survival => 0,
            GameType::Creative => 1,
            GameType::Adventure => 2,
            GameType::Spectator => 3,
        }
    }

    /// `GameType.getName()`.
    pub fn get_name(&self) -> &'static str {
        match self {
            GameType::Survival => "survival",
            GameType::Creative => "creative",
            GameType::Adventure => "adventure",
            GameType::Spectator => "spectator",
        }
    }

    /// `GameType.getSerializedName()` — same string as `getName`.
    pub fn get_serialized_name(&self) -> &'static str {
        self.get_name()
    }

    /// `GameType.byId(int)` — `VALUES[id]` with the ZERO fallback: any id
    /// outside `[0, 4)` (including negative) maps to `SURVIVAL`.
    pub fn by_id(id: i32) -> GameType {
        VALUES
            .get(id as usize)
            .copied()
            .unwrap_or(GameType::DEFAULT_MODE)
    }

    /// `GameType.byNullableId(int)` — `id == -1 ? null : byId(id)`.
    ///
    /// Only `-1` is null; any other id (including out-of-range values like
    /// `-2` or `4`) is `Some(byId(id))` with the ZERO fallback.
    pub fn by_nullable_id(id: i32) -> Option<GameType> {
        if id == -1 {
            None
        } else {
            Some(Self::by_id(id))
        }
    }

    /// `GameType.byName(String)` — `byName(name, SURVIVAL)`.
    ///
    /// `CODEC` is `StringRepresentable.fromEnum`, so the name lookup is the
    /// serialized name — identical to `getName` for every value — and an
    /// unknown name falls back to `DEFAULT_MODE` (never null). A lazily-built
    /// static here would be a `'static` `EnumCodec<GameType, Ops>` — the codec
    /// is ops-generic in the port, so it is a per-name call into
    /// `game_type_codec::<JsonOps>()`'s fresh lookup instead (see `by_name_or`
    /// for the shared helper).
    pub fn by_name(name: &str) -> GameType {
        Self::by_name_or(name, Some(Self::DEFAULT_MODE)).unwrap_or(Self::DEFAULT_MODE)
    }

    /// `GameType.getNullableId(@Nullable GameType)` — `gameType != null ?
    /// gameType.id : -1`.
    pub fn get_nullable_id(game_type: Option<GameType>) -> i32 {
        match game_type {
            Some(game_type) => game_type.get_id(),
            None => -1,
        }
    }

    /// `GameType.byName(String, @Nullable GameType defaultMode)` —
    /// `CODEC.byName(name) != null ? result : defaultMode`.
    ///
    /// The `Option<GameType>` default/result is the faithful port of Java's
    /// `@Nullable` parameter and return: `byName(name, null)` is how the
    /// command/selector paths (`GameModeArgument`, `EntitySelectorOptions`) ask
    /// for `null` on an unknown name. A non-null default is expressed by
    /// passing `Some` — `by_name` above does exactly that with
    /// `DEFAULT_MODE`. The codec is built fresh per call — the port has no
    /// ops-generic static to capture the name lookup once — but the lookup is
    /// a linear scan of 4 names, matching Java's `createNameLookup` for
    /// `values().length <= PRE_BUILT_MAP_THRESHOLD`.
    pub fn by_name_or(name: &str, default: Option<GameType>) -> Option<GameType> {
        game_type_codec::<rivet_serialization::json_ops::JsonOps>()
            .by_name(name)
            .or(default)
    }

    /// `GameType.isValidId(int)` — some value's id equals `id`.
    ///
    /// Dead surface until the command/handshake paths that call Java's
    /// `isValidId` land (spawn info never calls it, matching Java's
    /// `CommonPlayerSpawnInfo`).
    pub fn is_valid_id(id: i32) -> bool {
        matches!(id, 0..=3)
    }

    /// `GameType.isCreative()`.
    pub fn is_creative(&self) -> bool {
        *self == GameType::Creative
    }

    /// `GameType.isSurvival()` — `SURVIVAL || ADVENTURE`.
    pub fn is_survival(&self) -> bool {
        matches!(self, GameType::Survival | GameType::Adventure)
    }

    /// `GameType.isBlockPlacingRestricted()` — `ADVENTURE || SPECTATOR`.
    pub fn is_block_placing_restricted(&self) -> bool {
        matches!(self, GameType::Adventure | GameType::Spectator)
    }
}

impl rivet_util::string_representable::StringRepresentable for GameType {
    /// `GameType.getSerializedName()` — the enum's `name` field.
    fn get_serialized_name(&self) -> &str {
        self.get_name()
    }
}

impl rivet_util::string_representable::EnumOrdinal for GameType {
    /// `Enum<GameType>.ordinal()` — the declaration position.
    fn ordinal(&self) -> usize {
        self.get_id() as usize
    }
}

impl std::fmt::Display for GameType {
    /// `Enum.toString()` — the constant name (`SURVIVAL`, ...), not the
    /// serialized key. Only observable through the (unreachable for a real
    /// enum) `id_resolver_codec` encode error `"Element with unknown id: " + e`
    /// (StringRepresentableCodec's id branch never encodes an unknown value).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            GameType::Survival => "SURVIVAL",
            GameType::Creative => "CREATIVE",
            GameType::Adventure => "ADVENTURE",
            GameType::Spectator => "SPECTATOR",
        })
    }
}

/// `GameType.CODEC` — `StringRepresentable.fromEnum(GameType::values)`.
///
/// The `mc.util` port of `StringRepresentable` lives in
/// `rivet-util::string_representable`; its `from_enum` factory needs the
/// `EnumOrdinal` helper (Java's `Enum.ordinal()`, implemented above). The port
/// codecs are ops-generic, so the static Java constant becomes the
/// `game_type_codec::<Ops>()` factory (same shape as `Decoration.CODEC` in
/// `rivet-world` and `Rotations.CODEC` in `rivet-registry::core`).
pub fn game_type_codec<Ops: DynamicOps + 'static>() -> EnumCodec<GameType, Ops> {
    string_representable::from_enum::<GameType, Ops>(&VALUES)
}

/// `GameType.LEGACY_ID_CODEC` — `Codec.INT.xmap(GameType::byId,
/// GameType::getId)`, the deprecated int-id codec `PrimaryLevelData`/`LevelData`
/// still encode with.
///
/// Decode is `byId` — any int (including `-1`/out-of-range) maps to a value via
/// the ZERO fallback, so the legacy codec NEVER fails to decode; encode is
/// `getId` (`0`..`3`), which never fails either. The codec is ops-generic in the
/// port, hence the factory shape.
pub fn game_type_legacy_id_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<GameType, Ops>> {
    codec::xmap(
        codec::int_codec::<Ops>(),
        Arc::new(|id: &i32| GameType::by_id(*id)),
        Arc::new(GameType::get_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::{Decoder, Encoder};

    #[test]
    fn id_and_name() {
        let cases = [
            (GameType::Survival, 0, "survival"),
            (GameType::Creative, 1, "creative"),
            (GameType::Adventure, 2, "adventure"),
            (GameType::Spectator, 3, "spectator"),
        ];
        for (game_type, id, name) in cases {
            assert_eq!(game_type.get_id(), id);
            assert_eq!(game_type.get_name(), name);
            assert_eq!(game_type.get_serialized_name(), name);
        }
        assert_eq!(GameType::DEFAULT_MODE, GameType::Survival);
    }

    #[test]
    fn by_id_maps_ids_and_falls_back_to_survival() {
        // In-range ids map directly.
        assert_eq!(GameType::by_id(0), GameType::Survival);
        assert_eq!(GameType::by_id(1), GameType::Creative);
        assert_eq!(GameType::by_id(2), GameType::Adventure);
        assert_eq!(GameType::by_id(3), GameType::Spectator);
        // The ZERO fallback: any out-of-range id (negative or >= 4) -> SURVIVAL.
        assert_eq!(GameType::by_id(-1), GameType::Survival);
        assert_eq!(GameType::by_id(4), GameType::Survival);
        assert_eq!(GameType::by_id(-128), GameType::Survival);
        assert_eq!(GameType::by_id(i32::MAX), GameType::Survival);
        assert_eq!(GameType::by_id(i32::MIN), GameType::Survival);
    }

    #[test]
    fn nullable_id_round_trips() {
        // `-1` <-> null; anything else is `byId` (ZERO fallback).
        assert_eq!(GameType::by_nullable_id(-1), None);
        assert_eq!(GameType::get_nullable_id(None), -1);
        assert_eq!(GameType::by_nullable_id(2), Some(GameType::Adventure));
        assert_eq!(GameType::get_nullable_id(Some(GameType::Adventure)), 2);
        // An out-of-range nullable id is Some with the fallback, never None.
        assert_eq!(GameType::by_nullable_id(9), Some(GameType::Survival));
        assert_eq!(GameType::by_nullable_id(-2), Some(GameType::Survival));
    }

    #[test]
    fn valid_id_is_exactly_zero_to_three() {
        for id in 0..=3 {
            assert!(GameType::is_valid_id(id));
        }
        assert!(!GameType::is_valid_id(-1));
        assert!(!GameType::is_valid_id(4));
        assert!(!GameType::is_valid_id(i32::MAX));
    }

    #[test]
    fn predicates() {
        assert!(GameType::Creative.is_creative());
        assert!(!GameType::Survival.is_creative());
        assert!(GameType::Survival.is_survival());
        assert!(GameType::Adventure.is_survival());
        assert!(!GameType::Creative.is_survival());
        assert!(!GameType::Spectator.is_survival());
        assert!(GameType::Adventure.is_block_placing_restricted());
        assert!(GameType::Spectator.is_block_placing_restricted());
        assert!(!GameType::Survival.is_block_placing_restricted());
        assert!(!GameType::Creative.is_block_placing_restricted());
    }

    #[test]
    fn by_name_uses_serialized_names_with_survival_fallback() {
        // `GameType.byName(String)` — `byName(name, SURVIVAL)`: an unknown name
        // falls back to `DEFAULT_MODE`, never null.
        assert_eq!(GameType::by_name("survival"), GameType::Survival);
        assert_eq!(GameType::by_name("creative"), GameType::Creative);
        assert_eq!(GameType::by_name("adventure"), GameType::Adventure);
        assert_eq!(GameType::by_name("spectator"), GameType::Spectator);
        assert_eq!(GameType::by_name("not_a_mode"), GameType::Survival);
        // The name lookup keys on the serialized name, which equals `getName`.
        assert_eq!(
            GameType::by_name(GameType::Creative.get_name()),
            GameType::Creative
        );
    }

    #[test]
    fn by_name_with_explicit_default() {
        // `byName(name, _default)` — `CODEC.byName(name) != null ? result :
        // default`, with the `@Nullable` default/result ported to `Option`.
        assert_eq!(
            GameType::by_name_or("adventure", Some(GameType::Creative)),
            Some(GameType::Adventure)
        );
        assert_eq!(
            GameType::by_name_or("nope", Some(GameType::Spectator)),
            Some(GameType::Spectator)
        );
        // The default is returned for an unknown name even when it differs from
        // `DEFAULT_MODE`.
        assert_eq!(
            GameType::by_name_or("not_a_mode", Some(GameType::Creative)),
            Some(GameType::Creative)
        );
    }

    #[test]
    fn by_name_with_null_default_returns_none_for_unknown() {
        // `byName(name, null)` — how `GameModeArgument`/`EntitySelectorOptions`
        // ask for `null` on an unknown name. A known name is still `Some`.
        assert_eq!(
            GameType::by_name_or("adventure", None),
            Some(GameType::Adventure)
        );
        assert_eq!(GameType::by_name_or("not_a_mode", None), None);
        // `by_name(String)` is `byName(name, SURVIVAL)` — a `Some` default, so
        // it never surfaces the nullability.
        assert_eq!(GameType::by_name("not_a_mode"), GameType::Survival);
        assert_eq!(GameType::by_name("creative"), GameType::Creative);
    }

    #[test]
    fn codec_roundtrips_all_values_and_rejects_unknown() {
        let ops = JsonOps::INSTANCE;
        let codec = game_type_codec::<JsonOps>();
        for value in [
            GameType::Survival,
            GameType::Creative,
            GameType::Adventure,
            GameType::Spectator,
        ] {
            // Encode to the serialized-name string.
            let encoded = codec
                .encode_start(&ops, &value)
                .get_or_throw("encode")
                .clone();
            assert_eq!(
                encoded,
                ops.create_string(value.get_serialized_name().to_string())
            );
            // Decode back (the orCompressed string branch).
            let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
            assert_eq!(decoded.0, value);
        }
        // Unknown name fails the string branch.
        let unknown = ops.create_string("not_a_mode".to_string());
        assert!(codec.decode(&ops, &unknown).result().is_none());
    }

    #[test]
    fn codec_integer_ordinal_branch_only_under_compressed_ops() {
        // `orCompressed(stringResolver, idResolver)` routes on
        // `ops.compressMaps()` — the int-ordinal branch fires only for
        // compressed ops. Under `INSTANCE` (non-compressed) an int input goes
        // to the string branch and fails.
        let normal = JsonOps::INSTANCE;
        let compressed = JsonOps::COMPRESSED;
        let codec = game_type_codec::<JsonOps>();
        assert!(
            codec
                .decode(&normal, &normal.create_int(0))
                .result()
                .is_none()
        );
        // The id branch decodes the ordinal.
        assert_eq!(
            codec
                .decode(&compressed, &compressed.create_int(0))
                .get_or_throw("decode")
                .0,
            GameType::Survival
        );
        assert_eq!(
            codec
                .decode(&compressed, &compressed.create_int(3))
                .get_or_throw("decode")
                .0,
            GameType::Spectator
        );
        // Encoding under compressed ops emits the ordinal int.
        assert_eq!(
            codec
                .encode_start(&compressed, &GameType::Adventure)
                .get_or_throw("encode")
                .clone(),
            compressed.create_int(2)
        );
        // Out-of-range ids fail the id branch (ordinal, not the wire id).
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

    #[test]
    fn legacy_id_codec_is_int_by_id_with_zero_fallback() {
        let ops = JsonOps::INSTANCE;
        let codec = game_type_legacy_id_codec::<JsonOps>();
        // Decode: `Codec.INT.xmap(GameType::byId, GameType::getId)` — any int
        // (including -1/out-of-range) maps to a value via the ZERO fallback.
        for (id, expected) in [
            (0, GameType::Survival),
            (1, GameType::Creative),
            (2, GameType::Adventure),
            (3, GameType::Spectator),
            (-1, GameType::Survival),
            (4, GameType::Survival),
            (i32::MAX, GameType::Survival),
            (i32::MIN, GameType::Survival),
        ] {
            let decoded = codec
                .decode(&ops, &ops.create_int(id))
                .get_or_throw("decode")
                .0;
            assert_eq!(decoded, expected, "id = {id}");
        }
        // Encode: `GameType::getId` — the wire id.
        for (value, expected_id) in [
            (GameType::Survival, 0),
            (GameType::Creative, 1),
            (GameType::Adventure, 2),
            (GameType::Spectator, 3),
        ] {
            let encoded = codec
                .encode_start(&ops, &value)
                .get_or_throw("encode")
                .clone();
            assert_eq!(encoded, ops.create_int(expected_id), "value = {value:?}");
        }
    }

    #[test]
    fn display_is_the_java_constant_name() {
        // `Enum.toString()` returns the constant name, not the serialized key.
        assert_eq!(GameType::Survival.to_string(), "SURVIVAL");
        assert_eq!(GameType::Creative.to_string(), "CREATIVE");
        assert_eq!(GameType::Adventure.to_string(), "ADVENTURE");
        assert_eq!(GameType::Spectator.to_string(), "SPECTATOR");
    }
}
