//! Port of `net.minecraft.world.level.levelgen.carver.WorldCarver` (abstract
//! class, 26.2) — the carver identity/behavior split.
//!
//! Java `WorldCarver<C>` is an object with identity: the three static
//! constants (`CAVE`/`NETHER_CAVE`/`CANYON`) are `register(...)` calls into
//! `BuiltInRegistries.CARVER`, and `ConfiguredWorldCarver` dispatches through
//! it. The Rust port mirrors `Feature`'s identity split (`FeatureId` +
//! `FeatureBehavior`):
//! - `WorldCarverId` — the registry-held identity handle (`BuiltInRegistries.
//!   CARVER` element identity) plus its registry-key location.
//! - `WorldCarverBehavior<C>` — the overridable behavior contract the concrete
//!   carver structs implement.
//!
//! The abstract `carve` behavior is NOT part of this shell — its signature
//! needs `CarvingContext`, `Aquifer`, `Function<BlockPos, Holder<Biome>>` and
//! `ChunkAccess`'s block surface, none of which are ported yet (see the module
//! doc and RivetTodo(#180)). The concrete carvers (`CaveWorldCarver`,
//! `NetherWorldCarver`, `CanyonWorldCarver`), their configurations
//! (`CaveCarverConfiguration`, `CanyonCarverConfiguration`), `CarverDebugSettings`
//! and the `CAVE`/`NETHER_CAVE`/`CANYON` `BuiltInRegistries.CARVER`
//! registrations land with the `#180` algorithm. Until then the dispatch
//! (`carver_is_start_chunk`) is a pre-wire STUB that panics unconditionally.

use crate::levelgen::carver::CarverConfiguration;
use rivet_util::RandomSource;
use std::fmt::Debug;

/// `net.minecraft.world.level.levelgen.carver.WorldCarver<C extends
/// CarverConfiguration>` — the abstract carver base's behavior contract.
///
/// Java `WorldCarver<C>` is an object whose identity is registered in
/// `BuiltInRegistries.CARVER`; the Rust port splits it into `WorldCarverId`
/// (the identity handle `ConfiguredWorldCarver` stores) and this behavior
/// trait (Java's virtual methods). The trait is generic over the config type
/// and its `is_start_chunk` is generic over the random source (`RandomSource`
/// is `Sized`, not object-safe), so it is *not* object-safe: the
/// `carver_is_start_chunk` match downcasts the erased config and calls the
/// concrete carver's `is_start_chunk`.
pub trait WorldCarverBehavior<C: CarverConfiguration>: Debug + Send + Sync + 'static {
    /// `WorldCarver.isStartChunk(C, RandomSource)` — the abstract behavior;
    /// every concrete carver implements it (`CaveWorldCarver`/`CanyonWorldCarver`
    /// both check `random.nextFloat() <= configuration.probability`).
    fn is_start_chunk<R: RandomSource>(&self, configuration: &C, random: &mut R) -> bool;

    /// `WorldCarver.getRange()` — `4` by default; the concrete cave/canyon
    /// carvers scale their tunnel distance off it.
    fn get_range(&self) -> i32 {
        4
    }
}

// ---------------------------------------------------------------------------
// Carver identity + the dispatch hub
// ---------------------------------------------------------------------------

/// `net.minecraft.core.Registry` element identity for `BuiltInRegistries.CARVER`
/// — the per-carver `u32` id (element id == holder id == network id ==
/// insertion index, OWNERSHIP.md §Registries) plus the registry-key location
/// (`register("cave", …)` → `minecraft:cave`). `ConfiguredWorldCarver` holds
/// this handle; the `#180` carver port assigns the ids from the `CAVE`/
/// `NETHER_CAVE`/`CANYON` registration order. Identity-semantic (not `Copy`),
/// mirroring `FeatureId`/`PlacementModifierTypeId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldCarverId {
    /// The per-carver `u32` identity (insertion index in the carver registry).
    pub id: u32,
    /// The registry-key location of the carver's registration (`register("cave",
    /// …)` → `minecraft:cave`).
    pub location: &'static str,
}

