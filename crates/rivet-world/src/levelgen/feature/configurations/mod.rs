//! `net.minecraft.world.level.levelgen.feature.configurations` — feature
//! configuration value types and their DFU codecs.
//!
//! PROVENANCE: the `mc.world.level.levelgen.feature.configurations.core`
//! manifest unit — `FeatureConfiguration.java`, `NoneFeatureConfiguration.java`,
//! `package-info.java` (26.2), plus an out-of-unit proactive port of
//! `ProbabilityFeatureConfiguration.java` (owned by the
//! `mc.world.level.levelgen.feature.configurations.probabilityfeature` unit).
//! The remaining 36 configuration classes each live in their own manifest unit
//! under this package; they implement the `FeatureConfiguration` trait defined
//! here.

pub mod feature_configuration;
pub mod none_feature_configuration;
pub mod probability_feature_configuration;

pub use feature_configuration::{FeatureConfiguration, NONE};
pub use none_feature_configuration::NoneFeatureConfiguration;
pub use probability_feature_configuration::ProbabilityFeatureConfiguration;
