//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.RuleTest`
//! (abstract class, 26.2).
//!
//! Java is the abstract base of every template-system rule test: it carries
//! the dispatch codec `CODEC` (`BuiltInRegistries.RULE_TEST.byNameCodec()
//! .dispatch("predicate_type", RuleTest::getType, RuleTestType::codec)`), the
//! `testAgainstWorldState` shell (`this.test(level.getBlockState(pos), random)`),
//! and the abstract `test(BlockState, RandomSource)`/`getType()` pair. The Rust
//! port mirrors `BlockPredicate`'s identity split: [`RuleTest`] is the generic
//! behavior contract (its `test` draws from the random source, so it is *not*
//! object-safe — `RandomSource` is `Sized`), [`ErasedRuleTest`] is the
//! object-safe carrier the dispatch codec's `Arc<dyn ErasedRuleTest>` value is,
//! and the concrete `MapCodec`s are resolved by an in-module dispatch table.
//! `Any` (supertrait) enables the dispatch codec's downcast of an erased value
//! back to its concrete type on encode, via the explicit [`RuleTest::as_any`]
//! seam (the same pattern `BlockPredicate` uses).
//!
//! The registry `getType()`-by-name error and the missing-`predicate_type`-key
//! error reproduce Paper's exactly (see `rule_test_codec` / the by-name codec
//! below).
//!
//! `testAgainstWorldState` reads `level.getBlockState(pos)`. As in
//! `blockpredicates`, the real world-access is not ported (RivetTodo #399), so
//! the shell resolves through the [`crate::level::WorldGenLevel::get_block_state`]
//! seam — the same capability-unavailable boundary: `AlwaysTrueTest` overrides
//! it to return `true` without touching the level; every other rule test
//! surfaces the unavailable capability through the seam.

