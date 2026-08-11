//! Tests for the `KeyDispatchDataCodec` wrapper (issue #372).
//!
//! `KeyDispatchDataCodec<A>` is a plain value wrapper over a `MapCodec<A>`; in
//! Paper it is the per-type codec carried by `DensityFunction.codec()` /
//! surface-rule types, and the dispatch layer reads the wrapped `MapCodec`
//! back out of it. The port is a transparent newtype around
//! `Arc<dyn MapCodec<A, Ops>>`. These tests verify the wrap/accessor surface
//! and that the wrapped codec is what round-trips (the record wrapper adds no
//! behavior of its own).

mod common;

use rivet_serialization::codec as serde_codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::key_dispatch_data_codec::KeyDispatchDataCodec;
use rivet_serialization::record_builder::RecordCodecBuilder;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
struct Point {
    x: f32,
    y: f32,
}

fn point_codec<O: DynamicOps + 'static>() -> Arc<dyn rivet_serialization::MapCodec<Point, O>> {
    let x_builder = RecordCodecBuilder::of_named(
        Arc::new(|p: &Point| p.x),
        "x".to_string(),
        serde_codec::float_codec::<O>(),
    );
    let y_builder = RecordCodecBuilder::of_named(
        Arc::new(|p: &Point| p.y),
        "y".to_string(),
        serde_codec::float_codec::<O>(),
    );
    rivet_serialization::record_builder::map_codec(|instance| {
        instance
            .group(x_builder)
            .and(y_builder)
            .apply(instance, Arc::new(|x, y| Point { x, y }))
    })
}

#[test]
fn wraps_and_exposes_codec() {
    let kdc = KeyDispatchDataCodec::<Point, JsonOps>::of(point_codec());
    // The accessor returns the exact wrapped codec (record accessor `codec()`).
    let codec = kdc.codec();
    let ops = JsonOps::INSTANCE;
    let p = Point { x: 1.5, y: -2.25 };

    // `MapCodec` encode is `encode(input, ops, prefix)`; the Codec view
    // (`codec_of`) provides `encode_start`.
    let codec_view = rivet_serialization::map_codec::codec_of(codec);
    let encoded = codec_view
        .encode_start(&ops, &p)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, serde_json::json!({ "x": 1.5, "y": -2.25 }));

    let decoded: Point = codec_view
        .parse(&ops, &encoded)
        .get_or_throw("decode")
        .clone();
    assert_eq!(decoded, p);
}

#[test]
fn codec_view_round_trips_through_json() {
    for ops in [JsonOps::INSTANCE, JsonOps::COMPRESSED] {
        let kdc = KeyDispatchDataCodec::<Point, JsonOps>::of(point_codec());
        let codec = rivet_serialization::map_codec::codec_of(kdc.codec());
        let p = Point { x: -0.5, y: 7.75 };
        let encoded = codec.encode_start(&ops, &p).get_or_throw("encode").clone();
        let decoded: Point = codec.parse(&ops, &encoded).get_or_throw("decode").clone();
        assert_eq!(decoded, p, "ops compress={}", ops.compress_maps());
    }
}

#[test]
fn wrapped_map_codec_decodes_directly() {
    // The wrapped value is a `MapCodec`, usable directly on a MapLike input —
    // this is the DensityFunction dispatch usage (`KeyDispatchCodec` reads the
    // map codec out and applies it to the whole map).
    let kdc = KeyDispatchDataCodec::<Point, JsonOps>::of(point_codec());
    let codec = rivet_serialization::map_codec::codec_of(kdc.codec());
    let ops = JsonOps::INSTANCE;
    let encoded = serde_json::json!({ "x": 3.0, "y": 4.0 });
    let decoded: Point = codec.parse(&ops, &encoded).get_or_throw("decode").clone();
    assert_eq!(decoded, Point { x: 3.0, y: 4.0 });
}

#[test]
fn hostile_missing_field() {
    let kdc = KeyDispatchDataCodec::<Point, JsonOps>::of(point_codec());
    let codec = rivet_serialization::map_codec::codec_of(kdc.codec());
    let ops = JsonOps::INSTANCE;
    let bad = serde_json::json!({ "x": 1.0 });
    let decoded: DataResult<Point> = codec.parse(&ops, &bad);
    assert!(decoded.is_error());
    let msg = decoded.error_ref().unwrap().message().to_string();
    assert!(msg.contains("No key y"), "got: {msg}");
}
