//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.TagMatchTest`
//! (class, 26.2).
//!
//! Java: a rule test matching a block tag. Its `CODEC` is
//! `TagKey.codec(Registries.BLOCK).fieldOf("tag").xmap(...)`, and its `test` is
//! `blockState.is(this.tag)` (tag membership). The codec is ported here (as
//! the ops-generic `tag_match_test_map_codec::<Ops>()` factory) and lifted to
//! the erased carrier in `rule_test`.
//!
//! `Registries.BLOCK` is `createRegistryKey("block")`; `rivet-registry`'s
//! `BLOCK` static is typed over the generated `BlockType` placeholder, so the
//! `TagKey<Block>` registry key is reconstructed here (the same pattern
//! `tag_key.rs`'s own tests use).

use crate::block::Block;
use crate::levelgen::structure::templatesystem::rule_test::RuleTest;
use crate::levelgen::structure::templatesystem::rule_test_type::{RuleTestTypeId, RuleTestTypes};
use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_registry::block_state::BlockState;
use rivet_registry::registry::Registry;
use rivet_registry::tag_key::{self, TagKey};
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `Registries.BLOCK` over the port's `Block` handle — `createRegistryKey(
/// "block")`.
fn block_registry_key() -> ResourceKey<Registry<Block>> {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("block"))
}

/// `net.minecraft.world.level.levelgen.structure.templatesystem.TagMatchTest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagMatchTest {
    /// `tag` — the matched block tag.
    pub tag: TagKey<Block>,
}

impl TagMatchTest {
    /// `new TagMatchTest(TagKey<Block>)`.
    pub fn new(tag: TagKey<Block>) -> Self {
        TagMatchTest { tag }
    }

    /// `TagMatchTest.getTag()`.
    pub fn get_tag(&self) -> &TagKey<Block> {
        &self.tag
    }
}

impl RuleTest for TagMatchTest {
    /// `TagMatchTest.test` — `blockState.is(this.tag)`. The port's
    /// `BlockState::is_in_tag` is keyed by the tag's namespaced location string
    /// (`"minecraft:planks"`), so the tag's location is passed directly.
    fn test<R: RandomSource>(&self, state: &BlockState, _random: &mut R) -> bool {
        state.is_in_tag(&self.tag.location().to_string())
    }

    fn type_id(&self) -> RuleTestTypeId {
        RuleTestTypes::TAG_TEST
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `TagMatchTest.CODEC` — `TagKey.codec(Registries.BLOCK).fieldOf("tag")
/// .xmap(...)`, as the ops-generic `tag_match_test_map_codec::<Ops>()` factory.
pub fn tag_match_test_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<TagMatchTest, Ops>>
{
    let field = codec::field_of(
        tag_key::tag_key_codec::<Block, Ops>(&block_registry_key()),
        "tag".to_string(),
    );
    map_codec::xmap(
        field,
        Arc::new(|t: &TagKey<Block>| TagMatchTest::new(t.clone())),
        Arc::new(|t: &TagMatchTest| t.tag.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::structure::templatesystem::codec_test_util;
    use serde_json::json;

    fn planks_tag() -> TagKey<Block> {
        TagKey::create(&block_registry_key(), Identifier::parse("minecraft:planks"))
    }

    #[test]
    fn matches_block_tag() {
        let t = TagMatchTest::new(planks_tag());
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let oak_planks = crate::block::Block::from_name("minecraft:oak_planks")
            .unwrap()
            .default_block_state();
        let stone = crate::block::Block::from_name("minecraft:stone")
            .unwrap()
            .default_block_state();
        assert!(t.test(&oak_planks, &mut random));
        assert!(!t.test(&stone, &mut random));
    }

    #[test]
    fn codec_round_trips() {
        let codec = codec_test_util::codec(tag_match_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let t = TagMatchTest::new(planks_tag());
        let encoded = codec_test_util::encode(&codec, &t);
        assert_eq!(encoded, json!({"tag": "minecraft:planks"}));
        assert_eq!(codec_test_util::decode(&codec, &encoded), t);
    }
}
