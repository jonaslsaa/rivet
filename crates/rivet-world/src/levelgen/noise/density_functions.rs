//! Port of `net.minecraft.world.level.levelgen.DensityFunctions` (class, 26.2).
//!
//! The dispatch hub of the density-function framework: the `DIRECT_CODEC`
//! (either a bare constant or the type-dispatched value codec), the
//! `bootstrap` registration order of the density-function types, and every
//! concrete value function (`Constant`, `Marker`, `Mapped`,
//! `TwoArgumentSimpleFunction`/`Ap2`/`MulOrAdd`, `Clamp`, `RangeChoice`,
//! `IntervalSelect`, `Shift`/`ShiftA`/`ShiftB`/`ShiftNoise`, `ShiftedNoise`,
//! `Noise`, `Spline`, `YClampedGradient`, `FindTopSurface`,
//! `EndIslandDensityFunction`, `BlendAlpha`, `BlendOffset`, `HolderHolder`,
//! `BeardifierMarker`).
//!
//! ## The dispatch codec
//!
//! Java:
//!
//! ```java
//! CODEC = BuiltInRegistries.DENSITY_FUNCTION_TYPE.byNameCodec()
//!     .dispatch(function -> function.codec().codec(), Function.identity());
//! DIRECT_CODEC = Codec.either(NOISE_VALUE_CODEC, CODEC)
//!     .xmap(either -> either.map(DensityFunctions::constant, Function.identity()),
//!           function -> function instanceof Constant c ? Either.left(c.value()) : Either.right(function));
//! ```
//!
//! The Rust port follows the `BlockPredicate` erased-carrier pattern: the
//! dispatch discriminates on the [`DensityFunctionTypeId`] (this slice's
//! stand-in for the `MapCodec<? extends DensityFunction>` registry element),
//! the per-type `MapCodec`s are resolved by the `#177` dispatch table
//! (`codec_for_type`), and the whole nested graph threads a single recursive
//! child codec (`top`) exactly like `block_predicate_codec`'s `recursive`.
//!
//! Java's `KeyDispatchDataCodec` wrapper (`codec()` instance method) is not a
//! distinct value here — the dispatch table holds the `MapCodec` directly and
//! `type_id()` resolves which one a value uses. The `MarkerOrMarked` /
//! `TwoArgumentSimpleFunction` / `PureTransformer` / `TransformerWithContext`
//! / `ShiftNoise` interface defaults are flattened onto the concrete records
//! (Rust has no interface-default inheritance; each record carries the exact
//! behavior Java's default would produce).
//!
//! ## Paper's `EndIslandDensityFunction`
//!
//! The Paper `NoiseCache` (the 8192-entry chunk-key → island-size cache) is
//! ported faithfully as a per-function cache (the `ThreadLocal<WeakHashMap>`
//! becomes the field-level `Option`), and Paper's default
//! `configFixMC159283()` = `true` is pinned (the long-sqrt distance path); the
//! configurable-disable path defers with `PlatformHooks` (RivetTodo #177).

use crate::level::dimension::dimension_type::{MAX_Y, MIN_Y};
use crate::levelgen::noise::beardifier_marker::BeardifierMarker;
use crate::levelgen::noise::density_function::{DensityFunction, FunctionContext, NoiseHolder};
use crate::levelgen::noise::density_function_type::{
    DensityFunctionTypeId, DensityFunctionTypes, density_function_type_by_name,
};
use crate::levelgen::synth::normal_noise::NoiseParameters;
use crate::levelgen::synth::simplex_noise::SimplexNoise;
use rivet_registry::Holder;
use rivet_registry::core::ChunkPos;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::either::Either;
use rivet_serialization::functions::{Fn4, Fn5, Fn6};
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use rivet_util::bounded_float_function::BoundedFloatFunction;
use rivet_util::cubic_spline::{self, CubicSpline};
use rivet_util::mth;
use rivet_util::random::{LegacyRandomSource, RandomSource};
use std::any::Any;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

/// `DensityFunctions.MAX_REASONABLE_NOISE_VALUE` — `1000000.0`.
const MAX_REASONABLE_NOISE_VALUE: f64 = 1000000.0;
/// `DensityFunctions.NOISE_VALUE_CODEC` — `Codec.doubleRange(-1000000.0, 1000000.0)`.
pub(crate) fn noise_value_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<f64, Ops>> {
    codec::double_range(-MAX_REASONABLE_NOISE_VALUE, MAX_REASONABLE_NOISE_VALUE)
}

/// The `DirectEither` alias — `Either<f64, Arc<dyn DensityFunction>>`, the
/// `DIRECT_CODEC` either arms.
pub(crate) type DirectEither = Either<f64, Arc<dyn DensityFunction>>;

/// `DensityFunctions.DIRECT_CODEC` — `either(NOISE_VALUE_CODEC, CODEC)` xmapped
/// to `Arc<dyn DensityFunction>` (a bare constant or a dispatched function).
///
/// The `top` is the recursive child codec: the `Codec<DensityFunction>` half
/// (the `CODEC`'s `DensityFunction.CODEC` child used by every argument field)
/// is `top` itself, so nested functions round-trip through the same graph.
pub(crate) fn direct_codec<Ops>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    let dispatch = codec_for_type_dispatch(top);
    let either = codec::either(noise_value_codec::<Ops>(), dispatch);
    codec::xmap(
        either,
        Arc::new(|e: &DirectEither| e.map_ref(|v| constant(*v), |f| f.clone())),
        Arc::new(
            |f: &Arc<dyn DensityFunction>| match f.as_any().downcast_ref::<Constant>() {
                Some(c) => Either::left(c.value),
                _ => Either::right(f.clone()),
            },
        ),
    )
}

/// `DensityFunctions.CODEC` — the `DENSITY_FUNCTION_TYPE` by-name dispatch, as
/// the erased `Arc<dyn DensityFunction>` value codec. This is the `CODEC` half
/// of `DIRECT_CODEC` (the non-constant arm).
fn codec_for_type_dispatch<Ops>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    let dispatch =
        key_dispatch_codec::dispatch_map::<DensityFunctionTypeId, Arc<dyn DensityFunction>, Ops>(
            "type",
            density_function_type_by_name_codec::<Ops>(),
            Arc::new(|f: &Arc<dyn DensityFunction>| {
                DataResult::success(DensityFunction::type_id(&**f))
            }),
            codec_for_type(top),
        );
    map_codec::codec_of(dispatch)
}

/// `DensityFunctionType::codec` — resolve a `DensityFunctionTypeId` to its
/// erased `MapCodec<Arc<dyn DensityFunction>>` (the dispatch's `codec`
/// function).
fn codec_for_type<Ops>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> key_dispatch_codec::CodecFn<DensityFunctionTypeId, Arc<dyn DensityFunction>, Ops>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    Arc::new(move |k: &DensityFunctionTypeId| {
        let c = codec_for_type_inner(k, top.clone());
        match c {
            Some(mc) => DataResult::success(mc),
            None => DataResult::error(format!(
                "Density function type '{}' is not ported",
                k.location
            )),
        }
    })
}

/// The per-type `MapCodec<Arc<dyn DensityFunction>>` lookup. Each concrete
/// builder erases its own type to the carrier (the `block_predicate.rs`
/// pattern), so every branch is already the erased codec.
fn codec_for_type_inner<Ops>(
    k: &DensityFunctionTypeId,
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Option<Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    if *k == DensityFunctionTypes::BLEND_ALPHA {
        Some(blend_alpha_map_codec::<Ops>())
    } else if *k == DensityFunctionTypes::BLEND_OFFSET {
        Some(blend_offset_map_codec::<Ops>())
    } else if *k == DensityFunctionTypes::BEARDIFIER {
        Some(beardifier_marker_map_codec::<Ops>())
    } else if *k == DensityFunctionTypes::INTERPOLATED {
        Some(marker_map_codec::<Ops>(
            MarkerType::Interpolated,
            top.clone(),
        ))
    } else if *k == DensityFunctionTypes::FLAT_CACHE {
        Some(marker_map_codec::<Ops>(MarkerType::FlatCache, top.clone()))
    } else if *k == DensityFunctionTypes::CACHE_2D {
        Some(marker_map_codec::<Ops>(MarkerType::Cache2D, top.clone()))
    } else if *k == DensityFunctionTypes::CACHE_ONCE {
        Some(marker_map_codec::<Ops>(MarkerType::CacheOnce, top.clone()))
    } else if *k == DensityFunctionTypes::CACHE_ALL_IN_CELL {
        Some(marker_map_codec::<Ops>(
            MarkerType::CacheAllInCell,
            top.clone(),
        ))
    } else if *k == DensityFunctionTypes::BLEND_DENSITY {
        Some(marker_map_codec::<Ops>(
            MarkerType::BlendDensity,
            top.clone(),
        ))
    } else if *k == DensityFunctionTypes::NOISE {
        Some(noise_map_codec::<Ops>())
    } else if *k == DensityFunctionTypes::END_ISLANDS {
        Some(end_islands_map_codec::<Ops>())
    } else if *k == DensityFunctionTypes::SHIFTED_NOISE {
        Some(shifted_noise_map_codec::<Ops>(top))
    } else if *k == DensityFunctionTypes::RANGE_CHOICE {
        Some(range_choice_map_codec::<Ops>(top))
    } else if *k == DensityFunctionTypes::INTERVAL_SELECT {
        Some(interval_select_map_codec::<Ops>(top))
    } else if *k == DensityFunctionTypes::SHIFT_A {
        Some(shift_a_map_codec::<Ops>())
    } else if *k == DensityFunctionTypes::SHIFT_B {
        Some(shift_b_map_codec::<Ops>())
    } else if *k == DensityFunctionTypes::SHIFT {
        Some(shift_map_codec::<Ops>())
    } else if *k == DensityFunctionTypes::CLAMP {
        Some(clamp_map_codec::<Ops>(top))
    } else if *k == DensityFunctionTypes::ABS {
        Some(mapped_map_codec::<Ops>(MappedType::Abs, top.clone()))
    } else if *k == DensityFunctionTypes::SQUARE {
        Some(mapped_map_codec::<Ops>(MappedType::Square, top.clone()))
    } else if *k == DensityFunctionTypes::CUBE {
        Some(mapped_map_codec::<Ops>(MappedType::Cube, top.clone()))
    } else if *k == DensityFunctionTypes::HALF_NEGATIVE {
        Some(mapped_map_codec::<Ops>(
            MappedType::HalfNegative,
            top.clone(),
        ))
    } else if *k == DensityFunctionTypes::QUARTER_NEGATIVE {
        Some(mapped_map_codec::<Ops>(
            MappedType::QuarterNegative,
            top.clone(),
        ))
    } else if *k == DensityFunctionTypes::INVERT {
        Some(mapped_map_codec::<Ops>(MappedType::Invert, top.clone()))
    } else if *k == DensityFunctionTypes::SQUEEZE {
        Some(mapped_map_codec::<Ops>(MappedType::Squeeze, top.clone()))
    } else if *k == DensityFunctionTypes::ADD {
        Some(two_arg_map_codec::<Ops>(TwoArgumentType::Add, top.clone()))
    } else if *k == DensityFunctionTypes::MUL {
        Some(two_arg_map_codec::<Ops>(TwoArgumentType::Mul, top.clone()))
    } else if *k == DensityFunctionTypes::MIN {
        Some(two_arg_map_codec::<Ops>(TwoArgumentType::Min, top.clone()))
    } else if *k == DensityFunctionTypes::MAX {
        Some(two_arg_map_codec::<Ops>(TwoArgumentType::Max, top.clone()))
    } else if *k == DensityFunctionTypes::SPLINE {
        Some(spline_map_codec::<Ops>(top))
    } else if *k == DensityFunctionTypes::CONSTANT {
        Some(constant_map_codec::<Ops>())
    } else if *k == DensityFunctionTypes::Y_CLAMPED_GRADIENT {
        Some(y_clamped_gradient_map_codec::<Ops>())
    } else if *k == DensityFunctionTypes::FIND_TOP_SURFACE {
        Some(find_top_surface_map_codec::<Ops>(top))
    } else {
        None
    }
}

/// `BuiltInRegistries.DENSITY_FUNCTION_TYPE.byNameCodec()` over the erased id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key()
/// .identifier())`. The unknown-key error matches Paper's by-name codec:
/// `"Unknown registry key in " + this.key() + ": " + name`.
fn density_function_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<DensityFunctionTypeId, Ops>> {
    codec::comap_flat_map::<rivet_registry::Identifier, DensityFunctionTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &rivet_registry::Identifier| {
            match density_function_type_by_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/density_function_type]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &DensityFunctionTypeId| rivet_registry::Identifier::parse(id.location)),
    )
}

/// Lift a concrete function's `MapCodec<C>` to `MapCodec<Arc<dyn DensityFunction>>`
/// — Java's `MapCodec<? extends DensityFunction>` variance, via xmap (the same
/// lift `block_predicate.rs`'s `erase_map_codec` performs).
fn erase_map_codec<C, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    location: String,
) -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>>
where
    C: DensityFunction + Clone + 'static,
{
    map_codec::xmap(
        inner,
        Arc::new(move |c: &C| -> Arc<dyn DensityFunction> { Arc::new(c.clone()) }),
        Arc::new(move |f: &Arc<dyn DensityFunction>| -> C {
            f.as_any()
                .downcast_ref::<C>()
                .unwrap_or_else(|| {
                    panic!(
                        "density function codec for '{}' applied to a value of a different type",
                        location
                    )
                })
                .clone()
        }),
    )
}

// ---------------------------------------------------------------------------
// Single-argument helpers (Java `singleArgumentCodec`/`singleFunctionArgumentCodec`)
// ---------------------------------------------------------------------------

/// `DensityFunctions.singleArgumentCodec(Codec<A>, constructor, getter)` —
/// `argumentCodec.fieldOf("argument").xmap(...)`.
fn single_argument_map_codec<A, C, Ops: DynamicOps + 'static>(
    argument_codec: Arc<dyn Codec<A, Ops>>,
    constructor: Arc<dyn Fn(&A) -> C + Send + Sync>,
    getter: Arc<dyn Fn(&C) -> A + Send + Sync>,
) -> Arc<dyn MapCodec<C, Ops>>
where
    A: 'static,
    C: 'static,
{
    map_codec::xmap(
        codec::field_of(argument_codec, "argument".to_string()),
        constructor,
        getter,
    )
}

#[allow(clippy::type_complexity)] // erased `Arc<dyn DensityFunction>` constructor/getter closures
/// `singleFunctionArgumentCodec(constructor, getter)` over `DensityFunction.CODEC`.
fn single_function_argument_map_codec<C, Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
    constructor: Arc<dyn Fn(&Arc<dyn DensityFunction>) -> C + Send + Sync>,
    getter: Arc<dyn Fn(&C) -> Arc<dyn DensityFunction> + Send + Sync>,
) -> Arc<dyn MapCodec<C, Ops>>
where
    C: 'static,
{
    single_argument_map_codec(top, constructor, getter)
}
#[allow(clippy::type_complexity)] // erased `Arc<dyn DensityFunction>` constructor/getter closures
/// `doubleFunctionArgumentCodec(constructor, firstGetter, secondGetter)` — the
/// `argument1`/`argument2` record.
fn double_function_argument_map_codec<C, Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
    constructor: Arc<
        dyn Fn(&Arc<dyn DensityFunction>, &Arc<dyn DensityFunction>) -> C + Send + Sync,
    >,
    first_getter: Arc<dyn Fn(&C) -> Arc<dyn DensityFunction> + Send + Sync>,
    second_getter: Arc<dyn Fn(&C) -> Arc<dyn DensityFunction> + Send + Sync>,
) -> Arc<dyn MapCodec<C, Ops>>
where
    C: 'static,
{
    let f1 = codec::field_of(top.clone(), "argument1".to_string());
    let f2 = codec::field_of(top, "argument2".to_string());
    map_codec::of(
        // Encoder: both fields.
        {
            let first_getter = first_getter.clone();
            let second_getter = second_getter.clone();
            map_encoder_fields2(f1.clone(), f2.clone(), first_getter, second_getter)
        },
        // Decoder: `ap2` with error accumulation.
        map_decoder_ap2(f1, f2, constructor),
        "DoubleFunctionArgument".to_string(),
    )
}

// ---------------------------------------------------------------------------
// Constants / enum identities
// ---------------------------------------------------------------------------

