//! Tests for `NbtOps.NbtRecordBuilder` — the port of
//! `AbstractStringBuilder<Tag, CompoundTag>` (`NbtOps.java:475-507`).
//!
//! Grounded in the Java/DFU 10.0.21 sources: the encode half of a
//! `RecordCodecBuilder`/`MapCodec` writes string-keyed fields into the
//! builder, and `build` merges them into the prefix (accepting a null/`EndTag`
//! prefix as "no prefix", shallow-copying and overlaying a `CompoundTag`
//! prefix, and rejecting any other prefix with the exact `mergeToMap called
//! with not a map: <prefix>` diagnostic). Errors and lifecycle accumulate in
//! the builder state and surface in the `build` `DataResult`.

use crate::compound_tag::CompoundTag;
use crate::end_tag::EndTag;
use crate::int_tag::IntTag;
use crate::nbt_ops::NbtOps;
use crate::string_tag::StringTag;
use crate::tag::Tag;
use rivet_serialization::DataResult;
use rivet_serialization::codec::int_codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::record_builder::RecordCodecBuilder;
use std::sync::Arc;

/// `Tag.toString()` via the SNBT visitor (matches Java `StringTagVisitor`).
fn snbt(tag: &Tag) -> String {
    crate::string_tag_visitor::StringTagVisitor::to_string(tag)
}

/// The `CompoundTag` produced by a successful `build`.
fn compound_of(result: DataResult<Tag>) -> CompoundTag {
    match result.result() {
        Some(Tag::Compound(c)) => c.clone(),
        other => panic!("expected a CompoundTag result, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Encode through RecordCodecBuilder / MapCodec
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct Point {
    x: i32,
    y: i32,
}

fn point_codec() -> Arc<dyn rivet_serialization::Codec<Point, NbtOps>> {
    rivet_serialization::record_builder::create::<Point, NbtOps>(move |instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|p: &Point| p.x),
                "x".to_string(),
                int_codec(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|p: &Point| p.y),
                "y".to_string(),
                int_codec(),
            ))
            .apply(instance, Arc::new(|x: i32, y: i32| Point { x, y }))
    })
}

#[test]
fn record_codec_encode_produces_compound() {
    let ops = NbtOps::instance();
    let codec = point_codec();
    // `Codec.encodeStart(ops, value)` builds via `NbtOps.map_builder()`
    // (the `NbtRecordBuilder`) with a `Tag::End` prefix, then `build` returns
    // the accumulated `CompoundTag`.
    let encoded = codec
        .encode_start(&ops, &Point { x: 1, y: 2 })
        .get_or_throw("encodeStart")
        .clone();
    let c = compound_of(DataResult::success(encoded));
    assert_eq!(c.get("x"), Some(&Tag::Int(IntTag::new(1))));
    assert_eq!(c.get("y"), Some(&Tag::Int(IntTag::new(2))));
    assert_eq!(c.size(), 2);
}

#[test]
fn record_codec_round_trip_through_nbt() {
    let ops = NbtOps::instance();
    let codec = point_codec();
    let input = Point { x: -5, y: 42 };
    let encoded = codec
        .encode_start(&ops, &input)
        .get_or_throw("encodeStart")
        .clone();
    let (decoded, _rest) = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
    assert_eq!(decoded, input);
    // The encoded form must be a CompoundTag (not End or a list).
    assert!(matches!(encoded, Tag::Compound(_)));
}

#[test]
fn record_codec_encode_with_end_tag_prefix_via_map_codec() {
    // `MapCodec.encodeStart(ops, value)` — the prefix is `ops.empty()` =
    // `EndTag.INSTANCE`, which `build` must treat as "no prefix".
    let ops = NbtOps::instance();
    let codec = point_codec();
    let encoded = codec
        .encode_start(&ops, &Point { x: 7, y: 8 })
        .get_or_throw("encodeStart")
        .clone();
    assert_eq!(
        snbt(&encoded),
        "{x:7,y:8}",
        "empty-prefix encode must not include a prefix"
    );
}

// ---------------------------------------------------------------------------
// Direct builder — prefix handling on build
// ---------------------------------------------------------------------------

