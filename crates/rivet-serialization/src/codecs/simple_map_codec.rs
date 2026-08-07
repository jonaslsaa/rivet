//! Port of `com.mojang.serialization.codecs.SimpleMapCodec` (via the shared
//! `BaseMapCodec`).

use crate::codec::Codec;
use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use crate::lifecycle::Lifecycle;
use crate::map_codec::MapCodec;
use crate::map_decoder::MapDecoder;
use crate::map_encoder::MapEncoder;
use crate::pair::Pair;
use crate::unit::Unit;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

/// `BaseMapCodec<K, V>` — the shared decode/encode implementation for
/// `SimpleMapCodec` and `UnboundedMapCodec`.
pub trait BaseMapCodec<K, V, Ops: DynamicOps + 'static> {
    fn key_codec(&self) -> &Arc<dyn Codec<K, Ops>>;
    fn element_codec(&self) -> &Arc<dyn Codec<V, Ops>>;

    /// `BaseMapCodec.decode` — accumulates entries, records duplicates and
    /// failed entries as `errors`, and maps the error to append " missed
    /// input: ".
    fn decode_map(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<HashMap<K, V>>
    where
        K: Clone + std::hash::Hash + Eq + std::fmt::Display + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
    {
        let mut read: HashMap<K, V> = HashMap::new();
        let mut failed: Vec<Pair<Ops::Output, Ops::Output>> = Vec::new();

        let mut result: DataResult<Unit> =
            DataResult::success_with_lifecycle(Unit, Lifecycle::stable());
        let key_codec = self.key_codec().clone();
        let element_codec = self.element_codec().clone();

        for pair in input.entries() {
            let key = key_codec.parse(ops, &pair.first);
            let value = element_codec.parse(ops, &pair.second);

            // `key.apply2stable(Pair::of, value)`
            let entry_result: DataResult<(K, V)> =
                DataResult::apply2_stable(key, |k: &K, v: &V| (k.clone(), v.clone()), value);

            if let Some(entry) = entry_result.clone().result_or_partial_silent() {
                let k = entry.0.clone();
                match read.entry(k.clone()) {
                    std::collections::hash_map::Entry::Occupied(_) => {
                        failed.push(pair.clone());
                        result = result.apply2_stable(
                            |_u: &Unit, _p: &Unit| Unit,
                            DataResult::error(format!("Duplicate entry for key: '{}'", k)),
                        );
                    }
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        slot.insert(entry.1.clone());
                    }
                }
            }
            if entry_result.is_error() {
                failed.push(pair.clone());
            }
            let r = result.clone();
            result = r.apply2_stable(|_u: &Unit, _p: &Unit| Unit, entry_result.map(|_| Unit));
        }

        let elements = read.clone();
        let errors = ops.create_map(failed);
        result
            .map(|_| elements.clone())
            .set_partial(elements)
            .map_error(|e| format!("{} missed input: {:?}", e, errors))
    }

    /// `BaseMapCodec.encode` — adds each entry's encoded key/value to the
    /// builder.
    fn encode_map(
        &self,
        input: &HashMap<K, V>,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) where
        K: Clone,
        V: Clone,
    {
        let key_codec = self.key_codec().clone();
        let element_codec = self.element_codec().clone();
        for (k, v) in input {
            prefix.add_result_result(
                key_codec.encode_start(ops, k),
                element_codec.encode_start(ops, v),
            );
        }
    }
}

/// `SimpleMapCodec<K, V>`.
pub struct SimpleMapCodec<K, V, Ops: DynamicOps + 'static> {
    pub key_codec: Arc<dyn Codec<K, Ops>>,
    pub element_codec: Arc<dyn Codec<V, Ops>>,
    pub keys: Arc<dyn Keyable<Ops>>,
}

impl<K, V, Ops: DynamicOps + 'static> BaseMapCodec<K, V, Ops> for SimpleMapCodec<K, V, Ops> {
    fn key_codec(&self) -> &Arc<dyn Codec<K, Ops>> {
        &self.key_codec
    }
    fn element_codec(&self) -> &Arc<dyn Codec<V, Ops>> {
        &self.element_codec
    }
}

impl<K, V, Ops: DynamicOps + 'static> Keyable<Ops> for SimpleMapCodec<K, V, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.keys.keys(ops)
    }
}

impl<K, V, Ops: DynamicOps + 'static> MapDecoder<HashMap<K, V>, Ops> for SimpleMapCodec<K, V, Ops>
where
    K: Clone + std::hash::Hash + Eq + std::fmt::Display + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<HashMap<K, V>> {
        self.decode_map(ops, input)
    }
}

impl<K, V, Ops: DynamicOps + 'static> MapEncoder<HashMap<K, V>, Ops> for SimpleMapCodec<K, V, Ops>
where
    K: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn encode(
        &self,
        input: &HashMap<K, V>,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        self.encode_map(input, ops, prefix)
    }
}

impl<K, V, Ops: DynamicOps + 'static> MapCodec<HashMap<K, V>, Ops> for SimpleMapCodec<K, V, Ops>
where
    K: Clone + std::hash::Hash + Eq + std::fmt::Display + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<HashMap<K, V>> {
        self.decode_map(ops, input)
    }

    fn encode(
        &self,
        input: &HashMap<K, V>,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        self.encode_map(input, ops, prefix)
    }
}

impl<K, V, Ops: DynamicOps + 'static> Debug for SimpleMapCodec<K, V, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SimpleMapCodec[{:?} -> {:?}]",
            self.key_codec, self.element_codec
        )
    }
}