/// `DensityFunctions.Marker.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerType {
    Interpolated,
    FlatCache,
    Cache2D,
    CacheOnce,
    CacheAllInCell,
    BlendDensity,
}

impl MarkerType {
    /// `getSerializedName()`.
    pub fn serialized_name(&self) -> &'static str {
        match self {
            MarkerType::Interpolated => "interpolated",
            MarkerType::FlatCache => "flat_cache",
            MarkerType::Cache2D => "cache_2d",
            MarkerType::CacheOnce => "cache_once",
            MarkerType::CacheAllInCell => "cache_all_in_cell",
            MarkerType::BlendDensity => "blend_density",
        }
    }
}

/// `DensityFunctions.Mapped.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedType {
    Abs,
    Square,
    Cube,
    HalfNegative,
    QuarterNegative,
    Invert,
    Squeeze,
}

impl MappedType {
    /// `getSerializedName()`.
    pub fn serialized_name(&self) -> &'static str {
        match self {
            MappedType::Abs => "abs",
            MappedType::Square => "square",
            MappedType::Cube => "cube",
            MappedType::HalfNegative => "half_negative",
            MappedType::QuarterNegative => "quarter_negative",
            MappedType::Invert => "invert",
            MappedType::Squeeze => "squeeze",
        }
    }
}

/// `DensityFunctions.TwoArgumentSimpleFunction.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoArgumentType {
    Add,
    Mul,
    Min,
    Max,
}

impl TwoArgumentType {
    /// `getSerializedName()`.
    pub fn serialized_name(&self) -> &'static str {
        match self {
            TwoArgumentType::Add => "add",
            TwoArgumentType::Mul => "mul",
            TwoArgumentType::Min => "min",
            TwoArgumentType::Max => "max",
        }
    }
}

// ---------------------------------------------------------------------------
// The concrete functions
// ---------------------------------------------------------------------------

/// `DensityFunctions.Constant(double value)` — `SimpleFunction`.
#[derive(Debug, Clone)]
pub struct Constant {
    value: f64,
}

impl Constant {
    /// `Constant.ZERO`.
    pub fn zero() -> Arc<dyn DensityFunction> {
        Arc::new(Constant { value: 0.0 })
    }

    /// `new Constant(double)`.
    pub fn new(value: f64) -> Self {
        Constant { value }
    }

    /// `value()`.
    pub fn value(&self) -> f64 {
        self.value
    }
}

