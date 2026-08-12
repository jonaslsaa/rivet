//! Port of `com.mojang.serialization.codecs.PairMapCodec`.

use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use crate::map_codec::MapCodec;
use crate::pair::Pair;
use std::fmt::Debug;
use std::sync::Arc;

/// `PairMapCodec<F, S>` — `record PairMapCodec<F, S>(MapCodec<F> first,
/// MapCodec<S> second) implements MapCodec<Pair<F, S>>`.
///
/// Decode is sequential (`first.decode(ops, input).flatMap(...)` then
/// `second.decode(ops, input)`) — Java's `Applicative.super.ap2`, so errors do
/// not accumulate. Encode applies `second` first and `first` second into the
/// same builder (Java's `first.encode(pair.getFirst(), ops,
/// second.encode(pair.getSecond(), ops, prefix))`).
pub struct PairMapCodec<F, S, Ops: DynamicOps + 'static> {
    pub first: Arc<dyn MapCodec<F, Ops>>,
    pub second: Arc<dyn MapCodec<S, Ops>>,
}

impl<F, S, Ops: DynamicOps + 'static> Keyable<Ops> for PairMapCodec<F, S, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.first.keys(ops);
        keys.extend(self.second.keys(ops));
        keys
    }
}

impl<F, S, Ops: DynamicOps + 'static> MapCodec<Pair<F, S>, Ops> for PairMapCodec<F, S, Ops>
where
    F: Clone,
    S: Clone,
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<Pair<F, S>> {
        // Java `PairMapCodec.decode`:
        // `first.decode(ops, input).flatMap(p1 -> second.decode(ops, input).map(p2 -> Pair.of(p1, p2)))`.
        let first = self.first.clone();
        let second = self.second.clone();
        first.decode(ops, input).flat_map(move |p1| {
            second
                .decode(ops, input)
                .map_owned(move |p2| Pair::of(p1, p2))
        })
    }

    fn encode(
        &self,
        input: &Pair<F, S>,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        // Java `PairMapCodec.encode`:
        // `first.encode(input.getFirst(), ops, second.encode(input.getSecond(), ops, prefix))` —
        // the SECOND field is encoded first.
        let first = self.first.clone();
        let second = self.second.clone();
        let pair = input.clone();
        second.encode(&pair.second, ops, prefix);
        first.encode(&pair.first, ops, prefix);
    }
}

impl<F, S, Ops: DynamicOps + 'static> Debug for PairMapCodec<F, S, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PairMapCodec[{:?}, {:?}]", self.first, self.second)
    }
}
