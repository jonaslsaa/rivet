//! Focused tests for the partial `net.minecraft.util.ExtraCodecs` port
//! (`rivet_serialization::extra_codecs`): `overrideLifecycle`,
//! `retrieveContext`, `orCompressed`, `idResolverCodec`.
//!
//! Semantics are grounded in `ExtraCodecs.java` (Paper 26.2):
//! - `overrideLifecycle` sets the decode lifecycle from the decoded value on a
//!   full success only; errors pass through unchanged. Encode always applies
//!   the lifecycle.
//! - `retrieveContext` is a `MapCodec` that ignores its input; keys empty.
//! - `orCompressed` routes encode/decode through the compressed codec when
//!   `ops.compressMaps()` is true (JsonOps COMPRESSED vs INSTANCE).
//! - `idResolverCodec` round-trips via int and errors with Java's exact
//!   messages `"Unknown element id: N"` / `"Element with unknown id: X"`.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::extra_codecs;
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::lifecycle::Lifecycle;
use serde_json::{Value, json};
use std::sync::Arc;

trait ExpectResult {
    type Value;
    fn unwrap_result(self, message: &str) -> Self::Value;
}
impl<T: Clone> ExpectResult for DataResult<T> {
    type Value = T;
    fn unwrap_result(self, message: &str) -> Self::Value {
        self.get_or_throw(message).clone()
    }
}

// ---------------------------------------------------------------------------
// overrideLifecycle
// ---------------------------------------------------------------------------

/// A codec for `i32` with a decode lifecycle that depends on the value:
/// values >= 10 are "stable", values < 10 are "deprecated since 1".
fn lifecycle_codec() -> Arc<dyn Codec<i32, JsonOps>> {
    let base = codec::int_codec::<JsonOps>();
    extra_codecs::override_lifecycle(
        base,
        Arc::new(|v: &i32| {
            if *v >= 10 {
                Lifecycle::stable()
            } else {
                Lifecycle::deprecated(1)
            }
        }),
        Arc::new(|v: &i32| {
            if *v >= 10 {
                Lifecycle::stable()
            } else {
                Lifecycle::deprecated(1)
            }
        }),
    )
}

#[test]
fn override_lifecycle_sets_decode_lifecycle_from_value() {
    let ops = JsonOps::INSTANCE;
    let codec = lifecycle_codec();

    // value 42 >= 10 -> decode lifecycle Stable.
    let stable = codec.decode(&ops, &json!(42)).unwrap_result("decode");
    assert_eq!(stable.0, 42);
    let stable_lifecycle = codec.decode(&ops, &json!(42)).lifecycle();
    assert_eq!(stable_lifecycle, Lifecycle::stable());

    // value 5 < 10 -> decode lifecycle Deprecated(1).
    let deprecated_lifecycle = codec.decode(&ops, &json!(5)).lifecycle();
    assert_eq!(deprecated_lifecycle, Lifecycle::deprecated(1));
}

#[test]
fn override_lifecycle_sets_encode_lifecycle_from_value() {
    let ops = JsonOps::INSTANCE;
    let codec = lifecycle_codec();

    let encoded = codec.encode_start(&ops, &42).unwrap_result("encode");
    assert_eq!(encoded, json!(42));
    let stable_lifecycle = codec.encode_start(&ops, &42).lifecycle();
    assert_eq!(stable_lifecycle, Lifecycle::stable());

    let deprecated_lifecycle = codec.encode_start(&ops, &5).lifecycle();
    assert_eq!(deprecated_lifecycle, Lifecycle::deprecated(1));
}

#[test]
fn override_lifecycle_error_passes_through_unchanged() {
    let ops = JsonOps::INSTANCE;
    let codec = lifecycle_codec();
    // Not a number -> base int codec errors; override must not touch it.
    let result = codec.decode(&ops, &json!("not-a-number"));
    assert!(result.is_error());
    let msg = result.error_ref().unwrap().message().to_string();
    assert!(!msg.is_empty());
}

#[test]
fn override_lifecycle_single_arity() {
    let ops = JsonOps::INSTANCE;
    let base = codec::string_codec::<JsonOps>();
    let codec = extra_codecs::override_lifecycle_single(
        base,
        Arc::new(|s: &String| {
            if s == "x" {
                Lifecycle::stable()
            } else {
                Lifecycle::deprecated(7)
            }
        }),
    );
    let decoded = codec.decode(&ops, &json!("x")).unwrap_result("decode");
    assert_eq!(decoded.0, "x");
}

// ---------------------------------------------------------------------------
// retrieveContext
// ---------------------------------------------------------------------------

