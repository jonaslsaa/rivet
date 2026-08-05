//! Port of `com.mojang.serialization.codecs.PairCodec`.

use crate::codec::Codec;
use crate::data_result::DataResult;
use crate::dynamic_ops::DynamicOps;
use crate::pair::Pair;
use std::fmt::Debug;
use std::sync::Arc;

/// `PairCodec<F, S>` — `record PairCodec<F, S>(Codec<F> first, Codec<S> second)
/// implements Codec<Pair<F, S>>`.
pub struct PairCodec<F, S, Ops: DynamicOps + 'static> {
    pub first: Arc<dyn Codec<F, Ops>>,
    pub second: Arc<dyn Codec<S, Ops>>,
}

impl<F, S, Ops: DynamicOps + 'static> crate::Decoder<Pair<F, S>, Ops> for PairCodec<F, S, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(Pair<F, S>, Ops::Output)> {
        let first = self.first.clone();
        let second = self.second.clone();
        first.decode(ops, input).flat_map(move |p1| {
            let f = p1.0;
            second
                .decode(ops, &p1.1)
                .flat_map(move |p2| DataResult::success((Pair::of(f, p2.0), p2.1)))
        })
    }
}

impl<F, S, Ops: DynamicOps + 'static> crate::Encoder<Pair<F, S>, Ops> for PairCodec<F, S, Ops> {
    fn encode(&self, value: &Pair<F, S>, ops: &Ops, rest: &Ops::Output) -> DataResult<Ops::Output> {
        let second = self.second.clone();
        let first = self.first.clone();
        // Java: `second.encode(second, ops, rest).flatMap(f -> first.encode(first, ops, f))`.
        second
            .encode(&value.second, ops, rest)
            .flat_map(|f| first.encode(&value.first, ops, &f))
    }
}

impl<F, S, Ops: DynamicOps + 'static> Codec<Pair<F, S>, Ops> for PairCodec<F, S, Ops> {}

impl<F, S, Ops: DynamicOps + 'static> Debug for PairCodec<F, S, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PairCodec[{:?}, {:?}]", self.first, self.second)
    }
}
