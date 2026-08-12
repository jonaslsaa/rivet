//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! RuleBasedStateProvider` (class + `Rule` record, 26.2).
//!
//! Java: a provider holding an optional `fallback` and a `List<Rule>`, where
//! `Rule(BlockPredicate ifTrue, BlockStateProvider then)`. `getState` returns
//! `getOptionalState()` when present, else `level.getBlockState(pos)` — the
//! honest world-read seam (`WorldGenLevel.get_block_state` panics until the
//! world unit lands, RivetTodo #399; never fabricate a state). `getOptionalState`
//! returns the first matching rule's `then` state, else the fallback's state
//! (`None` when no fallback). `type()` is
//! `BlockStateProviderType.RULE_BASED_STATE_PROVIDER`.
//!
//! `CODEC` is the 2-field record over `"fallback"`
//! (`BlockStateProvider.CODEC.optionalFieldOf("fallback")` — the *strict*
//! optional field, DFU's `optionalField(name, codec, false)`) and `"rules"`
//! (`Rule.CODEC.listOf()`). `Rule.CODEC` pairs `"if_true"` (`BlockPredicate.
//! CODEC`) with `"then"` (`BlockStateProvider.CODEC`). `Ops` must implement
//! [`RegistryOpsLookup`]: the `matching_blocks`/`matching_fluids`/
//! `matching_biomes` predicates resolve the registry through the ops.

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::{BlockPredicate, block_predicate_codec};
use crate::levelgen::feature::stateproviders::block_state_provider::{
    BlockStateProvider, ErasedBlockStateProvider, block_state_provider_get_state, simple_block,
};
use crate::levelgen::feature::stateproviders::block_state_provider_type::{
    BlockStateProviderTypeId, BlockStateProviderTypes,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use std::sync::Arc;

/// `RuleBasedStateProvider.Rule` (record, 26.2) — a `BlockPredicate` paired
/// with the provider it supplies when the predicate tests true.
#[derive(Debug, Clone)]
pub struct Rule {
    /// `Rule.ifTrue`.
    pub if_true: Arc<dyn BlockPredicate>,
    /// `Rule.then`.
    pub then: Arc<dyn ErasedBlockStateProvider>,
}

impl Rule {
    /// `new Rule(BlockPredicate, BlockStateProvider)` — the record constructor.
    pub fn new(if_true: Arc<dyn BlockPredicate>, then: Arc<dyn ErasedBlockStateProvider>) -> Rule {
        Rule { if_true, then }
    }
}

/// `net.minecraft.world.level.levelgen.feature.stateproviders.RuleBasedStateProvider`.
#[derive(Debug, Clone)]
pub struct RuleBasedStateProvider {
    /// `this.fallback` — `None` is Java's `null` fallback.
    fallback: Option<Arc<dyn ErasedBlockStateProvider>>,
    /// `this.rules`.
    rules: Vec<Rule>,
}

impl RuleBasedStateProvider {
    /// `new RuleBasedStateProvider(@Nullable BlockStateProvider fallback,
    /// List<Rule> rules)`.
    pub fn new(
        fallback: Option<Arc<dyn ErasedBlockStateProvider>>,
        rules: Vec<Rule>,
    ) -> RuleBasedStateProvider {
        RuleBasedStateProvider { fallback, rules }
    }

    /// `ifTrueThenProvide(BlockPredicate, BlockStateProvider)` — a provider
    /// with no fallback and a single rule.
    pub fn if_true_then_provide(
        if_true: Arc<dyn BlockPredicate>,
        then: Arc<dyn ErasedBlockStateProvider>,
    ) -> RuleBasedStateProvider {
        RuleBasedStateProvider::new(None, vec![Rule::new(if_true, then)])
    }

    /// `ifTrueThenProvide(BlockPredicate, Block)` — a provider with a single
    /// rule whose `then` is `BlockStateProvider.simple(block)`.
    pub fn if_true_then_provide_block(
        if_true: Arc<dyn BlockPredicate>,
        block: BlockId,
    ) -> RuleBasedStateProvider {
        RuleBasedStateProvider::if_true_then_provide(
            if_true,
            Arc::new(simple_block(block)) as Arc<dyn ErasedBlockStateProvider>,
        )
    }

    /// `builder()` — a `Builder` with no fallback.
    pub fn builder() -> Builder {
        Builder::new(None)
    }

    /// `builder(@Nullable BlockStateProvider fallback)`.
    pub fn builder_with_fallback(fallback: Option<Arc<dyn ErasedBlockStateProvider>>) -> Builder {
        Builder::new(fallback)
    }
}

impl BlockStateProvider for RuleBasedStateProvider {
    fn get_state<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        random: &mut R,
        pos: &BlockPos,
    ) -> BlockState {
        // `BlockState result = this.getOptionalState(level, random, pos);
        // return result != null ? result : level.getBlockState(pos);`
        match self.get_optional_state(level, random, pos) {
            Some(state) => state,
            None => level.get_block_state(pos),
        }
    }

    fn get_optional_state<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        random: &mut R,
        pos: &BlockPos,
    ) -> Option<BlockState> {
        // `for (Rule rule : this.rules) { if (rule.ifTrue().test(level, pos))
        // return rule.then().getState(level, random, pos); }`
        for rule in &self.rules {
            if rule.if_true.test(level, pos) {
                return Some(block_state_provider_get_state(
                    rule.then.as_ref(),
                    level,
                    random,
                    pos,
                ));
            }
        }
        // `return this.fallback == null ? null : this.fallback.getState(...);`
        self.fallback
            .as_ref()
            .map(|fallback| block_state_provider_get_state(fallback.as_ref(), level, random, pos))
    }

    fn type_id(&self) -> BlockStateProviderTypeId {
        BlockStateProviderTypes::RULE_BASED_STATE_PROVIDER
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `RuleBasedStateProvider.Builder` — the mutable rule collector (Java's
/// static nested class). Each `ifTrueThenProvide` appends a rule; `build`
/// constructs the provider.
pub struct Builder {
    /// `this.fallback`.
    fallback: Option<Arc<dyn ErasedBlockStateProvider>>,
    /// `this.rules`.
    rules: Vec<Rule>,
}

impl Builder {
    /// `new Builder(@Nullable BlockStateProvider fallback)`.
    pub fn new(fallback: Option<Arc<dyn ErasedBlockStateProvider>>) -> Builder {
        Builder {
            fallback,
            rules: Vec::new(),
        }
    }

    /// `ifTrueThenProvide(BlockPredicate, BlockStateProvider)`.
    pub fn if_true_then_provide(
        mut self,
        if_true: Arc<dyn BlockPredicate>,
        then: Arc<dyn ErasedBlockStateProvider>,
    ) -> Builder {
        self.rules.push(Rule::new(if_true, then));
        self
    }

    /// `ifTrueThenProvide(BlockPredicate, Block)` — the `Block` form, `then` is
    /// `BlockStateProvider.simple(block)`.
    pub fn if_true_then_provide_block(
        mut self,
        if_true: Arc<dyn BlockPredicate>,
        block: BlockId,
    ) -> Builder {
        self.rules.push(Rule::new(
            if_true,
            Arc::new(simple_block(block)) as Arc<dyn ErasedBlockStateProvider>,
        ));
        self
    }

    /// `ifTrueThenProvide(BlockPredicate, BlockState)` — the `BlockState` form,
    /// `then` is `BlockStateProvider.simple(state)`.
    pub fn if_true_then_provide_state(
        mut self,
        if_true: Arc<dyn BlockPredicate>,
        state: BlockState,
    ) -> Builder {
        self.rules.push(Rule::new(
            if_true,
            Arc::new(crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider::new(
                state,
            )) as Arc<dyn ErasedBlockStateProvider>,
        ));
        self
    }

    /// `build()` — `new RuleBasedStateProvider(this.fallback, this.rules)`.
    pub fn build(self) -> RuleBasedStateProvider {
        RuleBasedStateProvider::new(self.fallback, self.rules)
    }
}

/// `Rule.CODEC` — the 2-field record (`"if_true"`/`"then"`), as the
/// ops-generic `rule_codec::<Ops>(top)` factory. `top` is the
/// `BlockStateProvider.CODEC` `RecursiveSelf` from the dispatch graph, so a
/// nested `then` round-trips through the single recursive codec.
pub fn rule_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    top: Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>>,
) -> Arc<dyn Codec<Rule, Ops>> {
    map_codec::codec_of(record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|r: &Rule| r.if_true.clone()),
                // `BlockPredicate.CODEC.fieldOf("if_true")`.
                codec::field_of::<Arc<dyn BlockPredicate>, Ops>(
                    block_predicate_codec::<Ops>(),
                    "if_true".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|r: &Rule| r.then.clone()),
                // `BlockStateProvider.CODEC.fieldOf("then")`.
                codec::field_of::<Arc<dyn ErasedBlockStateProvider>, Ops>(top, "then".to_string()),
            ))
            .apply(instance, Arc::new(Rule::new))
    }))
}

