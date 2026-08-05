//! Port of `com.mojang.serialization.OptionalDynamic`.
//!
//! `OptionalDynamic<T>` wraps a `DataResult<Dynamic<T>>` and forwards the
//! `DynamicLike` surface via `flatMap`. As with `Dynamic`, ops-dependent
//! methods take the ops as a parameter.

use crate::data_result::DataResult;
use crate::dynamic::Dynamic;
use crate::dynamic_ops::DynamicOps;
use crate::pair::Pair;

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
        O: Clone,
    {
        let ops = ops;
        let delegate = self.delegate.clone().flat_map(move |k| {
            let inner = k.get(ops, key);
            inner.delegate
        });
        OptionalDynamic {
            _ops: std::marker::PhantomData,
            delegate,
        }
    }

    /// `OptionalDynamic.orElseEmptyMap()`.
    pub fn or_else_empty_map(&self, ops: &impl DynamicOps<Output = O>) -> Dynamic<O>
    where
        O: Clone,
    {
        match self.result() {
            Some(d) => d.clone(),
            None => Dynamic::empty(ops),
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
