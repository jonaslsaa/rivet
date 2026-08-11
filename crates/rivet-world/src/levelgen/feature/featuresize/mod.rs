//! `net.minecraft.world.level.levelgen.feature.featuresize` — the feature-size
//! value layer (issue #391).
//!
//! PROVENANCE: the `mc.world.level.levelgen.feature.featuresize` manifest unit —
//! `FeatureSize.java`, `FeatureSizeType.java`, `TwoLayersFeatureSize.java`,
//! `ThreeLayersFeatureSize.java`, `package-info.java` (26.2). The unit was
//! DEFERRED on `FeatureSize.CODEC`'s dependency on
//! `BuiltInRegistries.FEATURE_SIZE_TYPE.byNameCodec()`, which landed as the
//! #394 by-name-codec slice (`rivet-registry::feature_size_type`); this wave
//! ports the full value hierarchy on top of it.
//!
//! The dispatch mirrors the `Feature`/`BlockPredicate`/`BlockState` identity
//! split: [`feature_size::FeatureSize`] is the object-safe behavior contract,
//! its registry identity is the erased
//! `rivet_registry::feature_size_type::FeatureSizeTypeId`, and the erased
//! carrier `Arc<dyn FeatureSize>` is what `FeatureSize.CODEC` (de)serializes
//! through the `"type"` by-name dispatch. Both vanilla feature-size types are
//! ported, so the dispatch table is total over the generated two-entry
//! registry — declaration-order codec dispatch with no fabricated fallback.

pub mod feature_size;
pub mod three_layers_feature_size;
pub mod two_layers_feature_size;

pub use feature_size::{
    FeatureSize, MAX_WIDTH, feature_size_codec, feature_size_type_by_name_codec,
};
pub use three_layers_feature_size::ThreeLayersFeatureSize;
pub use two_layers_feature_size::TwoLayersFeatureSize;
