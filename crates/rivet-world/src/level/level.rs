//! `net.minecraft.world.level.Level` — the abstract world class.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! Level.java` (2205 lines). The #232 value slice ports only the small
//! non-ticking value surface that anchors `getGameTime`: the dimension keys
//! and world-size constants plus the `dimension()` accessor. Everything else
//! — `getBlockState`/block mutation, tick, weather, entities, redstone, the
//! moonrise collision rewrites — defers with the full `mc.world.level` unit.
//!
//! The Java `Level` is an abstract class (`implements LevelAccessor`); per
//! PORTING.md, an abstract class becomes an embedded struct + trait with
//! required methods. This slice declares the `Level` trait on the
//! `LevelAccessor` chain; the concrete struct is the `ServerLevel` slice in
//! `rivet-server` (the `mc.server.level` residual unit) which will `impl Level`.
//!
//! The `Level.OVERWORLD`/`NETHER`/`END` `ResourceKey<Level>` constants are not
//! `const`: `ResourceKey` owns an `Identifier` (a `String`), so Java's static
//! finals become `fn` accessors (the `registries::DIMENSION` key is a
//! `LazyLock` in `rivet-registry`).

use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_registry::registries::{self, Level as LevelKey};

use super::level_accessor::LevelAccessor;

/// `Level.MAX_LEVEL_SIZE` — the world's horizontal half-extent.
pub const MAX_LEVEL_SIZE: i32 = 30000000;
/// `Level.ACROSS_THE_WHOLE_WORLD` — the world's full width (`2 * MAX_LEVEL_SIZE`).
pub const ACROSS_THE_WHOLE_WORLD: i32 = 60000000;
/// `Level.LONG_PARTICLE_CLIP_RANGE`.
pub const LONG_PARTICLE_CLIP_RANGE: i32 = 512;
/// `Level.SHORT_PARTICLE_CLIP_RANGE`.
pub const SHORT_PARTICLE_CLIP_RANGE: i32 = 32;
/// `Level.MAX_BRIGHTNESS`.
pub const MAX_BRIGHTNESS: i32 = 15;
/// `Level.MAX_ENTITY_SPAWN_Y`.
pub const MAX_ENTITY_SPAWN_Y: i32 = 20000000;
/// `Level.MIN_ENTITY_SPAWN_Y`.
pub const MIN_ENTITY_SPAWN_Y: i32 = -20000000;

/// `Level.OVERWORLD` — `ResourceKey.create(Registries.DIMENSION,
/// Identifier.withDefaultNamespace("overworld"))`.
pub fn overworld() -> ResourceKey<LevelKey> {
    ResourceKey::create(
        &*registries::DIMENSION,
        Identifier::with_default_namespace("overworld"),
    )
}

/// `Level.NETHER` — `...Identifier.withDefaultNamespace("the_nether")`.
pub fn nether() -> ResourceKey<LevelKey> {
    ResourceKey::create(
        &*registries::DIMENSION,
        Identifier::with_default_namespace("the_nether"),
    )
}

/// `Level.END` — `...Identifier.withDefaultNamespace("the_end")`.
pub fn end() -> ResourceKey<LevelKey> {
    ResourceKey::create(
        &*registries::DIMENSION,
        Identifier::with_default_namespace("the_end"),
    )
}

/// `net.minecraft.world.level.Level` — the abstract world.
///
/// RivetTodo(#232): the `Level` instance surface (block mutation, tick,
/// weather/time, entities, redstone, the `AutoCloseable` and moonrise
/// chunk-system interfaces, `RESOURCE_KEY_CODEC`,
/// `DEFAULT_EXPLOSION_BLOCK_PARTICLES`) defers with the full `mc.world.level`
/// unit. The concrete field-backed `Level` implementations of the getters
/// already declared on the chain (`getSkyDarken`/`isClientSide` on
/// `LevelReader`, `getLevelData` on `LevelAccessor`) defer with the concrete
/// world (`ServerLevel`). `Level.dimension()` and the constants are the value
/// slice.
pub trait Level: LevelAccessor {
    /// `Level.dimension()` — the `ResourceKey<Level>` dimension.
    fn dimension(&self) -> &ResourceKey<LevelKey>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_keys_match_java() {
        // The three canonical dimension keys.
        assert_eq!(overworld().identifier().to_string(), "minecraft:overworld");
        assert_eq!(nether().identifier().to_string(), "minecraft:the_nether");
        assert_eq!(end().identifier().to_string(), "minecraft:the_end");
    }

    #[test]
    fn world_size_constants() {
        assert_eq!(MAX_LEVEL_SIZE, 30000000);
        assert_eq!(ACROSS_THE_WHOLE_WORLD, 60000000);
        assert_eq!(LONG_PARTICLE_CLIP_RANGE, 512);
        assert_eq!(SHORT_PARTICLE_CLIP_RANGE, 32);
        assert_eq!(MAX_BRIGHTNESS, 15);
        assert_eq!(MAX_ENTITY_SPAWN_Y, 20000000);
        assert_eq!(MIN_ENTITY_SPAWN_Y, -20000000);
    }
}
