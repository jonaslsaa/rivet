//! Java-grounded boundary tests for the typed `Number` semantics through
//! `NbtOps` (`net.minecraft.nbt.NbtOps`).

use crate::byte_tag::ByteTag;
use crate::compound_tag::CompoundTag;
use crate::double_tag::DoubleTag;
use crate::float_tag::FloatTag;
use crate::int_tag::IntTag;
use crate::long_tag::LongTag;
use crate::nbt_ops::NbtOps;
use crate::short_tag::ShortTag;
use crate::tag::Tag;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::number::Number;

fn ops() -> NbtOps {
    NbtOps::instance()
}

/// `NbtOps.getNumberValue` returns the typed boxed `Number` (`NumericTag.box()`).
#[test]
fn get_number_value_returns_typed_variants() {
    let o = ops();
    assert_eq!(
        o.get_number_value(&Tag::Byte(ByteTag::new(1)))
            .result()
            .copied(),
        Some(Number::Byte(1))
    );
    assert_eq!(
        o.get_number_value(&Tag::Short(ShortTag::new(2)))
            .result()
            .copied(),
        Some(Number::Short(2))
    );
    assert_eq!(
        o.get_number_value(&Tag::Int(IntTag::new(3)))
            .result()
            .copied(),
        Some(Number::Int(3))
    );
    assert_eq!(
        o.get_number_value(&Tag::Long(LongTag::new(4)))
            .result()
            .copied(),
        Some(Number::Long(4))
    );
    assert_eq!(
        o.get_number_value(&Tag::Float(FloatTag::new(5.5)))
            .result()
            .copied(),
        Some(Number::Float(5.5))
    );
    assert_eq!(
        o.get_number_value(&Tag::Double(DoubleTag::new(6.5)))
            .result()
            .copied(),
        Some(Number::Double(6.5))
    );
    // Non-numeric tags error.
    assert!(
        o.get_number_value(&Tag::Compound(CompoundTag::new()))
            .is_error()
    );
}

/// `NbtOps.createNumeric` returns a `DoubleTag.valueOf(n.doubleValue())`.
#[test]
fn create_numeric_yields_double_tag() {
    let o = ops();
    assert_eq!(
        o.create_numeric(Number::Int(7)),
        Tag::Double(DoubleTag::value_of(7.0))
    );
    assert_eq!(
        o.create_numeric(Number::Double(0.5)),
        Tag::Double(DoubleTag::value_of(0.5))
    );
    // 2^63-1 is representable as a double (rounded to 2^63), matching
    // `DoubleTag.valueOf((double)(Long.MAX_VALUE))`.
    assert_eq!(
        o.create_numeric(Number::Long(i64::MAX)),
        Tag::Double(DoubleTag::value_of(i64::MAX as f64))
    );
}

/// `Codec.LONG` reads a LongTag with full i64 precision via
/// `getNumberValue().map(Number::longValue)` — the f64 surface would have lost
/// the low bits above 2^53.
#[test]
fn long_codec_preserves_i64_precision_through_nbt() {
    let o = ops();
    let long = codec::long_codec::<NbtOps>();
    let cases = [
        i64::MIN,
        i64::MAX,
        (1i64 << 53) - 1,
        (1i64 << 53) + 1,
        9_000_000_000,
        -9_000_000_000,
    ];
    for value in cases {
        let tag = o.create_long(value);
        let decoded = *long.parse(&o, &tag).get_or_throw("parse");
        assert_eq!(decoded, value, "Codec.LONG did not round-trip {value}");
    }
}

/// `Codec.LONG` encode → `LongTag`, then `getNumberValue` still returns the
/// exact Long.
#[test]
fn long_codec_encode_round_trip() {
    let o = ops();
    let long = codec::long_codec::<NbtOps>();
    let encoded = long
        .encode_start(&o, &i64::MAX)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, Tag::Long(LongTag::new(i64::MAX)));
    let n = o.get_number_value(&encoded).result().copied().unwrap();
    assert_eq!(n, Number::Long(i64::MAX));
    assert_eq!(n.long_value(), i64::MAX);
}

