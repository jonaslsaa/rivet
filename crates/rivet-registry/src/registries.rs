//! Port of `net.minecraft.core.registries.Registries` (MC 26.2).
//!
//! PROVENANCE: leaf of the `mc.core` manifest unit (`net.minecraft.core.registries`
//! subpackage -> `rivet-registry` via the `net.minecraft.core` crate rule).
//! Java source: `net/minecraft/core/registries/Registries.java` (343 lines,
//! 26.2).
//!
//! Ownership A scope: the const registry keys (`ROOT_REGISTRY_NAME`,
//! `ROOT_REGISTRY`), the representative `BLOCK` key + placeholder element
//! types, and the four path helpers. The full ~140-key set is the `mc.core`
//! unit's responsibility; this module only declares what the SCC's own
//! integration tests and `ResourceKey::create_registry_key` need.
//!
//! The keys are `LazyLock` statics, not `const`: `Identifier` owns its
//! `String` fields (value type, faithful to Java), and `String::from` is not a
//! `const fn`, so no `Identifier` value can be a `const`. A `LazyLock` is
//! thread-safe and initializes once; every consumer derefs through the
//! `Sync`-wrapping static. `ResourceKey::create_registry_key` reads
//! `ROOT_REGISTRY_NAME` by value (it is `Copy`-free — it clones), so the
//! `static` form is required and correct.
//!
//! The path helpers mirror Java exactly:
//!
//! ```java
//! public static String elementsDirPath(ResourceKey<? extends Registry<?>> k) { return k.identifier().getPath(); }
//! public static String tagsDirPath(...)      { return "tags/" + k.identifier().getPath(); }
//! public static String componentsDirPath(...) { return "components/" + k.identifier().getPath(); }
//! ```
//!
//! `level_stem_to_level`/`level_to_level_stem` swap the identifier between the
//! `DIMENSION` and `LEVEL_STEM` registries (`"dimension"` both), matching
//! Java's `ResourceKey.create(DIMENSION, levelStem.identifier())` and
//! `ResourceKey.create(LEVEL_STEM, level.identifier())`.

use crate::Identifier;
use crate::ResourceKey;
use crate::registry::Registry;

use std::sync::LazyLock;

/// `Registries.ROOT_REGISTRY_NAME` = `Identifier.withDefaultNamespace("root")` —
/// the registry-name of every `RegistryKey` (`createRegistryKey` roots on it).
pub static ROOT_REGISTRY_NAME: LazyLock<Identifier> =
    LazyLock::new(|| Identifier::with_default_namespace("root"));

/// The `Block` registry key — `createRegistryKey("block")`. `generated::blocks::BlockId`
/// is the id space; `BlockType` is the element placeholder until the block
/// unit lands.
pub static BLOCK: LazyLock<ResourceKey<Registry<BlockType>>> =
    LazyLock::new(|| ResourceKey::create_registry_key(Identifier::with_default_namespace("block")));

/// `Registries.DIMENSION` — `createRegistryKey("dimension")`, the `Level`
/// registry key (world unit placeholder).
pub static DIMENSION: LazyLock<ResourceKey<Registry<Level>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("dimension"))
});

/// `Registries.LEVEL_STEM` — `createRegistryKey("dimension")`, the `LevelStem`
/// registry key (world unit placeholder). Java gives DIMENSION and LEVEL_STEM
/// the SAME identifier `"dimension"`.
pub static LEVEL_STEM: LazyLock<ResourceKey<Registry<LevelStem>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("dimension"))
});

/// `Registries.DIMENSION_TYPE` — `createRegistryKey("dimension_type")`, the
/// `DimensionType` registry key (world unit placeholder). The #108 spawn-info
/// codecs resolve this key's holder wire form (`DimensionType.STREAM_CODEC` =
/// `ByteBufCodecs.holderRegistry(Registries.DIMENSION_TYPE)`); the full
/// `DimensionType` record value is the deferred `mc.world.level.dimension`
/// unit in `rivet-world`.
///
/// Reachability note for that future unit: `rivet-protocol` consumes this key
/// and `rivet-world → rivet-protocol` already exists, so the real
/// `DimensionType` in `rivet-world` can never be referenced from
/// `rivet-protocol` (that would be a cycle). This placeholder must remain a
/// distinct wire-marker type here, or `CommonPlayerSpawnInfo` must move to a
/// higher crate — the world unit cannot simply take over the type.
pub static DIMENSION_TYPE: LazyLock<ResourceKey<Registry<DimensionType>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("dimension_type"))
});

/// The `Block` registry element — a placeholder for
/// `net.minecraft.world.level.block.Block` (owned by the world/block unit, not
/// #124).
#[derive(Debug)]
pub struct BlockType;

/// `Registries.levelStemToLevel(ResourceKey<LevelStem>)`.
pub fn level_stem_to_level(level_stem: &ResourceKey<LevelStem>) -> ResourceKey<Level> {
    ResourceKey::create(&DIMENSION, level_stem.identifier().clone())
}