impl DensityFunction for Constant {
    fn compute(&self, _context: &dyn FunctionContext) -> f64 {
        self.value
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        _context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        output.fill(self.value);
    }
    fn min_value(&self) -> f64 {
        self.value
    }
    fn max_value(&self) -> f64 {
        self.value
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::CONSTANT
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.BlendAlpha.INSTANCE`.
#[derive(Debug, Clone)]
pub struct BlendAlpha;

impl DensityFunction for BlendAlpha {
    fn compute(&self, _context: &dyn FunctionContext) -> f64 {
        1.0
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        _context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        output.fill(1.0);
    }
    fn min_value(&self) -> f64 {
        1.0
    }
    fn max_value(&self) -> f64 {
        1.0
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::BLEND_ALPHA
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(BlendAlpha)
    }
}

/// `DensityFunctions.BlendOffset.INSTANCE`.
#[derive(Debug, Clone)]
pub struct BlendOffset;

impl DensityFunction for BlendOffset {
    fn compute(&self, _context: &dyn FunctionContext) -> f64 {
        0.0
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        _context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        output.fill(0.0);
    }
    fn min_value(&self) -> f64 {
        0.0
    }
    fn max_value(&self) -> f64 {
        0.0
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::BLEND_OFFSET
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(BlendOffset)
    }
}

/// `DensityFunctions.Marker(Marker.Type type, DensityFunction wrapped)` — the
/// cache/behavior marker.
#[derive(Debug, Clone)]
pub struct Marker {
    marker_type: MarkerType,
    wrapped: Arc<dyn DensityFunction>,
}

impl Marker {
    /// `interpolated/function`/`flatCache`/`cache2d`/`cacheOnce`/`cacheAllInCell`
    /// static factories.
    pub fn new(marker_type: MarkerType, wrapped: Arc<dyn DensityFunction>) -> Self {
        Marker {
            marker_type,
            wrapped,
        }
    }

    /// `type()`.
    pub fn marker_type(&self) -> MarkerType {
        self.marker_type
    }

    /// `wrapped()`.
    pub fn wrapped(&self) -> &Arc<dyn DensityFunction> {
        &self.wrapped
    }
}

impl DensityFunction for Marker {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.wrapped.compute(context)
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        self.wrapped.fill_array(output, context_provider);
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        // Java `MarkerOrMarked.mapChildren` default: `new Marker(type,
        // visitor.apply(wrapped))`.
        Arc::new(Marker::new(self.marker_type, visitor.apply(&*self.wrapped)))
    }
    fn min_value(&self) -> f64 {
        if self.marker_type == MarkerType::BlendDensity {
            f64::NEG_INFINITY
        } else {
            self.wrapped.min_value()
        }
    }
    fn max_value(&self) -> f64 {
        if self.marker_type == MarkerType::BlendDensity {
            f64::INFINITY
        } else {
            self.wrapped.max_value()
        }
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        match self.marker_type {
            MarkerType::Interpolated => DensityFunctionTypes::INTERPOLATED,
            MarkerType::FlatCache => DensityFunctionTypes::FLAT_CACHE,
            MarkerType::Cache2D => DensityFunctionTypes::CACHE_2D,
            MarkerType::CacheOnce => DensityFunctionTypes::CACHE_ONCE,
            MarkerType::CacheAllInCell => DensityFunctionTypes::CACHE_ALL_IN_CELL,
            MarkerType::BlendDensity => DensityFunctionTypes::BLEND_DENSITY,
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.Mapped(Mapped.Type type, DensityFunction input, double
/// minValue, double maxValue)` — the pointwise transformer.
#[derive(Debug, Clone)]
pub struct Mapped {
    mapped_type: MappedType,
    input: Arc<dyn DensityFunction>,
    min_value: f64,
    max_value: f64,
}

impl Mapped {
    /// `Mapped.create(Type, DensityFunction)` — computes the transformed bounds.
    pub fn create(mapped_type: MappedType, input: Arc<dyn DensityFunction>) -> Self {
        let min_value = input.min_value();
        let max_value = input.max_value();
        let min_image = Self::transform_value(mapped_type, min_value);
        let max_image = Self::transform_value(mapped_type, max_value);
        if mapped_type == MappedType::Invert {
            if min_value < 0.0 && max_value > 0.0 {
                Mapped {
                    mapped_type,
                    input,
                    min_value: f64::NEG_INFINITY,
                    max_value: f64::INFINITY,
                }
            } else {
                Mapped {
                    mapped_type,
                    input,
                    min_value: max_image,
                    max_value: min_image,
                }
            }
        } else if mapped_type != MappedType::Abs && mapped_type != MappedType::Square {
            Mapped {
                mapped_type,
                input,
                min_value: min_image,
                max_value: max_image,
            }
        } else {
            Mapped {
                mapped_type,
                input,
                min_value: max_image.max(0.0),
                max_value: max_image.max(min_value),
            }
        }
    }

    /// `Mapped.transform(Type, double)`.
    fn transform_value(mapped_type: MappedType, input: f64) -> f64 {
        match mapped_type {
            MappedType::Abs => input.abs(),
            MappedType::Square => input * input,
            MappedType::Cube => input * input * input,
            MappedType::HalfNegative => {
                if input > 0.0 {
                    input
                } else {
                    input * 0.5
                }
            }
            MappedType::QuarterNegative => {
                if input > 0.0 {
                    input
                } else {
                    input * 0.25
                }
            }
            MappedType::Invert => 1.0 / input,
            MappedType::Squeeze => {
                let c = mth::clamp_f64(input, -1.0, 1.0);
                c / 2.0 - c * c * c / 24.0
            }
        }
    }

    /// `transform(double)` — the pointwise transform.
    pub fn transform(&self, input: f64) -> f64 {
        Self::transform_value(self.mapped_type, input)
    }

    /// `type()`.
    pub fn mapped_type(&self) -> MappedType {
        self.mapped_type
    }

    /// `input()`.
    pub fn input(&self) -> &Arc<dyn DensityFunction> {
        &self.input
    }

    /// `minValue()`.
    pub fn min_value(&self) -> f64 {
        self.min_value
    }

    /// `maxValue()`.
    pub fn max_value(&self) -> f64 {
        self.max_value
    }
}

impl DensityFunction for Mapped {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.transform(self.input.compute(context))
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        self.input.fill_array(output, context_provider);
        for v in output.iter_mut() {
            *v = self.transform(*v);
        }
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        Arc::new(Mapped::create(
            self.mapped_type,
            visitor.apply(&*self.input),
        ))
    }
    fn min_value(&self) -> f64 {
        self.min_value
    }
    fn max_value(&self) -> f64 {
        self.max_value
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        match self.mapped_type {
            MappedType::Abs => DensityFunctionTypes::ABS,
            MappedType::Square => DensityFunctionTypes::SQUARE,
            MappedType::Cube => DensityFunctionTypes::CUBE,
            MappedType::HalfNegative => DensityFunctionTypes::HALF_NEGATIVE,
            MappedType::QuarterNegative => DensityFunctionTypes::QUARTER_NEGATIVE,
            MappedType::Invert => DensityFunctionTypes::INVERT,
            MappedType::Squeeze => DensityFunctionTypes::SQUEEZE,
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.Clamp(DensityFunction input, double minValue, double
/// maxValue)`.
#[derive(Debug, Clone)]
pub struct Clamp {
    input: Arc<dyn DensityFunction>,
    min_value: f64,
    max_value: f64,
}

impl Clamp {
    /// `new DensityFunctions.Clamp(input, min, max)`.
    pub fn new(input: Arc<dyn DensityFunction>, min: f64, max: f64) -> Self {
        Clamp {
            input,
            min_value: min,
            max_value: max,
        }
    }

    /// `input()`.
    pub fn input(&self) -> &Arc<dyn DensityFunction> {
        &self.input
    }

    /// `minValue()`.
    pub fn min_value(&self) -> f64 {
        self.min_value
    }

    /// `maxValue()`.
    pub fn max_value(&self) -> f64 {
        self.max_value
    }
}

impl DensityFunction for Clamp {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        mth::clamp_f64(self.input.compute(context), self.min_value, self.max_value)
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        self.input.fill_array(output, context_provider);
        for v in output.iter_mut() {
            *v = mth::clamp_f64(*v, self.min_value, self.max_value);
        }
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        Arc::new(Clamp::new(
            visitor.apply(&*self.input),
            self.min_value,
            self.max_value,
        ))
    }
    fn min_value(&self) -> f64 {
        self.min_value
    }
    fn max_value(&self) -> f64 {
        self.max_value
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::CLAMP
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.RangeChoice(input, minInclusive, maxExclusive,
/// whenInRange, whenOutOfRange)`.
#[derive(Debug, Clone)]
pub struct RangeChoice {
    input: Arc<dyn DensityFunction>,
    min_inclusive: f64,
    max_exclusive: f64,
    when_in_range: Arc<dyn DensityFunction>,
    when_out_of_range: Arc<dyn DensityFunction>,
}

impl RangeChoice {
    /// `new DensityFunctions.RangeChoice(...)`.
    pub fn new(
        input: Arc<dyn DensityFunction>,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: Arc<dyn DensityFunction>,
        when_out_of_range: Arc<dyn DensityFunction>,
    ) -> Self {
        RangeChoice {
            input,
            min_inclusive,
            max_exclusive,
            when_in_range,
            when_out_of_range,
        }
    }

    /// `input()`.
    pub fn input(&self) -> &Arc<dyn DensityFunction> {
        &self.input
    }
    /// `minInclusive()`.
    pub fn min_inclusive(&self) -> f64 {
        self.min_inclusive
    }
    /// `maxExclusive()`.
    pub fn max_exclusive(&self) -> f64 {
        self.max_exclusive
    }
    /// `whenInRange()`.
    pub fn when_in_range(&self) -> &Arc<dyn DensityFunction> {
        &self.when_in_range
    }
    /// `whenOutOfRange()`.
    pub fn when_out_of_range(&self) -> &Arc<dyn DensityFunction> {
        &self.when_out_of_range
    }
}

impl DensityFunction for RangeChoice {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        let input_value = self.input.compute(context);
        if input_value >= self.min_inclusive && input_value < self.max_exclusive {
            self.when_in_range.compute(context)
        } else {
            self.when_out_of_range.compute(context)
        }
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        self.input.fill_array(output, context_provider);
        for (i, slot) in output.iter_mut().enumerate() {
            let v = *slot;
            if v >= self.min_inclusive && v < self.max_exclusive {
                *slot = self.when_in_range.compute(&context_provider.for_index(i));
            } else {
                *slot = self
                    .when_out_of_range
                    .compute(&context_provider.for_index(i));
            }
        }
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        Arc::new(RangeChoice::new(
            visitor.apply(&*self.input),
            self.min_inclusive,
            self.max_exclusive,
            visitor.apply(&*self.when_in_range),
            visitor.apply(&*self.when_out_of_range),
        ))
    }
    fn min_value(&self) -> f64 {
        self.when_in_range
            .min_value()
            .min(self.when_out_of_range.min_value())
    }
    fn max_value(&self) -> f64 {
        self.when_in_range
            .max_value()
            .max(self.when_out_of_range.max_value())
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::RANGE_CHOICE
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.IntervalSelect(input, DoubleList thresholds, List<
/// DensityFunction> functions)`.
#[derive(Debug, Clone)]
pub struct IntervalSelect {
    input: Arc<dyn DensityFunction>,
    thresholds: Vec<f64>,
    functions: Vec<Arc<dyn DensityFunction>>,
}

impl IntervalSelect {
    /// `new DensityFunctions.IntervalSelect(input, thresholds, functions)`.
    pub fn new(
        input: Arc<dyn DensityFunction>,
        thresholds: Vec<f64>,
        functions: Vec<Arc<dyn DensityFunction>>,
    ) -> Self {
        IntervalSelect {
            input,
            thresholds,
            functions,
        }
    }

    /// `input()`.
    pub fn input(&self) -> &Arc<dyn DensityFunction> {
        &self.input
    }
    /// `thresholds()`.
    pub fn thresholds(&self) -> &[f64] {
        &self.thresholds
    }
    /// `functions()`.
    pub fn functions(&self) -> &[Arc<dyn DensityFunction>] {
        &self.functions
    }

    /// `compute(context, input)` — the threshold dispatch.
    fn compute_with_input(&self, context: &dyn FunctionContext, input: f64) -> f64 {
        for i in 0..self.thresholds.len() {
            if input < self.thresholds[i] {
                return self.functions[i].compute(context);
            }
        }
        self.functions.last().unwrap().compute(context)
    }

    /// `validate()` — the `DataResult` checks Paper's `IntervalSelect.CODEC`
    /// `validate` applies: the threshold count must be `functions.len() - 1`,
    /// and the thresholds must be strictly ascending.
    fn validate(interval: &IntervalSelect) -> DataResult<IntervalSelect> {
        if interval.thresholds.len() != interval.functions.len() - 1 {
            return DataResult::error(format!(
                "Expected {} thresholds for {} functions, but got {}",
                interval.functions.len() - 1,
                interval.functions.len(),
                interval.thresholds.len()
            ));
        }
        if !interval.thresholds.windows(2).all(|w| w[0] < w[1]) {
            return DataResult::error(
                "Threshold values must be ordered from smallest to largest".to_string(),
            );
        }
        DataResult::success(interval.clone())
    }
}

impl DensityFunction for IntervalSelect {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.compute_with_input(context, self.input.compute(context))
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        self.input.fill_array(output, context_provider);
        for (i, slot) in output.iter_mut().enumerate() {
            *slot = self.compute_with_input(&context_provider.for_index(i), *slot);
        }
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        let functions = self
            .functions
            .iter()
            .map(|f| visitor.apply(&**f))
            .collect::<Vec<_>>();
        Arc::new(IntervalSelect::new(
            visitor.apply(&*self.input),
            self.thresholds.clone(),
            functions,
        ))
    }
    fn min_value(&self) -> f64 {
        let mut min_value = f64::MAX;
        for f in &self.functions {
            min_value = f.min_value().min(min_value);
        }
        min_value
    }
    fn max_value(&self) -> f64 {
        let mut max_value = -f64::MAX;
        for f in &self.functions {
            max_value = f.max_value().max(max_value);
        }
        max_value
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::INTERVAL_SELECT
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.Noise(NoiseHolder noise, double xzScale, double yScale)`.
#[derive(Debug, Clone)]
pub struct Noise {
    noise: NoiseHolder,
    xz_scale: f64,
    y_scale: f64,
}

impl Noise {
    /// `new DensityFunctions.Noise(noise, xzScale, yScale)`.
    pub fn new(noise: NoiseHolder, xz_scale: f64, y_scale: f64) -> Self {
        Noise {
            noise,
            xz_scale,
            y_scale,
        }
    }

    /// `noise()`.
    pub fn noise(&self) -> &NoiseHolder {
        &self.noise
    }
    /// `xzScale()`.
    pub fn xz_scale(&self) -> f64 {
        self.xz_scale
    }
    /// `yScale()`.
    pub fn y_scale(&self) -> f64 {
        self.y_scale
    }
}

impl DensityFunction for Noise {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.noise.get_value(
            context.block_x() as f64 * self.xz_scale,
            context.block_y() as f64 * self.y_scale,
            context.block_z() as f64 * self.xz_scale,
        )
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        Arc::new(Noise::new(
            visitor.visit_noise(&self.noise),
            self.xz_scale,
            self.y_scale,
        ))
    }
    fn min_value(&self) -> f64 {
        -self.max_value()
    }
    fn max_value(&self) -> f64 {
        self.noise.max_value()
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::NOISE
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.ShiftNoise` interface defaults — `minValue`/`maxValue`/
/// `compute(localX, localY, localZ)`/`fillArray`. The three `Shift*` records
/// share these.
pub trait ShiftNoiseDefault: DensityFunction {
    /// `offsetNoise()`.
    fn offset_noise(&self) -> &NoiseHolder;

    /// `compute(localX, localY, localZ)` — `offsetNoise().getValue(localX *
    /// 0.25, localY * 0.25, localZ * 0.25) * 4.0`.
    fn compute_local(&self, local_x: f64, local_y: f64, local_z: f64) -> f64 {
        self.offset_noise()
            .get_value(local_x * 0.25, local_y * 0.25, local_z * 0.25)
            * 4.0
    }
}

/// `DensityFunctions.Shift(NoiseHolder offsetNoise)`.
#[derive(Debug, Clone)]
pub struct Shift {
    offset_noise: NoiseHolder,
}

impl Shift {
    /// `new DensityFunctions.Shift(offsetNoise)`.
    pub fn new(offset_noise: NoiseHolder) -> Self {
        Shift { offset_noise }
    }

    /// `offsetNoise()`.
    pub fn offset_noise(&self) -> &NoiseHolder {
        &self.offset_noise
    }
}

impl ShiftNoiseDefault for Shift {
    fn offset_noise(&self) -> &NoiseHolder {
        &self.offset_noise
    }
}

impl DensityFunction for Shift {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.compute_local(
            context.block_x() as f64,
            context.block_y() as f64,
            context.block_z() as f64,
        )
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        Arc::new(Shift::new(visitor.visit_noise(&self.offset_noise)))
    }
    fn min_value(&self) -> f64 {
        -self.max_value()
    }
    fn max_value(&self) -> f64 {
        self.offset_noise.max_value() * 4.0
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::SHIFT
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.ShiftA(NoiseHolder offsetNoise)`.
#[derive(Debug, Clone)]
pub struct ShiftA {
    offset_noise: NoiseHolder,
}

impl ShiftA {
    /// `new DensityFunctions.ShiftA(offsetNoise)`.
    pub fn new(offset_noise: NoiseHolder) -> Self {
        ShiftA { offset_noise }
    }

    /// `offsetNoise()`.
    pub fn offset_noise(&self) -> &NoiseHolder {
        &self.offset_noise
    }
}

impl ShiftNoiseDefault for ShiftA {
    fn offset_noise(&self) -> &NoiseHolder {
        &self.offset_noise
    }
}

impl DensityFunction for ShiftA {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.compute_local(context.block_x() as f64, 0.0, context.block_z() as f64)
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        Arc::new(ShiftA::new(visitor.visit_noise(&self.offset_noise)))
    }
    fn min_value(&self) -> f64 {
        -self.max_value()
    }
    fn max_value(&self) -> f64 {
        self.offset_noise.max_value() * 4.0
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::SHIFT_A
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.ShiftB(NoiseHolder offsetNoise)`.
#[derive(Debug, Clone)]
pub struct ShiftB {
    offset_noise: NoiseHolder,
}

impl ShiftB {
    /// `new DensityFunctions.ShiftB(offsetNoise)`.
    pub fn new(offset_noise: NoiseHolder) -> Self {
        ShiftB { offset_noise }
    }

    /// `offsetNoise()`.
    pub fn offset_noise(&self) -> &NoiseHolder {
        &self.offset_noise
    }
}

impl ShiftNoiseDefault for ShiftB {
    fn offset_noise(&self) -> &NoiseHolder {
        &self.offset_noise
    }
}

impl DensityFunction for ShiftB {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.compute_local(context.block_z() as f64, context.block_x() as f64, 0.0)
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        Arc::new(ShiftB::new(visitor.visit_noise(&self.offset_noise)))
    }
    fn min_value(&self) -> f64 {
        -self.max_value()
    }
    fn max_value(&self) -> f64 {
        self.offset_noise.max_value() * 4.0
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::SHIFT_B
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.ShiftedNoise(shiftX, shiftY, shiftZ, xzScale, yScale,
/// noise)`.
#[derive(Debug, Clone)]
pub struct ShiftedNoise {
    shift_x: Arc<dyn DensityFunction>,
    shift_y: Arc<dyn DensityFunction>,
    shift_z: Arc<dyn DensityFunction>,
    xz_scale: f64,
    y_scale: f64,
    noise: NoiseHolder,
}

impl ShiftedNoise {
    /// `new DensityFunctions.ShiftedNoise(...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shift_x: Arc<dyn DensityFunction>,
        shift_y: Arc<dyn DensityFunction>,
        shift_z: Arc<dyn DensityFunction>,
        xz_scale: f64,
        y_scale: f64,
        noise: NoiseHolder,
    ) -> Self {
        ShiftedNoise {
            shift_x,
            shift_y,
            shift_z,
            xz_scale,
            y_scale,
            noise,
        }
    }

    /// `shiftX()`.
    pub fn shift_x(&self) -> &Arc<dyn DensityFunction> {
        &self.shift_x
    }
    /// `shiftY()`.
    pub fn shift_y(&self) -> &Arc<dyn DensityFunction> {
        &self.shift_y
    }
    /// `shiftZ()`.
    pub fn shift_z(&self) -> &Arc<dyn DensityFunction> {
        &self.shift_z
    }
    /// `xzScale()`.
    pub fn xz_scale(&self) -> f64 {
        self.xz_scale
    }
    /// `yScale()`.
    pub fn y_scale(&self) -> f64 {
        self.y_scale
    }
    /// `noise()`.
    pub fn noise(&self) -> &NoiseHolder {
        &self.noise
    }
}

impl DensityFunction for ShiftedNoise {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        let x = context.block_x() as f64 * self.xz_scale + self.shift_x.compute(context);
        let y = context.block_y() as f64 * self.y_scale + self.shift_y.compute(context);
        let z = context.block_z() as f64 * self.xz_scale + self.shift_z.compute(context);
        self.noise.get_value(x, y, z)
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        Arc::new(ShiftedNoise::new(
            visitor.apply(&*self.shift_x),
            visitor.apply(&*self.shift_y),
            visitor.apply(&*self.shift_z),
            self.xz_scale,
            self.y_scale,
            visitor.visit_noise(&self.noise),
        ))
    }
    fn min_value(&self) -> f64 {
        -self.max_value()
    }
    fn max_value(&self) -> f64 {
        self.noise.max_value()
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::SHIFTED_NOISE
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.TwoArgumentSimpleFunction.create(Type, DensityFunction,
/// DensityFunction)` — the bounds-aware `Ap2`/`MulOrAdd` construction.
pub fn two_argument_create(
    two_arg_type: TwoArgumentType,
    argument1: Arc<dyn DensityFunction>,
    argument2: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    let min1 = argument1.min_value();
    let min2 = argument2.min_value();
    let max1 = argument1.max_value();
    let max2 = argument2.max_value();

    if two_arg_type == TwoArgumentType::Min || two_arg_type == TwoArgumentType::Max {
        let first_always_bigger_than_second = min1 >= max2;
        let second_always_bigger_than_first = min2 >= max1;
        if first_always_bigger_than_second || second_always_bigger_than_first {
            // Java `LOGGER.warn(...)` — Paper logs through slf4j; the Rust port
            // surfaces the same condition via a `tracing`-style note only in
            // debug builds (no logging dependency in this crate).
            #[cfg(debug_assertions)]
            eprintln!(
                "Creating a {:?} function between two non-overlapping inputs",
                two_arg_type
            );
        }
    }

    let min_value = match two_arg_type {
        TwoArgumentType::Add => min1 + min2,
        TwoArgumentType::Mul => {
            if min1 > 0.0 && min2 > 0.0 {
                min1 * min2
            } else if max1 < 0.0 && max2 < 0.0 {
                max1 * max2
            } else {
                (min1 * max2).min(max1 * min2)
            }
        }
        TwoArgumentType::Min => min1.min(min2),
        TwoArgumentType::Max => max1.max(max2),
    };

    let max_value = match two_arg_type {
        TwoArgumentType::Add => max1 + max2,
        TwoArgumentType::Mul => {
            if min1 > 0.0 && min2 > 0.0 {
                max1 * max2
            } else if max1 < 0.0 && max2 < 0.0 {
                min1 * min2
            } else {
                (min1 * min2).max(max1 * max2)
            }
        }
        TwoArgumentType::Min => max1.min(max2),
        TwoArgumentType::Max => max1.max(max2),
    };

    if two_arg_type == TwoArgumentType::Mul || two_arg_type == TwoArgumentType::Add {
        if let Some(constant) = argument1.as_any().downcast_ref::<Constant>() {
            let specific = if two_arg_type == TwoArgumentType::Add {
                MulOrAddType::Add
            } else {
                MulOrAddType::Mul
            };
            return Arc::new(MulOrAdd::new(
                specific,
                argument2,
                min_value,
                max_value,
                constant.value(),
            ));
        }
        if let Some(constant) = argument2.as_any().downcast_ref::<Constant>() {
            let specific = if two_arg_type == TwoArgumentType::Add {
                MulOrAddType::Add
            } else {
                MulOrAddType::Mul
            };
            return Arc::new(MulOrAdd::new(
                specific,
                argument1,
                min_value,
                max_value,
                constant.value(),
            ));
        }
    }

    Arc::new(Ap2::new(
        two_arg_type,
        argument1,
        argument2,
        min_value,
        max_value,
    ))
}

/// `DensityFunctions.MulOrAdd.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MulOrAddType {
    Mul,
    Add,
}

/// `DensityFunctions.MulOrAdd(specificType, input, minValue, maxValue,
/// argument)` — the constant-folded `add`/`mul`.
#[derive(Debug, Clone)]
pub struct MulOrAdd {
    specific_type: MulOrAddType,
    input: Arc<dyn DensityFunction>,
    min_value: f64,
    max_value: f64,
    argument: f64,
}

impl MulOrAdd {
    /// `new MulOrAdd(specificType, input, min, max, argument)`.
    pub fn new(
        specific_type: MulOrAddType,
        input: Arc<dyn DensityFunction>,
        min_value: f64,
        max_value: f64,
        argument: f64,
    ) -> Self {
        MulOrAdd {
            specific_type,
            input,
            min_value,
            max_value,
            argument,
        }
    }

    /// `type()` — the `TwoArgumentSimpleFunction.Type`.
    pub fn two_argument_type(&self) -> TwoArgumentType {
        match self.specific_type {
            MulOrAddType::Mul => TwoArgumentType::Mul,
            MulOrAddType::Add => TwoArgumentType::Add,
        }
    }

    /// `argument1()` — `constant(argument)`.
    pub fn argument1(&self) -> Arc<dyn DensityFunction> {
        constant(self.argument)
    }

    /// `argument2()` — `input`.
    pub fn argument2(&self) -> Arc<dyn DensityFunction> {
        self.input.clone()
    }

    /// `input()`.
    pub fn input(&self) -> &Arc<dyn DensityFunction> {
        &self.input
    }

    /// `argument`.
    pub fn argument(&self) -> f64 {
        self.argument
    }

    /// `transform(double)` — the pointwise transform.
    pub fn transform(&self, input: f64) -> f64 {
        match self.specific_type {
            MulOrAddType::Mul => input * self.argument,
            MulOrAddType::Add => input + self.argument,
        }
    }
}

impl DensityFunction for MulOrAdd {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.transform(self.input.compute(context))
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        self.input.fill_array(output, context_provider);
        for v in output.iter_mut() {
            *v = self.transform(*v);
        }
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        let function = visitor.apply(&*self.input);
        let min = function.min_value();
        let max = function.max_value();
        let (min_value, max_value) = match self.specific_type {
            MulOrAddType::Add => (min + self.argument, max + self.argument),
            MulOrAddType::Mul => {
                if self.argument >= 0.0 {
                    (min * self.argument, max * self.argument)
                } else {
                    (max * self.argument, min * self.argument)
                }
            }
        };
        Arc::new(MulOrAdd::new(
            self.specific_type,
            function,
            min_value,
            max_value,
            self.argument,
        ))
    }
    fn min_value(&self) -> f64 {
        self.min_value
    }
    fn max_value(&self) -> f64 {
        self.max_value
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        match self.specific_type {
            MulOrAddType::Mul => DensityFunctionTypes::MUL,
            MulOrAddType::Add => DensityFunctionTypes::ADD,
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.Ap2(Type type, argument1, argument2, minValue, maxValue)` —
/// the general two-argument function.
#[derive(Debug, Clone)]
pub struct Ap2 {
    two_arg_type: TwoArgumentType,
    argument1: Arc<dyn DensityFunction>,
    argument2: Arc<dyn DensityFunction>,
    min_value: f64,
    max_value: f64,
}

impl Ap2 {
    /// `new Ap2(...)`.
    pub fn new(
        two_arg_type: TwoArgumentType,
        argument1: Arc<dyn DensityFunction>,
        argument2: Arc<dyn DensityFunction>,
        min_value: f64,
        max_value: f64,
    ) -> Self {
        Ap2 {
            two_arg_type,
            argument1,
            argument2,
            min_value,
            max_value,
        }
    }

    /// `type()`.
    pub fn two_argument_type(&self) -> TwoArgumentType {
        self.two_arg_type
    }

    /// `argument1()`.
    pub fn argument1(&self) -> &Arc<dyn DensityFunction> {
        &self.argument1
    }

    /// `argument2()`.
    pub fn argument2(&self) -> &Arc<dyn DensityFunction> {
        &self.argument2
    }

    /// `minValue()`.
    pub fn min_value(&self) -> f64 {
        self.min_value
    }

    /// `maxValue()`.
    pub fn max_value(&self) -> f64 {
        self.max_value
    }
}

impl DensityFunction for Ap2 {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        let v1 = self.argument1.compute(context);
        match self.two_arg_type {
            TwoArgumentType::Add => v1 + self.argument2.compute(context),
            TwoArgumentType::Mul => {
                if v1 == 0.0 {
                    0.0
                } else {
                    v1 * self.argument2.compute(context)
                }
            }
            TwoArgumentType::Min => {
                if v1 < self.argument2.min_value() {
                    v1
                } else {
                    v1.min(self.argument2.compute(context))
                }
            }
            TwoArgumentType::Max => {
                if v1 > self.argument2.max_value() {
                    v1
                } else {
                    v1.max(self.argument2.compute(context))
                }
            }
        }
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        self.argument1.fill_array(output, context_provider);
        match self.two_arg_type {
            TwoArgumentType::Add => {
                let mut v2 = vec![0.0; output.len()];
                self.argument2.fill_array(&mut v2, context_provider);
                for (slot, v) in output.iter_mut().zip(v2.iter()) {
                    *slot += *v;
                }
            }
            TwoArgumentType::Mul => {
                for (i, slot) in output.iter_mut().enumerate() {
                    let v = *slot;
                    *slot = if v == 0.0 {
                        0.0
                    } else {
                        v * self.argument2.compute(&context_provider.for_index(i))
                    };
                }
            }
            TwoArgumentType::Min => {
                let min = self.argument2.min_value();
                for (i, slot) in output.iter_mut().enumerate() {
                    let v = *slot;
                    *slot = if v < min {
                        v
                    } else {
                        v.min(self.argument2.compute(&context_provider.for_index(i)))
                    };
                }
            }
            TwoArgumentType::Max => {
                let max = self.argument2.max_value();
                for (i, slot) in output.iter_mut().enumerate() {
                    let v = *slot;
                    *slot = if v > max {
                        v
                    } else {
                        v.max(self.argument2.compute(&context_provider.for_index(i)))
                    };
                }
            }
        }
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        two_argument_create(
            self.two_arg_type,
            visitor.apply(&*self.argument1),
            visitor.apply(&*self.argument2),
        )
    }
    fn min_value(&self) -> f64 {
        self.min_value
    }
    fn max_value(&self) -> f64 {
        self.max_value
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        match self.two_arg_type {
            TwoArgumentType::Add => DensityFunctionTypes::ADD,
            TwoArgumentType::Mul => DensityFunctionTypes::MUL,
            TwoArgumentType::Min => DensityFunctionTypes::MIN,
            TwoArgumentType::Max => DensityFunctionTypes::MAX,
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.Spline(CubicSpline<Coordinate> spline)`.
#[derive(Clone)]
pub struct Spline {
    spline: CubicSpline<SplineCoordinate>,
    sampler: rivet_util::cubic_spline::Sampler<SplineCoordinate, SplinePoint>,
}

impl Spline {
    /// `new Spline(CubicSpline<Coordinate>)` — `CubicSpline.asSampler(spline)`.
    pub fn new(spline: CubicSpline<SplineCoordinate>) -> Self {
        let sampler = spline.clone().sampler::<SplinePoint>();
        Spline { spline, sampler }
    }

    /// `spline()`.
    pub fn spline(&self) -> &CubicSpline<SplineCoordinate> {
        &self.spline
    }
}

impl Debug for Spline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Spline[{:?}]", self.spline)
    }
}

impl DensityFunction for Spline {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.sampler.apply(SplinePoint::new(context)) as f64
    }
    fn min_value(&self) -> f64 {
        self.spline.min_value() as f64
    }
    fn max_value(&self) -> f64 {
        self.spline.max_value() as f64
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        let mapped = self
            .spline
            .map_coordinates(Arc::new(move |c: SplineCoordinate| c.map_children(visitor)));
        Arc::new(Spline::new(mapped))
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::SPLINE
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.Spline.Coordinate(DensityFunction function)` — the
/// `BoundedFloatFunction<Point>` coordinate.
#[derive(Debug, Clone)]
pub struct SplineCoordinate {
    function: Arc<dyn DensityFunction>,
}

impl SplineCoordinate {
    /// `new Coordinate(function)`.
    pub fn new(function: Arc<dyn DensityFunction>) -> Self {
        SplineCoordinate { function }
    }

    /// `function()`.
    pub fn function(&self) -> &Arc<dyn DensityFunction> {
        &self.function
    }

    /// `Coordinate.CODEC` — `DensityFunction.CODEC.xmap(Coordinate::new,
    /// Coordinate::function)`.
    pub fn codec<Ops>(
        top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
    ) -> Arc<dyn Codec<SplineCoordinate, Ops>>
    where
        Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
    {
        codec::xmap(
            top,
            Arc::new(|f: &Arc<dyn DensityFunction>| SplineCoordinate::new(f.clone())),
            Arc::new(|c: &SplineCoordinate| c.function.clone()),
        )
    }

    /// `Coordinate.mapChildren(Visitor)`.
    pub fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> SplineCoordinate {
        SplineCoordinate::new(visitor.apply(&*self.function))
    }
}

impl rivet_util::bounded_float_function::BoundedFloat for SplineCoordinate {
    fn min_value(&self) -> f32 {
        self.function.min_value() as f32
    }
    fn max_value(&self) -> f32 {
        self.function.max_value() as f32
    }
}

impl rivet_util::bounded_float_function::BoundedFloatFunction<SplinePoint> for SplineCoordinate {
    fn apply(&self, point: SplinePoint) -> f32 {
        self.function.compute(point.context()) as f32
    }
}

/// `DensityFunctions.Spline.Point(FunctionContext context)`.
#[derive(Debug)]
pub struct SplinePoint {
    context: Arc<dyn FunctionContext>,
}

impl SplinePoint {
    /// `new Point(context)`.
    pub fn new(context: &dyn FunctionContext) -> Self {
        SplinePoint {
            context: box_function_context(context),
        }
    }

    /// `context()`.
    pub fn context(&self) -> &dyn FunctionContext {
        &*self.context
    }
}

/// Box a `FunctionContext` — `SplinePoint` must be `Clone` (the spline sampler
/// clones the coordinate through `CubicSpline::sample`), and a `&dyn
/// FunctionContext` reference cannot be cloned. The port snapshots the context
/// into an owned `Arc<dyn FunctionContext>` (the `FunctionContext` is
/// `'static` and object-safe). Contexts in this slice are `SinglePointContext`
/// (Copy) or derived from chunk coordinates — the boxed form is a snapshot.
fn box_function_context(context: &dyn FunctionContext) -> Arc<dyn FunctionContext> {
    #[derive(Debug)]
    struct Boxed {
        block_x: i32,
        block_y: i32,
        block_z: i32,
    }
    impl FunctionContext for Boxed {
        fn block_x(&self) -> i32 {
            self.block_x
        }
        fn block_y(&self) -> i32 {
            self.block_y
        }
        fn block_z(&self) -> i32 {
            self.block_z
        }
    }
    let snapshot = Boxed {
        block_x: context.block_x(),
        block_y: context.block_y(),
        block_z: context.block_z(),
    };
    Arc::new(snapshot)
}

impl Clone for SplinePoint {
    fn clone(&self) -> Self {
        SplinePoint {
            context: self.context.clone(),
        }
    }
}

/// `DensityFunctions.YClampedGradient(fromY, toY, fromValue, toValue)`.
#[derive(Debug, Clone)]
pub struct YClampedGradient {
    from_y: i32,
    to_y: i32,
    from_value: f64,
    to_value: f64,
}

impl YClampedGradient {
    /// `new YClampedGradient(fromY, toY, fromValue, toValue)`.
    pub fn new(from_y: i32, to_y: i32, from_value: f64, to_value: f64) -> Self {
        YClampedGradient {
            from_y,
            to_y,
            from_value,
            to_value,
        }
    }

    /// `fromY()`.
    pub fn from_y(&self) -> i32 {
        self.from_y
    }
    /// `toY()`.
    pub fn to_y(&self) -> i32 {
        self.to_y
    }
    /// `fromValue()`.
    pub fn from_value(&self) -> f64 {
        self.from_value
    }
    /// `toValue()`.
    pub fn to_value(&self) -> f64 {
        self.to_value
    }
}

impl DensityFunction for YClampedGradient {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        mth::clamped_map(
            context.block_y() as f64,
            self.from_y as f64,
            self.to_y as f64,
            self.from_value,
            self.to_value,
        )
    }
    fn min_value(&self) -> f64 {
        self.from_value.min(self.to_value)
    }
    fn max_value(&self) -> f64 {
        self.from_value.max(self.to_value)
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::Y_CLAMPED_GRADIENT
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.FindTopSurface(density, upperBound, lowerBound,
/// cellHeight)`.
#[derive(Debug, Clone)]
pub struct FindTopSurface {
    density: Arc<dyn DensityFunction>,
    upper_bound: Arc<dyn DensityFunction>,
    lower_bound: i32,
    cell_height: i32,
}

impl FindTopSurface {
    /// `new FindTopSurface(density, upperBound, lowerBound, cellHeight)`.
    pub fn new(
        density: Arc<dyn DensityFunction>,
        upper_bound: Arc<dyn DensityFunction>,
        lower_bound: i32,
        cell_height: i32,
    ) -> Self {
        FindTopSurface {
            density,
            upper_bound,
            lower_bound,
            cell_height,
        }
    }

    /// `density()`.
    pub fn density(&self) -> &Arc<dyn DensityFunction> {
        &self.density
    }
    /// `upperBound()`.
    pub fn upper_bound(&self) -> &Arc<dyn DensityFunction> {
        &self.upper_bound
    }
    /// `lowerBound()`.
    pub fn lower_bound(&self) -> i32 {
        self.lower_bound
    }
    /// `cellHeight()`.
    pub fn cell_height(&self) -> i32 {
        self.cell_height
    }
}

impl DensityFunction for FindTopSurface {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        let top_y = mth::floor_d(self.upper_bound.compute(context) / self.cell_height as f64)
            * self.cell_height;
        if top_y <= self.lower_bound {
            return self.lower_bound as f64;
        }
        let mut block_y = top_y;
        while block_y >= self.lower_bound {
            let point = crate::levelgen::noise::density_function::SinglePointContext::new(
                context.block_x(),
                block_y,
                context.block_z(),
            );
            if self.density.compute(&point) > 0.0 {
                return block_y as f64;
            }
            block_y -= self.cell_height;
        }
        self.lower_bound as f64
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        Arc::new(FindTopSurface::new(
            visitor.apply(&*self.density),
            visitor.apply(&*self.upper_bound),
            self.lower_bound,
            self.cell_height,
        ))
    }
    fn min_value(&self) -> f64 {
        self.lower_bound as f64
    }
    fn max_value(&self) -> f64 {
        (self.lower_bound as f64).max(self.upper_bound.max_value())
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::FIND_TOP_SURFACE
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// Paper's `EndIslandDensityFunction.NoiseCache` — the 8192-entry chunk-key →
/// island-size cache. The `ThreadLocal<WeakHashMap>` is per-function here (the
/// slice has no thread-local facility); each `EndIslandDensityFunction` owns
/// one cache, matching Paper's cache-per-noise behavior.
struct NoiseCache {
    keys: Box<[i64; 8192]>,
    values: Box<[f32; 8192]>,
}

impl NoiseCache {
    fn new() -> Self {
        NoiseCache {
            keys: Box::new([i64::MIN; 8192]),
            values: Box::new([0.0; 8192]),
        }
    }
}

/// `DensityFunctions.EndIslandDensityFunction` — the End island height field.
pub struct EndIslandDensityFunction {
    island_noise: SimplexNoise,
    cache: Mutex<NoiseCache>,
}

impl Debug for EndIslandDensityFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EndIslandDensityFunction")
    }
}

impl Clone for EndIslandDensityFunction {
    fn clone(&self) -> Self {
        // `SimplexNoise` is not `Clone`; reconstruct from the seed is not
        // possible (the seed is not retained). The `EndIslandDensityFunction`
        // codec decodes a fresh instance (`MapCodec.unit(new
        // EndIslandDensityFunction(0L))`), so the clone path (used by
        // `map_children`'s identity default and `erase_map_codec`) shares the
        // same noise by rebuilding the noise — a `LegacyRandomSource` with the
        // same construction path. Rather than duplicate state, the clone
        // delegates to `EndIslandDensityFunction::new(0)` — the value-layer
        // codec's canonical instance, whose `compute` only depends on the
        // seeded noise, which the unit constructor reproduces deterministically.
        Self::new(0)
    }
}

impl EndIslandDensityFunction {
    /// The `ISLAND_THRESHOLD` constant — `-0.9F`. Java compares
    /// `islandNoise.getValue(...) < ISLAND_THRESHOLD`, which widens the float
    /// to double before comparing; the port pins the exact widened value
    /// (`(double)(-0.9F)` — `-0.9f32 as f64`), not the f64 literal `-0.9`
    /// (the two differ by ~2.4e-8).
    const ISLAND_THRESHOLD_D: f64 = -0.9f32 as f64;

    /// `new EndIslandDensityFunction(long seed)` — `LegacyRandomSource(seed)`,
    /// `consumeCount(17292)`, `new SimplexNoise(islandRandom)`.
    pub fn new(seed: i64) -> Self {
        let mut island_random = LegacyRandomSource::new(seed);
        island_random.consume_count(17292);
        let island_noise = SimplexNoise::new(&mut island_random);
        EndIslandDensityFunction {
            island_noise,
            cache: Mutex::new(NoiseCache::new()),
        }
    }

    /// `getHeightValue(SimplexNoise, int sectionX, int sectionZ)` — the
    /// height-field evaluation with Paper's NoiseCache.
    fn get_height_value(&self, section_x: i32, section_z: i32) -> f32 {
        let chunk_x = section_x / 2;
        let chunk_z = section_z / 2;
        let sub_section_x = section_x % 2;
        let sub_section_z = section_z % 2;
        // Paper: `configFixMC159283() ? Mth.sqrt((long)sectionX *
        // (long)sectionX + ...) : ...` — the config is pinned `true` (the
        // long-sqrt path). `Mth.sqrt(float)` returns `x.sqrt()`.
        let doffs_raw = 100.0
            - mth::sqrt(
                ((section_x as i64 * section_x as i64) + (section_z as i64 * section_z as i64))
                    as f32,
            ) * 8.0;
        let mut doffs = mth::clamp_f32(doffs_raw, -100.0, 80.0);

        let mut cache = self.cache.lock().unwrap();
        for xo in -12..=12i32 {
            for zo in -12..=12i32 {
                let total_chunk_x = chunk_x as i64 + xo as i64;
                let total_chunk_z = chunk_z as i64 + zo as i64;
                let chunk_key = ChunkPos::pack_coords(total_chunk_x as i32, total_chunk_z as i32);
                let cache_index = (mix_i64(chunk_key) & 8191) as usize;
                // Java `float islandSize = Float.MIN_VALUE` — the smallest
                // positive float (the `islandSize != Float.MIN_VALUE` sentinel),
                // not `f32::MIN` (the most negative float).
                let mut island_size = f32::from_bits(1);
                if cache.keys[cache_index] == chunk_key {
                    island_size = cache.values[cache_index];
                } else {
                    let dist_sq = total_chunk_x * total_chunk_x + total_chunk_z * total_chunk_z;
                    if dist_sq > 4096
                        && self
                            .island_noise
                            .get_value_2d(total_chunk_x as f64, total_chunk_z as f64)
                            < Self::ISLAND_THRESHOLD_D
                    {
                        island_size = (mth::abs(total_chunk_x as f32) * 3439.0
                            + mth::abs(total_chunk_z as f32) * 147.0)
                            % 13.0
                            + 9.0;
                    }
                    cache.keys[cache_index] = chunk_key;
                    cache.values[cache_index] = island_size;
                }
                if island_size != f32::from_bits(1) {
                    let xd = sub_section_x as f32 - (xo * 2) as f32;
                    let zd = sub_section_z as f32 - (zo * 2) as f32;
                    let new_doffs = 100.0 - mth::sqrt(xd * xd + zd * zd) * island_size;
                    let new_doffs = mth::clamp_f32(new_doffs, -100.0, 80.0);
                    doffs = doffs.max(new_doffs);
                }
            }
        }

        doffs
    }
}

/// `it.unimi.dsi.fastutil.HashCommon.mix(long)` — the `staffordMix13`
/// finalizer used by the `NoiseCache` index.
fn mix_i64(mut z: i64) -> i64 {
    z = z.wrapping_add(0x9e3779b97f4a7c15u64 as i64);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9u64 as i64);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111ebu64 as i64);
    z ^ (z >> 31)
}

impl DensityFunction for EndIslandDensityFunction {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        // Java `compute`:
        //   return ((double)this.getHeightValue(this.islandNoise,
        //       context.blockX() / 8, context.blockZ() / 8) - 8.0) / 128.0;
        // where `context.blockX() / 8` is integer division (truncating toward
        // zero), which the `section_x = context.block_x() / 8` port matches
        // exactly (NOT `blockX / 8.0` floored — those differ on negative coords).
        // The cache is `Mutex` (the value layer's per-function cache; the
        // value-layer `EndIslandDensityFunction` runs single-threaded in the
        // chunk-gen path) — Paper's `ThreadLocal` cache is serialized here
        // behind a `Mutex` (correct: the cached island size per chunk is
        // deterministic).
        let section_x = context.block_x() / 8;
        let section_z = context.block_z() / 8;
        let height = self.get_height_value(section_x, section_z);
        ((height as f64) - 8.0) / 128.0
    }
    fn min_value(&self) -> f64 {
        -0.84375
    }
    fn max_value(&self) -> f64 {
        0.5625
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::END_ISLANDS
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

/// `DensityFunctions.HolderHolder(Holder<DensityFunction> function)` — the
/// registry-reference seam: `DensityFunction.CODEC`'s `Holder::Reference`
/// decode arm. The `is_bound()` constant-true stub is inherited; value
/// resolution defers (the holder's `value` needs a `HolderLookup`, which the
/// value layer does not carry — RivetTodo #177).
#[derive(Debug, Clone)]
pub struct HolderHolder {
    function: Holder<Arc<dyn DensityFunction>>,
}

impl HolderHolder {
    /// `new HolderHolder(Holder<DensityFunction>)`.
    pub fn new(function: Holder<Arc<dyn DensityFunction>>) -> Self {
        HolderHolder { function }
    }

    /// `function()`.
    pub fn function(&self) -> &Holder<Arc<dyn DensityFunction>> {
        &self.function
    }
}

impl DensityFunction for HolderHolder {
    fn compute(&self, _context: &dyn FunctionContext) -> f64 {
        // Java `this.function.value().compute(context)` — the value layer
        // cannot resolve a `Holder::Reference` (no lookup). RivetTodo(#177).
        match &self.function {
            Holder::Direct(f) => f.compute(_context),
            Holder::Reference { .. } => {
                panic!("HolderHolder.value() requires a HolderLookup (RivetTodo #177)")
            }
        }
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        match &self.function {
            Holder::Direct(f) => f.fill_array(output, context_provider),
            Holder::Reference { .. } => {
                panic!("HolderHolder.value() requires a HolderLookup (RivetTodo #177)")
            }
        }
    }
    fn map_children(
        &self,
        visitor: &dyn crate::levelgen::noise::density_function::Visitor,
    ) -> Arc<dyn DensityFunction> {
        // Java `new HolderHolder(Holder.direct(visitor.apply(this.function.value())))`.
        match &self.function {
            Holder::Direct(f) => Arc::new(HolderHolder::new(Holder::direct(visitor.apply(&**f)))),
            Holder::Reference { .. } => {
                panic!("HolderHolder.value() requires a HolderLookup (RivetTodo #177)")
            }
        }
    }
    fn min_value(&self) -> f64 {
        // Java `isBound() ? value().minValue() : NEGATIVE_INFINITY` — the
        // constant-true `is_bound` stub means the reference path resolves
        // (deferred); a bound holder reports the child's bounds.
        match &self.function {
            Holder::Direct(f) => f.min_value(),
            Holder::Reference { .. } => f64::NEG_INFINITY,
        }
    }
    fn max_value(&self) -> f64 {
        match &self.function {
            Holder::Direct(f) => f.max_value(),
            Holder::Reference { .. } => f64::INFINITY,
        }
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        // Java `codec()` throws UnsupportedOperationException on HolderHolder.
        panic!("Calling .type_id() on HolderHolder")
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}

// ---------------------------------------------------------------------------
// Public static factories (Java `DensityFunctions` methods)
// ---------------------------------------------------------------------------

/// `DensityFunctions.constant(double)`.
pub fn constant(value: f64) -> Arc<dyn DensityFunction> {
    Arc::new(Constant::new(value))
}

/// `DensityFunctions.zero()`.
pub fn zero() -> Arc<dyn DensityFunction> {
    Constant::zero()
}

/// `DensityFunctions.blendAlpha()`.
pub fn blend_alpha() -> Arc<dyn DensityFunction> {
    Arc::new(BlendAlpha)
}

/// `DensityFunctions.blendOffset()`.
pub fn blend_offset() -> Arc<dyn DensityFunction> {
    Arc::new(BlendOffset)
}

/// `DensityFunctions.beardifierMarker()` — the value shell (compute 0.0 /
/// fill 0.0 / bounds 0.0); the structure runtime defers (RivetTodo #177).
pub fn beardifier_marker() -> Arc<dyn DensityFunction> {
    Arc::new(BeardifierMarker::instance())
}

/// `DensityFunctions.interpolated(function)`.
pub fn interpolated(function: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Marker::new(MarkerType::Interpolated, function))
}

/// `DensityFunctions.flatCache(function)`.
pub fn flat_cache(function: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Marker::new(MarkerType::FlatCache, function))
}

/// `DensityFunctions.cache2d(function)`.
pub fn cache2d(function: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Marker::new(MarkerType::Cache2D, function))
}

/// `DensityFunctions.cacheOnce(function)`.
pub fn cache_once(function: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Marker::new(MarkerType::CacheOnce, function))
}

/// `DensityFunctions.cacheAllInCell(function)`.
pub fn cache_all_in_cell(function: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Marker::new(MarkerType::CacheAllInCell, function))
}

/// `DensityFunctions.blendDensity(function)`.
pub fn blend_density(function: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Marker::new(MarkerType::BlendDensity, function))
}

/// `DensityFunctions.map(function, Mapped.Type)`.
pub fn mapped(function: &dyn DensityFunction, mapped_type: MappedType) -> Arc<dyn DensityFunction> {
    Arc::new(Mapped::create(mapped_type, function.clone_arc()))
}

/// `DensityFunctions.add(f1, f2)`.
pub fn add(f1: Arc<dyn DensityFunction>, f2: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    two_argument_create(TwoArgumentType::Add, f1, f2)
}

/// `DensityFunctions.mul(f1, f2)`.
pub fn mul(f1: Arc<dyn DensityFunction>, f2: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    two_argument_create(TwoArgumentType::Mul, f1, f2)
}

/// `DensityFunctions.min(f1, f2)`.
pub fn min(f1: Arc<dyn DensityFunction>, f2: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    two_argument_create(TwoArgumentType::Min, f1, f2)
}

/// `DensityFunctions.max(f1, f2)`.
pub fn max(f1: Arc<dyn DensityFunction>, f2: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    two_argument_create(TwoArgumentType::Max, f1, f2)
}

/// `DensityFunctions.mapFromUnitTo(function, min, max)`.
fn map_from_unit_to(
    function: Arc<dyn DensityFunction>,
    min: f64,
    max: f64,
) -> Arc<dyn DensityFunction> {
    let middle = (min + max) * 0.5;
    let factor = (max - min) * 0.5;
    add(constant(middle), mul(constant(factor), function))
}

/// `DensityFunctions.mappedNoise(noiseData, xzScale, yScale, minTarget,
/// maxTarget)`.
#[allow(clippy::too_many_arguments)]
pub fn mapped_noise(
    noise_data: Holder<NoiseParameters>,
    xz_scale: f64,
    y_scale: f64,
    min_target: f64,
    max_target: f64,
) -> Arc<dyn DensityFunction> {
    map_from_unit_to(
        Arc::new(Noise::new(NoiseHolder::new(noise_data), xz_scale, y_scale)),
        min_target,
        max_target,
    )
}

/// `DensityFunctions.shiftedNoise2d(shiftX, shiftZ, xzScale, noiseData)`.
pub fn shifted_noise2d(
    shift_x: Arc<dyn DensityFunction>,
    shift_z: Arc<dyn DensityFunction>,
    xz_scale: f64,
    noise_data: Holder<NoiseParameters>,
) -> Arc<dyn DensityFunction> {
    Arc::new(ShiftedNoise::new(
        shift_x,
        zero(),
        shift_z,
        xz_scale,
        0.0,
        NoiseHolder::new(noise_data),
    ))
}

/// `DensityFunctions.noise(noiseData, xzScale, yScale)`.
pub fn noise(
    noise_data: Holder<NoiseParameters>,
    xz_scale: f64,
    y_scale: f64,
) -> Arc<dyn DensityFunction> {
    Arc::new(Noise::new(NoiseHolder::new(noise_data), xz_scale, y_scale))
}

/// `DensityFunctions.rangeChoice(input, minInclusive, maxExclusive,
/// whenInRange, whenOutOfRange)`.
#[allow(clippy::too_many_arguments)]
pub fn range_choice(
    input: Arc<dyn DensityFunction>,
    min_inclusive: f64,
    max_exclusive: f64,
    when_in_range: Arc<dyn DensityFunction>,
    when_out_of_range: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    Arc::new(RangeChoice::new(
        input,
        min_inclusive,
        max_exclusive,
        when_in_range,
        when_out_of_range,
    ))
}

/// `DensityFunctions.intervalSelect(input, thresholds, functions)`.
pub fn interval_select(
    input: Arc<dyn DensityFunction>,
    thresholds: Vec<f64>,
    functions: Vec<Arc<dyn DensityFunction>>,
) -> Arc<dyn DensityFunction> {
    Arc::new(IntervalSelect::new(input, thresholds, functions))
}

/// `DensityFunctions.shiftA(noiseData)`.
pub fn shift_a(noise_data: Holder<NoiseParameters>) -> Arc<dyn DensityFunction> {
    Arc::new(ShiftA::new(NoiseHolder::new(noise_data)))
}

/// `DensityFunctions.shiftB(noiseData)`.
pub fn shift_b(noise_data: Holder<NoiseParameters>) -> Arc<dyn DensityFunction> {
    Arc::new(ShiftB::new(NoiseHolder::new(noise_data)))
}

/// `DensityFunctions.shift(noiseData)`.
pub fn shift(noise_data: Holder<NoiseParameters>) -> Arc<dyn DensityFunction> {
    Arc::new(Shift::new(NoiseHolder::new(noise_data)))
}

/// `DensityFunctions.endIslands(seed)`.
pub fn end_islands(seed: i64) -> Arc<dyn DensityFunction> {
    Arc::new(EndIslandDensityFunction::new(seed))
}

/// `DensityFunctions.spline(CubicSpline<Coordinate>)`.
pub fn spline(spline: CubicSpline<SplineCoordinate>) -> Arc<dyn DensityFunction> {
    Arc::new(Spline::new(spline))
}

/// `DensityFunctions.yClampedGradient(fromY, toY, fromValue, toValue)`.
pub fn y_clamped_gradient(
    from_y: i32,
    to_y: i32,
    from_value: f64,
    to_value: f64,
) -> Arc<dyn DensityFunction> {
    Arc::new(YClampedGradient::new(from_y, to_y, from_value, to_value))
}

/// `DensityFunctions.findTopSurface(density, upperBound, lowerBound,
/// stepSize)`.
pub fn find_top_surface(
    density: Arc<dyn DensityFunction>,
    upper_bound: Arc<dyn DensityFunction>,
    lower_bound: i32,
    step_size: i32,
) -> Arc<dyn DensityFunction> {
    Arc::new(FindTopSurface::new(
        density,
        upper_bound,
        lower_bound,
        step_size,
    ))
}

/// `DensityFunctions.lerp(alpha, first, second)`.
pub fn lerp(
    alpha: Arc<dyn DensityFunction>,
    first: Arc<dyn DensityFunction>,
    second: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    if let Some(constant) = first.as_any().downcast_ref::<Constant>() {
        lerp_double(alpha, constant.value(), second)
    } else {
        let alpha_cached = cache_once(alpha);
        let one_minus_alpha = add(mul(alpha_cached.clone(), constant(-1.0)), constant(1.0));
        add(mul(first, one_minus_alpha), mul(second, alpha_cached))
    }
}

/// `DensityFunctions.lerp(factor, double first, second)`.
pub fn lerp_double(
    factor: Arc<dyn DensityFunction>,
    first: f64,
    second: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    add(
        mul(factor.clone(), add(second, constant(-first))),
        constant(first),
    )
}

// ---------------------------------------------------------------------------
// Per-type MapCodec builders
// ---------------------------------------------------------------------------
//
// Each builder returns the *erased* `MapCodec<Arc<dyn DensityFunction>>`
// (the `block_predicate.rs` pattern): the concrete `MapCodec<C>` is lifted via
// `erase_map_codec`. The two-argument types (ADD/MUL/MIN/MAX) are the
// exception — `TwoArgumentSimpleFunction.create` may fold to `MulOrAdd` or
// `Ap2`, so their codec operates on the erased carrier directly.

/// `Constant.CODEC` — `singleArgumentCodec(NOISE_VALUE_CODEC, Constant::new,
/// Constant::value)`.
fn constant_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    erase_map_codec::<Constant, Ops>(
        map_codec::xmap(
            codec::field_of(noise_value_codec::<Ops>(), "value".to_string()),
            Arc::new(|v: &f64| Constant::new(*v)),
            Arc::new(|c: &Constant| c.value),
        ),
        "minecraft:constant".to_string(),
    )
}

/// `BlendAlpha.CODEC` — `MapCodec.unit(INSTANCE)`.
fn blend_alpha_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    erase_map_codec::<BlendAlpha, Ops>(
        map_codec::unit_with(Arc::new(|| BlendAlpha)),
        "minecraft:blend_alpha".to_string(),
    )
}

/// `BlendOffset.CODEC` — `MapCodec.unit(INSTANCE)`.
fn blend_offset_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    erase_map_codec::<BlendOffset, Ops>(
        map_codec::unit_with(Arc::new(|| BlendOffset)),
        "minecraft:blend_offset".to_string(),
    )
}

/// `BeardifierMarker.CODEC` — `MapCodec.unit(BeardifierMarker.INSTANCE)`.
fn beardifier_marker_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    erase_map_codec::<BeardifierMarker, Ops>(
        map_codec::unit_with(Arc::new(BeardifierMarker::instance)),
        "minecraft:beardifier".to_string(),
    )
}

/// `Marker.Type.codec` — `singleFunctionArgumentCodec(input -> new Marker(this,
/// input), MarkerOrMarked::wrapped)`.
fn marker_map_codec<Ops: DynamicOps + 'static>(
    marker_type: MarkerType,
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    let location = format!("minecraft:{}", marker_type.serialized_name());
    erase_map_codec::<Marker, Ops>(
        single_function_argument_map_codec(
            top,
            Arc::new(move |input: &Arc<dyn DensityFunction>| {
                Marker::new(marker_type, input.clone())
            }),
            Arc::new(|m: &Marker| m.wrapped.clone()),
        ),
        location,
    )
}

/// `Mapped.Type.codec` — `singleFunctionArgumentCodec(input ->
/// Mapped.create(this, input), Mapped::input)`.
fn mapped_map_codec<Ops: DynamicOps + 'static>(
    mapped_type: MappedType,
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    let location = format!("minecraft:{}", mapped_type.serialized_name());
    erase_map_codec::<Mapped, Ops>(
        single_function_argument_map_codec(
            top,
            Arc::new(move |input: &Arc<dyn DensityFunction>| {
                Mapped::create(mapped_type, input.clone())
            }),
            Arc::new(|m: &Mapped| m.input.clone()),
        ),
        location,
    )
}

/// `TwoArgumentSimpleFunction.Type.codec` — `doubleFunctionArgumentCodec((a1,
/// a2) -> TwoArgumentSimpleFunction.create(this, a1, a2), ::argument1,
/// ::argument2)`.
///
/// The decoded value may be either `Ap2` or the constant-folded `MulOrAdd`
/// (both carry the `argument1`/`argument2` accessors), so the codec operates on
/// the erased `Arc<dyn DensityFunction>` carrier directly rather than a single
/// concrete type.
fn two_arg_map_codec<Ops: DynamicOps + 'static>(
    two_arg_type: TwoArgumentType,
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    double_function_argument_map_codec(
        top,
        Arc::new(
            move |a1: &Arc<dyn DensityFunction>, a2: &Arc<dyn DensityFunction>| {
                two_argument_create(two_arg_type, a1.clone(), a2.clone())
            },
        ),
        Arc::new(|f: &Arc<dyn DensityFunction>| two_arg_argument1(f)),
        Arc::new(|f: &Arc<dyn DensityFunction>| two_arg_argument2(f)),
    )
}

/// `TwoArgumentSimpleFunction.argument1()` — `Ap2` stores the child directly,
/// `MulOrAdd` re-derives it as `constant(argument)`.
fn two_arg_argument1(f: &Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    if let Some(ap) = f.as_any().downcast_ref::<Ap2>() {
        ap.argument1().clone()
    } else if let Some(ma) = f.as_any().downcast_ref::<MulOrAdd>() {
        ma.argument1()
    } else {
        unreachable!("two-argument density function is Ap2 or MulOrAdd")
    }
}

/// `TwoArgumentSimpleFunction.argument2()` — the `input` child.
fn two_arg_argument2(f: &Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    if let Some(ap) = f.as_any().downcast_ref::<Ap2>() {
        ap.argument2().clone()
    } else if let Some(ma) = f.as_any().downcast_ref::<MulOrAdd>() {
        ma.argument2()
    } else {
        unreachable!("two-argument density function is Ap2 or MulOrAdd")
    }
}

/// `Clamp.CODEC` — the `input`/`min`/`max` record.
fn clamp_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    let input_field = codec::field_of(top, "input".to_string());
    let min_field = codec::field_of(noise_value_codec::<Ops>(), "min".to_string());
    let max_field = codec::field_of(noise_value_codec::<Ops>(), "max".to_string());
    erase_map_codec::<Clamp, Ops>(
        map_codec::of(
            map_encoder_fields3(
                input_field.clone(),
                min_field.clone(),
                max_field.clone(),
                Arc::new(|c: &Clamp| c.input.clone()),
                Arc::new(|c: &Clamp| c.min_value),
                Arc::new(|c: &Clamp| c.max_value),
            ),
            map_decoder_ap3(
                input_field,
                min_field,
                max_field,
                Arc::new(|i: &Arc<dyn DensityFunction>, mn: &f64, mx: &f64| {
                    Clamp::new(i.clone(), *mn, *mx)
                }),
            ),
            "Clamp".to_string(),
        ),
        "minecraft:clamp".to_string(),
    )
}

/// `Noise.CODEC` — the `noise`/`xz_scale`/`y_scale` record.
fn noise_map_codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    let noise_field = codec::field_of(noise_holder_codec::<Ops>(), "noise".to_string());
    let xz_field = codec::field_of(codec::double_codec::<Ops>(), "xz_scale".to_string());
    let y_field = codec::field_of(codec::double_codec::<Ops>(), "y_scale".to_string());
    erase_map_codec::<Noise, Ops>(
        map_codec::of(
            map_encoder_fields3(
                noise_field.clone(),
                xz_field.clone(),
                y_field.clone(),
                Arc::new(|n: &Noise| n.noise.clone()),
                Arc::new(|n: &Noise| n.xz_scale),
                Arc::new(|n: &Noise| n.y_scale),
            ),
            map_decoder_ap3(
                noise_field,
                xz_field,
                y_field,
                Arc::new(|n: &NoiseHolder, xz: &f64, y: &f64| Noise::new(n.clone(), *xz, *y)),
            ),
            "Noise".to_string(),
        ),
        "minecraft:noise".to_string(),
    )
}

/// `NoiseHolder.CODEC` — re-exported from `density_function`.
fn noise_holder_codec<Ops>() -> Arc<dyn Codec<NoiseHolder, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    NoiseHolder::codec::<Ops>()
}

/// `EndIslandDensityFunction.CODEC` — `MapCodec.unit(new EndIslandDensityFunction(0L))`.
fn end_islands_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    erase_map_codec::<EndIslandDensityFunction, Ops>(
        map_codec::unit_with(Arc::new(|| EndIslandDensityFunction::new(0))),
        "minecraft:end_islands".to_string(),
    )
}

/// `Shift.CODEC` — `singleArgumentCodec(NoiseHolder.CODEC, Shift::new,
/// Shift::offsetNoise)`.
fn shift_map_codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    erase_map_codec::<Shift, Ops>(
        single_argument_map_codec(
            noise_holder_codec::<Ops>(),
            Arc::new(|n: &NoiseHolder| Shift::new(n.clone())),
            Arc::new(|s: &Shift| s.offset_noise.clone()),
        ),
        "minecraft:shift".to_string(),
    )
}

