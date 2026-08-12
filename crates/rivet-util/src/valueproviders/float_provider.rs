//! Port of `net.minecraft.util.valueproviders.FloatProvider` (interface, 26.2) —
//! the dispatch root of the float value-provider framework.
//!
//! Java is the interface (extending `SampledFloat`) every float provider
//! implements, with the dispatch codec `FloatProviders.CODEC`:
//!
//! ```text
//! CONSTANT_OR_DISPATCH_CODEC = Codec.either(
//!     Codec.FLOAT,
//!     BuiltInRegistries.FLOAT_PROVIDER_TYPE.byNameCodec()
//!         .dispatch(FloatProvider::codec, t -> t));
//! CODEC = CONSTANT_OR_DISPATCH_CODEC.xmap(
//!     either -> either.map(ConstantFloat::of, f -> (FloatProvider)f),
//!     f -> f instanceof ConstantFloat constantFloat ? Either.left(constantFloat.value()) : Either.right(f));
//! ```
//!
//! The port mirrors the sealed surface as a single [`FloatProvider`] enum over
//! the four variants (the same shape `HeightProvider` takes). No float provider
//! embeds a `FloatProvider`, so `CODEC` is *not* recursive (unlike
//! `IntProvider.CODEC`). The `"type"` registry codec reproduces Paper's exact
//! by-name error (`Unknown registry key in ResourceKey[minecraft:root /
//! minecraft:float_provider_type]: {name}`).

use crate::RandomSource;
use crate::valueproviders::clamped_normal_float::ClampedNormalFloat;
use crate::valueproviders::constant_float::ConstantFloat;
use crate::valueproviders::default_namespace;
use crate::valueproviders::float_provider_type::{
    FloatProviderTypeId, FloatProviderTypes, float_provider_type_by_name,
};
use crate::valueproviders::trapezoid_float::TrapezoidFloat;
use crate::valueproviders::uniform_float::UniformFloat;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::either::Either;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.FloatProvider` — the discriminated union
/// over the four concrete float providers.
///
/// Java models the interface as an open hierarchy; all implementors live in
/// this package and the dispatch (codec, `type()`) is a closed switch, so the
/// port collapses it to one enum.
///
/// Unlike `IntProvider`, the derived `PartialEq` here is faithful value
/// equality: all four concrete providers are Java records, whose `equals` IS
/// value equality. The identity-semantics case in this package is
/// `MultipliedFloats` (a plain class with no `equals` override), which lives on
/// `SampledFloat`, not here.
#[derive(Debug, Clone, PartialEq)]
pub enum FloatProvider {
    /// `ConstantFloat`.
    Constant(ConstantFloat),
    /// `UniformFloat`.
    Uniform(UniformFloat),
    /// `ClampedNormalFloat`.
    ClampedNormal(ClampedNormalFloat),
    /// `TrapezoidFloat`.
    Trapezoid(TrapezoidFloat),
}

impl FloatProvider {
    /// `sample(RandomSource)` — dispatch to the concrete provider's sample,
    /// preserving Java float arithmetic.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> f32 {
        match self {
            FloatProvider::Constant(p) => p.sample(random),
            FloatProvider::Uniform(p) => p.sample(random),
            FloatProvider::ClampedNormal(p) => p.sample(random),
            FloatProvider::Trapezoid(p) => p.sample(random),
        }
    }

    /// `FloatProvider.codec()` — the registry-held `MapCodec` this provider
    /// dispatches on, as the provider-type identity (the analogue of
    /// `type()` / the `FloatProviders` bootstrap's registration).
    pub fn type_id(&self) -> FloatProviderTypeId {
        match self {
            FloatProvider::Constant(_) => FloatProviderTypes::CONSTANT,
            FloatProvider::Uniform(_) => FloatProviderTypes::UNIFORM,
            FloatProvider::ClampedNormal(_) => FloatProviderTypes::CLAMPED_NORMAL,
            FloatProvider::Trapezoid(_) => FloatProviderTypes::TRAPEZOID,
        }
    }

    /// `min()` — dispatch to the concrete provider's lower bound.
    pub fn min(&self) -> f32 {
        match self {
            FloatProvider::Constant(p) => p.min(),
            FloatProvider::Uniform(p) => p.min(),
            FloatProvider::ClampedNormal(p) => p.min(),
            FloatProvider::Trapezoid(p) => p.min(),
        }
    }

    /// `max()` — dispatch to the concrete provider's upper bound.
    pub fn max(&self) -> f32 {
        match self {
            FloatProvider::Constant(p) => p.max(),
            FloatProvider::Uniform(p) => p.max(),
            FloatProvider::ClampedNormal(p) => p.max(),
            FloatProvider::Trapezoid(p) => p.max(),
        }
    }

    pub(crate) fn is_constant(&self) -> bool {
        matches!(self, FloatProvider::Constant(_))
    }

    pub(crate) fn as_constant_value(&self) -> f32 {
        match self {
            FloatProvider::Constant(c) => c.value(),
            _ => panic!("expected a constant float provider"),
        }
    }
}

impl fmt::Display for FloatProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FloatProvider::Constant(p) => write!(f, "{p}"),
            FloatProvider::Uniform(p) => write!(f, "{p}"),
            FloatProvider::ClampedNormal(p) => write!(f, "{p}"),
            FloatProvider::Trapezoid(p) => write!(f, "{p}"),
        }
    }
}

/// `FloatProviders.CODEC` — the constant-or-dispatch codec, as the ops-generic
/// `float_provider_codec::<Ops>()` factory. Not recursive: no float provider
/// embeds a `FloatProvider`.
pub fn float_provider_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<FloatProvider, Ops>> {
    // `BuiltInRegistries.FLOAT_PROVIDER_TYPE.byNameCodec().dispatch(...)`.
    let dispatch = map_codec::codec_of(key_dispatch_codec::dispatch_map::<
        FloatProviderTypeId,
        FloatProvider,
        Ops,
    >(
        "type",
        float_provider_type_by_name_codec::<Ops>(),
        Arc::new(|p: &FloatProvider| DataResult::success(FloatProvider::type_id(p))),
        codec_for_type(),
    ));
    // `Codec.either(Codec.FLOAT, dispatch)`.
    let either = codec::either::<f32, FloatProvider, Ops>(codec::float_codec::<Ops>(), dispatch);
    // `.xmap(either -> either.map(ConstantFloat::of, f -> (FloatProvider)f),
    //  f -> f instanceof ConstantFloat ? Either.left(value) : Either.right(f))`.
    codec::xmap(
        either,
        Arc::new(|e: &Either<f32, FloatProvider>| match e {
            Either::Left(v) => FloatProvider::Constant(ConstantFloat::of(*v)),
            Either::Right(provider) => provider.clone(),
        }),
        Arc::new(|p: &FloatProvider| {
            if p.is_constant() {
                Either::left(p.as_constant_value())
            } else {
                Either::right(p.clone())
            }
        }),
    )
}

