//! Port of `net.minecraft.world.level.levelgen.heightproviders.HeightProvider`
//! (abstract class, 26.2) — the dispatch root of the height-provider framework.
//!
//! Java is the abstract base of the six concrete height providers, with the
//! dispatch codec `CODEC`:
//!
//! ```text
//! CONSTANT_OR_DISPATCH_CODEC = Codec.either(
//!     VerticalAnchor.CODEC,
//!     BuiltInRegistries.HEIGHT_PROVIDER_TYPE.byNameCodec()
//!         .dispatch(HeightProvider::getType, HeightProviderType::codec));
//! CODEC = CONSTANT_OR_DISPATCH_CODEC.xmap(
//!     either -> either.map(ConstantHeight::of, f -> (HeightProvider)f),
//!     f -> f.getType() == HeightProviderType.CONSTANT
//!         ? Either.left(((ConstantHeight)f).getValue()) : Either.right(f));
//! ```
//!
//! The port mirrors the sealed surface as a single [`HeightProvider`] enum over
//! the six variants (the same shape `VerticalAnchor` takes): `type()` becomes
//! the [`HeightProvider::type_id`] accessor, `sample` dispatches by variant with
//! Java-int wrapping arithmetic, and the `CODEC` is the ops-generic
//! [`height_provider_codec::<Ops>()`] factory (the convention every static
//! `CODEC` constant takes in this codebase).
//!
//! ## Constant-or-dispatch shape
//!
//! A bare `VerticalAnchor` value round-trips through the Left branch as a
//! `ConstantHeight` (`{"absolute": 5}`); every other provider — including a
//! `ConstantHeight` written in its record form — dispatches on the `"type"` key
//! through the by-name registry codec. Because `WeightedListHeight` embeds a
//! recursive `WeightedList<HeightProvider>`, the whole codec is a
//! `codec::recursive` graph whose single `RecursiveSelf` threads into the
//! weighted-list element codec — the same pattern `BlockPredicate.CODEC` uses —
//! so arbitrary nesting round-trips. The `"type"` registry codec reproduces
//! Paper's exact by-name error (`Unknown registry key in ResourceKey[
//! minecraft:root / minecraft:height_provider_type]: {name}`).

