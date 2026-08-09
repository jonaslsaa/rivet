//! Port of `net.minecraft.world.level.levelgen.feature.configurations.FeatureConfiguration`
//! (interface, 26.2).
//!
//! Java: the interface declares the constant `NoneFeatureConfiguration NONE =
//! NoneFeatureConfiguration.INSTANCE` and a single `default
//! Stream<Holder<ConfiguredFeature<?, ?>>> getSubFeatures()` that returns an
//! empty stream. The `ConfiguredFeature` record (the stream's element) is owned
//! by the `mc.world.level.levelgen.feature.core` unit, so the Rust trait uses
//! the erased wildcard alias `ConfiguredFeatureErased` (Java's
//! `ConfiguredFeature<?, ?>`) as the `Holder` element type. A Java interface
//! constant is `static final` and shared; the trait itself is object-free
//! (every implementation is a value struct), so the shared `NONE` instance is
//! a plain `const` re-export.

use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use rivet_registry::Holder;
use std::fmt::Debug;

/// `net.minecraft.world.level.levelgen.feature.configurations.FeatureConfiguration`.
///
/// Implemented by every feature configuration value type. The `getSubFeatures`
/// default returns an empty stream; configuration types that reference other
/// configured features (`RandomFeatureConfiguration`,
/// `WeightedRandomFeatureConfiguration`, `CompositeFeatureConfiguration`,
/// `RandomBooleanFeatureConfiguration`) override it. The return type is a boxed
/// lazy iterator (`Box<dyn Iterator<...>>`), preserving Java's lazy `Stream`
/// semantics for overriders: sub-features are produced on demand and can be
/// short-circuited, never materialized eagerly.
pub trait FeatureConfiguration: Debug + Send + Sync + 'static {
    /// `getSubFeatures()` — the default returns an empty stream.
    ///
    /// The element type is the erased wildcard `ConfiguredFeatureErased`
    /// (`ConfiguredFeature<?, ?>` in Java). The boxed iterator mirrors Java's
    /// lazy `Stream`: overriders return an iterator chain that yields
    /// sub-features on demand. It is boxed so the method stays object-safe
    /// (configurations are held as `Arc<dyn FeatureConfiguration>`).
    fn get_sub_features(
        &self,
    ) -> Box<dyn Iterator<Item = Holder<crate::levelgen::feature::ConfiguredFeatureErased>> + '_>
    {
        Box::new(std::iter::empty())
    }
}

/// `FeatureConfiguration.NONE` — `NoneFeatureConfiguration.INSTANCE`.
pub const NONE: NoneFeatureConfiguration = NoneFeatureConfiguration::INSTANCE;

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal configuration exercising the default `getSubFeatures` (empty).
    #[derive(Debug)]
    struct LeafConfiguration;

    impl FeatureConfiguration for LeafConfiguration {}

    #[test]
    fn default_get_sub_features_is_empty() {
        // Java's default `getSubFeatures()` returns `Stream.empty()`.
        assert!(LeafConfiguration.get_sub_features().next().is_none());
    }

    #[test]
    fn none_constant_is_the_singleton() {
        assert_eq!(NONE, NoneFeatureConfiguration::INSTANCE);
    }
}
