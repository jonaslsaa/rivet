//! Port of `net.minecraft.util.valueproviders.IntProvider` (interface, 26.2) —
//! the dispatch root of the integer value-provider framework.
//!
//! Java is the interface every integer provider implements, with the dispatch
//! codec `IntProviders.CODEC`:
//!
//! ```text
//! CONSTANT_OR_DISPATCH_CODEC = Codec.either(
//!     Codec.INT,
//!     BuiltInRegistries.INT_PROVIDER_TYPE.byNameCodec()
//!         .dispatch(IntProvider::codec, t -> t));
//! CODEC = CONSTANT_OR_DISPATCH_CODEC.xmap(
//!     either -> either.map(ConstantInt::of, f -> (IntProvider)f),
//!     f -> f instanceof ConstantInt constantInt ? Either.left(constantInt.value()) : Either.right(f));
//! ```
//!
//! The port mirrors the sealed surface as a single [`IntProvider`] enum over
//! the seven variants (the same shape `HeightProvider` takes): `type()` becomes
//! [`IntProvider::type_id`], `sample`/`minInclusive`/`maxInclusive` dispatch by
//! variant with Java-int wrapping arithmetic, and the `CODEC` is the ops-generic
//! [`int_provider_codec::<Ops>()`] factory.
//!
//! Because `ClampedInt` embeds a `source: IntProvider` and `WeightedListInt` a
//! `WeightedList<IntProvider>`, the whole codec is a `codec::recursive` graph
//! whose single `RecursiveSelf` threads into the recursive field codecs — the
//! same pattern `BlockPredicate.CODEC` / `HeightProvider.CODEC` use — so
//! arbitrary nesting round-trips. The `"type"` registry codec reproduces
//! Paper's exact by-name error (`Unknown registry key in ResourceKey[
//! minecraft:root / minecraft:int_provider_type]: {name}`).

