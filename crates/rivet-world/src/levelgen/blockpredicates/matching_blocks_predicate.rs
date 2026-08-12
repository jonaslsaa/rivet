//! Port of `net.minecraft.world.level.levelgen.blockpredicates.MatchingBlocksPredicate`
//! (class, 26.2).
//!
//! Java: a `StateTestingPredicate` whose `test(BlockState)` is
//! `state.is(this.blocks)` (the state's block holder is a member of the
//! `HolderSet<Block>`) and whose `type()` is `BlockPredicateType.MATCHING_BLOCKS`.
//! Its `CODEC` is the shared state-testing offset field plus the required
//! `"blocks"` field — `RegistryCodecs.homogeneousList(Registries.BLOCK)`, a
//! `HolderSetCodec` whose element codec is `RegistryFixedCodec(Registries.BLOCK)`
//! (tag key or element-list form).
//!
//! `Block` is the id-handle placeholder [`BlockType`]; the set's `Holder`
//! members are `Reference`s whose element id is the block's registry id, so
//! `state.is(this.blocks)` becomes `set.contains_id(state.block().id())`.

use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::state_testing_predicate::{
    StateTestingPredicate, offset_field, state_testing_test,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Vec3i};
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.MatchingBlocksPredicate`.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchingBlocksPredicate {
    /// `this.offset` — the offset applied to the tested position.
    offset: Vec3i,
    /// `this.blocks` — the matching block holder set.
    blocks: HolderSet<BlockType>,
}

impl MatchingBlocksPredicate {
    /// `new MatchingBlocksPredicate(Vec3i, HolderSet<Block>)`.
    pub fn new(offset: Vec3i, blocks: HolderSet<BlockType>) -> Self {
        MatchingBlocksPredicate { offset, blocks }
    }

    /// `this.offset`.
    pub fn offset(&self) -> &Vec3i {
        &self.offset
    }

    /// `this.blocks`.
    pub fn blocks(&self) -> &HolderSet<BlockType> {
        &self.blocks
    }
}

impl StateTestingPredicate for MatchingBlocksPredicate {
    fn offset(&self) -> &Vec3i {
        &self.offset
    }

    fn test_state(&self, state: &BlockState) -> bool {
        // `state.is(this.blocks)` — the state's block holder is a `Reference`
        // in the block registry whose element id is the block id.
        self.blocks.contains_id(state.block().id() as u32)
    }
}

impl BlockPredicate for MatchingBlocksPredicate {
    fn test(&self, level: &dyn crate::level::WorldGenLevel, origin: &BlockPos) -> bool {
        state_testing_test(self, level, origin)
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::MATCHING_BLOCKS
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the `"blocks"` field
/// codec: a `HolderSetCodec` over the block registry, whose element codec is a
/// `RegistryFixedCodec` (tag key `#minecraft:...` or element-list form).
///
/// The concrete codec is not `Send + Sync` (its `RegistryOps` carries the
/// single-threaded `HolderLookupAdapter`, `RefCell` memo — OWNERSHIP's single
/// sync tick); the `Arc` is held by the ops-parameterized predicate codec and
/// never crosses threads.
fn blocks_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<HolderSet<BlockType>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<rivet_registry::holder::Holder<BlockType>, Ops>> = Arc::new(
        rivet_registry::registry_file_codec::RegistryFixedCodec::create(
            &rivet_registry::registries::BLOCK,
        ),
    );
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<BlockType>, Ops>> =
        Arc::new(rivet_registry::registry_file_codec::HolderSetCodec::create(
            &rivet_registry::registries::BLOCK,
            element,
            false,
        ));
    codec::field_of(holder_set, "blocks".to_string())
}

