//! Port of `net.minecraft.world.level.levelgen.blockpredicates.MatchingBlockTagPredicate`
//! (class, 26.2).
//!
//! Java: a `StateTestingPredicate` whose `test(BlockState)` is
//! `state.is(this.tag)` (the state's block is a member of the `TagKey<Block>`)
//! and whose `type()` is `BlockPredicateType.MATCHING_BLOCK_TAG`. Its `CODEC`
//! is the shared state-testing offset field plus the required `"tag"` field —
//! `TagKey.codec(Registries.BLOCK)`, the plain (non-hashed) tag-key codec that
//! encodes the tag's location identifier directly (unlike `HolderSetCodec`'s
//! `#`-prefixed hashed form).
//!
//! `BlockState.is(TagKey)` is served by the behavior-table tag query
//! (`is_in_tag`), which reads the tag's bound member list over the generated
//! block names.

use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::state_testing_predicate::{
    StateTestingPredicate, offset_field, state_testing_test,
};
use rivet_registry::TagKey;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Vec3i};
use rivet_registry::registries::BlockType;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.MatchingBlockTagPredicate`.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchingBlockTagPredicate {
    /// `this.offset` — the offset applied to the tested position.
    offset: Vec3i,
    /// `this.tag` — the matching block tag.
    tag: TagKey<BlockType>,
}

impl MatchingBlockTagPredicate {
    /// `new MatchingBlockTagPredicate(Vec3i, TagKey<Block>)`.
    pub fn new(offset: Vec3i, tag: TagKey<BlockType>) -> Self {
        MatchingBlockTagPredicate { offset, tag }
    }

    /// `this.offset`.
    pub fn offset(&self) -> &Vec3i {
        &self.offset
    }

    /// `this.tag`.
    pub fn tag(&self) -> &TagKey<BlockType> {
        &self.tag
    }
}

impl StateTestingPredicate for MatchingBlockTagPredicate {
    fn offset(&self) -> &Vec3i {
        &self.offset
    }

    fn test_state(&self, state: &BlockState) -> bool {
        state.is_in_tag(&self.tag.location().to_string())
    }
}

impl BlockPredicate for MatchingBlockTagPredicate {
    fn test(&self, level: &dyn crate::level::WorldGenLevel, origin: &BlockPos) -> bool {
        state_testing_test(self, level, origin)
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::MATCHING_BLOCK_TAG
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `MatchingBlockTagPredicate.CODEC` — the shared state-testing offset field
/// plus the required `"tag"` field (`TagKey.codec(Registries.BLOCK)`), as the
/// ops-generic `matching_block_tag_predicate_map_codec::<Ops>()` factory.
pub fn matching_block_tag_predicate_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<MatchingBlockTagPredicate, Ops>> {
    let tag_field = rivet_registry::tag_key::tag_key_codec::<BlockType, Ops>(
        &rivet_registry::registries::BLOCK,
    );
    record_builder::map_codec(|instance| {
        instance
            .group(offset_field::<MatchingBlockTagPredicate, Ops>(Arc::new(
                |p: &MatchingBlockTagPredicate| p.offset,
            )))
            .and(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|p: &MatchingBlockTagPredicate| p.tag.clone()),
                "tag".to_string(),
                tag_field,
            ))
            .apply(
                instance,
                Arc::new(|offset: Vec3i, tag: TagKey<BlockType>| {
                    MatchingBlockTagPredicate::new(offset, tag)
                }),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::block_predicate::block_predicate_codec;
    use rivet_registry::Identifier;
    use rivet_registry::TagKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// The test ops: a `RegistryOps` over JSON — the only ops that implement
    /// `RegistryOpsLookup` (the dispatch's holder-set fields require it). The
    /// tag-key field codec never touches a registry, so an empty access is
    /// enough.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn logs_tag() -> TagKey<BlockType> {
        TagKey::create(
            &*rivet_registry::registries::BLOCK,
            Identifier::parse("minecraft:logs"),
        )
    }

    #[test]
    fn test_state_checks_tag_membership() {
        // `state.is(this.tag)` — the behavior-table tag query: oak_log is in
        // `minecraft:logs`, air is not.
        let p = MatchingBlockTagPredicate::new(Vec3i::ZERO, logs_tag());
        let oak_log = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:oak_log").unwrap(),
        );
        let air = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:air").unwrap(),
        );
        assert!(p.test_state(&oak_log));
        assert!(!p.test_state(&air));
    }

    #[test]
    fn codec_round_trips_and_encodes_tag() {
        // `TagKey.codec(Registries.BLOCK)` encodes the tag's location directly
        // (no `#` prefix — that's the hashed form).
        let p: Arc<dyn BlockPredicate> = Arc::new(MatchingBlockTagPredicate::new(
            Vec3i::new(1, 2, 3),
            logs_tag(),
        ));
        let codec = block_predicate_codec::<TestOps>();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "type": "minecraft:matching_block_tag",
                "offset": [1, 2, 3],
                "tag": "minecraft:logs"
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            BlockPredicate::type_id(&*decoded),
            BlockPredicateTypes::MATCHING_BLOCK_TAG
        );
        let as_tag = decoded
            .as_any()
            .downcast_ref::<MatchingBlockTagPredicate>()
            .expect("decoded matching_block_tag predicate");
        assert_eq!(as_tag.tag(), &logs_tag());
        assert_eq!(as_tag.offset(), &Vec3i::new(1, 2, 3));
    }

    #[test]
    fn missing_tag_field_errors() {
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(&ops, &json!({"type": "minecraft:matching_block_tag"}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key tag"), "got: {msg}");
    }
}