use crate::RandomSource;
use crate::valueproviders::biased_to_bottom_int::BiasedToBottomInt;
use crate::valueproviders::clamped_int::ClampedInt;
use crate::valueproviders::clamped_normal_int::ClampedNormalInt;
use crate::valueproviders::constant_int::ConstantInt;
use crate::valueproviders::default_namespace;
use crate::valueproviders::int_provider_type::{
    IntProviderTypeId, IntProviderTypes, int_provider_type_by_name,
};
use crate::valueproviders::trapezoid_int::TrapezoidInt;
use crate::valueproviders::uniform_int::UniformInt;
use crate::valueproviders::weighted_list_int::WeightedListInt;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::either::Either;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.IntProvider` — the discriminated union
/// over the seven concrete integer providers.
///
/// Java models the interface as an open hierarchy; all implementors live in
/// this package and the dispatch (codec, `type()`) is a closed switch, so the
/// port collapses it to one enum — the same shape the codebase uses for sealed
/// worldgen hierarchies (`VerticalAnchor`, `HeightProvider`).
///
/// The derived `PartialEq` is value equality, whereas Java's plain (non-record)
/// provider classes have no `equals` override and so compare by reference
/// identity. Of the seven variants only `WeightedListInt` is such a class — the
/// other six are records, whose `equals` IS value equality. This is a deliberate
/// convention shared with `HeightProvider`/`VerticalAnchor`: the sealed port
/// collapses the hierarchy to value-like variants, and no ported code path
/// compares providers for identity, so the divergence is not observable here.
/// (Deriving `PartialEq` is also structurally required: `WeightedList<E>`
/// implements `PartialEq` only where `E: PartialEq`, matching Java's
/// `WeightedList.equals`, which delegates element `equals`.)
#[derive(Debug, Clone, PartialEq)]
pub enum IntProvider {
    /// `ConstantInt`.
    Constant(ConstantInt),
    /// `UniformInt`.
    Uniform(UniformInt),
    /// `BiasedToBottomInt`.
    BiasedToBottom(BiasedToBottomInt),
    /// `ClampedInt`.
    Clamped(ClampedInt),
    /// `WeightedListInt`.
    WeightedList(WeightedListInt),
    /// `ClampedNormalInt`.
    ClampedNormal(ClampedNormalInt),
    /// `TrapezoidInt`.
    Trapezoid(TrapezoidInt),
}

impl IntProvider {
    /// `sample(RandomSource)` — dispatch to the concrete provider's sample,
    /// preserving Java-int wrapping arithmetic.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> i32 {
        match self {
            IntProvider::Constant(p) => p.sample(random),
            IntProvider::Uniform(p) => p.sample(random),
            IntProvider::BiasedToBottom(p) => p.sample(random),
            IntProvider::Clamped(p) => p.sample(random),
            IntProvider::WeightedList(p) => p.sample(random),
            IntProvider::ClampedNormal(p) => p.sample(random),
            IntProvider::Trapezoid(p) => p.sample(random),
        }
    }

    /// `IntProvider.codec()` — the registry-held `MapCodec` this provider
    /// dispatches on. The concrete type is known from the variant, so this is
    /// the provider-type identity used by the dispatch (the analogue of
    /// `type()` / the `IntProviders` bootstrap's registration).
    pub fn type_id(&self) -> IntProviderTypeId {
        match self {
            IntProvider::Constant(_) => IntProviderTypes::CONSTANT,
            IntProvider::Uniform(_) => IntProviderTypes::UNIFORM,
            IntProvider::BiasedToBottom(_) => IntProviderTypes::BIASED_TO_BOTTOM,
            IntProvider::Clamped(_) => IntProviderTypes::CLAMPED,
            IntProvider::WeightedList(_) => IntProviderTypes::WEIGHTED_LIST,
            IntProvider::ClampedNormal(_) => IntProviderTypes::CLAMPED_NORMAL,
            IntProvider::Trapezoid(_) => IntProviderTypes::TRAPEZOID,
        }
    }

    /// `minInclusive()` — dispatch to the concrete provider's lower bound.
    pub fn min_inclusive(&self) -> i32 {
        match self {
            IntProvider::Constant(p) => p.min_inclusive(),
            IntProvider::Uniform(p) => p.min_inclusive(),
            IntProvider::BiasedToBottom(p) => p.min_inclusive(),
            IntProvider::Clamped(p) => p.effective_min_inclusive(),
            IntProvider::WeightedList(p) => p.min_inclusive(),
            IntProvider::ClampedNormal(p) => p.min_inclusive(),
            IntProvider::Trapezoid(p) => p.min_inclusive(),
        }
    }

    /// `maxInclusive()` — dispatch to the concrete provider's upper bound.
    pub fn max_inclusive(&self) -> i32 {
        match self {
            IntProvider::Constant(p) => p.max_inclusive(),
            IntProvider::Uniform(p) => p.max_inclusive(),
            IntProvider::BiasedToBottom(p) => p.max_inclusive(),
            IntProvider::Clamped(p) => p.effective_max_inclusive(),
            IntProvider::WeightedList(p) => p.max_inclusive(),
            IntProvider::ClampedNormal(p) => p.max_inclusive(),
            IntProvider::Trapezoid(p) => p.max_inclusive(),
        }
    }

    /// `IntProviders.CODEC` validation for the constant-or-dispatch `CODEC` —
    /// the erased `CODEC` is only exposed via `IntProviders.CODEC` and the
    /// validated `codec(minValue, maxValue)` / `NON_NEGATIVE_CODEC` /
    /// `POSITIVE_CODEC` factories.
    pub(crate) fn is_constant(&self) -> bool {
        matches!(self, IntProvider::Constant(_))
    }

    pub(crate) fn as_constant_value(&self) -> i32 {
        match self {
            IntProvider::Constant(c) => c.value(),
            _ => panic!("expected a constant int provider"),
        }
    }
}

impl fmt::Display for IntProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntProvider::Constant(p) => write!(f, "{p}"),
            IntProvider::Uniform(p) => write!(f, "{p}"),
            IntProvider::BiasedToBottom(p) => write!(f, "{p}"),
            // `ClampedInt` is a record with no `toString` override, so Java uses
            // the record's auto-generated `toString`
            // (`ClampedInt[source=..., minInclusive=..., maxInclusive=...]`, raw
            // component values); format it to match. `WeightedListInt` is a
            // regular class with identity-based `Object.toString` (an
            // unreproducible hash), so it falls back to the derived `Debug`.
            IntProvider::Clamped(p) => write!(
                f,
                "ClampedInt[source={}, minInclusive={}, maxInclusive={}]",
                p.source(),
                p.min_inclusive(),
                p.max_inclusive()
            ),
            IntProvider::WeightedList(p) => write!(f, "{p:?}"),
            IntProvider::ClampedNormal(p) => write!(f, "{p}"),
            IntProvider::Trapezoid(p) => write!(f, "{p}"),
        }
    }
}

/// `IntProviders.CODEC` — the recursive constant-or-dispatch codec, as the
/// ops-generic `int_provider_codec::<Ops>()` factory.
pub fn int_provider_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<IntProvider, Ops>> {
    codec::recursive("IntProvider".to_string(), Arc::new(create_dispatch))
}

/// The non-recursive codec body given the `RecursiveSelf` (`top`): the
/// constant-or-dispatch `Codec.either(...).xmap(...)`. Every provider that
/// recurses into `IntProvider.CODEC` (`ClampedInt`, `WeightedListInt`) receives
/// `top` as the child-element codec so the whole nested graph shares this
/// single recursive codec.
fn create_dispatch<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<IntProvider, Ops>>,
) -> Arc<dyn Codec<IntProvider, Ops>> {
    // `BuiltInRegistries.INT_PROVIDER_TYPE.byNameCodec().dispatch(...)`.
    let dispatch = map_codec::codec_of(key_dispatch_codec::dispatch_map::<
        IntProviderTypeId,
        IntProvider,
        Ops,
    >(
        "type",
        int_provider_type_by_name_codec::<Ops>(),
        Arc::new(|p: &IntProvider| DataResult::success(IntProvider::type_id(p))),
        codec_for_type(top),
    ));
    // `Codec.either(Codec.INT, dispatch)`.
    let either = codec::either::<i32, IntProvider, Ops>(codec::int_codec::<Ops>(), dispatch);
    // `.xmap(either -> either.map(ConstantInt::of, f -> (IntProvider)f),
    //  f -> f instanceof ConstantInt ? Either.left(value) : Either.right(f))`.
    codec::xmap(
        either,
        Arc::new(|e: &Either<i32, IntProvider>| match e {
            Either::Left(v) => IntProvider::Constant(ConstantInt::of(*v)),
            Either::Right(provider) => provider.clone(),
        }),
        Arc::new(|p: &IntProvider| {
            if p.is_constant() {
                Either::left(p.as_constant_value())
            } else {
                Either::right(p.clone())
            }
        }),
    )
}

