//! `net.minecraft.world.level.levelgen.feature.configurations` — feature
//! configuration value types and their DFU codecs.
//!
//! PROVENANCE: the `mc.world.level.levelgen.feature.configurations.core`
//! manifest unit — `FeatureConfiguration.java`, `NoneFeatureConfiguration.java`,
//! `package-info.java` (26.2), plus out-of-unit proactive ports of
//! `ProbabilityFeatureConfiguration.java` (owned by the
//! `mc.world.level.levelgen.feature.configurations.probabilityfeature` unit),
//! and — this wave (issue #391) — `EndGatewayConfiguration.java`,
//! `UnderwaterMagmaConfiguration.java`, `TwistingVinesConfig.java`,
//! `BlockStateConfiguration.java`, `BlockBlobConfiguration.java`,
//! `LayerConfiguration.java`, `SpikeConfiguration.java`, and — this wave —
//! `BlockPileConfiguration.java`, `DeltaFeatureConfiguration.java`,
//! `DiskConfiguration.java`, `HugeMushroomFeatureConfiguration.java`,
//! `NetherForestVegetationConfig.java`, `ReplaceSphereConfiguration.java`,
//! `SimpleBlockConfiguration.java`, and `SpeleothemConfiguration.java`, each
//! owned by its own manifest unit under this package. The value-provider
//! feature-config wave adds `CountConfiguration.java` (owned by the
//! `mc.world.level.levelgen.feature.configurations.count` manifest unit),
//! `ColumnFeatureConfiguration.java` (owned by the
//! `mc.world.level.levelgen.feature.configurations.columnfeature` manifest
//! unit), and `SculkPatchConfiguration.java` (owned by the
//! `mc.world.level.levelgen.feature.configurations.sculkpatch` manifest unit).
//! They all implement the `FeatureConfiguration` trait defined here.

pub mod block_blob_configuration;
pub mod block_column_configuration;
pub mod block_pile_configuration;
pub mod block_state_configuration;
pub mod column_feature_configuration;
pub mod count_configuration;
pub mod delta_feature_configuration;
pub mod disk_configuration;
pub mod end_gateway_configuration;
pub mod end_spike_configuration;
pub mod feature_configuration;
pub mod geode_configuration;
pub mod huge_mushroom_feature_configuration;
pub mod large_dripstone_configuration;
pub mod layer_configuration;
pub mod multiface_growth_configuration;
pub mod nether_forest_vegetation_config;
pub mod none_feature_configuration;
pub mod ore_configuration;
pub mod probability_feature_configuration;
pub mod replace_block_configuration;
pub mod replace_sphere_configuration;
pub mod sculk_patch_configuration;
pub mod simple_block_configuration;
pub mod speleothem_cluster_configuration;
pub mod speleothem_configuration;
pub mod spike_configuration;
pub mod spring_configuration;
pub mod template_feature_configuration;
pub mod tree_configuration;
pub mod twisting_vines_config;
pub mod underwater_magma_configuration;
pub mod vegetation_patch_configuration;

pub use block_blob_configuration::BlockBlobConfiguration;
pub use block_column_configuration::BlockColumnConfiguration;
pub use block_pile_configuration::{BlockPileConfiguration, block_pile_configuration_codec};
pub use block_state_configuration::BlockStateConfiguration;
pub use column_feature_configuration::ColumnFeatureConfiguration;
pub use count_configuration::CountConfiguration;
pub use delta_feature_configuration::DeltaFeatureConfiguration;
pub use disk_configuration::DiskConfiguration;
pub use end_gateway_configuration::EndGatewayConfiguration;
pub use end_spike_configuration::EndSpikeConfiguration;
pub use feature_configuration::{FeatureConfiguration, NONE};
pub use geode_configuration::GeodeConfiguration;
pub use huge_mushroom_feature_configuration::HugeMushroomFeatureConfiguration;
pub use large_dripstone_configuration::LargeDripstoneConfiguration;
pub use layer_configuration::LayerConfiguration;
pub use multiface_growth_configuration::MultifaceGrowthConfiguration;
pub use nether_forest_vegetation_config::NetherForestVegetationConfig;
pub use none_feature_configuration::NoneFeatureConfiguration;
pub use ore_configuration::{OreConfiguration, TargetBlockState};
pub use probability_feature_configuration::ProbabilityFeatureConfiguration;
pub use replace_block_configuration::ReplaceBlockConfiguration;
pub use replace_sphere_configuration::ReplaceSphereConfiguration;
pub use sculk_patch_configuration::SculkPatchConfiguration;
pub use simple_block_configuration::SimpleBlockConfiguration;
pub use speleothem_cluster_configuration::SpeleothemClusterConfiguration;
pub use speleothem_configuration::SpeleothemConfiguration;
pub use spike_configuration::SpikeConfiguration;
pub use spring_configuration::SpringConfiguration;
pub use template_feature_configuration::TemplateFeatureConfiguration;
pub use tree_configuration::TreeConfiguration;
pub use twisting_vines_config::TwistingVinesConfig;
pub use underwater_magma_configuration::UnderwaterMagmaConfiguration;
pub use vegetation_patch_configuration::VegetationPatchConfiguration;
