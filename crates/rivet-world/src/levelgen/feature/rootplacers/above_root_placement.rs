//! AboveRootPlacement record and codec (Paper 26.2).
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::{
    codec::{self, Codec},
    dynamic_ops::DynamicOps,
    record_builder::{self, RecordCodecBuilder},
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AboveRootPlacement {
    pub above_root_provider: Arc<dyn ErasedBlockStateProvider>,
    pub above_root_placement_chance: f32,
}
pub fn above_root_placement_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<AboveRootPlacement, Ops>> {
    record_builder::create(|i| {
        i.group(RecordCodecBuilder::of(
            Arc::new(|c: &AboveRootPlacement| c.above_root_provider.clone()),
            codec::field_of(
                block_state_provider_codec::<Ops>(),
                "above_root_provider".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|c: &AboveRootPlacement| c.above_root_placement_chance),
            codec::field_of(
                codec::float_range::<Ops>(0.0, 1.0),
                "above_root_placement_chance".to_string(),
            ),
        ))
        .apply(
            i,
            Arc::new(
                |above_root_provider, above_root_placement_chance| AboveRootPlacement {
                    above_root_provider,
                    above_root_placement_chance,
                },
            ),
        )
    })
}
