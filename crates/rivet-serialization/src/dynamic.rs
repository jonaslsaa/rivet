//! Port of `com.mojang.serialization.Dynamic`.
//!
//! Java `Dynamic<T>` extends `DynamicLike<T>` and stores its `DynamicOps<T>`.
//! This port keeps a `pub value` field and no ops (`Dynamic<O>` is constructed
//! from a temporary ops reference by rivet-nbt), so ops-dependent methods here
//! take the ops as a parameter. `castTyped`/`getOps`/`equals` identity checks
//! against the stored ops are therefore dropped (no ops is stored); the
//! ops-dependent `convert`/`decode`/`read` methods take the ops explicitly.

use crate::data_result::DataResult;
use crate::dynamic_ops::DynamicOps;
use crate::pair::Pair;
use std::fmt::Debug;

/// `com.mojang.serialization.Dynamic<T>`.
#[derive(Debug, Clone, PartialEq)]
pub struct Dynamic<O> {
    /// Phantom: Java stores the `DynamicOps<T>`; rivet-nbt constructs
    /// `Dynamic<Tag>` from a temporary ops reference, so no ops is retained.
    pub _ops: std::marker::PhantomData<O>,
    /// The wrapped value.
    pub value: O,
}

impl<O> Dynamic<O> {
    /// `new Dynamic<>(ops, value)` — `null` maps to `ops.empty()`.
    pub fn new(ops: &impl DynamicOps<Output = O>, value: O) -> Self {
        let _ = ops;
        Dynamic {
            _ops: std::marker::PhantomData,
            value,
        }
    }

    /// `new Dynamic<>(ops)` — `new Dynamic<>(ops, ops.empty())`.
    pub fn empty(ops: &impl DynamicOps<Output = O>) -> Self
    where
        O: Clone,
    {
        Dynamic {
            _ops: std::marker::PhantomData,
            value: ops.empty(),
        }
    }

    /// `Dynamic.getValue()`.
    pub fn get_value(&self) -> &O {
        &self.value
    }

    /// `Dynamic.map(Function)`.
    pub fn map(&self, function: impl FnOnce(&O) -> O) -> Dynamic<O>
    where
        O: Clone,
    {
        Dynamic {
            _ops: std::marker::PhantomData,
            value: function(&self.value),
        }
    }

    /// `Dynamic.cast(DynamicOps<U>)` — `castTyped(ops).getValue()`. Identity
    /// between ops cannot be checked (no ops stored); the value is cast
    /// (unsafely in Java) to the target ops' element type.
    pub fn cast<U: DynamicOps<Output = O>>(&self, _ops: &U) -> &O {
        &self.value
    }

    /// `Dynamic.convert(DynamicOps<U>)` — `new Dynamic<>(outOps,
    /// convert(ops, outOps, value))`.
    pub fn convert<U: DynamicOps>(
        &self,
        in_ops: &impl DynamicOps<Output = O>,
        out_ops: &U,
    ) -> Dynamic<U::Output>
    where
        O: Clone,
        U::Output: Clone,
    {
        let value: U::Output =
            Dynamic::<O>::convert_value::<O, U::Output>(in_ops, out_ops, &self.value);
        Dynamic {
            _ops: std::marker::PhantomData,
            value,
        }
    }

    /// `Dynamic.convert(DynamicOps<S>, DynamicOps<T>, S)` — returns the input
    /// unchanged when the two ops are the same instance; otherwise
    /// `inOps.convertTo(outOps, input)`. Ops identity cannot be checked here;
    /// the caller passes whether the ops are the same.
    pub fn convert_value<S, T>(
        in_ops: &impl DynamicOps<Output = S>,
        out_ops: &impl DynamicOps<Output = T>,
        input: &S,
    ) -> T {
        in_ops.convert_to(out_ops, input)
    }

    /// `Dynamic.remove(String)`.
    pub fn remove(&self, ops: &impl DynamicOps<Output = O>, key: &str) -> Dynamic<O>
    where
        O: Clone,
    {
        self.map(|v| ops.remove(v.clone(), key))
    }

    /// `Dynamic.set(String, Dynamic<?>)` — `map(v -> ops.set(v, key, value.cast(ops)))`.
    pub fn set(
        &self,
        ops: &impl DynamicOps<Output = O>,
        key: &str,
        value: &Dynamic<O>,
    ) -> Dynamic<O>
    where
        O: Clone,
    {
        self.map(|v| ops.set(v, key, value.value.clone()))
    }

