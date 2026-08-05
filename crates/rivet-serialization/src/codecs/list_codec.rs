//! Port of `com.mojang.serialization.codecs.ListCodec`.

use crate::codec::Codec;
use crate::data_result::DataResult;
use crate::dynamic_ops::DynamicOps;
use crate::lifecycle::Lifecycle;
use crate::unit::Unit;
use std::fmt::Debug;
use std::sync::Arc;

/// `ListCodec<E>` — `record ListCodec<E>(Codec<E> elementCodec, int minSize,
/// int maxSize) implements Codec<List<E>>`.
pub struct ListCodec<E, Ops: DynamicOps + 'static> {
    pub element_codec: Arc<dyn Codec<E, Ops>>,
    pub min_size: i32,
    pub max_size: i32,
}

impl<E, Ops: DynamicOps + 'static> ListCodec<E, Ops> {
    /// `createTooShortError(int)`.
    fn create_too_short_error<R>(&self, size: i32) -> DataResult<R> {
        DataResult::error(format!(
            "List is too short: {}, expected range [{}-{}]",
            size, self.min_size, self.max_size
        ))
    }

    /// `createTooLongError(int)`.
    fn create_too_long_error<R>(&self, size: i32) -> DataResult<R> {
        DataResult::error(format!(
            "List is too long: {}, expected range [{}-{}]",
            size, self.min_size, self.max_size
        ))
    }
}

impl<E, Ops: DynamicOps + 'static> crate::Encoder<Vec<E>, Ops> for ListCodec<E, Ops> {
    fn encode(&self, input: &Vec<E>, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        let size = input.len() as i32;
        if size < self.min_size {
            return self.create_too_short_error(size);
        }
        if size > self.max_size {
            return self.create_too_long_error(size);
        }
        let mut builder = ops.list_builder();
        for element in input {
            builder.add_result(self.element_codec.encode_start(ops, element));
        }
        builder.build(prefix.clone())
    }
}

impl<E, Ops: DynamicOps + 'static> crate::Decoder<Vec<E>, Ops> for ListCodec<E, Ops>
where
    E: Clone,
{
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(Vec<E>, Ops::Output)> {
        // Java: `ops.getList(input).setLifecycle(stable()).flatMap(...)`.
        let list = ops.get_list(input).set_lifecycle(Lifecycle::stable());
        list.flat_map(|stream| {
            let mut state = DecoderState {
                element_codec: self.element_codec.clone(),
                ops,
                min_size: self.min_size,
                max_size: self.max_size,
                elements: Vec::new(),
                failed: Vec::new(),
                result: DataResult::success_with_lifecycle(Unit, Lifecycle::stable()),
                total_count: 0,
            };
            let mut acc = |value: &Ops::Output| state.accept(value.clone());
            stream(&mut acc);
            state.build()
        })
    }
}

impl<E, Ops: DynamicOps + 'static> Codec<Vec<E>, Ops> for ListCodec<E, Ops> where E: Clone {}

impl<E, Ops: DynamicOps + 'static> Debug for ListCodec<E, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ListCodec[{:?}]", self.element_codec)
    }
}

/// `ListCodec.DecoderState<T>` — the private inner class that accumulates
/// decoded elements and failed values, propagating errors via
/// `DataResult.apply2stable`.
struct DecoderState<'a, E, Ops: DynamicOps + 'static> {
    element_codec: Arc<dyn Codec<E, Ops>>,
    ops: &'a Ops,
    min_size: i32,
    max_size: i32,
    elements: Vec<E>,
    failed: Vec<Ops::Output>,
    result: DataResult<Unit>,
    total_count: i32,
}

impl<'a, E, Ops: DynamicOps + 'static> DecoderState<'a, E, Ops>
where
    E: Clone,
{
    /// `DecoderState.accept(T value)`.
    fn accept(&mut self, value: Ops::Output) {
        self.total_count += 1;
        if (self.elements.len() as i32) >= self.max_size {
            self.failed.push(value);
            return;
        }
        let element_result = self.element_codec.decode(self.ops, &value);
        if element_result.error_ref().is_some() {
            self.failed.push(value.clone());
        }
        if let Some(pair) = element_result.clone().result_or_partial_silent() {
            self.elements.push(pair.0);
        }
        // `result.apply2stable((result, element) -> result, elementResult)`
        let r = self.result.clone();
        self.result = r.apply2_stable(|_result, _element| Unit, element_result.map(|_| Unit));
    }

    /// `DecoderState.build()`.
    fn build(&mut self) -> DataResult<(Vec<E>, Ops::Output)>
    where
        E: Clone,
    {
        if (self.elements.len() as i32) < self.min_size {
            return DataResult::error(format!(
                "List is too short: {}, expected range [{}-{}]",
                self.elements.len(),
                self.min_size,
                self.max_size
            ));
        }
        let errors = self.ops.create_list(self.failed.clone());
        let pair = (self.elements.clone(), errors);
        if self.total_count > self.max_size {
            self.result = DataResult::error(format!(
                "List is too long: {}, expected range [{}-{}]",
                self.total_count, self.min_size, self.max_size
            ));
        }
        self.result.clone().map(|_| pair.clone()).set_partial(pair)
    }
}