/// `FloatProvider::codec` — resolve a `FloatProviderTypeId` to its
/// `MapCodec<FloatProvider>` (the dispatch's `codec` function).
fn codec_for_type<Ops: DynamicOps + 'static>()
-> key_dispatch_codec::CodecFn<FloatProviderTypeId, FloatProvider, Ops> {
    Arc::new(|k: &FloatProviderTypeId| {
        if *k == FloatProviderTypes::CONSTANT {
            DataResult::success(erase_map_codec::<ConstantFloat, Ops>(
                crate::valueproviders::constant_float::constant_float_map_codec::<Ops>(),
                Arc::new(|c: &ConstantFloat| FloatProvider::Constant(*c)),
                Arc::new(|p: &FloatProvider| match p {
                    FloatProvider::Constant(c) => *c,
                    _ => panic!("float provider dispatch produced a non-constant value"),
                }),
            ))
        } else if *k == FloatProviderTypes::UNIFORM {
            DataResult::success(erase_map_codec::<UniformFloat, Ops>(
                crate::valueproviders::uniform_float::uniform_float_map_codec::<Ops>(),
                Arc::new(|u: &UniformFloat| FloatProvider::Uniform(*u)),
                Arc::new(|p: &FloatProvider| match p {
                    FloatProvider::Uniform(u) => *u,
                    _ => panic!("float provider dispatch produced a non-uniform value"),
                }),
            ))
        } else if *k == FloatProviderTypes::CLAMPED_NORMAL {
            DataResult::success(erase_map_codec::<ClampedNormalFloat, Ops>(
                crate::valueproviders::clamped_normal_float::clamped_normal_float_map_codec::<Ops>(
                ),
                Arc::new(|c: &ClampedNormalFloat| FloatProvider::ClampedNormal(*c)),
                Arc::new(|p: &FloatProvider| match p {
                    FloatProvider::ClampedNormal(c) => *c,
                    _ => panic!("float provider dispatch produced a non-clamped-normal value"),
                }),
            ))
        } else if *k == FloatProviderTypes::TRAPEZOID {
            DataResult::success(erase_map_codec::<TrapezoidFloat, Ops>(
                crate::valueproviders::trapezoid_float::trapezoid_float_map_codec::<Ops>(),
                Arc::new(|t: &TrapezoidFloat| FloatProvider::Trapezoid(*t)),
                Arc::new(|p: &FloatProvider| match p {
                    FloatProvider::Trapezoid(t) => *t,
                    _ => panic!("float provider dispatch produced a non-trapezoid value"),
                }),
            ))
        } else {
            DataResult::error(format!(
                "Unknown registry key in ResourceKey[minecraft:root / minecraft:float_provider_type]: {}",
                k.location
            ))
        }
    })
}

