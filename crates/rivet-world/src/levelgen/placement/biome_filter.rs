//! Port of `net.minecraft.world.level.levelgen.placement.BiomeFilter`
//! (class, 26.2).
//!
//! Java: a singleton (`INSTANCE`) `PlacementFilter` whose `shouldPlace` keeps
//! the origin when the enclosing `PlacedFeature` is in the biome's feature set
//! — `context.generator().getBiomeGenerationSettings(
//! context.getLevel().getBiome(origin)).hasFeature(context.topFeature()
//! .orElseThrow(...))` — and whose `type()` is
//! `PlacementModifierType.BIOME_FILTER`. Its `CODEC` is `MapCodec.unit(
//! INSTANCE)` — encodes to `{}` and always decodes to the singleton.
//!
//! The `shouldPlace` guard (`topFeature().orElseThrow(...)`, message exact)
//! and the biome read (`WorldGenLevel::get_biome`, the `#399` seam) are ported
//! faithfully; the final membership read — `getBiomeGenerationSettings(biome)
//! .hasFeature(feature)` — routes through the `ChunkGenerator` trait-default
//! seam (`get_biome_generation_settings_has_feature`), which fails explicitly
//! until the biome-value registry (`mc.data.worldgen.biome`) lands — never
//! fabricating a biome-membership result. Because the seam is a trait default,
//! a generator that does provide the surface keeps `shouldPlace` executable:
//! the filter behaves as a filter (a real boolean predicate) rather than
//! aborting the process.
//!
//! `BiomeFilter` is a Java singleton; `Clone` yields the same always-instance
//! filter.

use crate::levelgen::placement::placement_modifier_type::{
    PlacementModifierTypeId, PlacementModifierTypes,
};
use crate::levelgen::placement::{PlacementContext, PlacementFilter};
use rivet_registry::core::BlockPos;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.BiomeFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiomeFilter;

impl BiomeFilter {
    /// `BiomeFilter.INSTANCE`.
    pub const INSTANCE: BiomeFilter = BiomeFilter;

    /// `biome()` — the public factory.
    pub fn biome() -> Self {
        BiomeFilter
    }
}

impl PlacementFilter for BiomeFilter {
    fn should_place<R: RandomSource>(
        &self,
        context: &mut PlacementContext,
        _random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        // `context.topFeature().orElseThrow(() -> new IllegalStateException(
        // "Tried to biome check an unregistered feature, or a feature that
        // should not restrict the biome"))` — the Java message is exact.
        let feature = context.top_feature().unwrap_or_else(|| {
            panic!(
                "Tried to biome check an unregistered feature, or a feature that should not restrict the biome"
            )
        });
        // `context.getLevel().getBiome(origin)` — the `#399` biome-read seam.
        let biome = context.get_level().get_biome(origin);
        // `context.generator().getBiomeGenerationSettings(biome).hasFeature(feature)`
        // — routed through the `ChunkGenerator` trait-default seam: it fails
        // explicitly until the biome-value registry (`mc.data.worldgen.biome`)
        // lands, never fabricating a biome-membership result, but a generator
        // that provides the surface keeps `shouldPlace` a real predicate.
        context
            .generator()
            .get_biome_generation_settings_has_feature(&biome, feature)
    }

    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementModifierTypes::BIOME_FILTER
    }
}

/// `BiomeFilter.CODEC` — `MapCodec.unit(INSTANCE)`, as the ops-generic
/// `biome_filter_map_codec::<Ops>()` factory. Encodes to `{}` and always
/// decodes to the singleton.
pub fn biome_filter_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<BiomeFilter, Ops>> {
    map_codec::unit_with(Arc::new(|| BiomeFilter::INSTANCE))
}