use crate::level::WorldGenLevel;
use crate::levelgen::structure::templatesystem::always_true_test::AlwaysTrueTest;
use crate::levelgen::structure::templatesystem::block_match_test::BlockMatchTest;
use crate::levelgen::structure::templatesystem::block_state_match_test::BlockStateMatchTest;
use crate::levelgen::structure::templatesystem::random_block_match_test::RandomBlockMatchTest;
use crate::levelgen::structure::templatesystem::random_block_state_match_test::RandomBlockStateMatchTest;
use crate::levelgen::structure::templatesystem::rule_test_type::{
    RuleTestTypeId, RuleTestTypes, rule_test_type_by_name,
};
use crate::levelgen::structure::templatesystem::tag_match_test::TagMatchTest;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_util::RandomSource;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.structure.templatesystem.RuleTest` — the
/// behavior contract of every template rule test.
///
/// `test` is generic over the random source (`RandomSource` is `Sized`), so
/// concrete rule tests are dispatched monomorphically by the caller, not
/// through a `dyn`. `type_id` is the registry-held `RuleTestType<?>` identity
/// the dispatch codec keys on; `as_any` is the downcast seam for encode.
pub trait RuleTest: Any + Debug + Send + Sync + 'static {
    /// `RuleTest.test(BlockState, RandomSource)` — the rule's truth value for a
    /// block state.
    fn test<R: RandomSource>(&self, state: &BlockState, random: &mut R) -> bool;

    /// `type()` — the registry-held `RuleTestType<?>` identity this rule test
    /// dispatches on (the key `RuleTest.CODEC` uses).
    fn type_id(&self) -> RuleTestTypeId;

    /// `as_any` — the downcast seam (Java's erased `RuleTest` cast) the
    /// dispatch codec uses on encode to recover the concrete rule test type.
    fn as_any(&self) -> &dyn Any;

    /// `RuleTest.testAgainstWorldState(LevelReader, BlockPos, RandomSource)` —
    /// the Java shell: `this.test(level.getBlockState(pos), random)`.
    ///
    /// The default impl resolves `getBlockState` through the
    /// capability-unavailable seam ([`crate::level::WorldGenLevel::get_block_state`],
    /// RivetTodo #399): no production world provides it yet, so calling through
    /// panics rather than fabricating a state (the same explicit seam
    /// `blockpredicates` uses). Like Java, the shell is a trait method so the
    /// override can dispatch: `AlwaysTrueTest` overrides it to return `true`
    /// without touching the level; every other rule test surfaces the
    /// unavailable capability through the seam.
    fn test_against_world_state<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        pos: &BlockPos,
        random: &mut R,
    ) -> bool {
        let state = level.get_block_state(pos);
        self.test(&state, random)
    }
}

/// The object-safe carrier the dispatch codec (de)serializes — the Rust
/// analogue of Java's `RuleTest` value. Every `RuleTest` implements it via the
/// blanket impl, so the concrete leaf units only implement `RuleTest`.
///
/// Erased evaluation is deferred: `test` is not object-safe (`RandomSource` is
/// `Sized`) and no erased-path dispatch is ported (Java reaches `test` through
/// the abstract method's polymorphic call; the port's concrete leaf types are
/// known statically). A consumer holding an `Arc<dyn ErasedRuleTest>` can
/// re-encode it but must downcast to a concrete `RuleTest` to evaluate it.
pub trait ErasedRuleTest: Any + Debug + Send + Sync + 'static {
    /// `type()` — the registry-held type identity.
    fn type_id(&self) -> RuleTestTypeId;

    /// `as_any` — the downcast seam for encode.
    fn as_any(&self) -> &dyn Any;
}

impl<T: RuleTest + ?Sized> ErasedRuleTest for T {
    fn type_id(&self) -> RuleTestTypeId {
        RuleTest::type_id(self)
    }

    fn as_any(&self) -> &dyn Any {
        RuleTest::as_any(self)
    }
}

/// `RuleTest.CODEC` — the dispatch codec, as the ops-generic
/// `rule_test_codec::<Ops>()` factory (the same shape every static `CODEC`
/// constant takes in this codebase). `RuleTest` is not self-referential, so
/// this is a plain (non-`recursive`) dispatch.
pub fn rule_test_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Arc<dyn ErasedRuleTest>, Ops>>
{
    let dispatch = key_dispatch_codec::dispatch_map::<RuleTestTypeId, Arc<dyn ErasedRuleTest>, Ops>(
        "predicate_type",
        rule_test_type_by_name_codec::<Ops>(),
        Arc::new(|t: &Arc<dyn ErasedRuleTest>| DataResult::success(ErasedRuleTest::type_id(&**t))),
        codec_for_type(),
    );
    map_codec::codec_of(dispatch)
}

/// `RuleTestType::codec` — resolve a `RuleTestTypeId` to its
/// `MapCodec<Arc<dyn ErasedRuleTest>>` (the dispatch's `codec` function). All
/// six Paper types are in this unit's scope, so every registered type resolves.
fn codec_for_type<Ops: DynamicOps + 'static>()
-> key_dispatch_codec::CodecFn<RuleTestTypeId, Arc<dyn ErasedRuleTest>, Ops> {
    Arc::new(move |k: &RuleTestTypeId| {
        if *k == RuleTestTypes::ALWAYS_TRUE_TEST {
            DataResult::success(always_true_map_codec::<Ops>())
        } else if *k == RuleTestTypes::BLOCK_TEST {
            DataResult::success(block_match_map_codec::<Ops>())
        } else if *k == RuleTestTypes::BLOCKSTATE_TEST {
            DataResult::success(block_state_match_map_codec::<Ops>())
        } else if *k == RuleTestTypes::TAG_TEST {
            DataResult::success(tag_match_map_codec::<Ops>())
        } else if *k == RuleTestTypes::RANDOM_BLOCK_TEST {
            DataResult::success(random_block_match_map_codec::<Ops>())
        } else if *k == RuleTestTypes::RANDOM_BLOCKSTATE_TEST {
            DataResult::success(random_block_state_match_map_codec::<Ops>())
        } else {
            DataResult::error(format!(
                "Unknown registry key in ResourceKey[minecraft:root / minecraft:rule_test]: {}",
                k.location
            ))
        }
    })
}

/// `BuiltInRegistries.RULE_TEST.byNameCodec()` over the erased id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key().
/// identifier())`.
///
/// The unknown-key error reproduces Paper's exactly: `"Unknown registry key in "
/// + this.key() + ": " + name` where `this.key()` is `Registries.RULE_TEST`
/// (`createRegistryKey("rule_test")`, toString
/// `"ResourceKey[minecraft:root / minecraft:rule_test]"`).
pub fn rule_test_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<RuleTestTypeId, Ops>> {
    codec::comap_flat_map::<rivet_registry::Identifier, RuleTestTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &rivet_registry::Identifier| {
            match rule_test_type_by_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:rule_test]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &RuleTestTypeId| rivet_registry::Identifier::parse(id.location)),
    )
}

