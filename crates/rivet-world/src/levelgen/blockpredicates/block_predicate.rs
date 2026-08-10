//! Port of `net.minecraft.world.level.levelgen.blockpredicates.BlockPredicate`
//! (interface, 26.2).
//!
//! Java is the interface every block predicate implements (a
//! `BiPredicate<WorldGenLevel, BlockPos>` plus a `type()`), and its `CODEC` is
//! the dispatch root of the whole framework:
//!
//! ```text
//! CODEC = BuiltInRegistries.BLOCK_PREDICATE_TYPE.byNameCodec()
//!            .dispatch(BlockPredicate::type, BlockPredicateType::codec)
//! ```
//!
//! i.e. `fieldOf("type").dispatch(...)`: the `"type"` field names the predicate
//! type (via the by-name registry codec), whose `MapCodec` then applies to the
//! whole map. The Rust port mirrors `Feature`/`PlacementModifier`'s identity
//! split: [`BlockPredicate`] is the object-safe behavior contract, its registry
//! identity is the erased [`BlockPredicateTypeId`] handle, and the value that
//! combinators store (Java's `List<BlockPredicate>`) is the erased carrier
//! `Arc<dyn BlockPredicate>`.
//!
//! The recursive structure (an `all_of` contains `BlockPredicate`s) is threaded
//! through [`block_predicate_codec`]'s `codec::recursive` graph exactly like
//! `ComponentSerialization`'s: the combinators receive the single shared `top`
//! (the `RecursiveSelf` of this graph) as the child-element codec, so one graph
//! handles arbitrary nesting.
//!
//! ## Scope boundary (RivetTodo #399)
//!
//! The dispatch resolves codecs for the five in-scope types
//! (`inside_world_bounds`, `any_of`, `all_of`, `not`, `true`); the other nine
//! Paper types (`matching_*`, `has_sturdy_face`, `solid`, `replaceable`,
//! `would_survive`, `unobstructed`) are deferred with their owning units and
//! fail explicitly on the codec lookup (no fabricated behavior). The
//! `ONLY_IN_AIR_PREDICATE`/`ONLY_IN_AIR_OR_WATER_PREDICATE` constants and the
//! `matchesBlocks`/`matchesTag`/`matchesFluids`/`matchesBiomes`/
//! `replaceable`/`wouldSurvive`/`hasSturdyFace`/`solid`/`noFluid`/
//! `unobstructed` static factories defer with those concrete predicates.

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::all_of_predicate::AllOfPredicate;
use crate::levelgen::blockpredicates::any_of_predicate::AnyOfPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::inside_world_bounds_predicate::InsideWorldBoundsPredicate;
use crate::levelgen::blockpredicates::not_predicate::NotPredicate;
use crate::levelgen::blockpredicates::true_block_predicate::TrueBlockPredicate;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Vec3i;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.BlockPredicate` — the
/// behavior contract of every block predicate (Java's `BiPredicate<WorldGenLevel,
/// BlockPos>` + `type()`).
///
/// The erased carrier `Arc<dyn BlockPredicate>` is what combinators store and
/// what the dispatch codec (de)serializes — the Rust analogue of Java's
/// `List<BlockPredicate>` element and `Codec<BlockPredicate>` value. `Any`
/// (supertrait) enables the dispatch codec's downcast of an erased value back
/// to its concrete type on encode, via the explicit [`BlockPredicate::as_any`]
/// seam (the same pattern `AnyRegistry` uses — `Any::type_id` and the dispatch
/// `type_id()` would otherwise collide on an `Arc<dyn BlockPredicate>`).
pub trait BlockPredicate: Any + Debug + Send + Sync + 'static {
    /// `BlockPredicate.test(WorldGenLevel, BlockPos)` — the predicate's truth
    /// value at a world position (Java `BiPredicate.test`).
    fn test(&self, level: &dyn WorldGenLevel, origin: &BlockPos) -> bool;

    /// `type()` — the registry-held `BlockPredicateType<?>` identity this
    /// predicate dispatches on (the key `BlockPredicate.CODEC` uses).
    fn type_id(&self) -> BlockPredicateTypeId;

    /// `as_any` — the downcast seam (Java's erased `BlockPredicate` cast) the
    /// dispatch codec uses on encode to recover the concrete predicate type.
    fn as_any(&self) -> &dyn Any;
}

