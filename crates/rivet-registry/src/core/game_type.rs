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
//! the `mc.world.level` unit in `rivet-world`: `CODEC`/`LEGACY_ID_CODEC`
//! (`StringRepresentable.EnumCodec` + DFU `Codec.INT.xmap`, need the
//! serialization codec surface), the display `Component`s
//! (`selectWorld.gameMode.*` / `gameMode.*` translations), `getLongDisplayName`/
//! `getShortDisplayName`, `updatePlayerAbilities` (`Abilities`), and `byName`
//! (needs `CODEC.byName`). The pure id↔name surface ports here only as far as
//! the wire needs: `SURVIVAL(0)/CREATIVE(1)/ADVENTURE(2)/SPECTATOR(3)`,
//! `getId`/`getName`/`getSerializedName`, `byId` (ZERO fallback),
//! `byNullableId`/`getNullableId` (`-1` ⇄ null), and the
//! `isCreative`/`isSurvival`/`isBlockPlacingRestricted` predicates that are
//! id-pure. `isValidId` is ported for completeness but is dead surface until
//! the command/handshake paths that call Java's `isValidId` land. The Java
//! enum's `id`/`name` fields are `private final` with no setters; the Rust
//! fields are private and immutable.

/// `GameType` — the four game-mode values, keyed by their wire ids
/// (`0`..`3`, `survival`..`spectator`).
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

/// `BY_ID = ByIdMap.continuous(GameType::getId, values(), OutOfBoundsStrategy.ZERO)`
/// — the dense `id → value` array in enum order.
///
/// A `const` array, following the in-crate `Direction::BY_3D_DATA` precedent
/// (same Java origin, same `ByIdMap.continuous` declaration): the ZERO
/// strategy is exactly `BY_ID.get(id as usize).copied().unwrap_or(SURVIVAL)`
/// — a negative id casts to a huge `usize`, misses, and falls back. No
/// `ByIdMap` closure or `LazyLock` needed; Java's `BY_ID.apply(id)` behaves
/// identically for every `int`.
const BY_ID: [GameType; 4] = [
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

    /// `GameType.byId(int)` — `BY_ID.apply(id)`: any id outside `[0, 4)`
    /// (including negative) maps to the ZERO-fallback `SURVIVAL`.
    pub fn by_id(id: i32) -> GameType {
        BY_ID
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

    /// `GameType.getNullableId(@Nullable GameType)` — `gameType != null ?
    /// gameType.id : -1`.
    pub fn get_nullable_id(game_type: Option<GameType>) -> i32 {
        match game_type {
            Some(game_type) => game_type.get_id(),
            None => -1,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
