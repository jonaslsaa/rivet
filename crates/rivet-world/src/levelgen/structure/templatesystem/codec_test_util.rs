//! Shared test helpers for exercising the template-system rule-test `MapCodec`s
//! as full `Codec`s.
//!
//! A `MapCodec<A, Ops>` is a map-encoded decoder/encoder pair; the unit codecs
//! are ported as `Arc<dyn MapCodec<A, Ops>>`, and the tests below lift them to a
//! full `Codec` with `map_codec::codec_of` (the Rust analogue of Java's
//! `MapCodec.codec()`) and round-trip through `JsonOps` — the same pattern the
//! `blockpredicates` dispatch tests use (`encode_start` / `parse`). The
//! constructor is reached through the codec's ap machinery, so a panicking
//! constructor (e.g. `LinearPosTest`/`AxisAlignedLinearPosTest`'s range check)
//! propagates as a panic rather than a `DataResult` error — mirroring Java,
//! where the `IllegalArgumentException` escapes `codec.decode` instead of being
//! turned into a `DataResult.error`.

use rivet_serialization::codec::Codec;
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::map_codec::{self, MapCodec};
use serde_json::Value;
use std::sync::Arc;

/// Lift a `MapCodec<A, JsonOps>` to a full `Codec` (Java's `MapCodec.codec()`).
pub fn codec<A>(map: Arc<dyn MapCodec<A, JsonOps>>) -> Arc<dyn Codec<A, JsonOps>>
where
    A: 'static,
{
    map_codec::codec_of(map)
}

/// `map.encode(value).result()` — the encoded `JsonValue`.
pub fn encode<A>(codec: &Arc<dyn Codec<A, JsonOps>>, value: &A) -> Value {
    codec
        .encode_start(&JsonOps::INSTANCE, value)
        .result()
        .expect("encode should succeed")
        .clone()
}

/// `map.decode(input).result().getFirst()` — the decoded value. Use
/// [`decode_unwind`] when the decode is expected to panic (invalid ranges).
pub fn decode<A: Clone>(codec: &Arc<dyn Codec<A, JsonOps>>, input: &Value) -> A {
    codec
        .parse(&JsonOps::INSTANCE, input)
        .result()
        .expect("decode should succeed")
        .clone()
}

/// `map.decode(input)` under `catch_unwind` — the constructor's panic (Java's
/// `IllegalArgumentException` escaping `codec.decode`) surfaces as a panic.
pub fn decode_unwind<A>(codec: Arc<dyn Codec<A, JsonOps>>, input: Value)
where
    A: 'static,
{
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        codec.parse(&JsonOps::INSTANCE, &input)
    }));
    assert!(result.is_err(), "decode should have panicked");
}

/// `map.decode(input)` — returns the raw `DataResult` (for error-message
/// assertions on non-panicking decodes).
pub fn decode_result<A>(
    codec: &Arc<dyn Codec<A, JsonOps>>,
    input: &Value,
) -> rivet_serialization::data_result::DataResult<A> {
    codec.parse(&JsonOps::INSTANCE, input)
}
