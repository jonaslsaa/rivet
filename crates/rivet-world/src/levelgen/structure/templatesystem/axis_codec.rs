//! `Direction.Axis.CODEC` — the `StringRepresentable.fromEnum(Axis::values)`
//! codec, ported locally.
//!
//! `Axis` is a `StringRepresentable` enum in Java whose `CODEC` is
//! `StringRepresentable.fromEnum(Axis::values)`: an `orCompressed` of
//! `stringResolver(getSerializedName, createNameLookup(values))` and
//! `idResolverCodec(Enum::ordinal, fromInt, -1)`. In the port `Axis` lives in
//! `rivet-registry`, whose `Direction` codec (`direction_codec`) hand-builds
//! the same composition rather than implementing the `rivet-util`
//! `StringRepresentable`/`EnumOrdinal` traits; this unit mirrors that pattern
//! for `Axis`, building the enum codec from the same halves the ported
//! `string_representable::from_enum` composes: the string-resolver half (names
//! `"x"`/`"y"`/`"z"`) over the ordinal half (0/1/2), compressed via
//! `or_compressed`.

use rivet_registry::core::Axis;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::extra_codecs;
use std::sync::Arc;

/// `Axis.getSerializedName()` — `get_name()`.
fn axis_name(axis: &Axis) -> &'static str {
    axis.get_name()
}

/// `createNameLookup(Axis.values())` — `"x"` → `X`, `"y"` → `Y`, `"z"` → `Z`.
fn axis_by_name(name: &str) -> Option<Axis> {
    match name {
        "x" => Some(Axis::X),
        "y" => Some(Axis::Y),
        "z" => Some(Axis::Z),
        _ => None,
    }
}

/// `Axis.ordinal()` — declaration order 0, 1, 2.
fn axis_ordinal(axis: &Axis) -> i32 {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

/// `fromInt` — `i >= 0 && i < values.length ? values[i] : null`.
fn axis_from_int(i: i32) -> Option<Axis> {
    match i {
        0 => Some(Axis::X),
        1 => Some(Axis::Y),
        2 => Some(Axis::Z),
        _ => None,
    }
}

/// `Direction.Axis.CODEC` — `StringRepresentable.fromEnum(Axis::values)`, as
/// the ops-generic `axis_codec::<Ops>()` factory.
pub fn axis_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Axis, Ops>> {
    let string_part = codec::string_resolver::<Axis, Ops>(
        Arc::new(|a: &Axis| Some(axis_name(a).to_string())),
        Arc::new(|name: &String| axis_by_name(name)),
    );
    let id_part = extra_codecs::id_resolver_codec::<Axis, Ops>(
        Arc::new(axis_ordinal),
        Arc::new(axis_from_int),
        // `idResolverCodec(..., -1)`: the unknown-id sentinel.
        -1,
    );
    extra_codecs::or_compressed::<Axis, Ops>(string_part, id_part)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn axis_codec_round_trips_all_axes() {
        let codec = axis_codec::<JsonOps>();
        for (axis, name) in [(Axis::X, "x"), (Axis::Y, "y"), (Axis::Z, "z")] {
            let decoded = *codec
                .parse(&JsonOps::INSTANCE, &json!(name))
                .result()
                .expect("decode should succeed");
            assert_eq!(decoded, axis);
            let encoded = codec
                .encode_start(&JsonOps::INSTANCE, &axis)
                .result()
                .expect("encode should succeed")
                .clone();
            assert_eq!(encoded, json!(name));
        }
    }

    #[test]
    fn axis_codec_unknown_name_errors() {
        let codec = axis_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!("w"));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.contains("Unknown element name:"), "got: {msg}");
    }

    #[test]
    fn axis_codec_non_string_input_errors_under_uncompressed_ops() {
        // `JsonOps::INSTANCE` is uncompressed, so `orCompressed` routes decode
        // through the string half — a non-string input is a `JsonOps.getString`
        // error (`"Not a string: 5"`), exactly as Java's
        // `StringRepresentable.codec` behaves under `JsonOps`. The compressed
        // id half only fires for compressed ops (see
        // `axis_codec_compressed_ops_uses_the_id_half`).
        let codec = axis_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!(5));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Not a string: 5");
    }

    #[test]
    fn axis_codec_compressed_ops_uses_the_id_half() {
        // Under a compressed ops (`JsonOps::COMPRESSED`, `compressMaps()`), the
        // `orCompressed` codec routes through `idResolverCodec(Enum::ordinal,
        // fromInt, -1)`: decode by ordinal, encode by ordinal, and unknown ids
        // error with Java's exact `"Unknown element id: " + id` message.
        let codec = axis_codec::<JsonOps>();
        for (ordinal, axis) in [(0, Axis::X), (1, Axis::Y), (2, Axis::Z)] {
            let decoded = *codec
                .parse(&JsonOps::COMPRESSED, &json!(ordinal))
                .result()
                .expect("decode should succeed");
            assert_eq!(decoded, axis);
            let encoded = codec
                .encode_start(&JsonOps::COMPRESSED, &axis)
                .result()
                .expect("encode should succeed")
                .clone();
            assert_eq!(encoded, json!(ordinal));
        }
        let result = codec.parse(&JsonOps::COMPRESSED, &json!(5));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Unknown element id: 5");
    }
}
