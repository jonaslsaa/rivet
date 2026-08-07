//! Port of `com.mojang.serialization.codecs.XorCodec`.

use crate::codec::Codec;
use crate::data_result::DataResult;
use crate::dynamic_ops::DynamicOps;
use crate::either::Either;
use std::fmt::Debug;
use std::sync::Arc;

/// `XorCodec<F, S>` — `record XorCodec<F, S>(Codec<F> first, Codec<S> second)
/// implements Codec<Either<F, S>>`.
pub struct XorCodec<F, S, Ops: DynamicOps + 'static> {
    pub first: Arc<dyn Codec<F, Ops>>,
    pub second: Arc<dyn Codec<S, Ops>>,
}

impl<F, S, Ops: DynamicOps + 'static> crate::Decoder<Either<F, S>, Ops> for XorCodec<F, S, Ops>
where
    F: Clone + Debug + Send + Sync + 'static,
    S: Clone + Debug + Send + Sync + 'static,
{
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(Either<F, S>, Ops::Output)> {
        let first = self.first.clone();
        let second = self.second.clone();
        let first_read: DataResult<(Either<F, S>, Ops::Output)> = first
            .decode(ops, input)
            .map_owned(|vo| (Either::left(vo.0), vo.1));
        let second_read: DataResult<(Either<F, S>, Ops::Output)> = second
            .decode(ops, input)
            .map_owned(|vo| (Either::right(vo.0), vo.1));
        let first_result = first_read.result().cloned();
        let second_result = second_read.result().cloned();
        if let (Some(first), Some(second)) = (first_result.as_ref(), second_result.as_ref()) {
            return DataResult::error_with_partial(
                format!(
                    "Both alternatives read successfully, can not pick the correct one; first: {:?} second: {:?}",
                    first, second
                ),
                first.clone(),
            );
        }
        if first_result.is_some() {
            return first_read;
        }
        if second_result.is_some() {
            return second_read;
        }
        // `firstRead.apply2((f, s) -> s, secondRead)` — picks the second error,
        // accumulating messages and lifecycles.
        first_read.apply2(
            |_f: &(Either<F, S>, Ops::Output), s: &(Either<F, S>, Ops::Output)| s.clone(),
            second_read,
        )
    }
}

impl<F, S, Ops: DynamicOps + 'static> crate::Encoder<Either<F, S>, Ops> for XorCodec<F, S, Ops> {
    fn encode(
        &self,
        input: &Either<F, S>,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let first = self.first.clone();
        let second = self.second.clone();
        input.map_ref(
            |v| first.encode(v, ops, prefix),
            |v| second.encode(v, ops, prefix),
        )
    }
}

impl<F, S, Ops: DynamicOps + 'static> Codec<Either<F, S>, Ops> for XorCodec<F, S, Ops>
where
    F: Clone + Debug + Send + Sync + 'static,
    S: Clone + Debug + Send + Sync + 'static,
{
}

impl<F, S, Ops: DynamicOps + 'static> Debug for XorCodec<F, S, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "XorCodec[{:?}, {:?}]", self.first, self.second)
    }
}
