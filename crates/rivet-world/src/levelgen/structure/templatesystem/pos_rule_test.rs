//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.PosRuleTest`
//! (abstract class, 26.2).
//!
//! Java is the abstract base of every template-system position rule test: it
//! carries the dispatch codec `CODEC` (`BuiltInRegistries.POS_RULE_TEST
//! .byNameCodec().dispatch("predicate_type", PosRuleTest::getType,
//! PosRuleTestType::codec)`) and the abstract `test(BlockPos inTemplatePos,
//! BlockPos worldPos, BlockPos worldReference, RandomSource)`/`getType()` pair.
//! The Rust port mirrors `RuleTest`'s identity split: [`PosRuleTest`] is the
//! generic behavior contract (its `test` draws from the random source, so it
//! is *not* object-safe — `RandomSource` is `Sized`), [`ErasedPosRuleTest`] is
//! the object-safe carrier the dispatch codec's `Arc<dyn ErasedPosRuleTest>`
//! value is, and the concrete `MapCodec`s are resolved by an in-module dispatch
//! table. `Any` (supertrait) enables the dispatch codec's downcast of an
//! erased value back to its concrete type on encode.

use crate::levelgen::structure::templatesystem::axis_aligned_linear_pos_test::AxisAlignedLinearPosTest;
use crate::levelgen::structure::templatesystem::linear_pos_test::LinearPosTest;
use crate::levelgen::structure::templatesystem::pos_always_true_test::PosAlwaysTrueTest;
use crate::levelgen::structure::templatesystem::pos_rule_test_type::{
    PosRuleTestTypeId, PosRuleTestTypes, pos_rule_test_type_by_name,
};
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

/// `net.minecraft.world.level.levelgen.structure.templatesystem.PosRuleTest` —
/// the behavior contract of every template position rule test.
///
/// `test` is generic over the random source (`RandomSource` is `Sized`), so
/// concrete position rule tests are dispatched monomorphically by the caller,
/// not through a `dyn`. `type_id` is the registry-held `PosRuleTestType<?>`
/// identity the dispatch codec keys on; `as_any` is the downcast seam for
/// encode.
pub trait PosRuleTest: Any + Debug + Send + Sync + 'static {
    /// `PosRuleTest.test(BlockPos inTemplatePos, BlockPos worldPos, BlockPos
    /// worldReference, RandomSource)` — the rule's truth value for a position
    /// triple.
    fn test<R: RandomSource>(
        &self,
        in_template_pos: &BlockPos,
        world_pos: &BlockPos,
        world_reference: &BlockPos,
        random: &mut R,
    ) -> bool;

    /// `type()` — the registry-held `PosRuleTestType<?>` identity this
    /// position rule test dispatches on (the key `PosRuleTest.CODEC` uses).
    fn type_id(&self) -> PosRuleTestTypeId;

    /// `as_any` — the downcast seam (Java's erased `PosRuleTest` cast) the
    /// dispatch codec uses on encode to recover the concrete rule test type.
    fn as_any(&self) -> &dyn Any;
}

/// The object-safe carrier the dispatch codec (de)serializes — the Rust
/// analogue of Java's `PosRuleTest` value. Every `PosRuleTest` implements it
/// via the blanket impl, so the concrete leaf units only implement
/// `PosRuleTest`.
pub trait ErasedPosRuleTest: Any + Debug + Send + Sync + 'static {
    /// `type()` — the registry-held type identity.
    fn type_id(&self) -> PosRuleTestTypeId;

    /// `as_any` — the downcast seam for encode.
    fn as_any(&self) -> &dyn Any;
}

impl<T: PosRuleTest + ?Sized> ErasedPosRuleTest for T {
    fn type_id(&self) -> PosRuleTestTypeId {
        PosRuleTest::type_id(self)
    }

    fn as_any(&self) -> &dyn Any {
        PosRuleTest::as_any(self)
    }
}