/// `BiomeFilter.CODEC` as a `Codec` (`MapCodec.codec()` — `unit(...).codec()`),
/// the shape the `#181` generated dispatch's registration table consumes.
#[allow(dead_code)]
pub fn biome_filter_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn rivet_serialization::codec::Codec<BiomeFilter, Ops>> {
    map_codec::codec_of(biome_filter_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use crate::levelgen::placement::PlacedFeature;
    use rivet_registry::biome_id::BiomeId;
    use rivet_registry::holder::Holder;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// A minimal `WorldGenLevel` double over the overworld window; its
    /// `get_biome` returns a plains reference (the `#399` seam, used to reach
    /// the `#178`-deferred membership check).
    #[derive(Clone, Copy)]
    struct TestLevel {
        accessor: SimpleLevelHeightAccessor,
    }

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.accessor.get_height()
        }

        fn get_min_y(&self) -> i32 {
            self.accessor.get_min_y()
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }

        fn get_biome(&self, _pos: &BlockPos) -> Holder<BiomeId> {
            // The registry id is irrelevant here — the membership check is the
            // `#178`-deferred surface, so this value is never consumed.
            Holder::reference(rivet_registry::holder::RegistryId(0), 0)
        }
    }

    struct NoopGenerator;

    impl crate::chunk::ChunkGenerator for NoopGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    /// A generator whose biome-membership seam answers a fixed boolean — the
    /// `#178` surface provided, so the filter body is executable.
    struct AnsweringGenerator {
        member: bool,
    }

    impl crate::chunk::ChunkGenerator for AnsweringGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }

        fn get_biome_generation_settings_has_feature(
            &self,
            _biome: &Holder<BiomeId>,
            _feature: &PlacedFeature,
        ) -> bool {
            self.member
        }
    }

    #[test]
    fn biome_filter_type_identity_is_reported() {
        // `PlacementModifierType.BIOME_FILTER` is insertion index 4 in
        // `PlacementModifierType.java`'s registration order (the `"biome"`
        // key).
        let filter = BiomeFilter::biome();
        assert_eq!(
            PlacementFilter::type_id(&filter),
            PlacementModifierTypes::BIOME_FILTER
        );
    }

    #[test]
    fn should_place_throws_when_no_top_feature() {
        // `context.topFeature().orElseThrow(IllegalStateException(...))` — the
        // unregistered-feature guard fires first, before any world access.
        let mut level = TestLevel {
            accessor: create(-64, 384),
        };
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        let origin = BlockPos::new(0, 0, 0);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PlacementFilter::should_place(
                &BiomeFilter::INSTANCE,
                &mut context,
                &mut random,
                &origin,
            )
        }));
        assert!(
            result.is_err(),
            "orElseThrow must fire without a top feature"
        );
    }

    /// A minimal `ConfiguredFeatureErased` to stand in as the enclosing
    /// `PlacedFeature`'s feature — the membership seam ignores it, so only the
    /// holder needs to type-check.
    fn stub_feature() -> crate::levelgen::feature::ConfiguredFeatureErased {
        crate::levelgen::feature::ConfiguredFeatureErased {
            feature: crate::levelgen::feature::FeatureId::new(0),
            config: std::sync::Arc::new(
                crate::levelgen::feature::configurations::NoneFeatureConfiguration,
            ),
        }
    }

    #[test]
    fn should_place_keeps_origin_when_biome_membership_passes() {
        // `generator.getBiomeGenerationSettings(biome).hasFeature(feature)` —
        // with the membership seam answering `true`, the filter keeps the
        // origin (the `#178` surface provided, so `should_place` is a real
        // predicate, not an abort).
        let top_feature = PlacedFeature::new(
            rivet_registry::holder::Holder::direct(stub_feature()),
            Vec::new(),
        );
        let mut level = TestLevel {
            accessor: create(-64, 384),
        };
        let generator = AnsweringGenerator { member: true };
        let mut context = PlacementContext::new(&mut level, &generator, Some(&top_feature));
        let origin = BlockPos::new(0, 0, 0);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let result = PlacementFilter::should_place(
            &BiomeFilter::INSTANCE,
            &mut context,
            &mut random,
            &origin,
        );
        assert!(result, "member biome must keep the origin");
    }

    #[test]
    fn should_place_drops_origin_when_biome_membership_fails() {
        // The membership seam answering `false` drops the origin — the
        // non-member path of the exact Java predicate.
        let top_feature = PlacedFeature::new(
            rivet_registry::holder::Holder::direct(stub_feature()),
            Vec::new(),
        );
        let mut level = TestLevel {
            accessor: create(-64, 384),
        };
        let generator = AnsweringGenerator { member: false };
        let mut context = PlacementContext::new(&mut level, &generator, Some(&top_feature));
        let origin = BlockPos::new(0, 0, 0);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let result = PlacementFilter::should_place(
            &BiomeFilter::INSTANCE,
            &mut context,
            &mut random,
            &origin,
        );
        assert!(!result, "non-member biome must drop the origin");
    }

    #[test]
    fn codec_encodes_empty_map_and_decodes_singleton() {
        // `MapCodec.unit(INSTANCE)` — encodes to `{}`, always decodes to the
        // singleton.
        let ops = JsonOps::INSTANCE;
        let codec = biome_filter_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&ops, &BiomeFilter::INSTANCE)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .copied()
            .expect("decode should succeed");
        assert_eq!(decoded, BiomeFilter::INSTANCE);
        // A unit codec also decodes an arbitrary map to the singleton.
        let decoded_other = codec
            .parse(&ops, &json!({"anything": 1}))
            .result()
            .copied()
            .expect("unit codec always decodes");
        assert_eq!(decoded_other, BiomeFilter::INSTANCE);
    }
}