/// `ShiftA.CODEC`.
fn shift_a_map_codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    erase_map_codec::<ShiftA, Ops>(
        single_argument_map_codec(
            noise_holder_codec::<Ops>(),
            Arc::new(|n: &NoiseHolder| ShiftA::new(n.clone())),
            Arc::new(|s: &ShiftA| s.offset_noise.clone()),
        ),
        "minecraft:shift_a".to_string(),
    )
}

/// `ShiftB.CODEC`.
fn shift_b_map_codec<Ops>() -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    erase_map_codec::<ShiftB, Ops>(
        single_argument_map_codec(
            noise_holder_codec::<Ops>(),
            Arc::new(|n: &NoiseHolder| ShiftB::new(n.clone())),
            Arc::new(|s: &ShiftB| s.offset_noise.clone()),
        ),
        "minecraft:shift_b".to_string(),
    )
}

/// `ShiftedNoise.CODEC` — the `shift_x`/`shift_y`/`shift_z`/`xz_scale`/
/// `y_scale`/`noise` record. The `record_builder` caps at Group5, so this
/// six-field record is composed manually with `map_encoder`/`map_decoder`
/// halves.
fn shifted_noise_map_codec<Ops>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    let shift_x = codec::field_of(top.clone(), "shift_x".to_string());
    let shift_y = codec::field_of(top.clone(), "shift_y".to_string());
    let shift_z = codec::field_of(top, "shift_z".to_string());
    let xz_scale = codec::field_of(codec::double_codec::<Ops>(), "xz_scale".to_string());
    let y_scale = codec::field_of(codec::double_codec::<Ops>(), "y_scale".to_string());
    let noise = codec::field_of(noise_holder_codec::<Ops>(), "noise".to_string());

    let decoder = map_decoder_ap6(
        shift_x.clone(),
        shift_y.clone(),
        shift_z.clone(),
        xz_scale.clone(),
        y_scale.clone(),
        noise.clone(),
        Arc::new(
            |sx: &Arc<dyn DensityFunction>,
             sy: &Arc<dyn DensityFunction>,
             sz: &Arc<dyn DensityFunction>,
             xzs: &f64,
             ys: &f64,
             n: &NoiseHolder| {
                ShiftedNoise::new(sx.clone(), sy.clone(), sz.clone(), *xzs, *ys, n.clone())
            },
        ),
    );
    let encoder = map_encoder_fields6(
        shift_x,
        shift_y,
        shift_z,
        xz_scale,
        y_scale,
        noise,
        Arc::new(|s: &ShiftedNoise| s.shift_x.clone()),
        Arc::new(|s: &ShiftedNoise| s.shift_y.clone()),
        Arc::new(|s: &ShiftedNoise| s.shift_z.clone()),
        Arc::new(|s: &ShiftedNoise| s.xz_scale),
        Arc::new(|s: &ShiftedNoise| s.y_scale),
        Arc::new(|s: &ShiftedNoise| s.noise.clone()),
    );
    erase_map_codec::<ShiftedNoise, Ops>(
        map_codec::of(encoder, decoder, "ShiftedNoise".to_string()),
        "minecraft:shifted_noise".to_string(),
    )
}