/// `PosRuleTest.CODEC` — the dispatch codec, as the ops-generic
/// `pos_rule_test_codec::<Ops>()` factory. `PosRuleTest` is not
/// self-referential, so this is a plain (non-`recursive`) dispatch.
pub fn pos_rule_test_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<Arc<dyn ErasedPosRuleTest>, Ops>> {
    let dispatch =
        key_dispatch_codec::dispatch_map::<PosRuleTestTypeId, Arc<dyn ErasedPosRuleTest>, Ops>(
            "predicate_type",
            pos_rule_test_type_by_name_codec::<Ops>(),
            Arc::new(|t: &Arc<dyn ErasedPosRuleTest>| {
                DataResult::success(ErasedPosRuleTest::type_id(&**t))
            }),
            codec_for_type(),
        );
    map_codec::codec_of(dispatch)
}

/// `PosRuleTestType::codec` — resolve a `PosRuleTestTypeId` to its
/// `MapCodec<Arc<dyn ErasedPosRuleTest>>` (the dispatch's `codec` function).
/// All three Paper types are in this unit's scope, so every registered type
/// resolves.
fn codec_for_type<Ops: DynamicOps + 'static>()
-> key_dispatch_codec::CodecFn<PosRuleTestTypeId, Arc<dyn ErasedPosRuleTest>, Ops> {
    Arc::new(move |k: &PosRuleTestTypeId| {
        if *k == PosRuleTestTypes::ALWAYS_TRUE_TEST {
            DataResult::success(pos_always_true_map_codec::<Ops>())
        } else if *k == PosRuleTestTypes::LINEAR_POS_TEST {
            DataResult::success(linear_pos_map_codec::<Ops>())
        } else if *k == PosRuleTestTypes::AXIS_ALIGNED_LINEAR_POS_TEST {
            DataResult::success(axis_aligned_linear_pos_map_codec::<Ops>())
        } else {
            DataResult::error(format!(
                "Unknown registry key in ResourceKey[minecraft:root / minecraft:pos_rule_test]: {}",
                k.location
            ))
        }
    })
}

/// `BuiltInRegistries.POS_RULE_TEST.byNameCodec()` over the erased id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key().
/// identifier())`.
///
/// The unknown-key error reproduces Paper's exactly: `"Unknown registry key in "
/// + this.key() + ": " + name` where `this.key()` is `Registries.POS_RULE_TEST`
/// (`createRegistryKey("pos_rule_test")`, toString
/// `"ResourceKey[minecraft:root / minecraft:pos_rule_test]"`).
pub fn pos_rule_test_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<PosRuleTestTypeId, Ops>> {
    codec::comap_flat_map::<rivet_registry::Identifier, PosRuleTestTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &rivet_registry::Identifier| {
            match pos_rule_test_type_by_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:pos_rule_test]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &PosRuleTestTypeId| rivet_registry::Identifier::parse(id.location)),
    )
}

/// Lift a concrete position rule test's `MapCodec<C>` to
/// `MapCodec<Arc<dyn ErasedPosRuleTest>>` — Java's
/// `MapCodec<? extends PosRuleTest>` variance, via xmap.
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn ErasedPosRuleTest>, Ops>>
where
    C: PosRuleTest + Clone + 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(
        inner,
        Arc::new(|c: &C| -> Arc<dyn ErasedPosRuleTest> { Arc::new(c.clone()) }),
        Arc::new(downcast_erased::<C>),
    )
}

/// The encode-side `from` of the erase lift: downcast the erased value to its
/// concrete position rule test (safe — the dispatch guarantees the value's
/// type).
fn downcast_erased<C: PosRuleTest + Clone + 'static>(t: &Arc<dyn ErasedPosRuleTest>) -> C {
    t.as_any()
        .downcast_ref::<C>()
        .expect("pos rule test codec applied to a pos rule test of a different type")
        .clone()
}

