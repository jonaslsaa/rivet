//! Port of `net.minecraft.world.level.levelgen.feature.rootplacers.
//! MangroveRootPlacement` (record, 26.2).
//!
//! Java is the `MangroveRootPlacer.mangroveRootPlacement` field's value type:
//! the per-mangrove tuning of the root spread. Its `CODEC` is the six-field
//! record codec (`RegistryCodecs.homogeneousList(Registries.BLOCK)` over
//! `"can_grow_through"`/`"muddy_roots_in"`, `BlockStateProvider.CODEC` over
//! `"muddy_roots_provider"`, `Codec.intRange(1, 12)`/`intRange(1, 64)` over the
//! width/length, and `Codec.floatRange(0, 1)` over the skew chance), requiring
//! the `RegistryOpsLookup` ops surface for the block holder sets and the
//! embedded state-provider dispatch.
//!
//! The block holder sets are `HolderSet<BlockType>` (the block registry's
//! placeholder element), the provider an `Arc<dyn ErasedBlockStateProvider>`
//! (the erased `BlockStateProvider` carrier), mirroring
//! `VegetationPatchConfiguration`'s `"replaceable"` field and the state-provider
//! dispatch root.

use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.rootplacers.MangroveRootPlacement`.
#[derive(Debug, Clone)]
pub struct MangroveRootPlacement {
    /// `MangroveRootPlacement.canGrowThrough` — the `HolderSet<Block>` roots
    /// may grow through (`canPlaceRoot` extends `validTreePos` with this).
    pub can_grow_through: HolderSet<BlockType>,
    /// `MangroveRootPlacement.muddyRootsIn` — the `HolderSet<Block>` positions
    /// whose block is replaced by `muddyRootsProvider`'s state.
    pub muddy_roots_in: HolderSet<BlockType>,
    /// `MangroveRootPlacement.muddyRootsProvider` — the state placed over
    /// `muddyRootsIn` positions.
    pub muddy_roots_provider: Arc<dyn ErasedBlockStateProvider>,
    /// `MangroveRootPlacement.maxRootWidth`.
    pub max_root_width: i32,
    /// `MangroveRootPlacement.maxRootLength`.
    pub max_root_length: i32,
    /// `MangroveRootPlacement.randomSkewChance`.
    pub random_skew_chance: f32,
}

impl MangroveRootPlacement {
    /// `new MangroveRootPlacement(...)` — the record constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        can_grow_through: HolderSet<BlockType>,
        muddy_roots_in: HolderSet<BlockType>,
        muddy_roots_provider: Arc<dyn ErasedBlockStateProvider>,
        max_root_width: i32,
        max_root_length: i32,
        random_skew_chance: f32,
    ) -> MangroveRootPlacement {
        MangroveRootPlacement {
            can_grow_through,
            muddy_roots_in,
            muddy_roots_provider,
            max_root_width,
            max_root_length,
            random_skew_chance,
        }
    }
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the block holder-set
/// field codec (the same `"replaceable"`-style `HolderSetCodec` over the block
/// registry, tag key or element-list form).
fn block_holder_set_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    name: &str,
) -> Arc<dyn rivet_serialization::map_codec::MapCodec<HolderSet<BlockType>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn codec::Codec<rivet_registry::holder::Holder<BlockType>, Ops>> =
        Arc::new(rivet_registry::registry_file_codec::RegistryFixedCodec::create(
            &rivet_registry::registries::BLOCK,
        ));
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn codec::Codec<HolderSet<BlockType>, Ops>> =
        Arc::new(rivet_registry::registry_file_codec::HolderSetCodec::create(
            &rivet_registry::registries::BLOCK,
            element,
            false,
        ));
    codec::field_of(holder_set, name.to_string())
}

/// `MangroveRootPlacement.CODEC` — the record codec over the six fields, as the
/// ops-generic `mangrove_root_placement_map_codec::<Ops>()` factory.
pub fn mangrove_root_placement_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<MangroveRootPlacement, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &MangroveRootPlacement| p.can_grow_through.clone()),
                block_holder_set_field_codec::<Ops>("can_grow_through"),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &MangroveRootPlacement| p.muddy_roots_in.clone()),
                block_holder_set_field_codec::<Ops>("muddy_roots_in"),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|p: &MangroveRootPlacement| p.muddy_roots_provider.clone()),
                "muddy_roots_provider".to_string(),
                block_state_provider_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|p: &MangroveRootPlacement| p.max_root_width),
                "max_root_width".to_string(),
                codec::int_range::<Ops>(1, 12),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|p: &MangroveRootPlacement| p.max_root_length),
                "max_root_length".to_string(),
                codec::int_range::<Ops>(1, 64),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|p: &MangroveRootPlacement| p.random_skew_chance),
                "random_skew_chance".to_string(),
                codec::float_range::<Ops>(0.0, 1.0),
            ))
            .apply(instance, Arc::new(MangroveRootPlacement::new))
    })
}