/// `BlockPredicate.allOf(List<BlockPredicate>)` — the `all_of` combinator.
pub fn all_of(predicates: Vec<Arc<dyn BlockPredicate>>) -> AllOfPredicate {
    AllOfPredicate::new(predicates)
}

/// `BlockPredicate.anyOf(List<BlockPredicate>)` — the `any_of` combinator.
pub fn any_of(predicates: Vec<Arc<dyn BlockPredicate>>) -> AnyOfPredicate {
    AnyOfPredicate::new(predicates)
}

/// `BlockPredicate.not(BlockPredicate)` — the `not` combinator.
pub fn not(predicate: Arc<dyn BlockPredicate>) -> NotPredicate {
    NotPredicate::new(predicate)
}

/// `BlockPredicate.alwaysTrue()` — the `true` predicate singleton.
pub fn always_true() -> Arc<dyn BlockPredicate> {
    Arc::new(TrueBlockPredicate::instance())
}

/// `BlockPredicate.insideWorld(Vec3i)` — the `inside_world_bounds` predicate.
pub fn inside_world(offset: Vec3i) -> InsideWorldBoundsPredicate {
    InsideWorldBoundsPredicate::new(offset)
}

/// `BlockPredicate.CODEC` — the recursive dispatch codec, as the ops-generic
/// `block_predicate_codec::<Ops>()` factory (the same shape every static
/// `CODEC` constant takes in this codebase).
pub fn block_predicate_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<Arc<dyn BlockPredicate>, Ops>> {
    codec::recursive("BlockPredicate".to_string(), Arc::new(create_dispatch))
}

/// The non-recursive dispatch body given the `RecursiveSelf` (`top`): the
/// `"type"` by-name dispatch. Every combinator that recurses into
/// `BlockPredicate.CODEC` receives `top` as the child-element codec so the
/// whole nested graph shares this single recursive codec.
fn create_dispatch<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn BlockPredicate>, Ops>>,
) -> Arc<dyn Codec<Arc<dyn BlockPredicate>, Ops>> {
    let dispatch =
        key_dispatch_codec::dispatch_map::<BlockPredicateTypeId, Arc<dyn BlockPredicate>, Ops>(
            "type",
            block_predicate_type_by_name_codec::<Ops>(),
            Arc::new(|p: &Arc<dyn BlockPredicate>| {
                DataResult::success(BlockPredicate::type_id(&**p))
            }),
            codec_for_type(top),
        );
    map_codec::codec_of(dispatch)
}

/// `BlockPredicateType::codec` — resolve a `BlockPredicateTypeId` to its
/// `MapCodec<Arc<dyn BlockPredicate>>` (the dispatch's `codec` function).
///
/// The five in-scope types resolve to their codecs; the other nine Paper types
/// are not ported (RivetTodo #399) and fail explicitly — never fabricating a
/// behavior.
fn codec_for_type<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn BlockPredicate>, Ops>>,
) -> key_dispatch_codec::CodecFn<BlockPredicateTypeId, Arc<dyn BlockPredicate>, Ops> {
    Arc::new(move |k: &BlockPredicateTypeId| {
        if *k == BlockPredicateTypes::ALL_OF {
            DataResult::success(all_of_map_codec(top.clone()))
        } else if *k == BlockPredicateTypes::ANY_OF {
            DataResult::success(any_of_map_codec(top.clone()))
        } else if *k == BlockPredicateTypes::NOT {
            DataResult::success(not_map_codec(top.clone()))
        } else if *k == BlockPredicateTypes::TRUE {
            DataResult::success(true_map_codec())
        } else if *k == BlockPredicateTypes::INSIDE_WORLD_BOUNDS {
            DataResult::success(inside_world_bounds_map_codec())
        } else {
            DataResult::error(format!(
                "Block predicate type '{}' is not ported (RivetTodo #399)",
                k.location
            ))
        }
    })
}

