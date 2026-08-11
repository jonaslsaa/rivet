//! `net.minecraft.world.level.Level` — the abstract world class.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! Level.java` (2205 lines). The #232 value slice ports only the small
//! non-ticking value surface that anchors `getGameTime`: the dimension keys,
//! the `RESOURCE_KEY_CODEC` (wired on `Registries.DIMENSION`, #515), and
//! world-size constants plus the `dimension()` accessor. Everything else
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
//! `LazyLock` in `rivet-registry`). `dimension()` returns a `&ResourceKey`
//! mirroring the field-backed Java accessor.

use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_registry::registries::{self, Level as LevelKey};
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use std::sync::Arc;

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

/// `Level.RESOURCE_KEY_CODEC` — `ResourceKey.codec(Registries.DIMENSION)`.
///
/// Delegates to the generic `resource_key_codec` in `rivet-registry` (the
/// `ResourceKey.codec` port) wired over the `Registries.DIMENSION` registry
/// key — exactly the `static final` Java constant. Exposed as the ops-generic
/// `resource_key_codec::<Ops>()` factory. `GlobalPos.MAP_CODEC` keeps its
/// inline `resource_key_codec(Registries.DIMENSION)` composition in
/// `rivet-registry` (crate layering); this is the `level`-crate surface
/// `PrimaryLevelData.parse` reads `paperSpawnDimension` through.
pub fn resource_key_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ResourceKey<LevelKey>, Ops>>
where
    LevelKey: 'static,
{
    rivet_registry::resource_key::resource_key_codec::<LevelKey, Ops>(&*registries::DIMENSION)
}

/// `net.minecraft.world.level.Level` — the abstract world.
///
/// RivetTodo(#232): the `Level` instance surface (block mutation, tick,
/// weather/time, entities, redstone, the `AutoCloseable` and moonrise
/// chunk-system interfaces, `DEFAULT_EXPLOSION_BLOCK_PARTICLES`) defers with
/// the full `mc.world.level` unit. The concrete field-backed `Level`
/// implementations of the getters already declared on the chain
/// (`getSkyDarken`/`isClientSide` on `LevelReader`, `getLevelData` on
/// `LevelAccessor`) defer with the concrete world (`ServerLevel`).
/// `Level.dimension()`, the constants, and `RESOURCE_KEY_CODEC` are the value
/// slice.
pub trait Level: LevelAccessor {
    /// `Level.dimension()` — the `ResourceKey<Level>` dimension.
    fn dimension(&self) -> &ResourceKey<LevelKey>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::nbt_ops::NbtOps;
    use rivet_serialization::Dynamic;

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

    /// Decode the real 26.2 `level.dat` fixture's `paperSpawnDimension`
    /// through `Level.RESOURCE_KEY_CODEC` end-to-end.
    #[test]
    fn resource_key_codec_decodes_real_fixture_paper_spawn_dimension() {
        let path = workspace_root().join("tools/rivet-oracle/fixtures/level.dat");
        assert!(
            path.is_file(),
            "fixture {path:?} is missing — the committed 26.2 level.dat is git-tracked, so a missing fixture means this end-to-end codec test silently stopped exercising the codec"
        );
        let bytes = std::fs::read(&path).expect("level.dat readable");
        let tag = rivet_nbt::nbt_io::read_compressed(
            &bytes[..],
            &mut rivet_nbt::nbt_accounter::NbtAccounter::unlimited_heap(),
        )
        .expect("read_compressed must read Paper's gzip level.dat");
        let data = tag
            .get_compound("Data")
            .expect("level.dat must carry a Data compound");
        let ops = NbtOps::instance();
        let dynamic = Dynamic::new(&ops, rivet_nbt::tag::Tag::Compound(data.clone()));
        // Paper reads `paperSpawnDimension` through `Level.RESOURCE_KEY_CODEC`
        // (PrimaryLevelData.parse); the fixture records minecraft:overworld.
        let dimension = dynamic
            .get(&ops, "paperSpawnDimension")
            .decode(&ops, &*resource_key_codec::<NbtOps>())
            .result()
            .expect("paperSpawnDimension decode must succeed")
            .0
            .clone();
        assert_eq!(dimension, overworld());
    }

    /// `Level.RESOURCE_KEY_CODEC` round-trips the DIMENSION registry key
    /// through `NbtOps`, binding to `Registries.DIMENSION`.
    #[test]
    fn resource_key_codec_round_trips_dimension_registry_key() {
        let ops = NbtOps::instance();
        let codec = resource_key_codec::<NbtOps>();
        // Encode: `Identifier.CODEC` writes the string form.
        let encoded = codec
            .encode_start(&ops, &overworld())
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            ops.create_string("minecraft:overworld".to_string())
        );
        // Decode back to the DIMENSION-wired `ResourceKey<Level>`.
        let input = ops.create_string("minecraft:overworld".to_string());
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0, overworld());
        assert_eq!(decoded.0.registry(), registries::DIMENSION.identifier());
    }

    /// An invalid resource location errors through `Level.RESOURCE_KEY_CODEC`
    /// (`Identifier.CODEC`'s `Identifier::read`).
    #[test]
    fn resource_key_codec_rejects_invalid_dimension() {
        let ops = NbtOps::instance();
        let codec = resource_key_codec::<NbtOps>();
        let input = ops.create_string("a b:c".to_string());
        assert!(codec.decode(&ops, &input).result().is_none());
    }

    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    }
}
