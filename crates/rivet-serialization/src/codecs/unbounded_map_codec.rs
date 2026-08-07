//! Port of `com.mojang.serialization.codecs.UnboundedMapCodec`.

use crate::codec::Codec;
use crate::codecs::simple_map_codec::BaseMapCodec;
use crate::data_result::DataResult;
use crate::dynamic_ops::DynamicOps;
use crate::lifecycle::Lifecycle;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

/// `UnboundedMapCodec<K, V>` — "Key and value decoded independently, unknown
/// set of keys".
pub struct UnboundedMapCodec<K, V, Ops: DynamicOps + 'static> {
    pub key_codec: Arc<dyn Codec<K, Ops>>,
    pub element_codec: Arc<dyn Codec<V, Ops>>,
}

impl<K, V, Ops: DynamicOps + 'static> BaseMapCodec<K, V, Ops> for UnboundedMapCodec<K, V, Ops> {
    fn key_codec(&self) -> &Arc<dyn Codec<K, Ops>> {
        &self.key_codec
    }
    fn element_codec(&self) -> &Arc<dyn Codec<V, Ops>> {
        &self.element_codec
    }
}

impl<K, V, Ops: DynamicOps + 'static> crate::Decoder<HashMap<K, V>, Ops>
    for UnboundedMapCodec<K, V, Ops>
where
    K: Clone + std::hash::Hash + Eq + std::fmt::Display + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(HashMap<K, V>, Ops::Output)> {
        // `ops.getMap(input).setLifecycle(stable()).flatMap(map -> decode(ops, map))
        //  .map(r -> Pair.of(r, input))`
        ops.get_map(input)
            .set_lifecycle(Lifecycle::stable())
            .flat_map(|map| self.decode_map(ops, map.as_ref()))
            .map_owned(|r| (r, input.clone()))
    }
}

impl<K, V, Ops: DynamicOps + 'static> crate::Encoder<HashMap<K, V>, Ops>
    for UnboundedMapCodec<K, V, Ops>
where
    K: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn encode(
        &self,
        input: &HashMap<K, V>,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let mut builder = ops.map_builder();
        self.encode_map(input, ops, &mut *builder);
        builder.build(Some(prefix.clone()))
    }
}

impl<K, V, Ops: DynamicOps + 'static> Codec<HashMap<K, V>, Ops> for UnboundedMapCodec<K, V, Ops>
where
    K: Clone + std::hash::Hash + Eq + std::fmt::Display + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
}

impl<K, V, Ops: DynamicOps + 'static> Debug for UnboundedMapCodec<K, V, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "UnboundedMapCodec[{:?} -> {:?}]",
            self.key_codec, self.element_codec
        )
    }
}