/// `BuiltInRegistries.BLOCK_PREDICATE_TYPE.byNameCodec()` over the erased id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key().
/// identifier())`.
///
/// The unknown-key error reproduces Paper's exactly: `"Unknown registry key in "
/// + this.key() + ": " + name` where `this.key()` is `Registries.BLOCK_PREDICATE_TYPE`
/// (`createRegistryKey("block_predicate_type")`, toString
/// `"ResourceKey[minecraft:root / minecraft:block_predicate_type]"`).
pub fn block_predicate_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<BlockPredicateTypeId, Ops>> {
    codec::comap_flat_map::<rivet_registry::Identifier, BlockPredicateTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &rivet_registry::Identifier| {
            match crate::levelgen::blockpredicates::block_predicate_type::block_predicate_type_by_name(
                &name.to_string(),
            ) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:block_predicate_type]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &BlockPredicateTypeId| rivet_registry::Identifier::parse(id.location)),
    )
}

/// Lift a concrete predicate's `MapCodec<C>` to `MapCodec<Arc<dyn BlockPredicate>>`
/// — Java's `MapCodec<? extends BlockPredicate>` variance, via xmap (the same
/// lift `PlainTextContents.map_codec` performs onto its enum).
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn BlockPredicate>, Ops>>
where
    C: BlockPredicate + Clone + 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(
        inner,
        Arc::new(|c: &C| -> Arc<dyn BlockPredicate> { Arc::new(c.clone()) }),
        Arc::new(downcast_erased::<C>),
    )
}

/// The encode-side `from` of the erase lift: downcast the erased value to its
/// concrete predicate (safe — the dispatch guarantees the value's type).
fn downcast_erased<C: BlockPredicate + Clone + 'static>(p: &Arc<dyn BlockPredicate>) -> C {
    p.as_any()
        .downcast_ref::<C>()
        .expect("block predicate codec applied to a predicate of a different type")
        .clone()
}

/// `AllOfPredicate.CODEC`, erased to `MapCodec<Arc<dyn BlockPredicate>>`.
fn all_of_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn BlockPredicate>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn BlockPredicate>, Ops>> {
    erase_map_codec::<AllOfPredicate, Ops>(
        crate::levelgen::blockpredicates::combining_predicate::combining_predicate_codec::<
            AllOfPredicate,
            Ops,
        >(Arc::new(AllOfPredicate::new), top),
    )
}

/// `AnyOfPredicate.CODEC`, erased to `MapCodec<Arc<dyn BlockPredicate>>`.
fn any_of_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn BlockPredicate>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn BlockPredicate>, Ops>> {
    erase_map_codec::<AnyOfPredicate, Ops>(
        crate::levelgen::blockpredicates::combining_predicate::combining_predicate_codec::<
            AnyOfPredicate,
            Ops,
        >(Arc::new(AnyOfPredicate::new), top),
    )
}

/// `NotPredicate.CODEC`, erased to `MapCodec<Arc<dyn BlockPredicate>>`.
fn not_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn BlockPredicate>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn BlockPredicate>, Ops>> {
    erase_map_codec::<NotPredicate, Ops>(
        crate::levelgen::blockpredicates::not_predicate::not_predicate_map_codec(top),
    )
}

/// `TrueBlockPredicate.CODEC`, erased to `MapCodec<Arc<dyn BlockPredicate>>`.
fn true_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<Arc<dyn BlockPredicate>, Ops>> {
    erase_map_codec::<TrueBlockPredicate, Ops>(
        crate::levelgen::blockpredicates::true_block_predicate::true_block_predicate_map_codec(),
    )
}