/// `IntProvider::codec` — resolve an `IntProviderTypeId` to its
/// `MapCodec<IntProvider>` (the dispatch's `codec` function).
fn codec_for_type<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<IntProvider, Ops>>,
) -> key_dispatch_codec::CodecFn<IntProviderTypeId, IntProvider, Ops> {
    Arc::new(move |k: &IntProviderTypeId| {
        if *k == IntProviderTypes::CONSTANT {
            DataResult::success(erase_map_codec::<ConstantInt, Ops>(
                crate::valueproviders::constant_int::constant_int_map_codec::<Ops>(),
                Arc::new(|c: &ConstantInt| IntProvider::Constant(*c)),
                Arc::new(|p: &IntProvider| match p {
                    IntProvider::Constant(c) => *c,
                    _ => panic!("int provider dispatch produced a non-constant value"),
                }),
            ))
        } else if *k == IntProviderTypes::UNIFORM {
            DataResult::success(erase_map_codec::<UniformInt, Ops>(
                crate::valueproviders::uniform_int::uniform_int_map_codec::<Ops>(),
                Arc::new(|u: &UniformInt| IntProvider::Uniform(*u)),
                Arc::new(|p: &IntProvider| match p {
                    IntProvider::Uniform(u) => *u,
                    _ => panic!("int provider dispatch produced a non-uniform value"),
                }),
            ))
        } else if *k == IntProviderTypes::BIASED_TO_BOTTOM {
            DataResult::success(erase_map_codec::<BiasedToBottomInt, Ops>(
                crate::valueproviders::biased_to_bottom_int::biased_to_bottom_int_map_codec::<Ops>(
                ),
                Arc::new(|b: &BiasedToBottomInt| IntProvider::BiasedToBottom(*b)),
                Arc::new(|p: &IntProvider| match p {
                    IntProvider::BiasedToBottom(b) => *b,
                    _ => panic!("int provider dispatch produced a non-biased-to-bottom value"),
                }),
            ))
        } else if *k == IntProviderTypes::CLAMPED {
            DataResult::success(erase_map_codec::<ClampedInt, Ops>(
                crate::valueproviders::clamped_int::clamped_int_map_codec::<Ops>(top.clone()),
                Arc::new(|c: &ClampedInt| IntProvider::Clamped(c.clone())),
                Arc::new(|p: &IntProvider| match p {
                    IntProvider::Clamped(c) => c.clone(),
                    _ => panic!("int provider dispatch produced a non-clamped value"),
                }),
            ))
        } else if *k == IntProviderTypes::WEIGHTED_LIST {
            DataResult::success(erase_map_codec::<WeightedListInt, Ops>(
                crate::valueproviders::weighted_list_int::weighted_list_int_map_codec::<Ops>(
                    top.clone(),
                ),
                Arc::new(|w: &WeightedListInt| IntProvider::WeightedList(w.clone())),
                Arc::new(|p: &IntProvider| match p {
                    IntProvider::WeightedList(w) => w.clone(),
                    _ => panic!("int provider dispatch produced a non-weighted-list value"),
                }),
            ))
        } else if *k == IntProviderTypes::CLAMPED_NORMAL {
            DataResult::success(erase_map_codec::<ClampedNormalInt, Ops>(
                crate::valueproviders::clamped_normal_int::clamped_normal_int_map_codec::<Ops>(),
                Arc::new(|c: &ClampedNormalInt| IntProvider::ClampedNormal(*c)),
                Arc::new(|p: &IntProvider| match p {
                    IntProvider::ClampedNormal(c) => *c,
                    _ => panic!("int provider dispatch produced a non-clamped-normal value"),
                }),
            ))
        } else if *k == IntProviderTypes::TRAPEZOID {
            DataResult::success(erase_map_codec::<TrapezoidInt, Ops>(
                crate::valueproviders::trapezoid_int::trapezoid_int_map_codec::<Ops>(),
                Arc::new(|t: &TrapezoidInt| IntProvider::Trapezoid(*t)),
                Arc::new(|p: &IntProvider| match p {
                    IntProvider::Trapezoid(t) => *t,
                    _ => panic!("int provider dispatch produced a non-trapezoid value"),
                }),
            ))
        } else {
            DataResult::error(format!(
                "Unknown registry key in ResourceKey[minecraft:root / minecraft:int_provider_type]: {}",
                k.location
            ))
        }
    })
}