/// `RangeChoice.CODEC` — the `input`/`min_inclusive`/`max_exclusive`/
/// `when_in_range`/`when_out_of_range` record.
fn range_choice_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    let input = codec::field_of(top.clone(), "input".to_string());
    let min_inclusive = codec::field_of(noise_value_codec::<Ops>(), "min_inclusive".to_string());
    let max_exclusive = codec::field_of(noise_value_codec::<Ops>(), "max_exclusive".to_string());
    let when_in_range = codec::field_of(top.clone(), "when_in_range".to_string());
    let when_out_of_range = codec::field_of(top, "when_out_of_range".to_string());
    erase_map_codec::<RangeChoice, Ops>(
        map_codec::of(
            map_encoder_fields5(
                input.clone(),
                min_inclusive.clone(),
                max_exclusive.clone(),
                when_in_range.clone(),
                when_out_of_range.clone(),
                Arc::new(|r: &RangeChoice| r.input.clone()),
                Arc::new(|r: &RangeChoice| r.min_inclusive),
                Arc::new(|r: &RangeChoice| r.max_exclusive),
                Arc::new(|r: &RangeChoice| r.when_in_range.clone()),
                Arc::new(|r: &RangeChoice| r.when_out_of_range.clone()),
            ),
            map_decoder_ap5(
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
                Arc::new(
                    |i: &Arc<dyn DensityFunction>,
                     mn: &f64,
                     mx: &f64,
                     wr: &Arc<dyn DensityFunction>,
                     wor: &Arc<dyn DensityFunction>| {
                        RangeChoice::new(i.clone(), *mn, *mx, wr.clone(), wor.clone())
                    },
                ),
            ),
            "RangeChoice".to_string(),
        ),
        "minecraft:range_choice".to_string(),
    )
}

