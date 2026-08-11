//! `net.minecraft.world.level.levelgen.feature.configurations` — feature
//! configuration value types and their DFU codecs.
//!
//! PROVENANCE: the `mc.world.level.levelgen.feature.configurations.core`
//! manifest unit — `FeatureConfiguration.java`, `NoneFeatureConfiguration.java`,
//! `package-info.java` (26.2), plus out-of-unit proactive ports of
//! `ProbabilityFeatureConfiguration.java` (owned by the
//! `mc.world.level.levelgen.feature.configurations.probabilityfeature` unit),
//! and — this wave (issue #391) — `EndGatewayConfiguration.java`,
//! `UnderwaterMagmaConfiguration.java`, and `TwistingVinesConfig.java`, each
//! owned by its own manifest unit under this package. They implement the
//! `FeatureConfiguration` trait defined here.

pub mod end_gateway_configuration;
pub mod feature_configuration;
pub mod none_feature_configuration;
pub mod probability_feature_configuration;
pub mod twisting_vines_config;
pub mod underwater_magma_configuration;

pub use end_gateway_configuration::EndGatewayConfiguration;
pub use feature_configuration::{FeatureConfiguration, NONE};
pub use none_feature_configuration::NoneFeatureConfiguration;
pub use probability_feature_configuration::ProbabilityFeatureConfiguration;
pub use twisting_vines_config::TwistingVinesConfig;
pub use underwater_magma_configuration::UnderwaterMagmaConfiguration;