use crate::levelgen::heightproviders::biased_to_bottom_height::BiasedToBottomHeight;
use crate::levelgen::heightproviders::constant_height::ConstantHeight;
use crate::levelgen::heightproviders::height_provider_type::{
    HeightProviderTypeId, HeightProviderTypes, height_provider_type_by_name,
};
use crate::levelgen::heightproviders::trapezoid_height::TrapezoidHeight;
use crate::levelgen::heightproviders::uniform_height::UniformHeight;
use crate::levelgen::heightproviders::very_biased_to_bottom_height::VeryBiasedToBottomHeight;
use crate::levelgen::heightproviders::weighted_list_height::WeightedListHeight;
use crate::levelgen::vertical_anchor::VerticalAnchor;
use crate::levelgen::vertical_anchor::vertical_anchor_codec;
use crate::levelgen::world_generation_context::WorldGenerationContext;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::either::Either;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_util::RandomSource;
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.heightproviders.HeightProvider` — the
/// discriminated union over the six concrete providers.
///
/// Java models the abstract base as an open class hierarchy; all six
/// implementors live in this package and the dispatch (codec, `type()`) is a
/// closed switch, so the port collapses it to one enum — the same shape the
/// codebase uses for sealed worldgen hierarchies (`VerticalAnchor`,
/// `GenerationStep.Decoration`).
///
/// The derived `PartialEq` is value equality, whereas Java's plain (non-record)
/// provider classes have no `equals` override and so compare by reference
/// identity. This is a deliberate convention shared with `VerticalAnchor`: the
/// sealed port collapses the hierarchy to value-like variants, and no ported
/// code path compares providers for identity (the `WeightedList::contains` on
/// `HeightProvider` is unused), so the divergence is not observable here.
#[derive(Debug, Clone, PartialEq)]
pub enum HeightProvider {
    /// `ConstantHeight` — a fixed `VerticalAnchor`.
    Constant(ConstantHeight),
    /// `UniformHeight` — uniform between two anchors.
    Uniform(UniformHeight),
    /// `BiasedToBottomHeight` — biased toward the bottom anchor.
    BiasedToBottom(BiasedToBottomHeight),
    /// `VeryBiasedToBottomHeight` — more strongly biased toward the bottom.
    VeryBiasedToBottom(VeryBiasedToBottomHeight),
    /// `TrapezoidHeight` — a trapezoid/triangle distribution.
    Trapezoid(TrapezoidHeight),
    /// `WeightedListHeight` — a weighted list of providers.
    WeightedList(WeightedListHeight),
}

impl HeightProvider {
    /// `sample(RandomSource random, WorldGenerationContext heightAccessor)` —
    /// dispatch to the concrete provider's sample, preserving Java-int wrapping
    /// arithmetic (see the per-variant modules for the exact formulas).
    pub fn sample<R: RandomSource>(&self, random: &mut R, context: &WorldGenerationContext) -> i32 {
        match self {
            HeightProvider::Constant(p) => p.sample(random, context),
            HeightProvider::Uniform(p) => p.sample(random, context),
            HeightProvider::BiasedToBottom(p) => p.sample(random, context),
            HeightProvider::VeryBiasedToBottom(p) => p.sample(random, context),
            HeightProvider::Trapezoid(p) => p.sample(random, context),
            HeightProvider::WeightedList(p) => p.sample(random, context),
        }
    }

    /// `getType()` — the registry-held `HeightProviderType<?>` identity this
    /// provider dispatches on (the key `HeightProvider.CODEC` uses).
    pub fn type_id(&self) -> HeightProviderTypeId {
        match self {
            HeightProvider::Constant(_) => HeightProviderTypes::CONSTANT,
            HeightProvider::Uniform(_) => HeightProviderTypes::UNIFORM,
            HeightProvider::BiasedToBottom(_) => HeightProviderTypes::BIASED_TO_BOTTOM,
            HeightProvider::VeryBiasedToBottom(_) => HeightProviderTypes::VERY_BIASED_TO_BOTTOM,
            HeightProvider::Trapezoid(_) => HeightProviderTypes::TRAPEZOID,
            HeightProvider::WeightedList(_) => HeightProviderTypes::WEIGHTED_LIST,
        }
    }
}

impl fmt::Display for HeightProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeightProvider::Constant(p) => write!(f, "{p}"),
            HeightProvider::Uniform(p) => write!(f, "{p}"),
            HeightProvider::BiasedToBottom(p) => write!(f, "{p}"),
            HeightProvider::VeryBiasedToBottom(p) => write!(f, "{p}"),
            HeightProvider::Trapezoid(p) => write!(f, "{p}"),
            // `WeightedListHeight` has no `toString` in Java (identity-based
            // `Object.toString`), so `Display` falls back to the derived
            // `Debug` — the closest stable Rust analog of a default `toString`.
            HeightProvider::WeightedList(p) => write!(f, "{p:?}"),
        }
    }
}

/// `HeightProvider.CODEC` — the recursive constant-or-dispatch codec, as the
/// ops-generic `height_provider_codec::<Ops>()` factory.
pub fn height_provider_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<HeightProvider, Ops>> {
    codec::recursive("HeightProvider".to_string(), Arc::new(create_dispatch))
}

/// The non-recursive codec body given the `RecursiveSelf` (`top`): the
/// constant-or-dispatch `Codec.either(...).xmap(...)`. Every provider that
/// recurses into `HeightProvider.CODEC` (`WeightedListHeight`) receives `top`
/// as the child-element codec so the whole nested graph shares this single
/// recursive codec.
fn create_dispatch<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<HeightProvider, Ops>>,
) -> Arc<dyn Codec<HeightProvider, Ops>> {
    // `BuiltInRegistries.HEIGHT_PROVIDER_TYPE.byNameCodec().dispatch(...)`.
    let dispatch = map_codec::codec_of(key_dispatch_codec::dispatch_map::<
        HeightProviderTypeId,
        HeightProvider,
        Ops,
    >(
        "type",
        height_provider_type_by_name_codec::<Ops>(),
        Arc::new(|p: &HeightProvider| DataResult::success(HeightProvider::type_id(p))),
        codec_for_type(top),
    ));
    // `Codec.either(VerticalAnchor.CODEC, dispatch)`.
    let either = codec::either::<VerticalAnchor, HeightProvider, Ops>(
        vertical_anchor_codec::<Ops>(),
        dispatch,
    );
    // `.xmap(either -> either.map(ConstantHeight::of, f -> (HeightProvider)f),
    //  f -> f.getType() == CONSTANT ? Either.left(...getValue()) : Either.right(f))`.
    codec::xmap(
        either,
        Arc::new(|e: &Either<VerticalAnchor, HeightProvider>| match e {
            Either::Left(anchor) => HeightProvider::Constant(ConstantHeight::of(*anchor)),
            Either::Right(provider) => provider.clone(),
        }),
        Arc::new(|p: &HeightProvider| {
            if p.type_id() == HeightProviderTypes::CONSTANT {
                let HeightProvider::Constant(constant) = p else {
                    // Unreachable: `type_id` says CONSTANT only for the
                    // `Constant` variant.
                    unreachable!("CONSTANT type id must be a Constant variant");
                };
                Either::left(constant.get_value())
            } else {
                Either::right(p.clone())
            }
        }),
    )
}

