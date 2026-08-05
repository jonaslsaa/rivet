//! Port of `com.mojang.serialization.codecs.EitherCodec`.

use crate::codec::Codec;
use crate::data_result::DataResult;
use crate::dynamic_ops::DynamicOps;
use crate::either::Either;
use crate::pair::Pair;
use std::fmt::Debug;
use std::sync::Arc;

/// `EitherCodec<F, S>` — `record EitherCodec<F, S>(Codec<F> first, Codec<S>
/// second) implements Codec<Either<F, S>>`.
pub struct EitherCodec<F, S, Ops: DynamicOps + 'static> {
    pub first: Arc<dyn Codec<F, Ops>>,
    pub second: Arc<dyn Codec<S, Ops>>,
}

impl<F, S, Ops: DynamicOps + 'static> crate::Decoder<Either<F, S>, Ops> for EitherCodec<F, S, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(Either<F, S>, Ops::Output)> {
        let first = self.first.clone();
        let second = self.second.clone();
        let first_read: DataResult<(Either<F, S>, Ops::Output)> = first
            .decode(ops, input)
            .flat_map(|vo| DataResult::success((Either::left(vo.0), vo.1)));
        if first_read.is_success() {
            return first_read;
        }
        let second_read: DataResult<(Either<F, S>, Ops::Output)> = second
            .decode(ops, input)
            .flat_map(|vo| DataResult::success((Either::right(vo.0), vo.1)));
        if second_read.is_success() {
            return second_read;
        }
        if first_read.has_result_or_partial() {
            return first_read;
        }
        if second_read.has_result_or_partial() {
            return second_read;
        }
        let first_msg = first_read
            .error_ref()
            .map(|e| e.message().to_string())
            .unwrap_or_default();
        let second_msg = second_read
            .error_ref()
            .map(|e| e.message().to_string())
            .unwrap_or_default();
        DataResult::error(format!(
            "Failed to parse either. First: {}; Second: {}",
            first_msg, second_msg
        ))
    }
}

impl<F, S, Ops: DynamicOps + 'static> crate::Encoder<Either<F, S>, Ops> for EitherCodec<F, S, Ops> {
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

impl<F, S, Ops: DynamicOps + 'static> Codec<Either<F, S>, Ops> for EitherCodec<F, S, Ops> {}

impl<F, S, Ops: DynamicOps + 'static> Debug for EitherCodec<F, S, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EitherCodec[{:?}, {:?}]", self.first, self.second)
    }
}

pub(crate) type _Pair<F, S> = Pair<F, S>;