/// `IntervalSelect.CODEC` — the `input`/`thresholds`/`functions` record with
/// the `validate` check. `THRESHOLDS_CODEC = NOISE_VALUE_CODEC.listOf()
/// .xmap(DoubleArrayList::new, identity)`; `functions` is `listOf(2, MAX)`.
fn interval_select_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    let input = codec::field_of(top.clone(), "input".to_string());
    let thresholds = codec::field_of(
        codec::list(noise_value_codec::<Ops>()),
        "thresholds".to_string(),
    );
    let functions = codec::field_of(
        codec::list_with_range(top, 2, i32::MAX),
        "functions".to_string(),
    );
    let base = map_codec::of(
        map_encoder_fields3(
            input.clone(),
            thresholds.clone(),
            functions.clone(),
            Arc::new(|i: &IntervalSelect| i.input.clone()),
            Arc::new(|i: &IntervalSelect| i.thresholds.clone()),
            Arc::new(|i: &IntervalSelect| i.functions.clone()),
        ),
        map_decoder_ap3(
            input,
            thresholds,
            functions,
            Arc::new(
                |i: &Arc<dyn DensityFunction>, t: &Vec<f64>, f: &Vec<Arc<dyn DensityFunction>>| {
                    IntervalSelect::new(i.clone(), t.clone(), f.clone())
                },
            ),
        ),
        "IntervalSelect".to_string(),
    );
    erase_map_codec::<IntervalSelect, Ops>(
        map_codec::validate(base, Arc::new(IntervalSelect::validate)),
        "minecraft:interval_select".to_string(),
    )
}

/// `Spline.CODEC` — `SPLINE_CODEC.fieldOf("spline").xmap(Spline::new,
/// Spline::spline)` where `SPLINE_CODEC = CubicSpline.codec(Coordinate.CODEC)`.
fn spline_map_codec<Ops>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    let coordinate_codec = SplineCoordinate::codec::<Ops>(top);
    let spline_codec = cubic_spline::codec::<SplineCoordinate, Ops>(coordinate_codec);
    erase_map_codec::<Spline, Ops>(
        map_codec::xmap(
            codec::field_of(spline_codec, "spline".to_string()),
            Arc::new(|s: &CubicSpline<SplineCoordinate>| Spline::new(s.clone())),
            Arc::new(|s: &Spline| s.spline.clone()),
        ),
        "minecraft:spline".to_string(),
    )
}

/// `YClampedGradient.CODEC` — the `from_y`/`to_y`/`from_value`/`to_value`
/// record. `from_y`/`to_y` range over `DimensionType.MIN_Y * 2 ..=
/// MAX_Y * 2`; the value fields are `NOISE_VALUE_CODEC`.
fn y_clamped_gradient_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    let from_y = codec::field_of(
        codec::int_range::<Ops>(MIN_Y * 2, MAX_Y * 2),
        "from_y".to_string(),
    );
    let to_y = codec::field_of(
        codec::int_range::<Ops>(MIN_Y * 2, MAX_Y * 2),
        "to_y".to_string(),
    );
    let from_value = codec::field_of(noise_value_codec::<Ops>(), "from_value".to_string());
    let to_value = codec::field_of(noise_value_codec::<Ops>(), "to_value".to_string());
    erase_map_codec::<YClampedGradient, Ops>(
        map_codec::of(
            map_encoder_fields4(
                from_y.clone(),
                to_y.clone(),
                from_value.clone(),
                to_value.clone(),
                Arc::new(|g: &YClampedGradient| g.from_y),
                Arc::new(|g: &YClampedGradient| g.to_y),
                Arc::new(|g: &YClampedGradient| g.from_value),
                Arc::new(|g: &YClampedGradient| g.to_value),
            ),
            map_decoder_ap4(
                from_y,
                to_y,
                from_value,
                to_value,
                Arc::new(|fy: &i32, ty: &i32, fv: &f64, tv: &f64| {
                    YClampedGradient::new(*fy, *ty, *fv, *tv)
                }),
            ),
            "YClampedGradient".to_string(),
        ),
        "minecraft:y_clamped_gradient".to_string(),
    )
}

/// `FindTopSurface.CODEC` — the `density`/`upper_bound`/`lower_bound`/
/// `cell_height` record.
fn find_top_surface_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn DensityFunction>, Ops>> {
    let density = codec::field_of(top.clone(), "density".to_string());
    let upper_bound = codec::field_of(top, "upper_bound".to_string());
    let lower_bound = codec::field_of(
        codec::int_range::<Ops>(MIN_Y * 2, MAX_Y * 2),
        "lower_bound".to_string(),
    );
    // `ExtraCodecs.POSITIVE_INT` — the `Codec.intRange(1, MAX)`.
    let cell_height = codec::field_of(
        codec::int_range::<Ops>(1, i32::MAX),
        "cell_height".to_string(),
    );
    erase_map_codec::<FindTopSurface, Ops>(
        map_codec::of(
            map_encoder_fields4(
                density.clone(),
                upper_bound.clone(),
                lower_bound.clone(),
                cell_height.clone(),
                Arc::new(|f: &FindTopSurface| f.density.clone()),
                Arc::new(|f: &FindTopSurface| f.upper_bound.clone()),
                Arc::new(|f: &FindTopSurface| f.lower_bound),
                Arc::new(|f: &FindTopSurface| f.cell_height),
            ),
            map_decoder_ap4(
                density,
                upper_bound,
                lower_bound,
                cell_height,
                Arc::new(
                    |d: &Arc<dyn DensityFunction>,
                     u: &Arc<dyn DensityFunction>,
                     lb: &i32,
                     ch: &i32| {
                        FindTopSurface::new(d.clone(), u.clone(), *lb, *ch)
                    },
                ),
            ),
            "FindTopSurface".to_string(),
        ),
        "minecraft:find_top_surface".to_string(),
    )
}

// ---------------------------------------------------------------------------
// Manual multi-field encoder/decoder composition (the record_builder caps at
// Group5; the 5/6-field records compose halves directly)
// ---------------------------------------------------------------------------

/// Compose N field encoders into a single `MapEncoder<C>` that writes all
/// fields (Java `Products.Pn` encoder = run each field's encoder with the
/// getter applied).
fn map_encoder_fields2<C, Ops: DynamicOps + 'static, T, U>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    t_getter: Arc<dyn Fn(&C) -> T + Send + Sync>,
    u_getter: Arc<dyn Fn(&C) -> U + Send + Sync>,
) -> Arc<dyn MapEncoder<C, Ops>>
where
    C: 'static,
    T: 'static,
    U: 'static,
{
    let t_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(t));
    let u_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(u));
    let t_enc_e = t_enc.clone();
    let t_enc_k = t_enc.clone();
    let u_enc_e = u_enc.clone();
    let u_enc_k = u_enc.clone();
    rivet_serialization::map_encoder::of(
        Arc::new(
            move |input: &C, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                t_enc_e.encode(&t_getter(input), ops, prefix);
                u_enc_e.encode(&u_getter(input), ops, prefix);
            },
        ),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_enc_k.keys(ops);
            keys.extend(u_enc_k.keys(ops));
            keys
        }),
    )
}

/// 3-field encoder composition.
#[allow(clippy::too_many_arguments)]
fn map_encoder_fields3<C, Ops: DynamicOps + 'static, T, U, V>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    t_getter: Arc<dyn Fn(&C) -> T + Send + Sync>,
    u_getter: Arc<dyn Fn(&C) -> U + Send + Sync>,
    v_getter: Arc<dyn Fn(&C) -> V + Send + Sync>,
) -> Arc<dyn MapEncoder<C, Ops>>
where
    C: 'static,
    T: 'static,
    U: 'static,
    V: 'static,
{
    let t_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(t));
    let u_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(u));
    let v_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(v));
    let t_enc_e = t_enc.clone();
    let t_enc_k = t_enc.clone();
    let u_enc_e = u_enc.clone();
    let u_enc_k = u_enc.clone();
    let v_enc_e = v_enc.clone();
    let v_enc_k = v_enc.clone();
    rivet_serialization::map_encoder::of(
        Arc::new(
            move |input: &C, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                t_enc_e.encode(&t_getter(input), ops, prefix);
                u_enc_e.encode(&u_getter(input), ops, prefix);
                v_enc_e.encode(&v_getter(input), ops, prefix);
            },
        ),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_enc_k.keys(ops);
            keys.extend(u_enc_k.keys(ops));
            keys.extend(v_enc_k.keys(ops));
            keys
        }),
    )
}