#[test]
fn map_builder_build_accepts_null_and_end_prefix() {
    let ops = NbtOps::instance();

    // `build(null)` (Java `@Nullable`) — the accumulated compound.
    let mut b = ops.map_builder();
    b.add_string("k", Tag::Int(IntTag::new(1)));
    let c = compound_of(b.build(None));
    assert_eq!(c.get("k"), Some(&Tag::Int(IntTag::new(1))));

    // `build(EndTag.INSTANCE)` — treated identically to a null prefix.
    let mut b = ops.map_builder();
    b.add_string("k", Tag::Int(IntTag::new(2)));
    let c = compound_of(b.build(Some(Tag::End(EndTag))));
    assert_eq!(c.get("k"), Some(&Tag::Int(IntTag::new(2))));
}

#[test]
fn map_builder_build_shallow_copies_prefix_and_overlays() {
    let ops = NbtOps::instance();

    let mut prefix = CompoundTag::new();
    prefix.put("a".to_string(), Tag::Int(IntTag::new(1)));
    prefix.put("overlap".to_string(), Tag::Int(IntTag::new(99)));

    let mut b = ops.map_builder();
    b.add_string("overlap", Tag::Int(IntTag::new(2))); // builder wins
    b.add_string("b", Tag::String(StringTag::value_of("x".to_string())));

    let result = b.build(Some(Tag::Compound(prefix.clone())));
    let c = compound_of(result);

    // Prefix entries survive...
    assert_eq!(c.get("a"), Some(&Tag::Int(IntTag::new(1))));
    // ...and builder entries overlay on key collision.
    assert_eq!(c.get("overlap"), Some(&Tag::Int(IntTag::new(2))));
    assert_eq!(
        c.get("b"),
        Some(&Tag::String(StringTag::value_of("x".to_string())))
    );
    // The prefix itself must not be mutated (shallow copy).
    assert_eq!(prefix.get("overlap"), Some(&Tag::Int(IntTag::new(99))));
}

#[test]
fn map_builder_build_rejects_non_compound_prefix_with_exact_diagnostic() {
    let ops = NbtOps::instance();
    let mut b = ops.map_builder();
    b.add_string("k", Tag::Int(IntTag::new(1)));

    // Java: `DataResult.error(() -> "mergeToMap called with not a map: " +
    // prefix, prefix)`.
    let result = b.build(Some(Tag::Int(IntTag::new(7))));
    assert!(result.is_error());
    assert_eq!(
        result.error_ref().unwrap().message(),
        "mergeToMap called with not a map: 7"
    );
    // The offending prefix is carried as the partial value.
    assert_eq!(
        result.error_ref().unwrap().partial(),
        &Some(Tag::Int(IntTag::new(7)))
    );

    // A list prefix fails the same way.
    let mut b = ops.map_builder();
    b.add_string("k", Tag::Int(IntTag::new(1)));
    let result = b.build(Some(Tag::List(crate::list_tag::ListTag::new())));
    assert!(result.is_error());
    assert_eq!(
        result.error_ref().unwrap().message(),
        "mergeToMap called with not a map: []"
    );
}

// ---------------------------------------------------------------------------
// Direct builder — key resolution and error propagation
// ---------------------------------------------------------------------------

#[test]
fn map_builder_add_with_non_string_key_errors() {
    let ops = NbtOps::instance();
    let mut b = ops.map_builder();
    // Java `AbstractStringBuilder.add(T key, T value)`:
    // `ops().getStringValue(key).flatMap(...)` — a non-string key replaces the
    // builder state with `Not a string`.
    b.add(Tag::Int(IntTag::new(5)), Tag::Int(IntTag::new(1)));
    let result = b.build(None);
    assert!(result.is_error());
    assert_eq!(result.error_ref().unwrap().message(), "Not a string");
}

#[test]
fn map_builder_add_result_propagates_field_error() {
    let ops = NbtOps::instance();
    let mut b = ops.map_builder();
    b.add_result(
        Tag::String(StringTag::value_of("bad".to_string())),
        DataResult::error("field failed"),
    );
    let result = b.build(None);
    assert!(result.is_error());
    assert_eq!(result.error_ref().unwrap().message(), "field failed");

    // A later successful add on an already-failed builder keeps the error
    // (Java `builder.map(...)` maps the error's partial, keeping the message).
    let mut b = ops.map_builder();
    b.add_result(
        Tag::String(StringTag::value_of("bad".to_string())),
        DataResult::error("field failed"),
    );
    b.add_string("good", Tag::Int(IntTag::new(1)));
    let result = b.build(None);
    assert!(result.is_error());
    assert_eq!(result.error_ref().unwrap().message(), "field failed");
}