/// Lift a concrete rule test's `MapCodec<C>` to
/// `MapCodec<Arc<dyn ErasedRuleTest>>` — Java's `MapCodec<? extends RuleTest>`
/// variance, via xmap (the same lift `block_predicate` performs).
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn ErasedRuleTest>, Ops>>
where
    C: RuleTest + Clone + 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(
        inner,
        Arc::new(|c: &C| -> Arc<dyn ErasedRuleTest> { Arc::new(c.clone()) }),
        Arc::new(downcast_erased::<C>),
    )
}

/// The encode-side `from` of the erase lift: downcast the erased value to its
/// concrete rule test (safe — the dispatch guarantees the value's type).
fn downcast_erased<C: RuleTest + Clone + 'static>(t: &Arc<dyn ErasedRuleTest>) -> C {
    t.as_any()
        .downcast_ref::<C>()
        .expect("rule test codec applied to a rule test of a different type")
        .clone()
}

/// `AlwaysTrueTest.CODEC`, erased to `MapCodec<Arc<dyn ErasedRuleTest>>`.
fn always_true_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn ErasedRuleTest>, Ops>> {
    erase_map_codec::<AlwaysTrueTest, Ops>(
        crate::levelgen::structure::templatesystem::always_true_test::always_true_test_map_codec(),
    )
}

/// `BlockMatchTest.CODEC`, erased to `MapCodec<Arc<dyn ErasedRuleTest>>`.
fn block_match_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn ErasedRuleTest>, Ops>> {
    erase_map_codec::<BlockMatchTest, Ops>(
        crate::levelgen::structure::templatesystem::block_match_test::block_match_test_map_codec(),
    )
}

/// `BlockStateMatchTest.CODEC`, erased to `MapCodec<Arc<dyn ErasedRuleTest>>`.
fn block_state_match_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn ErasedRuleTest>, Ops>> {
    erase_map_codec::<BlockStateMatchTest, Ops>(crate::levelgen::structure::templatesystem::block_state_match_test::block_state_match_test_map_codec())
}

/// `TagMatchTest.CODEC`, erased to `MapCodec<Arc<dyn ErasedRuleTest>>`.
fn tag_match_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn ErasedRuleTest>, Ops>> {
    erase_map_codec::<TagMatchTest, Ops>(
        crate::levelgen::structure::templatesystem::tag_match_test::tag_match_test_map_codec(),
    )
}

/// `RandomBlockMatchTest.CODEC`, erased to `MapCodec<Arc<dyn ErasedRuleTest>>`.
fn random_block_match_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn ErasedRuleTest>, Ops>> {
    erase_map_codec::<RandomBlockMatchTest, Ops>(crate::levelgen::structure::templatesystem::random_block_match_test::random_block_match_test_map_codec())
}