/// `PosAlwaysTrueTest.CODEC`, erased to `MapCodec<Arc<dyn ErasedPosRuleTest>>`.
fn pos_always_true_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn ErasedPosRuleTest>, Ops>> {
    erase_map_codec::<PosAlwaysTrueTest, Ops>(crate::levelgen::structure::templatesystem::pos_always_true_test::pos_always_true_test_map_codec())
}

/// `LinearPosTest.CODEC`, erased to `MapCodec<Arc<dyn ErasedPosRuleTest>>`.
fn linear_pos_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn ErasedPosRuleTest>, Ops>> {
    erase_map_codec::<LinearPosTest, Ops>(
        crate::levelgen::structure::templatesystem::linear_pos_test::linear_pos_test_map_codec(),
    )
}

/// `AxisAlignedLinearPosTest.CODEC`, erased to
/// `MapCodec<Arc<dyn ErasedPosRuleTest>>`.
fn axis_aligned_linear_pos_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn ErasedPosRuleTest>, Ops>> {
    erase_map_codec::<AxisAlignedLinearPosTest, Ops>(crate::levelgen::structure::templatesystem::axis_aligned_linear_pos_test::axis_aligned_linear_pos_test_map_codec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// A minimal `PosRuleTest` whose identity is only the type id — the erased
    /// carrier must carry it through the blanket `ErasedPosRuleTest` impl.
    #[derive(Debug, Clone)]
    struct IdentityTest(PosRuleTestTypeId);

    impl PosRuleTest for IdentityTest {
        fn test<R: RandomSource>(
            &self,
            _in_template_pos: &BlockPos,
            _world_pos: &BlockPos,
            _world_reference: &BlockPos,
            _random: &mut R,
        ) -> bool {
            false
        }
        fn type_id(&self) -> PosRuleTestTypeId {
            self.0.clone()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn erased_carrier_forwards_the_type_identity() {
        let t = IdentityTest(PosRuleTestTypes::LINEAR_POS_TEST.clone());
        let erased: &dyn ErasedPosRuleTest = &t;
        assert_eq!(
            ErasedPosRuleTest::type_id(erased),
            PosRuleTestTypes::LINEAR_POS_TEST
        );
    }

    fn type_id_of(t: &Arc<dyn ErasedPosRuleTest>) -> PosRuleTestTypeId {
        ErasedPosRuleTest::type_id(&**t)
    }

    #[test]
    fn dispatch_missing_predicate_type_key_errors() {
        let codec = pos_rule_test_codec::<JsonOps>();
        let input = json!({"min_chance": 0.0});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key predicate_type"), "got: {msg}");
    }

    #[test]
    fn dispatch_unknown_type_errors_like_by_name_codec() {
        let codec = pos_rule_test_codec::<JsonOps>();
        let input = json!({"predicate_type": "minecraft:not_a_type"});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:pos_rule_test]: minecraft:not_a_type"),
            "got: {msg}"
        );
    }

    #[test]
    fn by_name_codec_resolves_every_registered_type() {
        let codec = pos_rule_test_type_by_name_codec::<JsonOps>();
        for id in [
            PosRuleTestTypes::ALWAYS_TRUE_TEST,
            PosRuleTestTypes::LINEAR_POS_TEST,
            PosRuleTestTypes::AXIS_ALIGNED_LINEAR_POS_TEST,
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
    fn pos_always_true_codec_round_trips_and_encodes_empty_map() {
        // `PosAlwaysTrueTest.CODEC = MapCodec.unit(INSTANCE)` — encodes to `{}`,
        // and the dispatch writes the `"predicate_type"` key after the value.
        let codec = pos_rule_test_codec::<JsonOps>();
        let t: Arc<dyn ErasedPosRuleTest> = Arc::new(PosAlwaysTrueTest::INSTANCE);
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
        assert_eq!(type_id_of(&decoded), PosRuleTestTypes::ALWAYS_TRUE_TEST);
    }
}