/// `HeightProviderType::codec` — resolve a `HeightProviderTypeId` to its
/// `MapCodec<HeightProvider>` (the dispatch's `codec` function).
fn codec_for_type<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<HeightProvider, Ops>>,
) -> key_dispatch_codec::CodecFn<HeightProviderTypeId, HeightProvider, Ops> {
    Arc::new(move |k: &HeightProviderTypeId| {
        if *k == HeightProviderTypes::CONSTANT {
            DataResult::success(erase_map_codec::<ConstantHeight, Ops>(
                crate::levelgen::heightproviders::constant_height::constant_height_map_codec::<Ops>(
                ),
                Arc::new(|c: &ConstantHeight| HeightProvider::Constant(*c)),
                Arc::new(|p: &HeightProvider| match p {
                    HeightProvider::Constant(c) => *c,
                    _ => panic!("height provider dispatch produced a non-constant value"),
                }),
            ))
        } else if *k == HeightProviderTypes::UNIFORM {
            DataResult::success(erase_map_codec::<UniformHeight, Ops>(
                crate::levelgen::heightproviders::uniform_height::uniform_height_map_codec::<Ops>(),
                Arc::new(|u: &UniformHeight| HeightProvider::Uniform(*u)),
                Arc::new(|p: &HeightProvider| match p {
                    HeightProvider::Uniform(u) => *u,
                    _ => panic!("height provider dispatch produced a non-uniform value"),
                }),
            ))
        } else if *k == HeightProviderTypes::BIASED_TO_BOTTOM {
            DataResult::success(erase_map_codec::<BiasedToBottomHeight, Ops>(
                crate::levelgen::heightproviders::biased_to_bottom_height::biased_to_bottom_height_map_codec::<
                    Ops,
                >(),
                Arc::new(|b: &BiasedToBottomHeight| HeightProvider::BiasedToBottom(*b)),
                Arc::new(|p: &HeightProvider| match p {
                    HeightProvider::BiasedToBottom(b) => *b,
                    _ => panic!("height provider dispatch produced a non-biased value"),
                }),
            ))
        } else if *k == HeightProviderTypes::VERY_BIASED_TO_BOTTOM {
            DataResult::success(erase_map_codec::<VeryBiasedToBottomHeight, Ops>(
                crate::levelgen::heightproviders::very_biased_to_bottom_height::very_biased_to_bottom_height_map_codec::<
                    Ops,
                >(),
                Arc::new(|v: &VeryBiasedToBottomHeight| {
                    HeightProvider::VeryBiasedToBottom(*v)
                }),
                Arc::new(|p: &HeightProvider| match p {
                    HeightProvider::VeryBiasedToBottom(v) => *v,
                    _ => panic!("height provider dispatch produced a non-very-biased value"),
                }),
            ))
        } else if *k == HeightProviderTypes::TRAPEZOID {
            DataResult::success(erase_map_codec::<TrapezoidHeight, Ops>(
                crate::levelgen::heightproviders::trapezoid_height::trapezoid_height_map_codec::<Ops>(
                ),
                Arc::new(|t: &TrapezoidHeight| HeightProvider::Trapezoid(*t)),
                Arc::new(|p: &HeightProvider| match p {
                    HeightProvider::Trapezoid(t) => *t,
                    _ => panic!("height provider dispatch produced a non-trapezoid value"),
                }),
            ))
        } else if *k == HeightProviderTypes::WEIGHTED_LIST {
            DataResult::success(erase_map_codec::<WeightedListHeight, Ops>(
                crate::levelgen::heightproviders::weighted_list_height::weighted_list_height_map_codec::<
                    Ops,
                >(top.clone()),
                Arc::new(|w: &WeightedListHeight| HeightProvider::WeightedList(w.clone())),
                Arc::new(|p: &HeightProvider| match p {
                    HeightProvider::WeightedList(w) => w.clone(),
                    _ => panic!("height provider dispatch produced a non-weighted-list value"),
                }),
            ))
        } else {
            DataResult::error(format!(
                "Height provider type '{}' is not ported",
                k.location
            ))
        }
    })
}