/// `RandomBlockStateMatchTest.CODEC`, erased to
/// `MapCodec<Arc<dyn ErasedRuleTest>>`.
fn random_block_state_match_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn ErasedRuleTest>, Ops>> {
    erase_map_codec::<RandomBlockStateMatchTest, Ops>(crate::levelgen::structure::templatesystem::random_block_state_match_test::random_block_state_match_test_map_codec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// A minimal `WorldGenLevel` double whose `get_block_state` is the
    /// unavailable capability (RivetTodo #399) — it panics, exactly like every
    /// production `WorldGenLevel` before the real world-access lands.
    #[derive(Clone, Copy)]
    struct CapabilityGapLevel;

    impl LevelHeightAccessor for CapabilityGapLevel {
        fn get_height(&self) -> i32 {
            384
        }
        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for CapabilityGapLevel {
        fn get_seed(&self) -> i64 {
            0
        }
        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    /// A minimal `RuleTest` whose identity is only the type id — the erased
    /// carrier must carry it through the blanket `ErasedRuleTest` impl.
    #[derive(Debug, Clone)]
    struct IdentityTest(RuleTestTypeId);

    impl RuleTest for IdentityTest {
        fn test<R: RandomSource>(&self, _state: &BlockState, _random: &mut R) -> bool {
            false
        }
        fn type_id(&self) -> RuleTestTypeId {
            self.0.clone()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn erased_carrier_forwards_the_type_identity() {
        let t = IdentityTest(RuleTestTypes::BLOCK_TEST.clone());
        let erased: &dyn ErasedRuleTest = &t;
        assert_eq!(ErasedRuleTest::type_id(erased), RuleTestTypes::BLOCK_TEST);
    }

    #[test]
    fn test_against_world_state_fails_loudly_when_world_access_unavailable() {
        // `testAgainstWorldState` resolves `level.getBlockState(pos)` — a
        // capability no production world provides yet — and must fail loudly
        // (never fabricate a state). `AlwaysTrueTest` overrides the shell to
        // avoid the seam; a generic rule test does not.
        let t = IdentityTest(RuleTestTypes::BLOCK_TEST);
        let origin = BlockPos::new(0, 0, 0);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            t.test_against_world_state(&CapabilityGapLevel, &origin, &mut random)
        }));
        assert!(
            result.is_err(),
            "testAgainstWorldState must fail loudly, not fabricate a state"
        );
    }

    fn type_id_of(t: &Arc<dyn ErasedRuleTest>) -> RuleTestTypeId {
        ErasedRuleTest::type_id(&**t)
    }

    #[test]
    fn dispatch_missing_predicate_type_key_errors() {
        let codec = rule_test_codec::<JsonOps>();
        let input = json!({"block": "minecraft:stone"});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        // Java: `fieldOf("predicate_type")` missing → "No key predicate_type in ...".
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key predicate_type"), "got: {msg}");
    }

    #[test]
    fn dispatch_unknown_type_errors_like_by_name_codec() {
        let codec = rule_test_codec::<JsonOps>();
        let input = json!({"predicate_type": "minecraft:not_a_type"});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:rule_test]: minecraft:not_a_type"),
            "got: {msg}"
        );
    }

    #[test]
    fn by_name_codec_resolves_every_registered_type() {
        let codec = rule_test_type_by_name_codec::<JsonOps>();
        for id in [
            RuleTestTypes::ALWAYS_TRUE_TEST,
            RuleTestTypes::BLOCK_TEST,
            RuleTestTypes::BLOCKSTATE_TEST,
            RuleTestTypes::TAG_TEST,
            RuleTestTypes::RANDOM_BLOCK_TEST,
            RuleTestTypes::RANDOM_BLOCKSTATE_TEST,
        ] {
            let decoded = codec
                .parse(&JsonOps::INSTANCE, &json!(id.location))
                .result()
                .expect("decode should succeed")
                .clone();
            assert_eq!(decoded, id);
        }
    }

    #[test]
    fn always_true_codec_round_trips_and_encodes_empty_map() {
        // `AlwaysTrueTest.CODEC = MapCodec.unit(INSTANCE)` — encodes to `{}`,
        // and the dispatch writes the `"predicate_type"` key after the value.
        let codec = rule_test_codec::<JsonOps>();
        let t: Arc<dyn ErasedRuleTest> = Arc::new(AlwaysTrueTest::INSTANCE);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &t)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"predicate_type": "minecraft:always_true"}));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(type_id_of(&decoded), RuleTestTypes::ALWAYS_TRUE_TEST);
    }

    #[test]
    fn dispatch_round_trips_blockstate_match() {
        // A record-based rule test round-trips through the full dispatch codec:
        // `"predicate_type"` picks the blockstate_match entry, whose
        // `BlockState.CODEC` half is the real ported codec.
        use crate::levelgen::structure::templatesystem::block_state_match_test::BlockStateMatchTest;
        use rivet_registry::generated::blocks::BlockId;

        let codec = rule_test_codec::<JsonOps>();
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let t: Arc<dyn ErasedRuleTest> = Arc::new(BlockStateMatchTest::new(stone));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &t)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "predicate_type": "minecraft:blockstate_match",
                "block_state": {"Name": "minecraft:stone"}
            })
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(type_id_of(&decoded), RuleTestTypes::BLOCKSTATE_TEST);
        let as_bsm = decoded
            .as_any()
            .downcast_ref::<BlockStateMatchTest>()
            .expect("decoded blockstate_match");
        assert_eq!(as_bsm.block_state, stone);
    }

    #[test]
    fn dispatch_round_trips_random_blockstate_match() {
        use crate::levelgen::structure::templatesystem::random_block_state_match_test::RandomBlockStateMatchTest;
        use rivet_registry::generated::blocks::BlockId;

        let codec = rule_test_codec::<JsonOps>();
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let t: Arc<dyn ErasedRuleTest> = Arc::new(RandomBlockStateMatchTest::new(stone, 0.5));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &t)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "predicate_type": "minecraft:random_blockstate_match",
                "block_state": {"Name": "minecraft:stone"},
                "probability": 0.5
            })
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(type_id_of(&decoded), RuleTestTypes::RANDOM_BLOCKSTATE_TEST);
        let as_rbsm = decoded
            .as_any()
            .downcast_ref::<RandomBlockStateMatchTest>()
            .expect("decoded random_blockstate_match");
        assert_eq!(as_rbsm.block_state, stone);
        assert_eq!(as_rbsm.probability, 0.5);
    }
}
