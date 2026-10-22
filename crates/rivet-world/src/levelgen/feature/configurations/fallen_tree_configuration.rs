//! FallenTreeConfiguration (26.2): trunk provider, log length and decorator lists.
use super::FeatureConfiguration;
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
use crate::levelgen::feature::treedecorators::{TreeDecorator, tree_decorator_codec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::{
    codec::{self, Codec},
    dynamic_ops::DynamicOps,
    record_builder::{self, RecordCodecBuilder},
};
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct FallenTreeConfiguration {
    pub trunk_provider: Arc<dyn ErasedBlockStateProvider>,
    pub log_length: IntProvider,
    pub stump_decorators: Vec<TreeDecorator>,
    pub log_decorators: Vec<TreeDecorator>,
}
impl FeatureConfiguration for FallenTreeConfiguration {}

pub fn fallen_tree_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<FallenTreeConfiguration, Ops>> {
    record_builder::create(|i| {
        i.group(RecordCodecBuilder::of(
            Arc::new(|c: &FallenTreeConfiguration| c.trunk_provider.clone()),
            codec::field_of(
                block_state_provider_codec::<Ops>(),
                "trunk_provider".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|c: &FallenTreeConfiguration| c.log_length.clone()),
            codec::field_of(
                int_provider_codec_with_bounds::<Ops>(0, 16),
                "log_length".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|c: &FallenTreeConfiguration| c.stump_decorators.clone()),
            codec::field_of(
                codec::list(tree_decorator_codec::<Ops>()),
                "stump_decorators".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|c: &FallenTreeConfiguration| c.log_decorators.clone()),
            codec::field_of(
                codec::list(tree_decorator_codec::<Ops>()),
                "log_decorators".to_string(),
            ),
        ))
        .apply(
            i,
            Arc::new(
                |trunk_provider, log_length, stump_decorators, log_decorators| {
                    FallenTreeConfiguration {
                        trunk_provider,
                        log_length,
                        stump_decorators,
                        log_decorators,
                    }
                },
            ),
        )
    })
}