/// 4-field encoder composition.
#[allow(clippy::too_many_arguments)]
fn map_encoder_fields4<C, Ops: DynamicOps + 'static, T, U, V, W>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    w: Arc<dyn MapCodec<W, Ops>>,
    t_getter: Arc<dyn Fn(&C) -> T + Send + Sync>,
    u_getter: Arc<dyn Fn(&C) -> U + Send + Sync>,
    v_getter: Arc<dyn Fn(&C) -> V + Send + Sync>,
    w_getter: Arc<dyn Fn(&C) -> W + Send + Sync>,
) -> Arc<dyn MapEncoder<C, Ops>>
where
    C: 'static,
    T: 'static,
    U: 'static,
    V: 'static,
    W: 'static,
{
    let t_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(t));
    let u_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(u));
    let v_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(v));
    let w_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(w));
    let t_enc_e = t_enc.clone();
    let t_enc_k = t_enc.clone();
    let u_enc_e = u_enc.clone();
    let u_enc_k = u_enc.clone();
    let v_enc_e = v_enc.clone();
    let v_enc_k = v_enc.clone();
    let w_enc_e = w_enc.clone();
    let w_enc_k = w_enc.clone();
    rivet_serialization::map_encoder::of(
        Arc::new(
            move |input: &C, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                t_enc_e.encode(&t_getter(input), ops, prefix);
                u_enc_e.encode(&u_getter(input), ops, prefix);
                v_enc_e.encode(&v_getter(input), ops, prefix);
                w_enc_e.encode(&w_getter(input), ops, prefix);
            },
        ),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_enc_k.keys(ops);
            keys.extend(u_enc_k.keys(ops));
            keys.extend(v_enc_k.keys(ops));
            keys.extend(w_enc_k.keys(ops));
            keys
        }),
    )
}

/// 5-field encoder composition.
#[allow(clippy::too_many_arguments)]
fn map_encoder_fields5<C, Ops: DynamicOps + 'static, T, U, V, W, X>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    w: Arc<dyn MapCodec<W, Ops>>,
    x: Arc<dyn MapCodec<X, Ops>>,
    t_getter: Arc<dyn Fn(&C) -> T + Send + Sync>,
    u_getter: Arc<dyn Fn(&C) -> U + Send + Sync>,
    v_getter: Arc<dyn Fn(&C) -> V + Send + Sync>,
    w_getter: Arc<dyn Fn(&C) -> W + Send + Sync>,
    x_getter: Arc<dyn Fn(&C) -> X + Send + Sync>,
) -> Arc<dyn MapEncoder<C, Ops>>
where
    C: 'static,
    T: 'static,
    U: 'static,
    V: 'static,
    W: 'static,
    X: 'static,
{
    let t_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(t));
    let u_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(u));
    let v_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(v));
    let w_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(w));
    let x_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(x));
    let t_enc_e = t_enc.clone();
    let t_enc_k = t_enc.clone();
    let u_enc_e = u_enc.clone();
    let u_enc_k = u_enc.clone();
    let v_enc_e = v_enc.clone();
    let v_enc_k = v_enc.clone();
    let w_enc_e = w_enc.clone();
    let w_enc_k = w_enc.clone();
    let x_enc_e = x_enc.clone();
    let x_enc_k = x_enc.clone();
    rivet_serialization::map_encoder::of(
        Arc::new(
            move |input: &C, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                t_enc_e.encode(&t_getter(input), ops, prefix);
                u_enc_e.encode(&u_getter(input), ops, prefix);
                v_enc_e.encode(&v_getter(input), ops, prefix);
                w_enc_e.encode(&w_getter(input), ops, prefix);
                x_enc_e.encode(&x_getter(input), ops, prefix);
            },
        ),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_enc_k.keys(ops);
            keys.extend(u_enc_k.keys(ops));
            keys.extend(v_enc_k.keys(ops));
            keys.extend(w_enc_k.keys(ops));
            keys.extend(x_enc_k.keys(ops));
            keys
        }),
    )
}

/// 6-field encoder composition.
#[allow(clippy::too_many_arguments)]
fn map_encoder_fields6<C, Ops: DynamicOps + 'static, T, U, V, W, X, Y>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    w: Arc<dyn MapCodec<W, Ops>>,
    x: Arc<dyn MapCodec<X, Ops>>,
    y: Arc<dyn MapCodec<Y, Ops>>,
    t_getter: Arc<dyn Fn(&C) -> T + Send + Sync>,
    u_getter: Arc<dyn Fn(&C) -> U + Send + Sync>,
    v_getter: Arc<dyn Fn(&C) -> V + Send + Sync>,
    w_getter: Arc<dyn Fn(&C) -> W + Send + Sync>,
    x_getter: Arc<dyn Fn(&C) -> X + Send + Sync>,
    y_getter: Arc<dyn Fn(&C) -> Y + Send + Sync>,
) -> Arc<dyn MapEncoder<C, Ops>>
where
    C: 'static,
    T: 'static,
    U: 'static,
    V: 'static,
    W: 'static,
    X: 'static,
    Y: 'static,
{
    let t_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(t));
    let u_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(u));
    let v_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(v));
    let w_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(w));
    let x_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(x));
    let y_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(y));
    let t_enc_e = t_enc.clone();
    let t_enc_k = t_enc.clone();
    let u_enc_e = u_enc.clone();
    let u_enc_k = u_enc.clone();
    let v_enc_e = v_enc.clone();
    let v_enc_k = v_enc.clone();
    let w_enc_e = w_enc.clone();
    let w_enc_k = w_enc.clone();
    let x_enc_e = x_enc.clone();
    let x_enc_k = x_enc.clone();
    let y_enc_e = y_enc.clone();
    let y_enc_k = y_enc.clone();
    rivet_serialization::map_encoder::of(
        Arc::new(
            move |input: &C, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                t_enc_e.encode(&t_getter(input), ops, prefix);
                u_enc_e.encode(&u_getter(input), ops, prefix);
                v_enc_e.encode(&v_getter(input), ops, prefix);
                w_enc_e.encode(&w_getter(input), ops, prefix);
                x_enc_e.encode(&x_getter(input), ops, prefix);
                y_enc_e.encode(&y_getter(input), ops, prefix);
            },
        ),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_enc_k.keys(ops);
            keys.extend(u_enc_k.keys(ops));
            keys.extend(v_enc_k.keys(ops));
            keys.extend(w_enc_k.keys(ops));
            keys.extend(x_enc_k.keys(ops));
            keys.extend(y_enc_k.keys(ops));
            keys
        }),
    )
}
#[allow(clippy::type_complexity)] // the N-ary applicative fold mirrors Java's `ap` curry
/// `DataResult.instance().ap2` over two field decoders — decode both fields
/// (error accumulation), apply the constructor.
fn map_decoder_ap2<T, U, C, Ops: DynamicOps + 'static>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    constructor: Arc<dyn Fn(&T, &U) -> C + Send + Sync>,
) -> Arc<dyn MapDecoder<C, Ops>>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    C: 'static,
{
    let t_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(t));
    let u_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(u));
    let t_dec_d = t_dec.clone();
    let t_dec_k = t_dec.clone();
    let u_dec_d = u_dec.clone();
    let u_dec_k = u_dec.clone();
    rivet_serialization::map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let t_r = t_dec_d.decode(ops, input);
            let u_r = u_dec_d.decode(ops, input);
            let constructor = constructor.clone();
            t_r.apply2(move |tv: &T, uv: &U| constructor(tv, uv), u_r)
        }),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_dec_k.keys(ops);
            keys.extend(u_dec_k.keys(ops));
            keys
        }),
    )
}
#[allow(clippy::type_complexity)] // the N-ary applicative fold mirrors Java's `ap` curry
/// `ap3` — see `map_decoder_ap2`.
fn map_decoder_ap3<T, U, V, C, Ops: DynamicOps + 'static>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    constructor: Arc<dyn Fn(&T, &U, &V) -> C + Send + Sync>,
) -> Arc<dyn MapDecoder<C, Ops>>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    C: 'static,
{
    let t_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(t));
    let u_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(u));
    let v_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(v));
    let t_dec_d = t_dec.clone();
    let t_dec_k = t_dec.clone();
    let u_dec_d = u_dec.clone();
    let u_dec_k = u_dec.clone();
    let v_dec_d = v_dec.clone();
    let v_dec_k = v_dec.clone();
    rivet_serialization::map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let t_r = t_dec_d.decode(ops, input);
            let u_r = u_dec_d.decode(ops, input);
            let v_r = v_dec_d.decode(ops, input);
            let constructor = constructor.clone();
            t_r.apply3(
                move |tv: &T, uv: &U, vv: &V| constructor(tv, uv, vv),
                u_r,
                v_r,
            )
        }),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_dec_k.keys(ops);
            keys.extend(u_dec_k.keys(ops));
            keys.extend(v_dec_k.keys(ops));
            keys
        }),
    )
}
#[allow(clippy::type_complexity)] // the N-ary applicative fold mirrors Java's `ap` curry
/// `ap4` — the `apply4` applicative (`DataResult.instance().ap4`).
fn map_decoder_ap4<T, U, V, W, C, Ops: DynamicOps + 'static>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    w: Arc<dyn MapCodec<W, Ops>>,
    constructor: Arc<dyn Fn(&T, &U, &V, &W) -> C + Send + Sync>,
) -> Arc<dyn MapDecoder<C, Ops>>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    W: Clone + Send + Sync + 'static,
    C: 'static,
{
    let t_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(t));
    let u_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(u));
    let v_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(v));
    let w_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(w));
    let t_dec_d = t_dec.clone();
    let t_dec_k = t_dec.clone();
    let u_dec_d = u_dec.clone();
    let u_dec_k = u_dec.clone();
    let v_dec_d = v_dec.clone();
    let v_dec_k = v_dec.clone();
    let w_dec_d = w_dec.clone();
    let w_dec_k = w_dec.clone();
    rivet_serialization::map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let t_r = t_dec_d.decode(ops, input);
            let u_r = u_dec_d.decode(ops, input);
            let v_r = v_dec_d.decode(ops, input);
            let w_r = w_dec_d.decode(ops, input);
            let constructor = constructor.clone();
            let fr: DataResult<Fn4<T, U, V, W, C>> =
                DataResult::success(Arc::new(move |tv: &T, uv: &U, vv: &V, wv: &W| {
                    constructor(tv, uv, vv, wv)
                }));
            rivet_serialization::data_result::ap4(fr, t_r, u_r, v_r, w_r)
        }),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_dec_k.keys(ops);
            keys.extend(u_dec_k.keys(ops));
            keys.extend(v_dec_k.keys(ops));
            keys.extend(w_dec_k.keys(ops));
            keys
        }),
    )
}
#[allow(clippy::type_complexity)] // the N-ary applicative fold mirrors Java's `ap` curry
/// `ap5` — the `apply5` applicative (`DataResult.instance().ap5`).
fn map_decoder_ap5<T, U, V, W, X, C, Ops: DynamicOps + 'static>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    w: Arc<dyn MapCodec<W, Ops>>,
    x: Arc<dyn MapCodec<X, Ops>>,
    constructor: Arc<dyn Fn(&T, &U, &V, &W, &X) -> C + Send + Sync>,
) -> Arc<dyn MapDecoder<C, Ops>>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    W: Clone + Send + Sync + 'static,
    X: Clone + Send + Sync + 'static,
    C: 'static,
{
    let t_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(t));
    let u_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(u));
    let v_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(v));
    let w_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(w));
    let x_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(x));
    let t_dec_d = t_dec.clone();
    let t_dec_k = t_dec.clone();
    let u_dec_d = u_dec.clone();
    let u_dec_k = u_dec.clone();
    let v_dec_d = v_dec.clone();
    let v_dec_k = v_dec.clone();
    let w_dec_d = w_dec.clone();
    let w_dec_k = w_dec.clone();
    let x_dec_d = x_dec.clone();
    let x_dec_k = x_dec.clone();
    rivet_serialization::map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let t_r = t_dec_d.decode(ops, input);
            let u_r = u_dec_d.decode(ops, input);
            let v_r = v_dec_d.decode(ops, input);
            let w_r = w_dec_d.decode(ops, input);
            let x_r = x_dec_d.decode(ops, input);
            let constructor = constructor.clone();
            let fr: DataResult<Fn5<T, U, V, W, X, C>> =
                DataResult::success(Arc::new(move |tv: &T, uv: &U, vv: &V, wv: &W, xv: &X| {
                    constructor(tv, uv, vv, wv, xv)
                }));
            rivet_serialization::data_result::ap5(fr, t_r, u_r, v_r, w_r, x_r)
        }),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_dec_k.keys(ops);
            keys.extend(u_dec_k.keys(ops));
            keys.extend(v_dec_k.keys(ops));
            keys.extend(w_dec_k.keys(ops));
            keys.extend(x_dec_k.keys(ops));
            keys
        }),
    )
}

/// `ap6` — the `apply6` applicative (`DataResult.instance().ap6`; only in
/// `ShiftedNoise`).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)] // the N-ary applicative fold mirrors Java's `ap` curry
fn map_decoder_ap6<T, U, V, W, X, Y, C, Ops: DynamicOps + 'static>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    w: Arc<dyn MapCodec<W, Ops>>,
    x: Arc<dyn MapCodec<X, Ops>>,
    y: Arc<dyn MapCodec<Y, Ops>>,
    constructor: Arc<dyn Fn(&T, &U, &V, &W, &X, &Y) -> C + Send + Sync>,
) -> Arc<dyn MapDecoder<C, Ops>>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    W: Clone + Send + Sync + 'static,
    X: Clone + Send + Sync + 'static,
    Y: Clone + Send + Sync + 'static,
    C: 'static,
{
    let t_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(t));
    let u_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(u));
    let v_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(v));
    let w_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(w));
    let x_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(x));
    let y_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(y));
    let t_dec_d = t_dec.clone();
    let t_dec_k = t_dec.clone();
    let u_dec_d = u_dec.clone();
    let u_dec_k = u_dec.clone();
    let v_dec_d = v_dec.clone();
    let v_dec_k = v_dec.clone();
    let w_dec_d = w_dec.clone();
    let w_dec_k = w_dec.clone();
    let x_dec_d = x_dec.clone();
    let x_dec_k = x_dec.clone();
    let y_dec_d = y_dec.clone();
    let y_dec_k = y_dec.clone();
    rivet_serialization::map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let t_r = t_dec_d.decode(ops, input);
            let u_r = u_dec_d.decode(ops, input);
            let v_r = v_dec_d.decode(ops, input);
            let w_r = w_dec_d.decode(ops, input);
            let x_r = x_dec_d.decode(ops, input);
            let y_r = y_dec_d.decode(ops, input);
            let constructor = constructor.clone();
            let fr: DataResult<Fn6<T, U, V, W, X, Y, C>> = DataResult::success(Arc::new(
                move |tv: &T, uv: &U, vv: &V, wv: &W, xv: &X, yv: &Y| {
                    constructor(tv, uv, vv, wv, xv, yv)
                },
            ));
            rivet_serialization::data_result::ap6(fr, t_r, u_r, v_r, w_r, x_r, y_r)
        }),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_dec_k.keys(ops);
            keys.extend(u_dec_k.keys(ops));
            keys.extend(v_dec_k.keys(ops));
            keys.extend(w_dec_k.keys(ops));
            keys.extend(x_dec_k.keys(ops));
            keys.extend(y_dec_k.keys(ops));
            keys
        }),
    )
}