/// `InsideWorldBoundsPredicate.CODEC`, erased to `MapCodec<Arc<dyn BlockPredicate>>`.
fn inside_world_bounds_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn BlockPredicate>, Ops>> {
    erase_map_codec::<InsideWorldBoundsPredicate, Ops>(
        crate::levelgen::blockpredicates::inside_world_bounds_predicate::inside_world_bounds_predicate_map_codec(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::blockpredicates::block_predicate_type::block_predicate_type_by_name;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// A minimal `WorldGenLevel` double over the overworld window (height
    /// access only — the block-state predicates need `get_block_state`, which
    /// no production world provides yet; the combinator/bounds predicates
    /// never touch it).
    #[derive(Clone, Copy)]
    struct TestLevel {
        min_y: i32,
        height: i32,
    }

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.height
        }

        fn get_min_y(&self) -> i32 {
            self.min_y
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            // RivetTodo(#399): the world-access implementation is not ported;
            // the state-testing predicates surface it explicitly. These tests
            // only exercise combinators/bounds, which never read block state.
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    fn overworld() -> TestLevel {
        TestLevel {
            min_y: -64,
            height: 384,
        }
    }

    fn wrap(p: impl BlockPredicate) -> Arc<dyn BlockPredicate> {
        Arc::new(p)
    }

    /// `BlockPredicate::type_id` over the erased carrier — a bare
    /// `.type_id()` on `Arc<dyn BlockPredicate>` resolves to `Any::type_id`.
    fn bp_type_id(p: &Arc<dyn BlockPredicate>) -> BlockPredicateTypeId {
        BlockPredicate::type_id(&**p)
    }

    #[test]
    fn all_of_truth_table() {
        // `all_of([])` is vacuously true; any false child short-circuits to false.
        let empty = wrap(all_of(vec![]));
        assert!(empty.test(&overworld(), &BlockPos::new(0, 0, 0)));
        let level = overworld();
        let origin = BlockPos::new(0, 0, 0);
        // Two always-true children.
        let both = wrap(all_of(vec![always_true(), always_true()]));
        assert!(both.test(&level, &origin));
        // One always-true, one inside-bounds at y=0 (inside overworld) — true.
        let inside = wrap(inside_world(Vec3i::ZERO));
        let comb = wrap(all_of(vec![always_true(), inside]));
        assert!(comb.test(&level, &origin));
        // inside-bounds at y=1000 (outside) — the all_of child is false.
        let far = wrap(inside_world(Vec3i::new(0, 1000, 0)));
        let comb_false = wrap(all_of(vec![always_true(), far]));
        assert!(!comb_false.test(&level, &origin));
    }

    #[test]
    fn any_of_truth_table() {
        let level = overworld();
        let origin = BlockPos::new(0, 0, 0);
        // `any_of([])` is vacuously false.
        let empty = wrap(any_of(vec![]));
        assert!(!empty.test(&level, &origin));
        // Any single true child makes it true.
        let inside = wrap(inside_world(Vec3i::new(0, 1000, 0)));
        let comb = wrap(any_of(vec![inside, always_true()]));
        assert!(comb.test(&level, &origin));
        // All children false → false.
        let far1 = wrap(inside_world(Vec3i::new(0, 1000, 0)));
        let far2 = wrap(inside_world(Vec3i::new(0, 2000, 0)));
        let all_false = wrap(any_of(vec![far1, far2]));
        assert!(!all_false.test(&level, &origin));
    }

    #[test]
    fn not_truth_table() {
        let level = overworld();
        let origin = BlockPos::new(0, 0, 0);
        let inside = wrap(inside_world(Vec3i::ZERO));
        let negated = wrap(not(inside));
        assert!(!negated.test(&level, &origin)); // inside is true → not is false
        let far = wrap(inside_world(Vec3i::new(0, 1000, 0)));
        let negated_far = wrap(not(far));
        assert!(negated_far.test(&level, &origin)); // far is false → not is true
    }

    #[test]
    fn inside_world_bounds_uses_offset_arithmetic() {
        let level = overworld();
        // min_y -64, height 384 → max_y 319. Origin y=0, offset 0 → inside.
        let p = wrap(inside_world(Vec3i::ZERO));
        assert!(p.test(&level, &BlockPos::new(0, 0, 0)));
        // Offset pushes to y=319 (max) → inside; to 320 → outside.
        let at_max = wrap(inside_world(Vec3i::new(0, 319, 0)));
        assert!(at_max.test(&level, &BlockPos::new(0, 0, 0)));
        let past_max = wrap(inside_world(Vec3i::new(0, 320, 0)));
        assert!(!past_max.test(&level, &BlockPos::new(0, 0, 0)));
        // Negative offset to -64 (min) → inside; -65 → outside.
        let at_min = wrap(inside_world(Vec3i::new(0, -64, 0)));
        assert!(at_min.test(&level, &BlockPos::new(0, 0, 0)));
        let below_min = wrap(inside_world(Vec3i::new(0, -65, 0)));
        assert!(!below_min.test(&level, &BlockPos::new(0, 0, 0)));
        // Wrapping offset arithmetic: origin + i32::MAX offset wraps.
        let wrapping = wrap(inside_world(Vec3i::new(0, i32::MAX, 0)));
        // 0 + i32::MAX wraps to i32::MAX (no overflow here), far outside.
        assert!(!wrapping.test(&level, &BlockPos::new(0, 0, 0)));
        // Origin y=319 + offset 1 wraps to -2147483648 (Java `+`), outside.
        let wrap_pos = wrap(inside_world(Vec3i::new(0, 1, 0)));
        assert!(!wrap_pos.test(&level, &BlockPos::new(0, 319, 0)));
    }

    #[test]
    fn true_predicate_always_true() {
        let level = overworld();
        let p = always_true();
        assert!(p.test(&level, &BlockPos::new(0, -1000, 0)));
        assert!(p.test(&level, &BlockPos::new(i32::MIN, i32::MAX, 0)));
    }

    fn round_trip(predicate: Arc<dyn BlockPredicate>) -> Arc<dyn BlockPredicate> {
        let codec = block_predicate_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &predicate)
            .result()
            .expect("encode should succeed")
            .clone();
        let result = codec.parse(&JsonOps::INSTANCE, &encoded);
        result.result().expect("decode should succeed").clone()
    }

    #[test]
    fn true_codec_round_trips_and_encodes_empty_map() {
        // `TrueBlockPredicate.CODEC = MapCodec.unit(INSTANCE)` — encodes to `{}`.
        let codec = block_predicate_codec::<JsonOps>();
        let p = always_true();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"type": "minecraft:true"}));
        let decoded = round_trip(p);
        assert_eq!(bp_type_id(&decoded), BlockPredicateTypes::TRUE);
        assert!(decoded.test(&overworld(), &BlockPos::new(0, 0, 0)));
    }

    #[test]
    fn inside_world_bounds_codec_round_trips_and_defaults_offset() {
        // `InsideWorldBoundsPredicate.CODEC` — the `"offset"` optional field
        // (`Vec3i.offsetCodec(16)`), default `Vec3i.ZERO`. `BlockPos.ZERO` and
        // `Vec3i.ZERO` are the same value (offset codec defaults to ZERO).
        let codec = block_predicate_codec::<JsonOps>();
        let p = wrap(inside_world(Vec3i::ZERO));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"type": "minecraft:inside_world_bounds"}));
        // With a non-zero offset.
        let p2 = wrap(inside_world(Vec3i::new(1, 2, 3)));
        let encoded2 = codec
            .encode_start(&JsonOps::INSTANCE, &p2)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded2,
            json!({"type": "minecraft:inside_world_bounds", "offset": [1, 2, 3]})
        );
        let decoded = round_trip(p2);
        assert_eq!(
            bp_type_id(&decoded),
            BlockPredicateTypes::INSIDE_WORLD_BOUNDS
        );
        // The default offset decodes to ZERO — absent offset == [0, 0, 0].
        let decoded_default = round_trip(p);
        let as_bounds = downcast_erased::<InsideWorldBoundsPredicate>(&decoded_default);
        assert_eq!(as_bounds.offset(), &Vec3i::ZERO);
    }

    #[test]
    fn all_of_codec_round_trips_nested() {
        let nested = wrap(all_of(vec![
            always_true(),
            wrap(inside_world(Vec3i::new(1, 2, 3))),
        ]));
        let decoded = round_trip(nested);
        assert_eq!(bp_type_id(&decoded), BlockPredicateTypes::ALL_OF);
        let as_all = downcast_erased::<AllOfPredicate>(&decoded);
        assert_eq!(as_all.predicates().len(), 2);
        assert_eq!(
            bp_type_id(&as_all.predicates()[0]),
            BlockPredicateTypes::TRUE
        );
        assert_eq!(
            bp_type_id(&as_all.predicates()[1]),
            BlockPredicateTypes::INSIDE_WORLD_BOUNDS
        );
    }

    #[test]
    fn not_codec_round_trips() {
        let p = wrap(not(always_true()));
        let decoded = round_trip(p);
        assert_eq!(bp_type_id(&decoded), BlockPredicateTypes::NOT);
        let as_not = downcast_erased::<NotPredicate>(&decoded);
        assert_eq!(bp_type_id(as_not.predicate()), BlockPredicateTypes::TRUE);
    }

    #[test]
    fn any_of_codec_round_trips() {
        let p = wrap(any_of(vec![always_true(), wrap(inside_world(Vec3i::ZERO))]));
        let decoded = round_trip(p);
        assert_eq!(bp_type_id(&decoded), BlockPredicateTypes::ANY_OF);
        let as_any = downcast_erased::<AnyOfPredicate>(&decoded);
        assert_eq!(as_any.predicates().len(), 2);
    }

    #[test]
    fn dispatch_decodes_by_type_name() {
        let codec = block_predicate_codec::<JsonOps>();
        let input = json!({"type": "minecraft:not", "predicate": {"type": "minecraft:true"}});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(bp_type_id(decoded), BlockPredicateTypes::NOT);
    }

    #[test]
    fn dispatch_missing_type_key_errors() {
        let codec = block_predicate_codec::<JsonOps>();
        let input = json!({"predicates": []});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        // Java: `fieldOf("type")` missing → "No key type in ...".
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key type"), "got: {msg}");
    }

    #[test]
    fn dispatch_unknown_type_errors_like_by_name_codec() {
        let codec = block_predicate_codec::<JsonOps>();
        let input = json!({"type": "minecraft:not_a_type"});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:block_predicate_type]: minecraft:not_a_type"),
            "got: {msg}"
        );
    }

    #[test]
    fn dispatch_unported_type_fails_explicitly() {
        // `matching_blocks` (id 0) is a real Paper type but not in this
        // slice's scope — the codec lookup fails explicitly, never fabricating.
        let codec = block_predicate_codec::<JsonOps>();
        let input = json!({"type": "minecraft:matching_blocks", "blocks": "minecraft:stone"});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.contains("not ported"), "got: {msg}");
        // The registry itself still resolves the type (the table has all 14).
        assert_eq!(
            block_predicate_type_by_name("minecraft:matching_blocks"),
            Some(BlockPredicateTypes::MATCHING_BLOCKS)
        );
    }

    #[test]
    fn dispatch_round_trip_uses_type_name_order() {
        // Encode writes the element fields then the `"type"` key (Java
        // `KeyDispatchCodec` encodes key AFTER value), so the JSON has the
        // `not`-body fields first and `"type"` last.
        let codec = block_predicate_codec::<JsonOps>();
        let p = wrap(not(always_true()));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"predicate": {"type": "minecraft:true"}, "type": "minecraft:not"})
        );
    }
}