/// Lift a concrete provider's `MapCodec<C>` to `MapCodec<HeightProvider>` —
/// Java's `MapCodec<? extends HeightProvider>` variance, via xmap (the same
/// lift `BlockPredicate`'s `erase_map_codec` performs).
pub(crate) fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    wrap: Arc<dyn Fn(&C) -> HeightProvider + Send + Sync>,
    unwrap: Arc<dyn Fn(&HeightProvider) -> C + Send + Sync>,
) -> Arc<dyn MapCodec<HeightProvider, Ops>>
where
    C: 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(inner, wrap, unwrap)
}

/// `BuiltInRegistries.HEIGHT_PROVIDER_TYPE.byNameCodec()` over the type id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key().
/// identifier())`.
///
/// The unknown-key error reproduces Paper's exactly: `"Unknown registry key in "
/// + this.key() + ": " + name` where `this.key()` is
/// `Registries.HEIGHT_PROVIDER_TYPE` (`createRegistryKey("height_provider_type")`,
/// toString `"ResourceKey[minecraft:root / minecraft:height_provider_type]"`).
pub fn height_provider_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<HeightProviderTypeId, Ops>> {
    codec::comap_flat_map::<rivet_registry::Identifier, HeightProviderTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &rivet_registry::Identifier| {
            match height_provider_type_by_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:height_provider_type]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &HeightProviderTypeId| rivet_registry::Identifier::parse(id.location)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use crate::levelgen::heightproviders::biased_to_bottom_height::BiasedToBottomHeight;
    use crate::levelgen::heightproviders::constant_height::ConstantHeight;
    use crate::levelgen::heightproviders::trapezoid_height::TrapezoidHeight;
    use crate::levelgen::heightproviders::uniform_height::UniformHeight;
    use crate::levelgen::heightproviders::very_biased_to_bottom_height::VeryBiasedToBottomHeight;
    use crate::levelgen::heightproviders::weighted_list_height::WeightedListHeight;
    use rivet_registry::core::BlockPos;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::WeightedList;
    use rivet_util::random::{LegacyRandomSource, XoroshiroRandomSource};
    use serde_json::json;

    /// A `ChunkGenerator` double exposing a fixed worldgen window.
    struct TestGenerator {
        min_y: i32,
        depth: i32,
    }
    impl ChunkGenerator for TestGenerator {
        fn create_biomes(&self) {}
        fn apply_carvers(&self) {}
        fn build_surface(&self) {}
        fn spawn_original_mobs(&self) {}
        fn fill_from_noise(&self) {}
        fn get_min_y(&self) -> i32 {
            self.min_y
        }
        fn get_gen_depth(&self) -> i32 {
            self.depth
        }
    }

    /// A `WorldGenLevel` double over a fixed window.
    struct TestLevel(SimpleLevelHeightAccessor);
    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.0.get_height()
        }
        fn get_min_y(&self) -> i32 {
            self.0.get_min_y()
        }
    }
    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            // RivetTodo(#399): the world-access implementation is not ported;
            // this test double never reads block state — sampling only touches
            // the height window.
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    fn context(min_y: i32, height: i32, gen_depth: i32) -> WorldGenerationContext {
        let level = TestLevel(create(min_y, height));
        let generator = TestGenerator {
            min_y,
            depth: gen_depth,
        };
        WorldGenerationContext::new(&generator, &level)
    }

    fn overworld() -> WorldGenerationContext {
        context(-64, 384, 384)
    }

    #[test]
    fn type_ids_match_declaration_order() {
        let constant = HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::absolute(3)));
        assert_eq!(constant.type_id(), HeightProviderTypes::CONSTANT);
        let uniform = HeightProvider::Uniform(UniformHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(10),
        ));
        assert_eq!(uniform.type_id(), HeightProviderTypes::UNIFORM);
    }

    #[test]
    fn codec_round_trips_a_bare_anchor_as_constant() {
        // Java's Left branch: a bare `VerticalAnchor` decodes to a
        // ConstantHeight, and encoding a ConstantHeight emits the bare anchor.
        let codec = height_provider_codec::<JsonOps>();
        let input = json!({"absolute": 5});
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(decoded.type_id(), HeightProviderTypes::CONSTANT);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"absolute": 5}));
    }

    #[test]
    fn codec_dispatch_round_trips_constant_record_form() {
        // The `"constant"` dispatch branch (the record form
        // `{"type": "minecraft:constant", "value": {...anchor...}}`) is
        // exercised through the top-level `HeightProvider.CODEC`, distinct from
        // the bare-anchor Left branch. `CODEC.xmap` special-cases CONSTANT on
        // encode, so the round trip re-encodes as the bare anchor.
        let codec = height_provider_codec::<JsonOps>();
        let input = json!({
            "type": "minecraft:constant",
            "value": {"absolute": 5}
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(decoded.type_id(), HeightProviderTypes::CONSTANT);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"absolute": 5}));
    }

    #[test]
    fn codec_dispatch_round_trips_each_type() {
        // One provider per `HeightProviderType` (all six dispatch branches of
        // `codec_for_type`), round-tripped through the top-level
        // `HeightProvider.CODEC`'s `"type"`-key dispatch. Constant is covered
        // by the bare-anchor path here (the Left branch of the
        // constant-or-dispatch either); its record form is covered by the
        // per-type codec tests in `constant_height`.
        let codec = height_provider_codec::<JsonOps>();
        let providers = [
            HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::absolute(3))),
            HeightProvider::Uniform(UniformHeight::of(
                VerticalAnchor::above_bottom(4),
                VerticalAnchor::below_top(2),
            )),
            HeightProvider::BiasedToBottom(BiasedToBottomHeight::of(
                VerticalAnchor::absolute(0),
                VerticalAnchor::absolute(9),
                1,
            )),
            HeightProvider::VeryBiasedToBottom(VeryBiasedToBottomHeight::of(
                VerticalAnchor::absolute(0),
                VerticalAnchor::absolute(9),
                1,
            )),
            HeightProvider::Trapezoid(TrapezoidHeight::of(
                VerticalAnchor::absolute(0),
                VerticalAnchor::absolute(9),
                0,
            )),
            HeightProvider::WeightedList(WeightedListHeight::new(WeightedList::of_values(&[
                HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::absolute(1))),
                HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::absolute(2))),
            ]))),
        ];
        for provider in providers {
            let encoded = codec
                .encode_start(&JsonOps::INSTANCE, &provider)
                .result()
                .expect("encode should succeed")
                .clone();
            let decoded_result = codec.parse(&JsonOps::INSTANCE, &encoded);
            let decoded = decoded_result.result().expect("decode should succeed");
            assert_eq!(*decoded, provider);
        }
    }

    #[test]
    fn codec_dispatch_encodes_type_name_order() {
        // Encode writes the element fields then the `"type"` key (Java
        // `KeyDispatchCodec` encodes key AFTER value), so the uniform body
        // fields come first and `"type"` last.
        let codec = height_provider_codec::<JsonOps>();
        let uniform = HeightProvider::Uniform(UniformHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(10),
        ));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &uniform)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "min_inclusive": {"absolute": 0},
                "max_inclusive": {"absolute": 10},
                "type": "minecraft:uniform"
            })
        );
    }

    #[test]
    fn codec_unknown_type_errors_like_by_name_codec() {
        let codec = height_provider_codec::<JsonOps>();
        let input = json!({"type": "minecraft:not_a_type", "min_inclusive": {"absolute": 0}});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:height_provider_type]: minecraft:not_a_type"),
            "got: {msg}"
        );
    }

    #[test]
    fn codec_dispatch_missing_body_field_errors() {
        // `fieldOf("min_inclusive")` is required — a dispatch with the type key
        // but no body field must fail, never default.
        let codec = height_provider_codec::<JsonOps>();
        let input = json!({"type": "minecraft:uniform"});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        // The `CODEC` is `either(anchor, dispatch)`, so the combined failure
        // lists the anchor keys first and the required dispatch body fields
        // (`min_inclusive`/`max_inclusive`) second — assert the required field
        // is reported rather than the first token of the wrapper message.
        assert!(msg.contains("No key min_inclusive"), "got: {msg}");
    }

    #[test]
    fn constant_sample_resolves_anchor() {
        let p = HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::absolute(7)));
        let mut random = LegacyRandomSource::new(1);
        // A constant ignores the random source entirely.
        assert_eq!(p.sample(&mut random, &overworld()), 7);
        let above = HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::above_bottom(16)));
        assert_eq!(above.sample(&mut random, &overworld()), -48);
    }

    #[test]
    fn uniform_sample_uses_random_between_inclusive() {
        // min 0, max 10 -> Mth.randomBetweenInclusive; golden sequence pinned
        // against Paper's LCG (LegacyRandomSource seed 12345).
        let p = HeightProvider::Uniform(UniformHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(10),
        ));
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..8)
            .map(|_| p.sample(&mut random, &overworld()))
            .collect();
        assert_eq!(samples, [6, 6, 6, 10, 1, 10, 7, 6]);
    }

    #[test]
    fn uniform_sample_empty_range_returns_min() {
        // min > max: Java warns (dropped) and returns min — no RNG consumed.
        let p = HeightProvider::Uniform(UniformHeight::of(
            VerticalAnchor::absolute(10),
            VerticalAnchor::absolute(0),
        ));
        let mut random = LegacyRandomSource::new(1);
        assert_eq!(p.sample(&mut random, &overworld()), 10);
        // The RNG is untouched.
        assert_eq!(random.next_int(), -1155869325);
    }

    #[test]
    fn biased_sample_golden() {
        use crate::levelgen::heightproviders::biased_to_bottom_height::BiasedToBottomHeight;
        let p = HeightProvider::BiasedToBottom(BiasedToBottomHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(9),
            1,
        ));
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..8)
            .map(|_| p.sample(&mut random, &overworld()))
            .collect();
        assert_eq!(samples, [0, 6, 4, 0, 0, 0, 7, 4]);
    }

    #[test]
    fn very_biased_sample_golden() {
        use crate::levelgen::heightproviders::very_biased_to_bottom_height::VeryBiasedToBottomHeight;
        let p = HeightProvider::VeryBiasedToBottom(VeryBiasedToBottomHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(9),
            1,
        ));
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..8)
            .map(|_| p.sample(&mut random, &overworld()))
            .collect();
        assert_eq!(samples, [0, 2, 0, 0, 0, 0, 5, 1]);
    }

    #[test]
    fn trapezoid_sample_golden() {
        use crate::levelgen::heightproviders::trapezoid_height::TrapezoidHeight;
        // plateau 0 -> triangle.
        let p = HeightProvider::Trapezoid(TrapezoidHeight::of(
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(9),
            0,
        ));
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..8)
            .map(|_| p.sample(&mut random, &overworld()))
            .collect();
        assert_eq!(samples, [1, 6, 5, 3, 5, 7, 5, 6]);
    }

    #[test]
    fn weighted_list_sample_dispatch_recurses() {
        // The recursive codec threads through the WeightedList element codec.
        let codec = height_provider_codec::<JsonOps>();
        let input = json!({
            "type": "minecraft:weighted_list",
            "distribution": [
                {"data": {"type": "minecraft:constant", "value": {"absolute": 1}}, "weight": 1},
                {"data": {"type": "minecraft:constant", "value": {"absolute": 2}}, "weight": 3}
            ]
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(decoded.type_id(), HeightProviderTypes::WEIGHTED_LIST);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        // Paper's `CODEC.xmap` special-cases CONSTANT to emit the bare anchor,
        // so the record-form constants re-encode as `{"absolute": 1}`.
        assert_eq!(
            encoded,
            json!({
                // The dispatch `"type"` key is encoded last (Java's
                // `KeyDispatchCodec` encodes key AFTER value), and constants
                // re-encode as bare anchors.
                "distribution": [
                    {"data": {"absolute": 1}, "weight": 1},
                    {"data": {"absolute": 2}, "weight": 3}
                ],
                "type": "minecraft:weighted_list"
            })
        );
        // Sampling picks the weighted branch and samples the child constant.
        let mut random = LegacyRandomSource::new(12345);
        let samples: Vec<i32> = (0..4)
            .map(|_| decoded.sample(&mut random, &overworld()))
            .collect();
        // total weight 4 (flat): selection 0 -> value 1; 1,2,3 -> value 2.
        assert_eq!(samples, [2, 2, 2, 2]);
        // Under Xoroshiro too.
        let mut xor = XoroshiroRandomSource::new(12345);
        let xsamples: Vec<i32> = (0..4)
            .map(|_| decoded.sample(&mut xor, &overworld()))
            .collect();
        assert_eq!(xsamples, [1, 2, 2, 1]);
    }
}
