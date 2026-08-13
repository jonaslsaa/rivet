//! Port of `net.minecraft.world.level.levelgen.DensityFunction` (interface,
//! 26.2).
//!
//! The behavior contract of every density function — `compute`,
//! `fillArray`, `mapChildren`, `minValue`, `maxValue` — plus the nested
//! `ContextProvider`/`FunctionContext`/`NoiseHolder`/`SimpleFunction`/
//! `SinglePointContext`/`Visitor` types and the default combinator methods
//! (`clamp`/`abs`/`square`/`cube`/`halfNegative`/`quarterNegative`/`invert`/
//! `squeeze`).
//!
//! ## Erased-carrier dispatch (the `BlockPredicate` pattern)
//!
//! Java's `DensityFunction` is the erased `List<DensityFunction>` element and
//! the `Codec<DensityFunction>` value. The Rust port follows the established
//! `BlockPredicate` model: the behavior contract is the object-safe
//! [`DensityFunction`] trait, and the value combinators store — and the codecs
//! (de)serialize — `Arc<dyn DensityFunction>`. `Any` (supertrait) enables the
//! dispatch codec's downcast of an erased value back to its concrete type on
//! encode, via the explicit [`DensityFunction::as_any`] seam (the same reason
//! `BlockPredicate` uses `as_any`).
//!
//! Java's `codec()` instance method (used by the dispatch's type function)
//! is replaced by [`DensityFunction::type_id`] — the erased value's registry
//! identity — and the `#177` dispatch table in `density_functions` resolves
//! that id to the concrete `MapCodec`. `clone_arc` is the object-safe clone the
//! `SimpleFunction::map_children` identity default needs (Java `return this`).
//!
//! ## `DensityFunction.CODEC`
//!
//! Java:
//!
//! ```java
//! CODEC = RegistryFileCodec.create(Registries.DENSITY_FUNCTION, DensityFunctions.DIRECT_CODEC)
//!    .xmap(holder -> switch (holder) {
//!        case Direct d -> (DensityFunction)d.value();
//!        case Reference r -> new DensityFunctions.HolderHolder(r);
//!    }, value -> switch (value) {
//!        case HolderHolder h -> h.function();
//!        default -> Holder.direct(value);
//!    });
//! ```
//!
//! The port builds it (ops-generic) in [`density_function_codec`], threading
//! the recursive child codec (`top`) through `DIRECT_CODEC`'s dispatch so
//! nested functions round-trip. The `Holder::Reference -> HolderHolder` arm is
//! the registry-aware seam: the value layer cannot resolve a live reference
//! (no lookup), so `HolderHolder`'s value-resolution defers (RivetTodo #177).

use crate::levelgen::noise::density_function_type::DensityFunctionTypeId;
use crate::levelgen::noise::density_functions;
use crate::levelgen::synth::normal_noise::{NoiseParameters, NormalNoise};
use rivet_registry::Holder;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use std::any::Any;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// A `HashMap` key that hashes on object identity and holds its
/// `Arc<dyn DensityFunction>` key strongly.
///
/// Java's visitor wrap caches (`NoiseChunk.wrapped`,
/// `RandomState`'s anonymous visitors) are `HashMap<DensityFunction,
/// DensityFunction>` keyed on reference identity, and the map keeps its keys
/// strongly reachable. The `#177` value model has no `Hash` on
/// `dyn DensityFunction`, so identity is the Arc allocation address — but the
/// key must ALSO be retained: `mapChildren` produces fresh intermediate `Arc`s
/// that would otherwise be dropped and their addresses recycled by a later
/// allocation, giving a spurious cache hit (Java's strong keys make that
/// impossible).
#[derive(Clone)]
pub struct IdentityKey(Arc<dyn DensityFunction>);

impl IdentityKey {
    /// Wraps an owned function reference as an identity key.
    pub fn new(arc: Arc<dyn DensityFunction>) -> Self {
        IdentityKey(arc)
    }
}

impl PartialEq for IdentityKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for IdentityKey {}

impl Hash for IdentityKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0).cast::<()>() as usize).hash(state)
    }
}

