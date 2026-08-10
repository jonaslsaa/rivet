//! Port of `com.mojang.serialization.OptionalDynamic`.
//!
//! `OptionalDynamic<T>` wraps a `DataResult<Dynamic<T>>` and forwards the
//! `DynamicLike` surface via `flatMap`. As with `Dynamic`, ops-dependent
//! methods take the ops as a parameter.

use crate::data_result::DataResult;
use crate::dynamic::Dynamic;
use crate::dynamic_ops::DynamicOps;
use crate::number::Number;

/// `com.mojang.serialization.OptionalDynamic<T>`.
#[derive(Debug, Clone)]
pub struct OptionalDynamic<O> {
    pub _ops: std::marker::PhantomData<O>,
    pub delegate: DataResult<Dynamic<O>>,
}

impl<O> OptionalDynamic<O> {
    /// `new OptionalDynamic<>(ops, DataResult<Dynamic<T>>)`.
    pub fn new(ops: &impl DynamicOps<Output = O>, delegate: DataResult<Dynamic<O>>) -> Self {
        let _ = ops;
        OptionalDynamic {
            _ops: std::marker::PhantomData,
            delegate,
        }
    }

    /// `OptionalDynamic.get()`.
    pub fn get(&self) -> &DataResult<Dynamic<O>> {
        &self.delegate
    }

    /// `OptionalDynamic.result()`.
    pub fn result(&self) -> Option<&Dynamic<O>> {
        self.delegate.result()
    }

    /// `OptionalDynamic.map(Function)`.
    pub fn map<U>(&self, mapper: impl FnOnce(&Dynamic<O>) -> U) -> DataResult<U>
    where
        O: Clone,
    {
        self.delegate.clone().map(mapper)
    }

    /// `OptionalDynamic.flatMap(Function)`.
    pub fn flat_map<U>(&self, mapper: impl FnOnce(Dynamic<O>) -> DataResult<U>) -> DataResult<U>
    where
        O: Clone,
    {
        self.delegate.clone().flat_map(mapper)
    }

    /// `OptionalDynamic.get(String)`.
    pub fn get_field(&self, ops: &impl DynamicOps<Output = O>, key: &str) -> OptionalDynamic<O>
    where
        O: Clone + std::fmt::Debug,
    {
        let delegate = self.delegate.clone().flat_map(move |k| {
            let inner = k.get(ops, key);
            inner.delegate
        });
        OptionalDynamic {
            _ops: std::marker::PhantomData,
            delegate,
        }
    }

    /// `OptionalDynamic.asNumber()` — Java forwards `DynamicLike::asNumber`
    /// via `flatMap`: `delegate.flatMap(DynamicLike::asNumber)`.
    pub fn as_number(&self, ops: &impl DynamicOps<Output = O>) -> DataResult<Number>
    where
        O: Clone,
    {
        self.flat_map(|d| d.as_number(ops))
    }

    /// `DynamicLike.asNumber(Number default)` — `asNumber().result().orElse(default)`.
    pub fn as_number_or(&self, ops: &impl DynamicOps<Output = O>, default: Number) -> Number
    where
        O: Clone,
    {
        self.as_number(ops).result().copied().unwrap_or(default)
    }

    /// `DynamicLike.asInt(int default)` — the default is a primitive `int`
    /// (autoboxed to `Integer` for the `asNumber(Number)` fallback), not a
    /// `Number` of arbitrary variant.
    pub fn as_int_or(&self, ops: &impl DynamicOps<Output = O>, default: i32) -> i32
    where
        O: Clone,
    {
        self.as_number_or(ops, Number::Int(default)).int_value()
    }

    /// `DynamicLike.asLong(long default)` — the default is a primitive `long`
    /// (autoboxed to `Long` for the `asNumber(Number)` fallback), not a
    /// `Number` of arbitrary variant.
    pub fn as_long_or(&self, ops: &impl DynamicOps<Output = O>, default: i64) -> i64
    where
        O: Clone,
    {
        self.as_number_or(ops, Number::Long(default)).long_value()
    }

    /// `DynamicLike.asBoolean(boolean default)` — `asBoolean().result().orElse(default)`.
    pub fn as_boolean_or(&self, ops: &impl DynamicOps<Output = O>, default: bool) -> bool
    where
        O: Clone,
    {
        self.flat_map(|d| d.as_boolean(ops))
            .result()
            .copied()
            .unwrap_or(default)
    }

    /// `DynamicLike.asString(String default)`.
    pub fn as_string_or(&self, ops: &impl DynamicOps<Output = O>, default: &str) -> String
    where
        O: Clone,
    {
        self.flat_map(|d| d.as_string(ops))
            .result()
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    /// `OptionalDynamic.orElseEmptyMap()` — `result().orElseGet(this::emptyMap)`
    /// where `emptyMap()` = `new Dynamic<>(ops, ops.emptyMap())` (the empty MAP,
    /// not the raw empty element).
    pub fn or_else_empty_map(&self, ops: &impl DynamicOps<Output = O>) -> Dynamic<O>
    where
        O: Clone,
    {
        match self.result() {
            Some(d) => d.clone(),
            None => Dynamic::new(ops, ops.empty_map()),
        }
    }

    /// `OptionalDynamic.orElseEmptyList()`.
    pub fn or_else_empty_list(&self, ops: &impl DynamicOps<Output = O>) -> Dynamic<O>
    where
        O: Clone,
    {
        match self.result() {
            Some(d) => d.clone(),
            None => Dynamic::new(ops, ops.empty_list()),
        }
    }

    /// `OptionalDynamic.decode(Decoder)`.
    pub fn decode<A, Ops: DynamicOps<Output = O> + 'static>(
        &self,
        ops: &Ops,
        decoder: &dyn crate::Decoder<A, Ops>,
    ) -> DataResult<(A, O)>
    where
        O: Clone,
    {
        self.delegate.clone().flat_map(|t| t.decode(ops, decoder))
    }
}