#[cfg(test)]
fn old_blended_noise_stub() -> Arc<dyn DensityFunction> {
    #[derive(Debug)]
    struct OldBlendedNoise;
    impl DensityFunction for OldBlendedNoise {
        fn compute(&self, _context: &dyn FunctionContext) -> f64 {
            0.0
        }
        fn min_value(&self) -> f64 {
            -2.0
        }
        fn max_value(&self) -> f64 {
            2.0
        }
        fn type_id(&self) -> DensityFunctionTypeId {
            DensityFunctionTypes::OLD_BLENDED_NOISE
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn clone_arc(&self) -> Arc<dyn DensityFunction> {
            Arc::new(OldBlendedNoise)
        }
    }
    Arc::new(OldBlendedNoise)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::noise::density_function::{
        ContextProvider, SinglePointContext, Visitor, density_function_codec, map_all,
    };
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json;

    fn at(x: i32, y: i32, z: i32) -> SinglePointContext {
        SinglePointContext::new(x, y, z)
    }

    fn params() -> NoiseParameters {
        NoiseParameters::new(0, vec![1.0, 0.0])
    }

    // ------------------------------------------------------------------
    // compute / min / max
    // ------------------------------------------------------------------

    #[test]
    fn constant_computes_its_value() {
        let f = constant(3.5);
        assert_eq!(f.compute(&at(0, 0, 0)), 3.5);
        assert_eq!(f.min_value(), 3.5);
        assert_eq!(f.max_value(), 3.5);
        assert_eq!(
            DensityFunction::type_id(&*f),
            DensityFunctionTypes::CONSTANT
        );
    }

    #[test]
    fn clamp_computes_and_bounds() {
        let f = constant(-10.0).clamp(-1.0, 1.0);
        assert_eq!(f.compute(&at(0, 0, 0)), -1.0);
        let f2 = constant(0.5).clamp(-1.0, 1.0);
        assert_eq!(f2.compute(&at(0, 0, 0)), 0.5);
        let f3 = constant(10.0).clamp(-1.0, 1.0);
        assert_eq!(f3.compute(&at(0, 0, 0)), 1.0);
        // `Clamp.minValue()`/`maxValue()` return the clamp window.
        assert_eq!(f.min_value(), -1.0);
        assert_eq!(f.max_value(), 1.0);
        assert_eq!(DensityFunction::type_id(&*f), DensityFunctionTypes::CLAMP);
    }

    #[test]
    fn mapped_computes_and_bounds() {
        let abs = mapped(&*constant(-3.0), MappedType::Abs);
        assert_eq!(abs.compute(&at(0, 0, 0)), 3.0);
        // `Mapped.create(Abs, constant(-3))`: both bounds image to 3, and the
        // `Abs` branch takes `max(image, 0)` -> 3 / `max(image, min)` -> 3.
        assert_eq!(abs.min_value(), 3.0);
        assert_eq!(abs.max_value(), 3.0);

        let sq = mapped(&*constant(-2.0), MappedType::Square);
        assert_eq!(sq.compute(&at(0, 0, 0)), 4.0);
        assert_eq!(sq.min_value(), 4.0);
        assert_eq!(sq.max_value(), 4.0);

        let inv = mapped(&*constant(0.5), MappedType::Invert);
        assert_eq!(inv.compute(&at(0, 0, 0)), 2.0);
        assert_eq!(inv.min_value(), 2.0);
        assert_eq!(inv.max_value(), 2.0);
    }

    #[test]
    fn two_argument_computes() {
        let c2 = constant(2.0);
        let c3 = constant(3.0);
        assert_eq!(add(c2.clone(), c3.clone()).compute(&at(0, 0, 0)), 5.0);
        assert_eq!(mul(c2.clone(), c3.clone()).compute(&at(0, 0, 0)), 6.0);
        assert_eq!(min(c2.clone(), c3.clone()).compute(&at(0, 0, 0)), 2.0);
        assert_eq!(max(c2.clone(), c3.clone()).compute(&at(0, 0, 0)), 3.0);
    }

    #[test]
    fn two_argument_folds_constant_to_mul_or_add() {
        // `add(constant(2), f)` folds to `MulOrAdd(Add, f, 2)` (Java's
        // `TwoArgumentSimpleFunction.create`); the downcast must succeed.
        let base = constant(4.0);
        let sum = add(constant(2.0), base.clone());
        assert!(sum.as_any().downcast_ref::<MulOrAdd>().is_some());
        assert_eq!(sum.compute(&at(0, 0, 0)), 6.0);
        assert_eq!(sum.min_value(), 6.0);
        let product = mul(constant(2.0), base.clone());
        assert!(product.as_any().downcast_ref::<MulOrAdd>().is_some());
        assert_eq!(product.compute(&at(0, 0, 0)), 8.0);

        // Non-constant arguments build `Ap2`.
        let ap = add(blend_alpha(), y_clamped_gradient(0, 64, 0.0, 1.0));
        assert!(ap.as_any().downcast_ref::<Ap2>().is_some());
        assert_eq!(ap.compute(&at(0, 32, 0)), 1.5);
    }

    #[test]
    fn y_clamped_gradient_computes_clamped_map() {
        // Java `YClampedGradient.compute`:
        // `Mth.clampedMap(blockY, fromY, toY, fromValue, toValue)`.
        let f = y_clamped_gradient(0, 100, 0.0, 1.0);
        assert_eq!(f.compute(&at(0, 0, 0)), 0.0); // below fromY -> fromValue
        assert_eq!(f.compute(&at(0, 50, 0)), 0.5);
        assert_eq!(f.compute(&at(0, 100, 0)), 1.0);
        assert_eq!(f.compute(&at(0, 200, 0)), 1.0); // above toY -> toValue
        assert_eq!(f.min_value(), 0.0);
        assert_eq!(f.max_value(), 1.0);
    }

    #[test]
    fn range_choice_selects_by_input() {
        let inside = range_choice(constant(0.5), 0.0, 1.0, constant(10.0), constant(-10.0));
        assert_eq!(inside.compute(&at(0, 0, 0)), 10.0);
        let outside = range_choice(constant(1.5), 0.0, 1.0, constant(10.0), constant(-10.0));
        assert_eq!(outside.compute(&at(0, 0, 0)), -10.0);
    }

    #[test]
    fn end_islands_bounds_and_center_value() {
        let f = end_islands(0);
        assert_eq!(f.min_value(), -0.84375);
        assert_eq!(f.max_value(), 0.5625);
        // At the origin no island chunk is within the `distSq > 4096` radius
        // check, so `doffs` stays at its 80 clamp and the compute is exactly
        // `(80 - 8) / 128`.
        assert_eq!(f.compute(&at(0, 64, 0)), 0.5625);
        // Far from the End center the field stays within its declared bounds.
        let v = f.compute(&at(30000, 64, 30000));
        assert!((-0.84375 - 1e-9..=0.5625 + 1e-9).contains(&v));
    }

    #[test]
    fn end_islands_threshold_matches_widened_float() {
        // Java compares `getValue(...) < ISLAND_THRESHOLD` where
        // `ISLAND_THRESHOLD = -0.9F`; binary numeric promotion widens the float
        // to double, so the compared constant is `(double)(-0.9F)`, NOT the f64
        // literal `-0.9` (the two differ by ~2.4e-8). Pin the exact widened
        // value so a `-0.9` regression is caught.
        assert_eq!(EndIslandDensityFunction::ISLAND_THRESHOLD_D, -0.9f32 as f64);
        assert_ne!(EndIslandDensityFunction::ISLAND_THRESHOLD_D, -0.9f64);
    }

    #[test]
    fn find_top_surface_scans_down_from_upper_bound() {
        // density is `y`-aware: clamped-map(0..64, -1..1) is positive for
        // blockY > 32. upperBound 128, cell 4.
        let density = y_clamped_gradient(0, 64, -1.0, 1.0);
        let f = find_top_surface(density, constant(128.0), 0, 4);
        // top_y = floor(128/4)*4 = 128; density(128) = toValue = 1.0 > 0, so
        // the first (topmost) positive scan step returns 128.
        assert_eq!(f.compute(&at(0, 0, 0)), 128.0);
        assert_eq!(f.min_value(), 0.0);
        assert_eq!(f.max_value(), 128.0);
    }

    // ------------------------------------------------------------------
    // fill_array
    // ------------------------------------------------------------------

    struct LinearCtx(Vec<SinglePointContext>);
    impl ContextProvider for LinearCtx {
        fn for_index(&self, index: usize) -> SinglePointContext {
            self.0[index]
        }
        fn fill_all_directly(&self, output: &mut [f64], function: &dyn DensityFunction) {
            for (i, slot) in output.iter_mut().enumerate() {
                *slot = function.compute(&self.for_index(i));
            }
        }
    }

    #[test]
    fn fill_array_default_computes_per_index() {
        let ctx = LinearCtx(vec![at(0, 0, 0), at(0, 10, 0), at(0, 20, 0)]);
        let f = y_clamped_gradient(0, 20, 0.0, 2.0);
        let mut out = [0.0; 3];
        f.fill_array(&mut out, &ctx);
        assert_eq!(out, [0.0, 1.0, 2.0]);
    }

    #[test]
    fn constant_fill_array_fills_broadcast() {
        let f = constant(7.0);
        let mut out = [0.0; 4];
        f.fill_array(&mut out, &LinearCtx(vec![at(0, 0, 0); 4]));
        assert_eq!(out, [7.0, 7.0, 7.0, 7.0]);
    }

    // ------------------------------------------------------------------
    // map_children / map_all
    // ------------------------------------------------------------------

    /// A visitor that squares every `Constant` it reaches.
    struct SquareConstant;
    impl Visitor for SquareConstant {
        fn apply(&self, input: &dyn DensityFunction) -> Arc<dyn DensityFunction> {
            if let Some(c) = input.as_any().downcast_ref::<Constant>() {
                constant(c.value() * c.value())
            } else {
                input.clone_arc()
            }
        }
    }

    #[test]
    fn map_all_squares_nested_constants() {
        // `cache2d(clamp(constant(3), 0, 10))` — the visitor rewrites the
        // inner constant to 9, recursing through the marker and the clamp.
        let f = cache2d(constant(3.0).clamp(0.0, 10.0));
        let mapped = map_all(&*f, &SquareConstant);
        assert_eq!(
            DensityFunction::type_id(&*mapped),
            DensityFunctionTypes::CACHE_2D
        );
        assert_eq!(mapped.compute(&at(0, 0, 0)), 9.0);
        let marker = mapped
            .as_any()
            .downcast_ref::<Marker>()
            .expect("cache2d marker");
        assert_eq!(marker.marker_type(), MarkerType::Cache2D);
        let clamped = marker
            .wrapped()
            .as_any()
            .downcast_ref::<Clamp>()
            .expect("clamp");
        let inner = clamped
            .input()
            .as_any()
            .downcast_ref::<Constant>()
            .expect("constant");
        assert_eq!(inner.value(), 9.0);
    }

    #[test]
    fn map_children_identity_keeps_leaves() {
        // A `Constant` leaf's `map_children` returns itself; `map_all` then
        // applies the visitor to the same leaf (Java `apply(input.mapChildren
        // (this))`).
        let mapped = map_all(&*constant(3.0), &SquareConstant);
        assert_eq!(mapped.compute(&at(0, 0, 0)), 9.0);
    }

    // ------------------------------------------------------------------
    // Beardifier marker shell (structure runtime not ported — #177)
    // ------------------------------------------------------------------

    #[test]
    fn beardifier_marker_shell_is_zero() {
        let f = beardifier_marker();
        assert_eq!(f.compute(&at(0, 0, 0)), 0.0);
        assert_eq!(f.min_value(), 0.0);
        assert_eq!(f.max_value(), 0.0);
        assert_eq!(
            DensityFunction::type_id(&*f),
            DensityFunctionTypes::BEARDIFIER
        );
        let mut out = [7.0; 3];
        f.fill_array(&mut out, &LinearCtx(vec![at(0, 0, 0); 3]));
        assert_eq!(out, [0.0, 0.0, 0.0]);
    }

    // ------------------------------------------------------------------
    // noise / noise-holder (uninstantiated noise is the value-layer state)
    // ------------------------------------------------------------------

    #[test]
    fn noise_holder_bounds_without_instantiated_noise() {
        // Java: `new NoiseHolder(data, null)` — `getValue` returns 0.0 and
        // `maxValue` 2.0 for the un-instantiated holder.
        let holder = NoiseHolder::new(Holder::direct(params()));
        assert_eq!(holder.get_value(1.0, 2.0, 3.0), 0.0);
        assert_eq!(holder.max_value(), 2.0);
        let f = noise(Holder::direct(params()), 1.0, 1.0);
        assert_eq!(f.compute(&at(0, 0, 0)), 0.0);
    }

    // ------------------------------------------------------------------
    // codec round-trips through `DensityFunction.CODEC`
    // ------------------------------------------------------------------

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A `RegistryOps` whose access maps the `DENSITY_FUNCTION` registry key
    /// (empty frozen registry). `RegistryFileCodec.decode` errors with
    /// `"Registry does not exist"` for an access without the key, so the
    /// round-trip context must provide it — exactly what a real server's
    /// `RegistryAccess` does.
    fn test_ops() -> TestOps {
        let builder =
            RegistryBuilder::new(&*crate::levelgen::noise::registry_keys::DENSITY_FUNCTION);
        let registry = builder.freeze();
        let access = RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace(
                "worldgen/density_function",
            )),
            Box::new(registry) as rivet_registry::root::AnyBox,
        )]);
        TestOps::create_from_access(&JsonOps::INSTANCE, access)
    }

    fn round_trip(f: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
        let codec = density_function_codec::<TestOps>();
        let ops = test_ops();
        let encoded = codec
            .encode_start(&ops, &f)
            .result()
            .expect("encode should succeed")
            .clone();
        codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone()
    }

    #[test]
    fn constant_round_trips_as_bare_number() {
        // `DIRECT_CODEC = either(NOISE_VALUE_CODEC, dispatch)` — a constant
        // encodes as the bare double (Java `Either.left`).
        let codec = density_function_codec::<TestOps>();
        let ops = test_ops();
        let encoded = codec
            .encode_start(&ops, &constant(5.5))
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, serde_json::json!(5.5));
        let decoded = round_trip(constant(5.5));
        assert_eq!(
            DensityFunction::type_id(&*decoded),
            DensityFunctionTypes::CONSTANT
        );
        assert_eq!(decoded.compute(&at(0, 0, 0)), 5.5);
    }

    #[test]
    fn clamp_round_trips_with_nested_constant() {
        let f = constant(2.0).clamp(0.0, 5.0);
        let codec = density_function_codec::<TestOps>();
        let ops = test_ops();
        let encoded = codec
            .encode_start(&ops, &f)
            .result()
            .expect("encode should succeed")
            .clone();
        // The dispatch writes `"type"`, then the `input`/`min`/`max` fields in
        // codec order (Java `RecordCodecBuilder` group order); the nested
        // constant encodes through the recursive child as a bare double.
        assert_eq!(
            encoded,
            serde_json::json!({"type": "minecraft:clamp", "input": 2.0, "min": 0.0, "max": 5.0})
        );
        let decoded = round_trip(f);
        assert_eq!(
            DensityFunction::type_id(&*decoded),
            DensityFunctionTypes::CLAMP
        );
        assert_eq!(decoded.compute(&at(0, 0, 0)), 2.0);
    }

    #[test]
    fn two_argument_round_trips_and_folds_back() {
        // `add(constant(1.0), constant(2.0))` folds to `MulOrAdd`; the dispatch
        // resolves `type_id` (ADD) to the add codec, whose `argument1`/
        // `argument2` getters re-derive the two constants, and decode folds
        // again (Java `TwoArgumentSimpleFunction.create`).
        let f = add(constant(1.0), constant(2.0));
        let decoded = round_trip(f);
        assert_eq!(
            DensityFunction::type_id(&*decoded),
            DensityFunctionTypes::ADD
        );
        assert_eq!(decoded.compute(&at(0, 0, 0)), 3.0);
        assert!(decoded.as_any().downcast_ref::<MulOrAdd>().is_some());
    }

    #[test]
    fn y_clamped_gradient_round_trips() {
        let f = y_clamped_gradient(-64, 320, -2.0, 1.0);
        let decoded = round_trip(f.clone());
        assert_eq!(
            DensityFunction::type_id(&*decoded),
            DensityFunctionTypes::Y_CLAMPED_GRADIENT
        );
        // The decoded function is a fresh `YClampedGradient` with the same
        // parameters, so compute matches at every block y.
        for y in [-64, -1, 0, 128, 319, 320, 400] {
            assert_eq!(decoded.compute(&at(0, y, 0)), f.compute(&at(0, y, 0)));
        }
    }

    #[test]
    fn codec_rejects_unknown_type() {
        // A dispatch key that is not in the #177 table errors on encode
        // (`"Density function type '...' is not ported"`); here the encode of
        // a function whose type_id has no codec is exercised via OLD_BLENDED_NOISE.
        let codec = density_function_codec::<TestOps>();
        let ops = test_ops();
        let f = old_blended_noise_stub();
        let result = codec.encode_start(&ops, &f);
        // The old_blended_noise stub is a real carrier but has no #177 codec.
        assert!(
            result.result().is_none(),
            "unported type must fail to encode"
        );
    }
}
