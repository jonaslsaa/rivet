//! Port of `com.mojang.serialization.Encoder`.
//!
//! Java `Encoder<A>` is generic over the `DynamicOps<T>` used at call time; the
//! Rust port pins the ops as a type parameter (`Encoder<A, Ops>`). The trait is
//! kept minimal (only the non-generic `encode`) so it is dyn-compatible and
//! codecs compose via `Arc<dyn Encoder<A, Ops>>`; Java's default combinators
//! (`comap`, `flatComap`, `fieldOf`, `withLifecycle`) are free functions.

use crate::data_result::DataResult;
use crate::dynamic_ops::DynamicOps;
use crate::map_encoder::MapEncoder;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.serialization.Encoder<A>`.
pub trait Encoder<A, Ops: DynamicOps + 'static>: Debug {
    /// `Encoder.encode(A input, DynamicOps<T> ops, T prefix)`.
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output>;

    /// `Encoder.encodeStart(DynamicOps<T> ops, A input)` —
    /// `encode(input, ops, ops.empty())`.
    fn encode_start(&self, ops: &Ops, input: &A) -> DataResult<Ops::Output> {
        self.encode(input, ops, &ops.empty())
    }
}

/// `Encoder.fieldOf(String)` — `new FieldEncoder<>(name, this)`.
pub fn field_of<A, Ops: DynamicOps + 'static>(
    name: String,
    element_codec: Arc<dyn Encoder<A, Ops>>,
) -> Arc<dyn MapEncoder<A, Ops>>
where
    A: 'static,
{
    crate::map_encoder::field_encoder(name, element_codec)
}

/// `Encoder.comap(Function)`.
pub fn comap<A, B, Ops: DynamicOps + 'static>(
    inner: Arc<dyn Encoder<A, Ops>>,
    function: Arc<dyn Fn(&B) -> A>,
) -> Arc<dyn Encoder<B, Ops>>
where
    A: 'static,
    B: 'static,
{
    Arc::new(ComappedEncoder { function, inner })
}

/// `Encoder.flatComap(Function)`.
pub fn flat_comap<A, B, Ops: DynamicOps + 'static>(
    inner: Arc<dyn Encoder<A, Ops>>,
    function: Arc<dyn Fn(&B) -> DataResult<A>>,
) -> Arc<dyn Encoder<B, Ops>>
where
    A: 'static,
    B: 'static,
{
    Arc::new(FlatComappedEncoder { function, inner })
}

/// `Encoder.withLifecycle(Lifecycle)`.
pub fn with_lifecycle<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn Encoder<A, Ops>>,
    lifecycle: crate::lifecycle::Lifecycle,
) -> Arc<dyn Encoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(WithLifecycleEncoder { lifecycle, inner })
}

/// `Encoder.empty()` — the `MapEncoder.empty()` as an `Encoder`.
pub fn empty<A, Ops: DynamicOps + 'static>() -> Arc<dyn Encoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(EmptyEncoder)
}

/// `Encoder.error(String)`.
pub fn error<A: Debug + 'static, Ops: DynamicOps + 'static>(
    error_message: String,
) -> Arc<dyn Encoder<A, Ops>> {
    Arc::new(ErrorEncoder {
        error: error_message,
    })
}

/// `Encoder.comap(Function)` result.
pub struct ComappedEncoder<B, A, Ops: DynamicOps + 'static> {
    function: Arc<dyn Fn(&B) -> A>,
    inner: Arc<dyn Encoder<A, Ops>>,
}
impl<B, A, Ops: DynamicOps + 'static> std::fmt::Debug for ComappedEncoder<B, A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ComappedEncoder")
    }
}

impl<B, A, Ops: DynamicOps + 'static> Encoder<B, Ops> for ComappedEncoder<B, A, Ops> {
    fn encode(&self, input: &B, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        self.inner.encode(&(self.function)(input), ops, prefix)
    }
}

/// `Encoder.flatComap(Function)` result.
pub struct FlatComappedEncoder<B, A, Ops: DynamicOps + 'static> {
    function: Arc<dyn Fn(&B) -> DataResult<A>>,
    inner: Arc<dyn Encoder<A, Ops>>,
}
impl<B, A, Ops: DynamicOps + 'static> std::fmt::Debug for FlatComappedEncoder<B, A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FlatComappedEncoder")
    }
}

impl<B, A, Ops: DynamicOps + 'static> Encoder<B, Ops> for FlatComappedEncoder<B, A, Ops> {
    fn encode(&self, input: &B, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        (self.function)(input).flat_map(|a| self.inner.encode(&a, ops, prefix))
    }
}

/// `Encoder.withLifecycle(Lifecycle)` result.
pub struct WithLifecycleEncoder<A, Ops: DynamicOps + 'static> {
    lifecycle: crate::lifecycle::Lifecycle,
    inner: Arc<dyn Encoder<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for WithLifecycleEncoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WithLifecycleEncoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Encoder<A, Ops> for WithLifecycleEncoder<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        self.inner
            .encode(input, ops, prefix)
            .set_lifecycle(self.lifecycle)
    }
}

/// `Encoder.empty()` result.
pub struct EmptyEncoder;
impl std::fmt::Debug for EmptyEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EmptyEncoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Encoder<A, Ops> for EmptyEncoder {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        let _ = (input, ops);
        DataResult::success(prefix.clone())
    }
}

/// `Encoder.error(String)` result.
pub struct ErrorEncoder {
    error: String,
}
impl std::fmt::Debug for ErrorEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ErrorEncoder")
    }
}

impl<A: Debug, Ops: DynamicOps + 'static> Encoder<A, Ops> for ErrorEncoder {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        let _ = (ops, prefix);
        DataResult::error(format!("{} {:?}", self.error, input))
    }
}
