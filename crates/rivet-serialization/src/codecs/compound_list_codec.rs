//! Port of `com.mojang.serialization.codecs.CompoundListCodec`.

use crate::codec::Codec;
use crate::data_result::DataResult;
use crate::dynamic_ops::DynamicOps;
use crate::lifecycle::Lifecycle;
use crate::pair::Pair;
use crate::unit::Unit;
use std::fmt::Debug;
use std::sync::Arc;

/// `CompoundListCodec<K, V>`.
pub struct CompoundListCodec<K, V, Ops: DynamicOps + 'static> {
    pub key_codec: Arc<dyn Codec<K, Ops>>,
    pub element_codec: Arc<dyn Codec<V, Ops>>,
}

impl<K, V, Ops: DynamicOps + 'static> crate::Decoder<Vec<Pair<K, V>>, Ops>
    for CompoundListCodec<K, V, Ops>
where
    K: Clone + 'static,
    V: Clone + 'static,
{
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(Vec<Pair<K, V>>, Ops::Output)> {
        // `ops.getMapEntries(input).flatMap(...)`.
        ops.get_map_entries(input).flat_map(|map| {
            let mut read: Vec<Pair<K, V>> = Vec::new();
            let mut failed: Vec<Pair<Ops::Output, Ops::Output>> = Vec::new();
            let mut result: DataResult<Unit> =
                DataResult::success_with_lifecycle(Unit, Lifecycle::experimental());

            let key_codec = self.key_codec.clone();
            let element_codec = self.element_codec.clone();

            let mut acc = |key: &Ops::Output, value: &Ops::Output| {
                let k = key_codec.parse(ops, key);
                let v = element_codec.parse(ops, value);
                // `k.apply2stable(Pair::new, v)`
                let read_entry: DataResult<(K, V)> =
                    DataResult::apply2_stable(k, |a, b| (a.clone(), b.clone()), v);

                if read_entry.error_ref().is_some() {
                    failed.push(Pair::of(key.clone(), value.clone()));
                }

                // `result.apply2stable((u, e) -> { read.add(e); return u; }, readEntry)`
                // — the entry is pushed here on success (the `apply2_stable`
                // function no longer needs to capture `read`).
                if let Some(e) = read_entry.clone().result_or_partial_silent() {
                    read.push(Pair::of(e.0, e.1));
                }
                let r = result.clone();
                result = r.apply2_stable(|_u: &Unit, _e: &(K, V)| Unit, read_entry);
            };
            map(&mut acc);

            let elements = read.clone();
            let errors = ops.create_map(failed);
            let pair = (elements, errors);
            result.map(|_| pair.clone()).set_partial(pair)
        })
    }
}

impl<K, V, Ops: DynamicOps + 'static> crate::Encoder<Vec<Pair<K, V>>, Ops>
    for CompoundListCodec<K, V, Ops>
where
    K: 'static,
    V: 'static,
{
    fn encode(
        &self,
        input: &Vec<Pair<K, V>>,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let mut builder = ops.map_builder();
        let key_codec = self.key_codec.clone();
        let element_codec = self.element_codec.clone();
        for pair in input {
            builder.add_result_result(
                key_codec.encode_start(ops, &pair.first),
                element_codec.encode_start(ops, &pair.second),
            );
        }
        builder.build(Some(prefix.clone()))
    }
}

impl<K, V, Ops: DynamicOps + 'static> Codec<Vec<Pair<K, V>>, Ops> for CompoundListCodec<K, V, Ops>
where
    K: Clone + 'static,
    V: Clone + 'static,
{
}

impl<K, V, Ops: DynamicOps + 'static> Debug for CompoundListCodec<K, V, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CompoundListCodec[{:?} -> {:?}]",
            self.key_codec, self.element_codec
        )
    }
}