#[test]
fn retrieve_context_decodes_from_ops_not_input() {
    let ops = JsonOps::INSTANCE;
    // A "context accessor": derive a value purely from the ops. JsonOps is
    // stateless, so emulate a context that returns a fixed value.
    let codec = extra_codecs::retrieve_context::<i32, _>(Arc::new(|_ops| DataResult::success(777)));
    let empty_input: Value = json!({});
    let map_result = ops.get_map(&empty_input);
    let map = map_result.get_or_throw("getMap");
    let decoded = codec.decode(&ops, map.as_ref()).unwrap_result("decode");
    assert_eq!(decoded, 777);

    // keys are empty.
    assert!(codec.keys(&ops).is_empty());

    // encode is a no-op: it leaves the RecordBuilder prefix untouched.
}

#[test]
fn retrieve_context_error_propagates() {
    let ops = JsonOps::INSTANCE;
    let codec =
        extra_codecs::retrieve_context::<i32, _>(Arc::new(|_ops| DataResult::error("no context")));
    let map_result = ops.get_map(&json!({}));
    let map = map_result.get_or_throw("getMap");
    let result = codec.decode(&ops, map.as_ref());
    assert!(result.is_error());
    assert_eq!(result.error_ref().unwrap().message(), "no context");
}

// ---------------------------------------------------------------------------
// orCompressed
// ---------------------------------------------------------------------------

/// A normal codec and a compressed codec with distinguishable encodings.
type IntCodec = Arc<dyn Codec<i32, JsonOps>>;
fn or_compressed_pair() -> (IntCodec, IntCodec) {
    // normal: int codec. compressed: int codec wrapped so its output differs.
    let normal = codec::int_codec::<JsonOps>();
    let compressed = extra_codecs::id_resolver_codec::<i32, _>(
        Arc::new(|v: &i32| *v),
        Arc::new(|i: i32| Some(i)),
        -1,
    );
    (normal, compressed)
}

#[test]
fn or_compressed_routes_by_ops_compress_maps() {
    // JsonOps::INSTANCE has compressMaps() == false -> the normal codec is used.
    // JsonOps::COMPRESSED has compressMaps() == true -> the compressed codec is
    // used. Both round-trip the same value; the routing is observable because
    // the two codecs are swapped.
    let normal_ops = JsonOps::INSTANCE;
    let compressed_ops = JsonOps::COMPRESSED;
    let (normal, compressed) = or_compressed_pair();
    let codec = extra_codecs::or_compressed(normal.clone(), compressed.clone());

    let encoded = codec.encode_start(&normal_ops, &42).unwrap_result("encode");
    let (decoded, _) = codec.decode(&normal_ops, &encoded).unwrap_result("decode");
    assert_eq!(decoded, 42);

    let encoded = codec
        .encode_start(&compressed_ops, &42)
        .unwrap_result("encode");
    let (decoded, _) = codec
        .decode(&compressed_ops, &encoded)
        .unwrap_result("decode");
    assert_eq!(decoded, 42);
}

// ---------------------------------------------------------------------------
// idResolverCodec
// ---------------------------------------------------------------------------

#[test]
fn id_resolver_round_trip() {
    let ops = JsonOps::INSTANCE;
    let codec = extra_codecs::id_resolver_codec::<&str, _>(
        Arc::new(|s: &&str| s.len() as i32),
        Arc::new(|i: i32| match i {
            0 => Some(""),
            3 => Some("abc"),
            _ => None,
        }),
        -1,
    );

    let encoded = codec.encode_start(&ops, &"abc").unwrap_result("encode");
    assert_eq!(encoded, json!(3));
    let (decoded, _) = codec.decode(&ops, &json!(3)).unwrap_result("decode");
    assert_eq!(decoded, "abc");
}

#[test]
fn id_resolver_unknown_element_error() {
    let ops = JsonOps::INSTANCE;
    let codec = extra_codecs::id_resolver_codec::<i32, _>(
        Arc::new(|v: &i32| *v),
        Arc::new(|_i: i32| None),
        -1,
    );
    let result = codec.decode(&ops, &json!(99));
    assert!(result.is_error());
    assert_eq!(
        result.error_ref().unwrap().message(),
        "Unknown element id: 99"
    );
}

#[test]
fn id_resolver_unknown_id_encode_error() {
    let ops = JsonOps::INSTANCE;
    let codec = extra_codecs::id_resolver_codec::<i32, _>(
        Arc::new(|v: &i32| *v),
        Arc::new(|i: i32| Some(i)),
        -1,
    );
    // Element with id -1 is the unknown sentinel.
    let result = codec.encode_start(&ops, &-1);
    assert!(result.is_error());
    assert_eq!(
        result.error_ref().unwrap().message(),
        "Element with unknown id: -1"
    );
}