/// `net.minecraft.world.level.levelgen.DensityFunction` — the behavior contract
/// of every density function.
///
/// The erased carrier `Arc<dyn DensityFunction>` is what combinators store and
/// what the dispatch codec (de)serializes.
pub trait DensityFunction: Any + Debug + Send + Sync + 'static {
    /// `compute(FunctionContext)`.
    fn compute(&self, context: &dyn FunctionContext) -> f64;

    /// `fillArray(double[], ContextProvider)`.
    ///
    /// The default is `DensityFunction.SimpleFunction`'s — `contextProvider
    /// .fillAllDirectly(output, this)` — matching Java's `SimpleFunction`
    /// interface providing the default. Composite functions that fill the
    /// array field-by-field override it. It is object-safe: `fillAllDirectly`
    /// is exactly the per-index `output[i] = function.compute(provider
    /// .forIndex(i))` loop, inlined here because forwarding `self` as
    /// `&dyn DensityFunction` would need `Self: Sized` (and the default must
    /// run through a trait object).
    fn fill_array(&self, output: &mut [f64], context_provider: &dyn ContextProvider) {
        for (i, slot) in output.iter_mut().enumerate() {
            let context = context_provider.for_index(i);
            *slot = self.compute(context);
        }
    }

    /// `mapChildren(Visitor)`.
    ///
    /// The default is `DensityFunction.SimpleFunction`'s — identity (`return
    /// this`), via `clone_arc`. Functions whose children carry transformable
    /// state (child functions, `NoiseHolder`s) override it.
    fn map_children(&self, _visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        self.clone_arc()
    }

    /// `minValue()`.
    fn min_value(&self) -> f64;

    /// `maxValue()`.
    fn max_value(&self) -> f64;

    /// The registry-held identity this function dispatches on (the key the
    /// `#177` dispatch table uses — Rust's stand-in for Java's `codec()`).
    fn type_id(&self) -> DensityFunctionTypeId;

    /// The downcast seam (Java's erased `DensityFunction` cast) the dispatch
    /// codec uses on encode to recover the concrete function type.
    fn as_any(&self) -> &dyn Any;

    /// Object-safe clone — `Arc::new(self.clone())` on the concrete type. The
    /// `SimpleFunction::map_children` identity default needs it (Java
    /// `return this`).
    fn clone_arc(&self) -> Arc<dyn DensityFunction>;
}

/// `DensityFunction.mapAll(Visitor)` — the recursive visitor: `apply(input
/// .mapChildren(this))`. Ported as a free function because Rust has no
/// anonymous local classes; the `RecursiveVisitor` is a local struct.
///
/// The function is handed to the visitor as the owned `&Arc` so a wrap cache
/// can key on object identity AND retain the key (see [`IdentityKey`]) —
/// Java's `mapAll` passes its `DensityFunction` references and the visitor's
/// `HashMap` keeps them strongly reachable, so a rebuilt intermediate whose
/// address would otherwise be recycled can never spuriously alias a live cache
/// key.
pub fn map_all(
    function: &Arc<dyn DensityFunction>,
    visitor: &dyn Visitor,
) -> Arc<dyn DensityFunction> {
    struct RecursiveVisitor<'a> {
        visitor: &'a dyn Visitor,
    }

    impl Visitor for RecursiveVisitor<'_> {
        fn apply(&self, input: &Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
            let mapped = input.map_children(self);
            self.visitor.apply(&mapped)
        }

        fn visit_noise(&self, noise: &NoiseHolder) -> NoiseHolder {
            self.visitor.visit_noise(noise)
        }
    }

    RecursiveVisitor { visitor }.apply(function)
}

// ---------------------------------------------------------------------------
// Default combinator methods
// ---------------------------------------------------------------------------

impl dyn DensityFunction {
    /// `DensityFunction.clamp(double min, double max)`.
    pub fn clamp(&self, min: f64, max: f64) -> Arc<dyn DensityFunction> {
        Arc::new(density_functions::Clamp::new(self.clone_arc(), min, max))
    }

    /// `DensityFunction.abs()`.
    pub fn abs(&self) -> Arc<dyn DensityFunction> {
        density_functions::mapped(self, density_functions::MappedType::Abs)
    }

    /// `DensityFunction.square()`.
    pub fn square(&self) -> Arc<dyn DensityFunction> {
        density_functions::mapped(self, density_functions::MappedType::Square)
    }

    /// `DensityFunction.cube()`.
    pub fn cube(&self) -> Arc<dyn DensityFunction> {
        density_functions::mapped(self, density_functions::MappedType::Cube)
    }