/// Lift a concrete provider's `MapCodec<C>` to `MapCodec<FloatProvider>` —
/// Java's `MapCodec<? extends FloatProvider>` variance, via xmap.
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    wrap: Arc<dyn Fn(&C) -> FloatProvider + Send + Sync>,
    unwrap: Arc<dyn Fn(&FloatProvider) -> C + Send + Sync>,
) -> Arc<dyn MapCodec<FloatProvider, Ops>>
where
    C: 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(inner, wrap, unwrap)
}

/// `BuiltInRegistries.FLOAT_PROVIDER_TYPE.byNameCodec()` over the type id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key().
/// identifier())`.
///
/// The unknown-key error reproduces Paper's exactly: `"Unknown registry key in "
/// + this.key() + ": " + name` where `this.key()` is
/// `Registries.FLOAT_PROVIDER_TYPE` (`createRegistryKey("float_provider_type")`,
/// toString `"ResourceKey[minecraft:root / minecraft:float_provider_type]"`).
/// Bare names default to the `minecraft:` namespace (the `Identifier`
/// single-string constructor). The only documented divergence from Paper is the
/// `Identifier` malformed-string diagnostic (see `valueproviders`' module doc).
pub fn float_provider_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<FloatProviderTypeId, Ops>> {
    codec::comap_flat_map::<String, FloatProviderTypeId, Ops>(
        codec::string_codec::<Ops>(),
        Arc::new(|name: &String| {
            let namespaced = default_namespace(name);
            match float_provider_type_by_name(&namespaced) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:float_provider_type]: {}",
                    namespaced
                )),
            }
        }),
        Arc::new(|id: &FloatProviderTypeId| id.location.to_string()),
    )
}

/// `FloatProviders.codec(float minValue, float maxValue)` — `CODEC.validate`
/// with both bounds.
pub fn float_provider_codec_with_bounds<Ops: DynamicOps + 'static>(
    min_value: f32,
    max_value: f32,
) -> Arc<dyn Codec<FloatProvider, Ops>> {
    codec::validate(
        float_provider_codec::<Ops>(),
        Arc::new(move |v: &FloatProvider| {
            if v.min() < min_value {
                DataResult::error(format!(
                    "Value provider too low: {} [{}-{}]",
                    crate::java_float_format::java_float_to_string(min_value),
                    crate::java_float_format::java_float_to_string(v.min()),
                    crate::java_float_format::java_float_to_string(v.max())
                ))
            } else if v.max() > max_value {
                DataResult::error(format!(
                    "Value provider too high: {} [{}-{}]",
                    crate::java_float_format::java_float_to_string(max_value),
                    crate::java_float_format::java_float_to_string(v.min()),
                    crate::java_float_format::java_float_to_string(v.max())
                ))
            } else {
                DataResult::success(v.clone())
            }
        }),
    )
}

/// `FloatProviders.codec(float minValue)` — `CODEC.validate` with only the
/// lower bound.
pub fn float_provider_codec_with_min<Ops: DynamicOps + 'static>(
    min_value: f32,
) -> Arc<dyn Codec<FloatProvider, Ops>> {
    codec::validate(
        float_provider_codec::<Ops>(),
        Arc::new(move |v: &FloatProvider| {
            if v.min() < min_value {
                DataResult::error(format!(
                    "Value provider too low: {} [{}-{}]",
                    crate::java_float_format::java_float_to_string(min_value),
                    crate::java_float_format::java_float_to_string(v.min()),
                    crate::java_float_format::java_float_to_string(v.max())
                ))
            } else {
                DataResult::success(v.clone())
            }
        }),
    )
}