/// `Registries.levelToLevelStem(ResourceKey<Level>)`.
pub fn level_to_level_stem(level: &ResourceKey<Level>) -> ResourceKey<LevelStem> {
    ResourceKey::create(&LEVEL_STEM, level.identifier().clone())
}

/// `Registries.elementsDirPath(ResourceKey)` — `registryKey.identifier().path()`.
pub fn elements_dir_path(registry_key: &ResourceKey<Registry<()>>) -> String {
    registry_key.identifier().path().to_string()
}

/// `Registries.tagsDirPath(ResourceKey)` — `"tags/" + path`.
pub fn tags_dir_path(registry_key: &ResourceKey<Registry<()>>) -> String {
    format!("tags/{}", registry_key.identifier().path())
}

/// `Registries.componentsDirPath(ResourceKey)` — `"components/" + path`.
pub fn components_dir_path(registry_key: &ResourceKey<Registry<()>>) -> String {
    format!("components/{}", registry_key.identifier().path())
}

/// `net.minecraft.world.level.Level` (world unit placeholder).
#[derive(Debug)]
pub struct Level;
/// `net.minecraft.world.level.dimension.LevelStem` (world unit placeholder).
#[derive(Debug)]
pub struct LevelStem;
/// `net.minecraft.world.level.dimension.DimensionType` (world unit placeholder).
///
/// Only the wire key is owned here (#108): the value type's record shape,
/// constants, validating constructor, and `DIRECT_CODEC`/`NETWORK_CODEC` are
/// the deferred `mc.world.level.dimension` unit in `rivet-world`. The value
/// derives are what `Holder<T>` needs for its own derives (the holder codec
/// tests compare `Holder::Reference` values).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DimensionType;

/// `Registries.registryDirPath` (private in Java) — the registry key's path.
pub fn registry_dir_path(registry_key: &ResourceKey<Registry<()>>) -> String {
    registry_key.identifier().path().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_key() -> ResourceKey<Registry<()>> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("block"))
    }

    #[test]
    fn root_registry_name_is_minecraft_root() {
        assert_eq!(ROOT_REGISTRY_NAME.to_string(), "minecraft:root");
        assert_eq!(ROOT_REGISTRY_NAME.namespace(), "minecraft");
        assert_eq!(ROOT_REGISTRY_NAME.path(), "root");
    }

    #[test]
    fn block_key_is_blocks_registry_key() {
        assert_eq!(
            BLOCK.registry(),
            &Identifier::with_default_namespace("root")
        );
        assert_eq!(
            BLOCK.identifier(),
            &Identifier::with_default_namespace("block")
        );
    }

    #[test]
    fn dir_path_helpers() {
        let key = block_key();
        assert_eq!(elements_dir_path(&key), "block");
        assert_eq!(tags_dir_path(&key), "tags/block");
        assert_eq!(components_dir_path(&key), "components/block");
        assert_eq!(registry_dir_path(&key), "block");
    }

    #[test]
    fn dimension_type_key_is_dimension_type() {
        // Java: `Registries.DIMENSION_TYPE = createRegistryKey("dimension_type")`.
        assert_eq!(
            DIMENSION_TYPE.identifier(),
            &Identifier::with_default_namespace("dimension_type")
        );
        assert_eq!(DIMENSION_TYPE.registry(), &*ROOT_REGISTRY_NAME);
    }

    #[test]
    fn dimension_keys_share_the_dimension_identifier() {
        // Java: DIMENSION and LEVEL_STEM are BOTH `createRegistryKey("dimension")`.
        let dimension = DIMENSION.identifier().clone();
        let level_stem = LEVEL_STEM.identifier().clone();
        assert_eq!(dimension, level_stem);
        assert_eq!(dimension.to_string(), "minecraft:dimension");
    }

    #[test]
    fn level_stem_to_level_and_back() {
        // `levelStemToLevel` moves the identifier into the DIMENSION registry.
        let stem = ResourceKey::create(
            &ResourceKey::create_registry_key(Identifier::with_default_namespace("dimension")),
            Identifier::with_default_namespace("overworld"),
        );
        let level = level_stem_to_level(&stem);
        assert_eq!(
            level.registry(),
            &Identifier::with_default_namespace("dimension")
        );
        assert_eq!(
            level.identifier(),
            &Identifier::with_default_namespace("overworld")
        );

        let back = level_to_level_stem(&level);
        assert_eq!(
            back.registry(),
            &Identifier::with_default_namespace("dimension")
        );
        assert_eq!(
            back.identifier(),
            &Identifier::with_default_namespace("overworld")
        );
    }

    #[test]
    fn keys_are_stable_singletons() {
        // Two derefs of the same `LazyLock` yield value-equal keys (and the
        // static is a single allocation — pointer-stable).
        assert_eq!(*BLOCK, *BLOCK);
        assert_eq!(BLOCK.registry(), BLOCK.registry());
        // `Eq`/`Clone` on these keys compile with no element-type bound.
        let _ = (*BLOCK).clone();
    }
}