    /// `DensityFunction.halfNegative()`.
    pub fn half_negative(&self) -> Arc<dyn DensityFunction> {
        density_functions::mapped(self, density_functions::MappedType::HalfNegative)
    }

    /// `DensityFunction.quarterNegative()`.
    pub fn quarter_negative(&self) -> Arc<dyn DensityFunction> {
        density_functions::mapped(self, density_functions::MappedType::QuarterNegative)
    }

    /// `DensityFunction.invert()`.
    pub fn invert(&self) -> Arc<dyn DensityFunction> {
        density_functions::mapped(self, density_functions::MappedType::Invert)
    }

    /// `DensityFunction.squeeze()`.
    pub fn squeeze(&self) -> Arc<dyn DensityFunction> {
        density_functions::mapped(self, density_functions::MappedType::Squeeze)
    }
}

// ---------------------------------------------------------------------------
// Nested types
// ---------------------------------------------------------------------------

/// `DensityFunction.ContextProvider` — per-index context + direct array fill.
///
/// Java's `forIndex(int)` returns the *owning* context (`NoiseChunk.this` for
/// both `NoiseChunk` and its `sliceFillingContextProvider`), so the inner
/// functions reached through the per-index fill paths take the interpolation
/// loop branch (`context != NoiseChunk.this` is false). The Rust trait returns
/// a `&dyn FunctionContext` borrow of the provider so the concrete
/// `NoiseChunk` identity survives — the `NoiseChunk` impls return `&self`, and
/// [`crate::levelgen::noisegen::noise_chunk`]'s `is_owning_chunk` recognizes
/// them by downcast + shared-state identity exactly like Java's reference
/// comparison.
pub trait ContextProvider {
    /// `forIndex(int index)` — the owning context for that cell index.
    fn for_index(&self, index: usize) -> &dyn FunctionContext;

    /// `fillAllDirectly(double[], DensityFunction)` — fills `output` by
    /// `compute`ing the function once per index (the `SimpleFunction` default
    /// delegates here).
    fn fill_all_directly(&self, output: &mut [f64], function: &dyn DensityFunction);
}

/// `DensityFunction.FunctionContext` — the block coordinates a function reads.
///
/// `Any` is a supertrait (like `DensityFunction`) because the noisegen unit's
/// `NoiseChunk` inner classes must distinguish their owning chunk from an
/// arbitrary context — Java's `context != NoiseChunk.this` reference-identity
/// check — by downcasting the context (`(context as &dyn Any).downcast_ref`).
/// `SinglePointContext` is `'static`, so the bound adds no new impl burden.
pub trait FunctionContext: Debug + Send + Sync + Any {
    /// `blockX()`.
    fn block_x(&self) -> i32;

    /// `blockY()`.
    fn block_y(&self) -> i32;

    /// `blockZ()`.
    fn block_z(&self) -> i32;
}

/// `DensityFunction.SinglePointContext(int blockX, int blockY, int blockZ)` —
/// the trivial `FunctionContext` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinglePointContext {
    block_x: i32,
    block_y: i32,
    block_z: i32,
}

impl SinglePointContext {
    /// `SinglePointContext(int, int, int)`.
    pub fn new(block_x: i32, block_y: i32, block_z: i32) -> Self {
        SinglePointContext {
            block_x,
            block_y,
            block_z,
        }
    }
}

impl FunctionContext for SinglePointContext {
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

/// `DensityFunction.Visitor` — the `mapChildren`/`mapAll` transformer.
pub trait Visitor: Send + Sync {
    /// `apply(DensityFunction)`.
    ///
    /// Receives the owned `&Arc` so an identity-keyed wrap cache can retain the
    /// key (Java's `HashMap<DensityFunction, DensityFunction>` keeps its keys
    /// strongly reachable; an address-keyed cache must too, or a freed
    /// intermediate's recycled address would spuriously hit a live entry).
    fn apply(&self, input: &Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction>;

    /// `visitNoise(NoiseHolder)` — default identity.
    fn visit_noise(&self, noise: &NoiseHolder) -> NoiseHolder {
        noise.clone()
    }
}

/// `DensityFunction.NoiseHolder(Holder<NoiseParameters>, @Nullable NormalNoise)`.
///
/// The value-layer codec decodes the holder into `noise == None` (Java's CODEC
/// xmaps to `new NoiseHolder(data, null)`); the noise is instantiated later by
/// the noisegen layer, so `get_value` returns `0.0` and `max_value` `2.0` for
/// an uninstantiated holder — exactly Java's null-noise behavior.
#[derive(Debug, Clone)]
pub struct NoiseHolder {
    noise_data: Holder<NoiseParameters>,
    noise: Option<NormalNoise>,
}

impl NoiseHolder {
    /// `NoiseHolder(Holder<NoiseParameters>)` — `noise == null`.
    pub fn new(noise_data: Holder<NoiseParameters>) -> Self {
        NoiseHolder {
            noise_data,
            noise: None,
        }
    }

