//! MangroveRootPlacement record, including registry-backed block holder sets.
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
use rivet_registry::{
    holder::Holder,
    holder_set::HolderSet,
    registries::BlockType,
    registry_file_codec::{HolderSetCodec, RegistryFixedCodec},
    registry_ops::RegistryOpsLookup,
};
use rivet_serialization::{
    codec::{self, Codec},
    dynamic_ops::DynamicOps,
    record_builder::{self, RecordCodecBuilder},
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct MangroveRootPlacement {
    pub can_grow_through: HolderSet<BlockType>,
    pub muddy_roots_in: HolderSet<BlockType>,
    pub muddy_roots_provider: Arc<dyn ErasedBlockStateProvider>,
    pub max_root_width: i32,
    pub max_root_length: i32,
    pub random_skew_chance: f32,
}
pub fn mangrove_root_placement_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<MangroveRootPlacement, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<Holder<BlockType>, Ops>> = Arc::new(RegistryFixedCodec::create(
        &rivet_registry::registries::BLOCK,
    ));
    #[allow(clippy::arc_with_non_send_sync)]
    let blocks: Arc<dyn Codec<HolderSet<BlockType>, Ops>> = Arc::new(HolderSetCodec::create(
        &rivet_registry::registries::BLOCK,
        element,
        false,
    ));
    record_builder::create(|i| {
        i.group(RecordCodecBuilder::of(
            Arc::new(|c: &MangroveRootPlacement| c.can_grow_through.clone()),
            codec::field_of(blocks.clone(), "can_grow_through".to_string()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|c: &MangroveRootPlacement| c.muddy_roots_in.clone()),
            codec::field_of(blocks.clone(), "muddy_roots_in".to_string()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|c: &MangroveRootPlacement| c.muddy_roots_provider.clone()),
            codec::field_of(
                block_state_provider_codec::<Ops>(),
                "muddy_roots_provider".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|c: &MangroveRootPlacement| c.max_root_width),
            codec::field_of(codec::int_range::<Ops>(1, 12), "max_root_width".to_string()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|c: &MangroveRootPlacement| c.max_root_length),
            codec::field_of(
                codec::int_range::<Ops>(1, 64),
                "max_root_length".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|c: &MangroveRootPlacement| c.random_skew_chance),
            codec::field_of(
                codec::float_range::<Ops>(0.0, 1.0),
                "random_skew_chance".to_string(),
            ),
        ))
        .apply(
            i,
            Arc::new(
                |can_grow_through,
                 muddy_roots_in,
                 muddy_roots_provider,
                 max_root_width,
                 max_root_length,
                 random_skew_chance| MangroveRootPlacement {
                    can_grow_through,
                    muddy_roots_in,
                    muddy_roots_provider,
                    max_root_width,
                    max_root_length,
                    random_skew_chance,
                },
            ),
        )
    })
}