#[test]
fn map_builder_add_result_result_resolves_key() {
    let ops = NbtOps::instance();
    let mut b = ops.map_builder();
    b.add_result_result(
        DataResult::success(Tag::String(StringTag::value_of("k".to_string()))),
        DataResult::success(Tag::Int(IntTag::new(3))),
    );
    let c = compound_of(b.build(None));
    assert_eq!(c.get("k"), Some(&Tag::Int(IntTag::new(3))));

    // A non-string DataResult key fails resolution.
    let mut b = ops.map_builder();
    b.add_result_result(
        DataResult::success(Tag::Int(IntTag::new(9))),
        DataResult::success(Tag::Int(IntTag::new(3))),
    );
    let result = b.build(None);
    assert!(result.is_error());
    assert_eq!(result.error_ref().unwrap().message(), "Not a string");
}

// ---------------------------------------------------------------------------
// Lifecycle propagation
// ---------------------------------------------------------------------------

#[test]
fn map_builder_set_lifecycle_survives_build_as_experimental() {
    // Java `AbstractBuilder.build` is `builder.flatMap(b -> build(b, prefix))`;
    // `NbtRecordBuilder.build` returns `DataResult.success(builder)` (which
    // defaults to `Lifecycle.experimental()`), and `flatMap` combines that with
    // the accumulated builder lifecycle via `addLifecycle` — experimental wins.
    // So whatever lifecycle the encoder set, the build result is always
    // experimental (matching Java exactly).
    let ops = NbtOps::instance();
    let mut b = ops.map_builder();
    b.add_string("k", Tag::Int(IntTag::new(1)));
    b.set_lifecycle(Lifecycle::deprecated(123));
    let result = b.build(None);
    assert!(!result.is_error());
    assert_eq!(result.lifecycle(), Lifecycle::experimental());
}

#[test]
fn record_codec_deprecated_point_encode_yields_experimental_lifecycle() {
    // `RecordCodecBuilder.deprecated(instance, since)` encodes by calling
    // `prefix.set_lifecycle(deprecated)`, but the build result is wrapped in
    // `DataResult.success` (experimental) which wins in the `flatMap` lifecycle
    // join — so the encoded DataResult is experimental, not deprecated. This is
    // the exact Java `AbstractBuilder.build` behavior.
    let ops = NbtOps::instance();
    let codec: Arc<dyn rivet_serialization::Codec<i32, NbtOps>> =
        rivet_serialization::record_builder::create::<i32, NbtOps>(move |instance| {
            instance
                .group(RecordCodecBuilder::deprecated(42_i32, 123))
                .apply(instance, Arc::new(|v: i32| v))
        });
    let result = codec.encode_start(&ops, &42);
    assert!(!result.is_error());
    // The point codec writes no fields, so the build is an empty compound.
    assert_eq!(result.result(), Some(&Tag::Compound(CompoundTag::new())));
    assert_eq!(result.lifecycle(), Lifecycle::experimental());
}

#[test]
fn map_builder_map_error_rewrites_build_error() {
    let ops = NbtOps::instance();
    let mut b = ops.map_builder();
    b.add_result(
        Tag::String(StringTag::value_of("bad".to_string())),
        DataResult::error("field failed"),
    );
    // `AbstractBuilder.mapError(UnaryOperator<String>)`.
    b.map_error(Box::new(|m: String| format!("mapped: {m}")));
    let result = b.build(None);
    assert!(result.is_error());
    assert_eq!(
        result.error_ref().unwrap().message(),
        "mapped: field failed"
    );
}

#[test]
fn map_builder_resets_after_build() {
    // `AbstractBuilder.build` resets `builder` to a fresh init — a second
    // build from the same builder has no entries.
    let ops = NbtOps::instance();
    let mut b = ops.map_builder();
    b.add_string("k", Tag::Int(IntTag::new(1)));
    let first = b.build(None);
    assert_eq!(compound_of(first).get("k"), Some(&Tag::Int(IntTag::new(1))));

    let second = b.build(None);
    assert_eq!(compound_of(second).size(), 0);
}