/// Lift a concrete provider's `MapCodec<C>` to `MapCodec<IntProvider>` —
/// Java's `MapCodec<? extends IntProvider>` variance, via xmap (the same lift
/// `HeightProvider`'s `erase_map_codec` performs).
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    wrap: Arc<dyn Fn(&C) -> IntProvider + Send + Sync>,
    unwrap: Arc<dyn Fn(&IntProvider) -> C + Send + Sync>,
) -> Arc<dyn MapCodec<IntProvider, Ops>>
where
    C: 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(inner, wrap, unwrap)
}

/// `BuiltInRegistries.INT_PROVIDER_TYPE.byNameCodec()` over the type id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key().
/// identifier())`.
///
/// The unknown-key error reproduces Paper's exactly: `"Unknown registry key in "
/// + this.key() + ": " + name` where `this.key()` is
/// `Registries.INT_PROVIDER_TYPE` (`createRegistryKey("int_provider_type")`,
/// toString `"ResourceKey[minecraft:root / minecraft:int_provider_type]"`).
/// Bare names default to the `minecraft:` namespace (the `Identifier`
/// single-string constructor). The only documented divergence from Paper is the
/// `Identifier` malformed-string diagnostic (see `valueproviders`' module doc).
pub fn int_provider_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<IntProviderTypeId, Ops>> {
    codec::comap_flat_map::<String, IntProviderTypeId, Ops>(
        codec::string_codec::<Ops>(),
        Arc::new(|name: &String| {
            let namespaced = default_namespace(name);
            match int_provider_type_by_name(&namespaced) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:int_provider_type]: {}",
                    namespaced
                )),
            }
        }),
        Arc::new(|id: &IntProviderTypeId| id.location.to_string()),
    )
}

