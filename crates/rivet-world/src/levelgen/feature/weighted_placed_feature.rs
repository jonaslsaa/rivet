//! Port of `net.minecraft.world.level.levelgen.feature.WeightedPlacedFeature`
//! (record, 26.2).
//!
//! Java: a two-field record `record WeightedPlacedFeature(Holder<PlacedFeature>
//! feature, float chance)` — the `RandomFeatureConfiguration` list element. Its
//! `CODEC` is a `RecordCodecBuilder` over the required `"feature"` field
//! (`PlacedFeature.CODEC` — a `RegistryFileCodec` over
//! `Registries.PLACED_FEATURE`) and the required `"chance"` field
//! (`Codec.floatRange(0.0F, 1.0F)`). Its `place` delegates to the wrapped
//! placed feature. It is owned by the `mc.world.level.levelgen.feature.selector`
//! manifest unit (see MANIFEST.tsv) and ported here (the vegetation-family
//! wave, issue #600).
//!
//! `place` needs the lookups `Holder.value` and `PlacedFeature.place` resolve
//! their holders through — the placed-feature registry for `this.feature` and
//! the configured-feature registry for the resolved `PlacedFeature`'s holder —
//! which the concrete selector features thread from
//! `WorldGenLevel::registry_access`.

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::ConfiguredFeatureErased;
use crate::levelgen::feature::configurations::vegetation_patch_configuration::placed_feature_codec;
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::Holder;
use rivet_registry::core::BlockPos;
use rivet_registry::holder_lookup::HolderLookup;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.WeightedPlacedFeature` — a
/// placed feature paired with its selection chance.
#[derive(Debug, Clone)]
pub struct WeightedPlacedFeature {
    /// `feature` — `Holder<PlacedFeature>`.
    pub feature: Holder<PlacedFeature>,
    /// `chance` — `[0.0, 1.0]`.
    pub chance: f32,
}

impl WeightedPlacedFeature {
    /// `new WeightedPlacedFeature(Holder<PlacedFeature>, float)` — the record
    /// constructor (the codec's `apply` function).
    pub fn new(feature: Holder<PlacedFeature>, chance: f32) -> Self {
        WeightedPlacedFeature { feature, chance }
    }

    /// `WeightedPlacedFeature.feature()` — the accessor.
    pub fn feature(&self) -> &Holder<PlacedFeature> {
        &self.feature
    }

    /// `WeightedPlacedFeature.chance()` — the accessor.
    pub fn chance(&self) -> f32 {
        self.chance
    }

    /// `WeightedPlacedFeature.place(WorldGenLevel, ChunkGenerator, RandomSource,
    /// BlockPos)` — `this.feature.value().place(...)`. The `placed_lookup`
    /// resolves `this.feature`; `configured_lookup` is threaded to the resolved
    /// `PlacedFeature.place` (the Java holder stores its value; the Rust
    /// `Reference` resolves by id — the back-reference rule).
    pub fn place<R: RandomSource>(
        &self,
        placed_lookup: &dyn HolderLookup<PlacedFeature>,
        configured_lookup: &dyn HolderLookup<ConfiguredFeatureErased>,
        level: &mut dyn WorldGenLevel,
        chunk_generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        self.feature.value(placed_lookup).place(
            configured_lookup,
            level,
            chunk_generator,
            random,
            origin,
        )
    }
}

/// `WeightedPlacedFeature.CODEC` — a record codec over the required
/// `"feature"` field (`PlacedFeature.CODEC`) and the required `"chance"` field
/// (`Codec.floatRange(0.0F, 1.0F)`), as the ops-generic
/// `weighted_placed_feature_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     PlacedFeature.CODEC.fieldOf("feature").forGetter(f -> f.feature),
///     Codec.floatRange(0.0F, 1.0F).fieldOf("chance").forGetter(f -> f.chance))
///     .apply(i, WeightedPlacedFeature::new))
/// ```
pub fn weighted_placed_feature_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<WeightedPlacedFeature, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|f: &WeightedPlacedFeature| f.feature.clone()),
                codec::field_of(placed_feature_codec::<Ops>(), "feature".to_string()),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|f: &WeightedPlacedFeature| f.chance),
                "chance".to_string(),
                codec::float_range::<Ops>(0.0, 1.0),
            ))
            .apply(instance, Arc::new(WeightedPlacedFeature::new))
    })
}
