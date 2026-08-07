//! Port of `com.mojang.serialization.codecs.KeyDispatchCodec`.
//!
//! `KeyDispatchCodec<K, V>` is the `MapCodec<V>` behind `MapCodec.dispatchMap`
//! (and `Codec.dispatch`'s decode half). The discriminator key is decoded from
//! the map via `keyCodec` (typically `fieldOf(typeKey, Codec<K>)`), then the
//! value codec is looked up by that key and applied to the whole map. In
//! compressed mode the element is read from a `"value"` entry instead of the
//! map itself.

use crate::codec::Codec;
use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use crate::map_codec::{MapCodec, MapCodecEncoderHalf};
use crate::map_decoder::MapDecoder;
use crate::map_encoder::MapEncoder;
use std::fmt::Debug;
use std::sync::Arc;

/// `Function<V, DataResult<K>>` — the discriminator-key producer.
pub type TypeFn<K, V> = Arc<dyn Fn(&V) -> DataResult<K> + Send + Sync>;
/// `Function<K, DataResult<MapCodec<V>>>` — the key-to-codec lookup.
pub type CodecFn<K, V, Ops> =
    Arc<dyn Fn(&K) -> DataResult<Arc<dyn MapCodec<V, Ops>>> + Send + Sync>;
/// `Function<V, DataResult<MapEncoder<V>>>` — the encoder lookup.
pub type EncoderFn<V, Ops> =
    Arc<dyn Fn(&V) -> DataResult<Arc<dyn MapEncoder<V, Ops>>> + Send + Sync>;

/// `com.mojang.serialization.codecs.KeyDispatchCodec<K, V>`.
pub struct KeyDispatchCodec<K, V, Ops: DynamicOps + 'static> {
    pub key_codec: Arc<dyn MapCodec<K, Ops>>,
    pub type_fn: TypeFn<K, V>,
    pub codec_fn: CodecFn<K, V, Ops>,
    pub encoder_fn: EncoderFn<V, Ops>,
}

/// `Codec.dispatchMap(String typeKey, Function, Function)` — `fieldOf(typeKey)
/// .dispatchMap(type, codec)` with `keyCodec = Codec<K>` (Java
/// `fieldOf(typeKey)` over the `K` codec). `type` maps `V -> K`; `codec` maps
/// `K` to the `MapCodec<V>`.
pub fn dispatch_map<K, V, Ops: DynamicOps + 'static>(
    type_field_name: &str,
    key_codec: Arc<dyn Codec<K, Ops>>,
    type_fn: TypeFn<K, V>,
    codec_fn: CodecFn<K, V, Ops>,
) -> Arc<dyn MapCodec<V, Ops>>
where
    K: 'static + Clone,
    V: 'static,
{
    let field = crate::codec::field_of(key_codec, type_field_name.to_string());
    key_dispatch(field, type_fn, codec_fn)
}

/// `MapCodec.dispatchMap(Function, Function)` — the `KeyDispatchCodec` over an
/// already-built `MapCodec<K>` discriminator. The encoder half is derived from
/// `type`+`codec` exactly as Java's 3-arg constructor does.
pub fn key_dispatch<K, V, Ops: DynamicOps + 'static>(
    key_codec: Arc<dyn MapCodec<K, Ops>>,
    type_fn: TypeFn<K, V>,
    codec_fn: CodecFn<K, V, Ops>,
) -> Arc<dyn MapCodec<V, Ops>>
where
    K: 'static + Clone,
    V: 'static,
{
    let type_for_encoder = type_fn.clone();
    let codec_for_encoder = codec_fn.clone();
    let encoder_fn: EncoderFn<V, Ops> = Arc::new(move |input: &V| {
        let codec_for_encoder = codec_for_encoder.clone();
        type_for_encoder(input).flat_map(move |k| {
            codec_for_encoder(&k)
                .map(|c| Arc::new(MapCodecEncoderHalf(c.clone())) as Arc<dyn MapEncoder<V, Ops>>)
        })
    });
    Arc::new(KeyDispatchCodec {
        key_codec,
        type_fn,
        codec_fn,
        encoder_fn,
    })
}

impl<K: 'static + Clone, V: 'static, Ops: DynamicOps + 'static> Debug
    for KeyDispatchCodec<K, V, Ops>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeyDispatchCodec[{:?}]", self.key_codec)
    }
}

impl<K: 'static + Clone, V: 'static, Ops: DynamicOps + 'static> Keyable<Ops>
    for KeyDispatchCodec<K, V, Ops>
{
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.key_codec.keys(ops);
        keys.push(ops.create_string("value".to_string()));
        keys
    }
}

impl<K: 'static + Clone, V: 'static, Ops: DynamicOps + 'static> MapDecoder<V, Ops>
    for KeyDispatchCodec<K, V, Ops>
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<V> {
        self.key_codec.decode(ops, input).flat_map(|k| {
            (self.codec_fn)(&k).flat_map(|element_decoder| {
                if ops.compress_maps() {
                    match input.get(&ops.create_string("value".to_string())) {
                        None => DataResult::error(format!(
                            "Input does not have a \"value\" entry: {:?}",
                            input
                        )),
                        Some(value) => element_decoder.compressed_decode(ops, &value),
                    }
                } else {
                    element_decoder.decode(ops, input)
                }
            })
        })
    }
}

impl<K: 'static + Clone, V: 'static, Ops: DynamicOps + 'static> MapEncoder<V, Ops>
    for KeyDispatchCodec<K, V, Ops>
{
    fn encode(&self, input: &V, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        let encoder_result = (self.encoder_fn)(input);
        let type_result = (self.type_fn)(input);
        // Clone the resolved values out first, then thread the error state
        // through the builder by consuming the results.
        let element_encoder = encoder_result.result().cloned();
        let key = type_result.result().cloned();
        prefix.with_errors_from(&encoder_result.map(|_| ()));
        prefix.with_errors_from(&type_result.map(|_| ()));
        let (Some(element_encoder), Some(key)) = (element_encoder, key) else {
            return;
        };
        if ops.compress_maps() {
            self.key_codec.encode(&key, ops, prefix);
            // `MapEncoder.encodeStart`: encode into a fresh (compressed) builder
            // with the empty prefix.
            let value =
                crate::map_encoder::encoder(element_encoder.clone()).encode_start(ops, input);
            prefix.add_result(ops.create_string("value".to_string()), value);
        } else {
            // Encode key AFTER value. This is important for fixing types with
            // remainder, since it will contain old fields, including type.
            element_encoder.encode(input, ops, prefix);
            self.key_codec.encode(&key, ops, prefix);
        }
    }
}

impl<K: 'static + Clone, V: 'static, Ops: DynamicOps + 'static> MapCodec<V, Ops>
    for KeyDispatchCodec<K, V, Ops>
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<V> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(&self, input: &V, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        MapEncoder::encode(self, input, ops, prefix)
    }
}