/// `MatchingBlocksPredicate.CODEC` — the shared state-testing offset field plus
/// the required `"blocks"` holder-set field, as the ops-generic
/// `matching_blocks_predicate_map_codec::<Ops>()` factory.
pub fn matching_blocks_predicate_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<MatchingBlocksPredicate, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(offset_field::<MatchingBlocksPredicate, Ops>(Arc::new(
                |p: &MatchingBlocksPredicate| p.offset,
            )))
            .and(record_builder::RecordCodecBuilder::of(
                Arc::new(|p: &MatchingBlocksPredicate| p.blocks.clone()),
                blocks_field_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|offset: Vec3i, blocks: HolderSet<BlockType>| {
                    MatchingBlocksPredicate::new(offset, blocks)
                }),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::block_predicate::block_predicate_codec;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::holder::Holder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A block registry with `stone` (id 0) and `oak_log` (id 1), wrapped in a
    /// `RegistryAccess` under `Registries.BLOCK` — the holder-set field's
    /// element codec resolves the block through it.
    fn block_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*rivet_registry::registries::BLOCK);
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:stone"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:oak_log"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("block")),
            Box::new(registry) as AnyBox,
        )])
    }

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, block_access())
    }

    #[test]
    fn test_state_checks_membership_by_block_id() {
        // `state.is(this.blocks)` — `test_state` compares the state's REAL
        // generated block id against the set members (stone=1, oak_log=49,
        // air=0 in the vanilla block registry). The set carries those real
        // ids, so no test registry is involved in this pure predicate test.
        let set = HolderSet::direct(vec![
            Holder::reference(rivet_registry::holder::RegistryId(0), 1),
            Holder::reference(rivet_registry::holder::RegistryId(0), 49),
        ]);
        let p = MatchingBlocksPredicate::new(Vec3i::ZERO, set);
        let stone = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:stone").unwrap(),
        );
        let oak_log = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:oak_log").unwrap(),
        );
        let air = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:air").unwrap(),
        );
        assert!(p.test_state(&stone));
        assert!(p.test_state(&oak_log));
        assert!(!p.test_state(&air));
    }

    #[test]
    fn codec_round_trips_and_encodes_block_list() {
        // The 2-member holder set encodes as the compact list form
        // `["minecraft:stone", "minecraft:oak_log"]` (`alwaysUseList=false`); a
        // decode resolves the identifiers back to references through the
        // access. The dispatch encodes the `"blocks"` field and `"type"` (map
        // key order is not semantically significant).
        //
        // One access builds BOTH the set's reference holders and the ops: each
        // `freeze()` allocates a fresh `RegistryId` from the global counter, so
        // the holders must carry the same registry id the ops' access reads.
        let access = block_access();
        let registry = rivet_registry::access::RegistryAccess::lookup(
            &access,
            &*rivet_registry::registries::BLOCK,
        )
        .expect("block registry");
        let p: Arc<dyn BlockPredicate> = Arc::new(MatchingBlocksPredicate::new(
            Vec3i::new(1, 2, 3),
            HolderSet::direct(vec![
                Holder::reference(registry.registry_id(), 0),
                Holder::reference(registry.registry_id(), 1),
            ]),
        ));
        let codec = block_predicate_codec::<TestOps>();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "type": "minecraft:matching_blocks",
                "offset": [1, 2, 3],
                "blocks": ["minecraft:stone", "minecraft:oak_log"]
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            BlockPredicate::type_id(&*decoded),
            BlockPredicateTypes::MATCHING_BLOCKS
        );
        let as_blocks = downcast_matching_blocks(&decoded);
        assert_eq!(as_blocks.blocks().size(), 2);
        assert_eq!(as_blocks.offset(), &Vec3i::new(1, 2, 3));
    }

    fn downcast_matching_blocks(p: &Arc<dyn BlockPredicate>) -> &MatchingBlocksPredicate {
        p.as_any()
            .downcast_ref::<MatchingBlocksPredicate>()
            .expect("decoded matching_blocks predicate")
    }

    #[test]
    fn missing_blocks_field_errors() {
        // Java `fieldOf("blocks")` is required — a dispatch with the type key
        // but no `"blocks"` field fails with "No key blocks in ...".
        let ops = ops();
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(&ops, &json!({"type": "minecraft:matching_blocks"}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key blocks"), "got: {msg}");
    }

    #[test]
    fn unknown_block_name_errors() {
        // `RegistryFixedCodec.decode` — an identifier not in the registry errors
        // `"Failed to get element <name>"`.
        let ops = ops();
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(
            &ops,
            &json!({"type": "minecraft:matching_blocks", "blocks": "minecraft:not_a_block"}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Failed to get element minecraft:not_a_block"),
            "got: {msg}"
        );
    }

    #[test]
    fn single_member_encodes_compacted() {
        // A single-member set encodes as the bare element (`compactListCodec`),
        // not a one-element list.
        let access = block_access();
        let registry = rivet_registry::access::RegistryAccess::lookup(
            &access,
            &*rivet_registry::registries::BLOCK,
        )
        .expect("block registry");
        let single = HolderSet::direct(vec![Holder::reference(registry.registry_id(), 0)]);
        let p: Arc<dyn BlockPredicate> =
            Arc::new(MatchingBlocksPredicate::new(Vec3i::ZERO, single));
        let codec = block_predicate_codec::<TestOps>();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"type": "minecraft:matching_blocks", "blocks": "minecraft:stone"})
        );
    }
}