/// `RuleBasedStateProvider.CODEC` — the 2-field record (`"fallback"`/`"rules"`),
/// as the ops-generic `rule_based_state_provider_map_codec::<Ops>(top)`
/// factory. `top` is the `BlockStateProvider.CODEC` `RecursiveSelf` from the
/// dispatch graph, so the nested fallback/rules `then` providers round-trip
/// through the single recursive codec.
pub fn rule_based_state_provider_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    top: Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>>,
) -> Arc<dyn MapCodec<RuleBasedStateProvider, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &RuleBasedStateProvider| p.fallback.clone()),
                // `BlockStateProvider.CODEC.optionalFieldOf("fallback")` — the
                // STRICT optional field (DFU's `optionalField(name, codec,
                // false)`), so an absent key decodes to `None` and a present
                // but malformed value errors.
                codec::optional_field::<Arc<dyn ErasedBlockStateProvider>, Ops>(
                    "fallback".to_string(),
                    top.clone(),
                    false,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &RuleBasedStateProvider| p.rules.clone()),
                // `Rule.CODEC.listOf().fieldOf("rules")`.
                codec::field_of::<Vec<Rule>, Ops>(
                    codec::list(rule_codec::<Ops>(top)),
                    "rules".to_string(),
                ),
            ))
            .apply(instance, Arc::new(RuleBasedStateProvider::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::blockpredicates::always_true;
    use crate::levelgen::feature::stateproviders::block_state_provider::block_state_provider_codec;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn test_ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, rivet_registry::RegistryAccess::empty())
    }

    fn air() -> BlockState {
        BlockState::of(BlockId::from_id(0))
    }

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_id(1))
    }

    fn simple_source(state: BlockState) -> Arc<dyn ErasedBlockStateProvider> {
        Arc::new(crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider::new(
            state,
        )) as Arc<dyn ErasedBlockStateProvider>
    }

    #[test]
    fn rule_codec_round_trips_if_true_then() {
        let codec = rule_codec::<TestOps>(block_state_provider_codec::<TestOps>());
        let input = json!({
            "if_true": {"type": "minecraft:true"},
            "then": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:stone"}}
        });
        let decoded_result = codec.parse(&test_ops(), &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            decoded.then.type_id(),
            BlockStateProviderTypes::SIMPLE_STATE_PROVIDER
        );
        let encoded = codec
            .encode_start(&test_ops(), decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_round_trips_with_fallback() {
        let codec = map_codec::codec_of(rule_based_state_provider_map_codec::<TestOps>(
            block_state_provider_codec::<TestOps>(),
        ));
        let input = json!({
            "fallback": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:dirt"}},
            "rules": [
                {"if_true": {"type": "minecraft:true"}, "then": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:stone"}}}
            ]
        });
        let decoded_result = codec.parse(&test_ops(), &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            BlockStateProvider::type_id(decoded),
            BlockStateProviderTypes::RULE_BASED_STATE_PROVIDER
        );
        assert!(decoded.fallback.is_some());
        assert_eq!(decoded.rules.len(), 1);
        let encoded = codec
            .encode_start(&test_ops(), decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_decodes_without_fallback() {
        let codec = map_codec::codec_of(rule_based_state_provider_map_codec::<TestOps>(
            block_state_provider_codec::<TestOps>(),
        ));
        let input = json!({
            "rules": [
                {"if_true": {"type": "minecraft:true"}, "then": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:stone"}}}
            ]
        });
        let decoded_result = codec.parse(&test_ops(), &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert!(decoded.fallback.is_none());
        assert_eq!(decoded.rules.len(), 1);
    }

    #[test]
    fn get_state_returns_the_matching_rules_state() {
        // A single `true` rule with a constant `then` → that state.
        let provider =
            RuleBasedStateProvider::if_true_then_provide(always_true(), simple_source(stone()));
        let mut random = rivet_util::random::LegacyRandomSource::new(1);
        let state = provider.get_state(&TestLevel, &mut random, &BlockPos::new(0, 0, 0));
        assert_eq!(state, stone());
    }

    #[test]
    fn get_optional_state_is_none_without_match_or_fallback() {
        // No rules match (a `true` predicate only matches when tested; here the
        // rule list is empty) and no fallback → `None`.
        let provider = RuleBasedStateProvider::new(None, Vec::new());
        let mut random = rivet_util::random::LegacyRandomSource::new(1);
        let optional =
            provider.get_optional_state(&TestLevel, &mut random, &BlockPos::new(0, 0, 0));
        assert!(optional.is_none());
        // `getState` therefore falls through to `level.getBlockState(pos)` —
        // the honest #399 seam, which fails explicitly (never fabricated).
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            provider.get_state(&TestLevel, &mut random, &BlockPos::new(0, 0, 0))
        }));
        assert!(result.is_err());
    }

    #[test]
    fn builder_collects_rules_in_order() {
        let provider = RuleBasedStateProvider::builder()
            .if_true_then_provide(always_true(), simple_source(stone()))
            .if_true_then_provide_state(always_true(), air())
            .build();
        assert_eq!(provider.rules.len(), 2);
        // The first rule matches (`true`), so its `then` wins.
        let mut random = rivet_util::random::LegacyRandomSource::new(1);
        let state = provider.get_state(&TestLevel, &mut random, &BlockPos::new(0, 0, 0));
        assert_eq!(state, stone());
    }

    struct TestLevel;

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            // RivetTodo(#399): never read here.
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }
}
