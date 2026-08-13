//! Port of `net.minecraft.world.level.dimension.LevelStem` (record, 26.2) — the
//! out-of-unit value shell `WorldDimensions` (this unit) stores.
//!
//! `LevelStem` is owned by the pending `mc.world.level.dimension` manifest unit
//! (RivetTodo #388); the settings wave needs the record value (the
//! `WorldDimensions` map's element), so the shape is shelled here and the real
//! record replaces it when that unit lands. The `type` field holds the
//! `DIMENSION_TYPE` registry element placeholder (the full `DimensionType`
//! record/codec defers with the dimension unit — this module only has the height
//! constants), and `generator` is the `Arc<dyn ChunkGenerator>` the two settings
//! realizations (`FlatLevelSource`/`DebugLevelSource`) implement.
//!
//! ### The `CODEC` seam
//!
//! `LevelStem.CODEC` reads `DimensionType.CODEC` (pending, #388) and
//! `ChunkGenerator.CODEC` (the generator dispatch codec, pending, #185). The
//! codec factory returns a poison codec that fails with a `DataResult::error`
//! naming both deferrals rather than fabricating a value — the same
//! capability-unavailable boundary the `WorldDimensions`/`WorldGenSettings`
//! codecs inherit until the owning units land.

use crate::chunk::chunk_generator::ChunkGenerator;
use rivet_registry::holder::Holder;
use rivet_registry::registries;
use rivet_registry::{Identifier, ResourceKey};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::decoder;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::encoder;
use std::fmt;
use std::sync::{Arc, LazyLock};

/// `LevelStem.OVERWORLD` — `ResourceKey.create(Registries.LEVEL_STEM,
/// Identifier.withDefaultNamespace("overworld"))`.
pub static OVERWORLD: LazyLock<ResourceKey<registries::LevelStem>> = LazyLock::new(|| {
    ResourceKey::create(
        &registries::LEVEL_STEM,
        Identifier::with_default_namespace("overworld"),
    )
});

/// `LevelStem.NETHER` — `...withDefaultNamespace("the_nether")`.
pub static NETHER: LazyLock<ResourceKey<registries::LevelStem>> = LazyLock::new(|| {
    ResourceKey::create(
        &registries::LEVEL_STEM,
        Identifier::with_default_namespace("the_nether"),
    )
});

/// `LevelStem.END` — `...withDefaultNamespace("the_end")`.
pub static END: LazyLock<ResourceKey<registries::LevelStem>> = LazyLock::new(|| {
    ResourceKey::create(
        &registries::LEVEL_STEM,
        Identifier::with_default_namespace("the_end"),
    )
});

/// `net.minecraft.world.level.dimension.LevelStem` — the dimension stem record.
#[derive(Clone)]
pub struct LevelStem {
    /// `type` — the `DIMENSION_TYPE` registry holder. `DimensionType` is the
    /// registry element placeholder until the `mc.world.level.dimension` unit
    /// lands (RivetTodo #388).
    pub ty: Holder<registries::DimensionType>,
    /// `generator` — the chunk generator.
    pub generator: Arc<dyn ChunkGenerator>,
}

impl LevelStem {
    /// The record constructor (the codec's `apply` function).
    pub fn new(ty: Holder<registries::DimensionType>, generator: Arc<dyn ChunkGenerator>) -> Self {
        LevelStem { ty, generator }
    }

    /// `type()`.
    pub fn ty(&self) -> &Holder<registries::DimensionType> {
        &self.ty
    }

    /// `generator()`.
    pub fn generator(&self) -> &Arc<dyn ChunkGenerator> {
        &self.generator
    }
}

/// `Debug` for the `LevelStem` shell — the `Arc<dyn ChunkGenerator>` behavior
/// field is not `Debug` (the trait has no `Debug` supertrait), so the generator
/// prints a placeholder (the `ty` holder is the debuggable half).
impl fmt::Debug for LevelStem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LevelStem")
            .field("ty", &self.ty)
            .field("generator", &"<dyn ChunkGenerator>")
            .finish()
    }
}

/// `LevelStem.CODEC` — the ops-generic `level_stem_codec::<Ops>()` factory.
///
/// The record codec reads `DimensionType.CODEC` (pending, #388) and
/// `ChunkGenerator.CODEC` (the generator dispatch codec, pending, #185); both
/// are unavailable, so the factory returns a poison codec that fails with a
/// `DataResult::error` naming the deferrals whenever an encode/decode reaches a
/// stem. The `WorldDimensions`/`WorldGenSettings` codecs are structured over
/// this leaf, so they inherit the boundary until the owning units land.
pub fn level_stem_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<LevelStem, Ops>> {
    let message = "LevelStem.CODEC is not implemented (RivetTodo #388/#185): needs DimensionType.CODEC (mc.world.level.dimension) and ChunkGenerator.CODEC (mc.world.level.chunk.generator)".to_string();
    codec::of(
        encoder::error::<LevelStem, Ops>(message.clone()),
        decoder::error::<LevelStem, Ops>(message.clone()),
        "LevelStem.CODEC (unavailable)".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_java() {
        assert_eq!(OVERWORLD.identifier().to_string(), "minecraft:overworld");
        assert_eq!(NETHER.identifier().to_string(), "minecraft:the_nether");
        assert_eq!(END.identifier().to_string(), "minecraft:the_end");
        for key in [&*OVERWORLD, &*NETHER, &*END] {
            assert!(key.is_for(&registries::LEVEL_STEM));
        }
    }

    #[test]
    fn codec_errors_with_the_deferral_message() {
        use rivet_serialization::json_ops::JsonOps;
        // The poison codec fails with a DataResult::error naming the deferrals.
        let codec = level_stem_codec::<JsonOps>();
        let source = crate::levelgen::settings::debug_level_source::DebugLevelSource::new(
            Holder::direct(rivet_registry::biome_id::BiomeId::from_id(40)),
        );
        let stem = LevelStem::new(Holder::direct(registries::DimensionType), Arc::new(source));
        let encoded = codec.encode_start(&JsonOps::INSTANCE, &stem);
        let error = encoded
            .error_ref()
            .expect("the LevelStem codec is a seam and must error");
        let message = error.message();
        assert!(
            message.contains("RivetTodo #388"),
            "the seam must name the #388 deferral, got: {message}"
        );
    }
}