/// `IntProviders.codec(int minValue, int maxValue)` — `validateCodec(minValue,
/// maxValue, CODEC)`.
pub fn int_provider_codec_with_bounds<Ops: DynamicOps + 'static>(
    min_value: i32,
    max_value: i32,
) -> Arc<dyn Codec<IntProvider, Ops>> {
    int_provider_validate_codec(min_value, max_value, int_provider_codec::<Ops>())
}

/// `IntProviders.validateCodec(int minValue, int maxValue, Codec<T> codec)` —
/// validate an arbitrary `IntProvider`-typed codec (Java's
/// `Codec<T extends IntProvider>` collapses to `Codec<IntProvider>` in the
/// closed-enum port). `codec(minValue, maxValue)` delegates here with `CODEC`,
/// mirroring Java's `codec()`/`validateCodec()` split.
pub fn int_provider_validate_codec<Ops: DynamicOps + 'static>(
    min_value: i32,
    max_value: i32,
    codec: Arc<dyn Codec<IntProvider, Ops>>,
) -> Arc<dyn Codec<IntProvider, Ops>> {
    codec::validate(
        codec,
        Arc::new(move |v: &IntProvider| validate_int_provider(min_value, max_value, v)),
    )
}

/// `IntProviders.NON_NEGATIVE_CODEC` — `codec(0, Integer.MAX_VALUE)`.
pub fn non_negative_int_provider_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<IntProvider, Ops>> {
    int_provider_codec_with_bounds(0, i32::MAX)
}

/// `IntProviders.POSITIVE_CODEC` — `codec(1, Integer.MAX_VALUE)`.
pub fn positive_int_provider_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<IntProvider, Ops>>
{
    int_provider_codec_with_bounds(1, i32::MAX)
}

/// `IntProviders.validate(int minValue, int maxValue, T value)` — the shared
/// lower/upper bound check with Paper's exact messages.
fn validate_int_provider(
    min_value: i32,
    max_value: i32,
    value: &IntProvider,
) -> DataResult<IntProvider> {
    if value.min_inclusive() < min_value {
        DataResult::error(format!(
            "Value provider too low: {} [{}-{}]",
            min_value,
            value.min_inclusive(),
            value.max_inclusive()
        ))
    } else if value.max_inclusive() > max_value {
        DataResult::error(format!(
            "Value provider too high: {} [{}-{}]",
            max_value,
            value.min_inclusive(),
            value.max_inclusive()
        ))
    } else {
        DataResult::success(value.clone())
    }
}