    /// `Dynamic.get(String)` — `new OptionalDynamic<>(ops, ops.getMap(value)
    /// .flatMap(m -> ...))`.
    ///
    /// Paper gates the `" in " + value` suffix behind
    /// `-DPaper.debugDynamicMissingKeys` (default false), so the default error
    /// is just `"key missing: {key}"`.
    pub fn get(&self, ops: &impl DynamicOps<Output = O>, key: &str) -> OptionalDynamic<O>
    where
        O: Clone + Debug,
    {
        let delegate = ops
            .get_map(&self.value)
            .flat_map(move |m| match m.get_string(key) {
                Some(v) => DataResult::success(Dynamic::new(ops, v)),
                // Paper default: `"key missing: " + key` (the value is only
                // appended with `Paper.debugDynamicMissingKeys` enabled).
                None => DataResult::error(format!("key missing: {}", key)),
            });
        OptionalDynamic {
            _ops: std::marker::PhantomData,
            delegate,
        }
    }

    /// `Dynamic.getMapValues()`.
    pub fn get_map_values(
        &self,
        ops: &impl DynamicOps<Output = O>,
    ) -> DataResult<Vec<Pair<Dynamic<O>, Dynamic<O>>>>
    where
        O: Clone,
    {
        ops.get_map_values(&self.value).map(|map| {
            map.iter()
                .map(|p| {
                    Pair::of(
                        Dynamic::new(ops, p.first.clone()),
                        Dynamic::new(ops, p.second.clone()),
                    )
                })
                .collect()
        })
    }

    /// `Dynamic.asStreamOpt()`.
    pub fn as_stream_opt(&self, ops: &impl DynamicOps<Output = O>) -> DataResult<Vec<Dynamic<O>>>
    where
        O: Clone,
    {
        ops.get_stream(&self.value)
            .map(|s| s.iter().map(|e| Dynamic::new(ops, e.clone())).collect())
    }

    /// `Dynamic.asMapOpt()`.
    pub fn as_map_opt(
        &self,
        ops: &impl DynamicOps<Output = O>,
    ) -> DataResult<Vec<Pair<Dynamic<O>, Dynamic<O>>>>
    where
        O: Clone,
    {
        ops.get_map_values(&self.value).map(|s| {
            s.iter()
                .map(|p| {
                    Pair::of(
                        Dynamic::new(ops, p.first.clone()),
                        Dynamic::new(ops, p.second.clone()),
                    )
                })
                .collect()
        })
    }

    /// `Dynamic.asNumber()` — the boxed `Number`.
    pub fn as_number(
        &self,
        ops: &impl DynamicOps<Output = O>,
    ) -> DataResult<crate::number::Number> {
        ops.get_number_value(&self.value)
    }

    /// `Dynamic.asString()`.
    pub fn as_string(&self, ops: &impl DynamicOps<Output = O>) -> DataResult<String> {
        ops.get_string_value(&self.value)
    }

    /// `Dynamic.asBoolean()`.
    pub fn as_boolean(&self, ops: &impl DynamicOps<Output = O>) -> DataResult<bool> {
        ops.get_boolean_value(&self.value)
    }

    /// `Dynamic.asByteBufferOpt()`.
    pub fn as_byte_buffer_opt(&self, ops: &impl DynamicOps<Output = O>) -> DataResult<Vec<u8>> {
        ops.get_byte_buffer(&self.value)
    }

    /// `Dynamic.asIntStreamOpt()`.
    pub fn as_int_stream_opt(&self, ops: &impl DynamicOps<Output = O>) -> DataResult<Vec<i32>> {
        ops.get_int_stream(&self.value)
    }

    /// `Dynamic.asLongStreamOpt()`.
    pub fn as_long_stream_opt(&self, ops: &impl DynamicOps<Output = O>) -> DataResult<Vec<i64>> {
        ops.get_long_stream(&self.value)
    }

    /// `Dynamic.decode(Decoder<? extends A>)`.
    pub fn decode<A, Ops: DynamicOps<Output = O> + 'static>(
        &self,
        ops: &Ops,
        decoder: &dyn crate::Decoder<A, Ops>,
    ) -> DataResult<(A, O)> {
        decoder.decode(ops, &self.value)
    }
}

use crate::optional_dynamic::OptionalDynamic;
