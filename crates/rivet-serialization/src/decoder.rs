//! Port of `com.mojang.serialization.Decoder`.
//!
//! Java `Decoder<A>` is generic over the `DynamicOps<T>` used at call time; the
//! Rust port pins the ops as a type parameter (`Decoder<A, Ops>`). The trait is
//! kept minimal (only the non-generic `decode`) so it is dyn-compatible and
//! codecs compose via `Arc<dyn Decoder<A, Ops>>`; Java's default combinators
//! (`flatMap`, `map`, `fieldOf`, `withLifecycle`, `promotePartial`) are free
//! functions.

use crate::data_result::DataResult;
use crate::dynamic_ops::DynamicOps;
use crate::map_decoder::MapDecoder;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.serialization.Decoder<A>`.
pub trait Decoder<A, Ops: DynamicOps + 'static>: Debug {
    /// `Decoder.decode(DynamicOps<T> ops, T input)` — returns the value plus
    /// the unconsumed remainder (`Pair<A, T>`).
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)>;

    /// `Decoder.parse(DynamicOps<T> ops, T input)` — `decode(ops, input)
    /// .map(Pair::getFirst)`.
    fn parse(&self, ops: &Ops, input: &Ops::Output) -> DataResult<A> {
        self.decode(ops, input)
            .flat_map(|pair| DataResult::success(pair.0))
    }
}

/// `Decoder.fieldOf(String)` — `new FieldDecoder<>(name, this)`.
pub fn field_of<A, Ops: DynamicOps + 'static>(
    name: String,
    element_codec: Arc<dyn Decoder<A, Ops>>,
) -> Arc<dyn MapDecoder<A, Ops>>
where
    A: 'static,
{
    crate::map_decoder::field_decoder(name, element_codec)
}

/// `Decoder.flatMap(Function)` — Java's `Function<? super A, ? extends
/// DataResult<? extends B>>`; takes the value by reference.
pub fn flat_map<A, B, Ops: DynamicOps + 'static>(
    inner: Arc<dyn Decoder<A, Ops>>,
    function: Arc<dyn Fn(&A) -> DataResult<B>>,
) -> Arc<dyn Decoder<B, Ops>>
where
    A: 'static,
    B: 'static,
{
    Arc::new(FlatMappedDecoder { function, inner })
}

/// `Decoder.map(Function)` — Java's `Function<? super A, ? extends B>`; takes
/// the value by reference (matching `validate`'s checker shape).
pub fn map<A, B, Ops: DynamicOps + 'static>(
    inner: Arc<dyn Decoder<A, Ops>>,
    function: Arc<dyn Fn(&A) -> B>,
) -> Arc<dyn Decoder<B, Ops>>
where
    A: 'static,
    B: 'static,
{
    Arc::new(MappedDecoder { function, inner })
}

/// `Decoder.promotePartial(Consumer<String>)`.
pub fn promote_partial<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn Decoder<A, Ops>>,
    on_error: Arc<dyn Fn(&str)>,
) -> Arc<dyn Decoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(PromotePartialDecoder { on_error, inner })
}

/// `Decoder.withLifecycle(Lifecycle)`.
pub fn with_lifecycle<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn Decoder<A, Ops>>,
    lifecycle: crate::lifecycle::Lifecycle,
) -> Arc<dyn Decoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(WithLifecycleDecoder { lifecycle, inner })
}

/// `Decoder.unit(A)`.
pub fn unit<A: Clone + 'static, Ops: DynamicOps + 'static>(
    instance: A,
) -> Arc<dyn Decoder<A, Ops>> {
    unit_with(Arc::new(move || instance.clone()))
}

/// `Decoder.unit(Supplier<A>)`.
pub fn unit_with<A: 'static, Ops: DynamicOps + 'static>(
    instance: Arc<dyn Fn() -> A>,
) -> Arc<dyn Decoder<A, Ops>> {
    Arc::new(UnitDecoder {
        instance,
        _ops: std::marker::PhantomData,
    })
}

/// `Decoder.error(String)`.
pub fn error<A, Ops: DynamicOps + 'static>(error_message: String) -> Arc<dyn Decoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(ErrorDecoder {
        error: error_message,
    })
}

/// `Decoder.flatMap(Function)` result.
pub struct FlatMappedDecoder<A, B, Ops: DynamicOps + 'static> {
    function: Arc<dyn Fn(&A) -> DataResult<B>>,
    inner: Arc<dyn Decoder<A, Ops>>,
}
impl<A, B, Ops: DynamicOps + 'static> std::fmt::Debug for FlatMappedDecoder<A, B, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FlatMappedDecoder")
    }
}

impl<A, B, Ops: DynamicOps + 'static> Decoder<B, Ops> for FlatMappedDecoder<A, B, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(B, Ops::Output)> {
        self.inner
            .decode(ops, input)
            .flat_map(|p| (self.function)(&p.0).flat_map(|r| DataResult::success((r, p.1))))
    }
}

/// `Decoder.map(Function)` result.
pub struct MappedDecoder<A, B, Ops: DynamicOps + 'static> {
    function: Arc<dyn Fn(&A) -> B>,
    inner: Arc<dyn Decoder<A, Ops>>,
}
impl<A, B, Ops: DynamicOps + 'static> std::fmt::Debug for MappedDecoder<A, B, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MappedDecoder")
    }
}

impl<A, B, Ops: DynamicOps + 'static> Decoder<B, Ops> for MappedDecoder<A, B, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(B, Ops::Output)> {
        self.inner
            .decode(ops, input)
            .flat_map(|p| DataResult::success(((self.function)(&p.0), p.1)))
    }
}

/// `Decoder.promotePartial(Consumer<String>)` result.
pub struct PromotePartialDecoder<A, Ops: DynamicOps + 'static> {
    on_error: Arc<dyn Fn(&str)>,
    inner: Arc<dyn Decoder<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for PromotePartialDecoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PromotePartialDecoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Decoder<A, Ops> for PromotePartialDecoder<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        let on_error = self.on_error.clone();
        self.inner
            .decode(ops, input)
            .promote_partial(move |e| on_error(e))
    }
}

/// `Decoder.withLifecycle(Lifecycle)` result.
pub struct WithLifecycleDecoder<A, Ops: DynamicOps + 'static> {
    lifecycle: crate::lifecycle::Lifecycle,
    inner: Arc<dyn Decoder<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for WithLifecycleDecoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WithLifecycleDecoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Decoder<A, Ops> for WithLifecycleDecoder<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        self.inner.decode(ops, input).set_lifecycle(self.lifecycle)
    }
}

/// `Decoder.unit(Supplier)` result.
pub struct UnitDecoder<A, Ops: DynamicOps + 'static> {
    instance: Arc<dyn Fn() -> A>,
    _ops: std::marker::PhantomData<Ops>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for UnitDecoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UnitDecoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Decoder<A, Ops> for UnitDecoder<A, Ops> {
    fn decode(&self, _ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        DataResult::success(((self.instance)(), input.clone()))
    }
}

/// `Decoder.error(String)` result.
pub struct ErrorDecoder {
    error: String,
}
impl std::fmt::Debug for ErrorDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ErrorDecoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Decoder<A, Ops> for ErrorDecoder {
    fn decode(&self, _ops: &Ops, _input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        DataResult::error(self.error.clone())
    }
}