    /// `NoiseHolder(Holder<NoiseParameters>, @Nullable NormalNoise)`.
    pub fn new_with_noise(noise_data: Holder<NoiseParameters>, noise: Option<NormalNoise>) -> Self {
        NoiseHolder { noise_data, noise }
    }

    /// `noiseData()` (record accessor).
    pub fn noise_data(&self) -> &Holder<NoiseParameters> {
        &self.noise_data
    }

    /// `noise()` (record accessor).
    pub fn noise(&self) -> Option<&NormalNoise> {
        self.noise.as_ref()
    }

    /// `getValue(double x, double y, double z)` — `noise == null ? 0.0 :
    /// noise.getValue(x, y, z)`.
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        match &self.noise {
            Some(noise) => noise.get_value(x, y, z),
            None => 0.0,
        }
    }

    /// `maxValue()` — `noise == null ? 2.0 : noise.maxValue()`.
    pub fn max_value(&self) -> f64 {
        match &self.noise {
            Some(noise) => noise.max_value(),
            None => 2.0,
        }
    }

    /// `NoiseHolder.CODEC` — `NormalNoise.NoiseParameters.CODEC.xmap(data ->
    /// new NoiseHolder(data, null), NoiseHolder::noiseData)`.
    pub fn codec<Ops>() -> Arc<dyn Codec<NoiseHolder, Ops>>
    where
        Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
    {
        let holder_codec = crate::levelgen::synth::normal_noise::noise_parameters_codec::<Ops>();
        codec::xmap(
            holder_codec,
            Arc::new(|data: &Holder<NoiseParameters>| NoiseHolder::new(data.clone())),
            Arc::new(|holder: &NoiseHolder| holder.noise_data.clone()),
        )
    }
}

// ---------------------------------------------------------------------------
// DensityFunction.CODEC
// ---------------------------------------------------------------------------

/// `DensityFunction.CODEC` — the recursive registry-file codec, as the
/// ops-generic `density_function_codec::<Ops>()` factory.
///
/// The recursion graph mirrors the static-initializer cycle: `CODEC` wraps
/// `DIRECT_CODEC` (either a constant double or the type dispatch), whose
/// per-function argument fields are themselves `DensityFunction.CODEC`. The
/// single `RecursiveSelf` (`top`) threads through the dispatch so arbitrary
/// nesting round-trips.
pub fn density_function_codec<Ops>() -> Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    codec::recursive(
        "DensityFunction".to_string(),
        Arc::new(|top: Arc<dyn Codec<Arc<dyn DensityFunction>, Ops>>| {
            // DIRECT_CODEC = either(NOISE_VALUE_CODEC, dispatch(top)).
            let direct = crate::levelgen::noise::density_functions::direct_codec(top);
            let key = crate::levelgen::noise::noises::noise_registry_key_for_density_function();
            let file = rivet_registry::registry_file_codec::RegistryFileCodec::create(key, direct);
            // xmap: Holder::Direct(v) -> v; Holder::Reference(r) -> HolderHolder(r);
            //       HolderHolder(h) -> h.function(); else Holder::direct(v).
            codec::xmap(
                Arc::new(file),
                Arc::new(
                    |holder: &Holder<Arc<dyn DensityFunction>>| -> Arc<dyn DensityFunction> {
                        match holder {
                            Holder::Direct(value) => value.clone(),
                            Holder::Reference { .. } => {
                                Arc::new(density_functions::HolderHolder::new(holder.clone()))
                            }
                        }
                    },
                ),
                Arc::new(
                    |value: &Arc<dyn DensityFunction>| -> Holder<Arc<dyn DensityFunction>> {
                        match value
                            .as_any()
                            .downcast_ref::<density_functions::HolderHolder>()
                        {
                            Some(holder_holder) => holder_holder.function().clone(),
                            _ => Holder::direct(value.clone()),
                        }
                    },
                ),
            )
        }),
    )
}
