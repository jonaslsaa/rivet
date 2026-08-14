//! STUB(mc.world.level.levelgen.feature.selector) — the
//! `getSubFeatures` flattening helper the selector/composite configurations
//! share.
//!
//! Java's `PlacedFeature.getFeatures()` is `Stream.concat(Stream.of(this.feature),
//! this.feature.value().getSubFeatures())` — a lazy stream of the configured
//! feature holders a placed feature's subtree references. The Rust port's
//! `Holder` is a pure `(RegistryId, id)` pair (the back-reference rule), so
//! resolving a `Reference` holder needs the owning `HolderLookup` — but the
//! `FeatureConfiguration::get_sub_features` trait surface (owned by
//! `feature.core`) cannot thread one.
//!
//! This helper covers the case the port can resolve lookup-free — a `Direct`
//! holder, whose value is inline — and fails explicitly (never fabricating) for
//! a `Reference`, whose value is reachable only through a threaded lookup. The
//! reference case is unreachable in this unit today: no production caller
//! reaches `get_sub_features` (the `#181` codegen hub is not emitted), and the
//! selector-codec tests exercise `Direct` holders. When the feature-registry
//! dispatch lands, the trait surface gains the lookup parameter and this STUB
//! is replaced.

use crate::levelgen::feature::ConfiguredFeatureErased;
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::Holder;

/// The full `PlacedFeature.getFeatures()` shape for a held placed feature —
/// `Stream.concat(Stream.of(this.feature), this.feature.value().getSubFeatures())`.
///
/// A `Direct` placed feature yields its contained configured-feature holder
/// (which is itself `Direct` when inlined) and that configured feature's own
/// sub-features (the recursion terminates at leaf configurations, whose default
/// `get_sub_features` is empty). A `Reference` cannot resolve without the
/// placed-feature lookup — an explicit STUB failure, never a fabricated stream.
pub fn placed_sub_features(
    holder: &Holder<PlacedFeature>,
) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
    match holder {
        Holder::Direct(placed) => match &placed.feature {
            Holder::Direct(configured) => Box::new(
                std::iter::once(placed.feature.clone()).chain(configured.get_sub_features()),
            ),
            Holder::Reference { .. } => panic!(
                "STUB(mc.world.level.levelgen.feature.selector): getSubFeatures on a \
                 Reference configured-feature holder inside a placed feature needs the \
                 configured-feature HolderLookup, which the FeatureConfiguration trait cannot \
                 thread"
            ),
        },
        Holder::Reference { .. } => panic!(
            "STUB(mc.world.level.levelgen.feature.selector): getSubFeatures on a \
             Reference placed-feature holder needs the placed-feature HolderLookup, which the \
             FeatureConfiguration trait cannot thread"
        ),
    }
}