/// Signed narrowing through the byte/short codecs over NBT numbers
/// (`Number.byteValue`/`shortValue` wrap via `(int)`).
#[test]
fn byte_and_short_codecs_narrow_through_nbt() {
    let o = ops();
    let byte = codec::byte_codec::<NbtOps>();
    // DoubleTag 300 → (int)300 → (byte)300 == 44.
    assert_eq!(
        byte.parse(&o, &Tag::Double(DoubleTag::new(300.0)))
            .get_or_throw("parse")
            .clone(),
        44i8
    );
    assert_eq!(
        byte.parse(&o, &Tag::Double(DoubleTag::new(-300.0)))
            .get_or_throw("parse")
            .clone(),
        -44i8
    );
    // LongTag 300 → (byte)300 == 44.
    assert_eq!(
        byte.parse(&o, &Tag::Long(LongTag::new(300)))
            .get_or_throw("parse")
            .clone(),
        44i8
    );
    let short = codec::short_codec::<NbtOps>();
    assert_eq!(
        short
            .parse(&o, &Tag::Int(IntTag::new(70_000)))
            .get_or_throw("parse")
            .clone(),
        (70_000i32 as i16)
    );
}

/// Float/double codecs read NBT numbers via `Number.floatValue`/`doubleValue`.
#[test]
fn float_double_codecs_round_trip_through_nbt() {
    let o = ops();
    let float = codec::float_codec::<NbtOps>();
    assert_eq!(
        float
            .parse(&o, &Tag::Float(FloatTag::new(1.5)))
            .get_or_throw("parse")
            .clone(),
        1.5f32
    );
    assert_eq!(
        float
            .parse(&o, &Tag::Double(DoubleTag::new(1.5)))
            .get_or_throw("parse")
            .clone(),
        1.5f32
    );
    let double = codec::double_codec::<NbtOps>();
    assert_eq!(
        double
            .parse(&o, &Tag::Int(IntTag::new(7)))
            .get_or_throw("parse")
            .clone(),
        7.0
    );
}

/// NBT preserves NaN/infinity (unlike JSON). `getNumberValue` returns them and
/// `createNumeric` stores them in a DoubleTag.
#[test]
fn nan_and_infinity_round_trip_through_nbt() {
    let o = ops();
    let double = codec::double_codec::<NbtOps>();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let tag = o.create_double(value);
        let decoded = *double.parse(&o, &tag).get_or_throw("parse");
        // Java `Double.compare` treats NaN == NaN; ordinary `==` is false for
        // NaN, so compare via bits.
        if value.is_nan() {
            assert!(decoded.is_nan());
        } else {
            assert_eq!(decoded, value);
        }
        let n = o.get_number_value(&tag).result().copied().unwrap();
        assert_eq!(n, Number::Double(value));
    }
}

/// `getBooleanValue` is `doubleValue() != 0.0` (`NbtOps.getBooleanValue`).
#[test]
fn get_boolean_value_uses_double_value_nonzero() {
    let o = ops();
    assert_eq!(
        o.get_boolean_value(&Tag::Byte(ByteTag::new(0)))
            .result()
            .copied(),
        Some(false)
    );
    assert_eq!(
        o.get_boolean_value(&Tag::Byte(ByteTag::new(1)))
            .result()
            .copied(),
        Some(true)
    );
    // A fractional DoubleTag is truthy.
    assert_eq!(
        o.get_boolean_value(&Tag::Double(DoubleTag::new(0.5)))
            .result()
            .copied(),
        Some(true)
    );
    assert_eq!(
        o.get_boolean_value(&Tag::Double(DoubleTag::new(-0.0)))
            .result()
            .copied(),
        Some(false)
    );
}