impl WorldCarverId {
    /// `new WorldCarverId(u32, location)` — a carver's registry identity.
    pub const fn new(id: u32, location: &'static str) -> WorldCarverId {
        WorldCarverId { id, location }
    }
}

/// Resolve a `WorldCarverId` + erased config to its start-chunk test — the
/// `ConfiguredWorldCarver.isStartChunk` dispatch (`this.worldCarver.
/// isStartChunk(this.config, random)`).
///
/// STUB(mc.world.level.levelgen.carver) — the concrete carver bindings
/// (`CaveWorldCarver`/`NetherWorldCarver`/`CanyonWorldCarver`, registered into
/// `BuiltInRegistries.CARVER`) land with the `#180` algorithm. Until then this
/// panics unconditionally — the pre-wire stand-in for the `#180` match, whose
/// unknown-id path throws `IllegalStateException` like Java's
/// `Registry.getValueOrThrow` (Java throws only when the key is genuinely
/// missing).
pub fn carver_is_start_chunk<R: RandomSource>(
    world_carver: WorldCarverId,
    _config: &dyn CarverConfiguration,
    _random: &mut R,
) -> bool {
    panic!(
        "Trying to check start chunk for world carver '{}' with no registered behavior (#180)",
        world_carver.location
    );
    // The remaining parameters (`_config`, `_random`) are unused only because
    // this stub panics before the `#180` match would downcast the erased
    // config and call the concrete carver's `is_start_chunk`; the `_` prefixes
    // keep them in the signature (so the stub shape matches the dispatch
    // exactly) without tripping `-Dwarnings`.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::carver::CarverConfiguration;

    /// A minimal configuration exercising the `CarverConfiguration` bound.
    #[derive(Debug)]
    struct TestCarverConfiguration;

    impl CarverConfiguration for TestCarverConfiguration {}

    /// A carver that keeps `getRange()`'s Java default of 4.
    #[derive(Debug)]
    struct DefaultRangeCarver;

    impl WorldCarverBehavior<TestCarverConfiguration> for DefaultRangeCarver {
        fn is_start_chunk<R: RandomSource>(
            &self,
            _configuration: &TestCarverConfiguration,
            _random: &mut R,
        ) -> bool {
            false
        }
    }

    /// A carver overriding `getRange()` (the Java base-class default is
    /// overridable — the concrete carvers rely on the default, but a subtype
    /// may change it).
    #[derive(Debug)]
    struct WideRangeCarver;

    impl WorldCarverBehavior<TestCarverConfiguration> for WideRangeCarver {
        fn is_start_chunk<R: RandomSource>(
            &self,
            _configuration: &TestCarverConfiguration,
            _random: &mut R,
        ) -> bool {
            false
        }

        fn get_range(&self) -> i32 {
            8
        }
    }

    #[test]
    fn get_range_defaults_to_four() {
        // Java `WorldCarver.getRange()` returns 4 (no concrete carver overrides
        // it — CaveWorldCarver/CanyonWorldCarver scale off the default).
        assert_eq!(DefaultRangeCarver.get_range(), 4);
    }

    #[test]
    fn get_range_is_overridable() {
        // Counterfactual: a subtype that overrides `getRange` reports its own
        // value, proving the default is not hard-wired into the dispatch.
        assert_eq!(WideRangeCarver.get_range(), 8);
    }

    #[test]
    fn id_carries_the_registry_location() {
        // `WorldCarver.register("cave", ...)` → element id 0 at
        // `minecraft:cave` (the `CAVE` constant's registration order).
        let cave = WorldCarverId::new(0, "minecraft:cave");
        assert_eq!(cave.id, 0);
        assert_eq!(cave.location, "minecraft:cave");
    }

    #[test]
    #[should_panic(expected = "Trying to check start chunk for world carver 'minecraft:cave'")]
    fn dispatch_stub_panics_on_unregistered_carver() {
        // The concrete carver bindings are not wired yet; dispatching panics
        // unconditionally (the pre-wire stand-in for the `#180` match, whose
        // unknown-id path throws like Java's `Registry.getValueOrThrow`).
        let carver = WorldCarverId::new(0, "minecraft:cave");
        let config = TestCarverConfiguration;
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let _ = carver_is_start_chunk(carver, &config, &mut random);
    }
}
