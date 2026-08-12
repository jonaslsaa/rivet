//! STUB(mc.util) — `net.minecraft.util.InclusiveRange` (record, 26.2).
//!
//! The full type is owned by the `mc.util` manifest unit; this module is a
//! minimal cross-unit stub scoped to what the block-state-providers unit needs:
//! `DualNoiseProvider.CODEC`'s `variety` field
//! (`InclusiveRange.codec(Codec.INT, 1, 64)`).
//!
//! Only the surface consumed here is ported: the two `i32` bounds, the
//! canonical-constructor validation (`min_inclusive must be less than or equal
//! to max_inclusive`), and the ops-generic `codec(elementCodec, minAllowed,
//! maxAllowed)` factory. The interval codec itself is NOT rebuilt here: it
//! delegates to the shared `rivet_serialization::extra_codecs::interval_codec`
//! (the DFU-exact `ExtraCodecs.intervalCodec`, merged with #557), which is then
//! `.validate`d to the allowed bounds with Paper's exact messages. The rest of
//! the record (`INT`, the `(T)` single-value constructor, `map`,
//! `isValueInRange`, `contains`, `toString`) is owned by `mc.util` and lands
//! with that unit.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::extra_codecs;
use std::sync::Arc;

/// `net.minecraft.util.InclusiveRange<Integer>` — an inclusive `[min, max]`
/// integer range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InclusiveRange {
    /// `InclusiveRange.minInclusive()`.
    pub min_inclusive: i32,
    /// `InclusiveRange.maxInclusive()`.
    pub max_inclusive: i32,
}

impl InclusiveRange {
    /// `new InclusiveRange(min, max)` — the canonical constructor, which throws
    /// `IllegalArgumentException("min_inclusive must be less than or equal to
    /// max_inclusive")` when `min > max` (Java's compact constructor; the Rust
    /// analog panics).
    pub fn new(min_inclusive: i32, max_inclusive: i32) -> InclusiveRange {
        if min_inclusive > max_inclusive {
            panic!("min_inclusive must be less than or equal to max_inclusive");
        }
        InclusiveRange {
            min_inclusive,
            max_inclusive,
        }
    }

    /// `InclusiveRange.create(T min, T max)` — the interval codec's
    /// `DataResult` factory: success when `min <= max`, else the error the
    /// `intervalCodec` surfaces.
    fn create(min_inclusive: i32, max_inclusive: i32) -> DataResult<InclusiveRange> {
        if min_inclusive <= max_inclusive {
            DataResult::success(InclusiveRange::new(min_inclusive, max_inclusive))
        } else {
            DataResult::error("min_inclusive must be less than or equal to max_inclusive")
        }
    }
}

/// `InclusiveRange.codec(Codec<T>, T minAllowedInclusive, T maxAllowedInclusive)`
/// — `intervalCodec(elementCodec, "min_inclusive", "max_inclusive", ...)`
/// validated to the allowed bounds.
///
/// The interval codec accepts the range three ways (the DFU `intervalCodec`
/// shape): a single point (`"min_inclusive": 1` → `[1, 1]`), a two-element
/// array (`[min, max]`), or an object (`{"min_inclusive": .., "max_inclusive":
/// ..}`). The unbounded form delegates to the shared
/// `extra_codecs::interval_codec` (the DFU-exact `ExtraCodecs.intervalCodec`,
/// merged with #557), and the bounded variant adds Paper's exact `.validate`
/// messages on top.
pub fn inclusive_range_codec<Ops: DynamicOps + 'static>(
    element_codec: Arc<dyn Codec<i32, Ops>>,
    min_allowed_inclusive: i32,
    max_allowed_inclusive: i32,
) -> Arc<dyn Codec<InclusiveRange, Ops>> {
    // `ExtraCodecs.intervalCodec(elementCodec, "min_inclusive", "max_inclusive",
    //  InclusiveRange::create, InclusiveRange::minInclusive,
    //  InclusiveRange::maxInclusive, Objects::equals)`.
    let unbounded = extra_codecs::interval_codec::<i32, InclusiveRange, Ops>(
        element_codec,
        "min_inclusive".to_string(),
        "max_inclusive".to_string(),
        Arc::new(|min: &i32, max: &i32| InclusiveRange::create(*min, *max)),
        Arc::new(|r: &InclusiveRange| r.min_inclusive),
        Arc::new(|r: &InclusiveRange| r.max_inclusive),
        Arc::new(|a: &i32, b: &i32| a == b),
    );
    // `.validate(...)` with the bound messages.
    codec::validate(
        unbounded,
        Arc::new(move |value: &InclusiveRange| {
            if value.min_inclusive < min_allowed_inclusive {
                DataResult::error(format!(
                    "Range limit too low, expected at least {} [{}-{}]",
                    min_allowed_inclusive, value.min_inclusive, value.max_inclusive
                ))
            } else if value.max_inclusive > max_allowed_inclusive {
                DataResult::error(format!(
                    "Range limit too high, expected at most {} [{}-{}]",
                    max_allowed_inclusive, value.min_inclusive, value.max_inclusive
                ))
            } else {
                DataResult::success(*value)
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn codec() -> Arc<dyn Codec<InclusiveRange, JsonOps>> {
        inclusive_range_codec::<JsonOps>(codec::int_codec(), 1, 64)
    }

    fn err(result: &DataResult<InclusiveRange>) -> String {
        result
            .error_ref()
            .unwrap_or_else(|| panic!("expected error, got {:?}", result.result()))
            .message()
            .to_string()
    }

    #[test]
    fn round_trips_object_form() {
        // The object form is decode-only: Java's `withAlternative(array, object)`
        // tries the ARRAY codec first on encode, so a non-point range always
        // re-encodes as `[min, max]` (the object is never emitted).
        let input = json!({"min_inclusive": 1, "max_inclusive": 3});
        let result = codec().parse(&JsonOps::INSTANCE, &input);
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, InclusiveRange::new(1, 3));
        let encoded = codec()
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!([1, 3]));
    }

    #[test]
    fn round_trips_array_form() {
        let result = codec().parse(&JsonOps::INSTANCE, &json!([2, 4]));
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, InclusiveRange::new(2, 4));
    }

    #[test]
    fn round_trips_point_form() {
        // `Codec.either(point, ...)`: a bare int decodes to `[n, n]`.
        let result = codec().parse(&JsonOps::INSTANCE, &json!(1));
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, InclusiveRange::new(1, 1));
        // A collapsed range re-encodes as the point form (`Objects.equals`).
        let encoded = codec()
            .encode_start(&JsonOps::INSTANCE, &InclusiveRange::new(1, 1))
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!(1));
    }

    #[test]
    fn rejects_min_greater_than_max() {
        // `[4, 2]` fails the point codec, then the array codec (`create`'s
        // `min_inclusive must be less than or equal to max_inclusive`), then the
        // object codec — the error surfaces through the full `either` wrap
        // chain, exactly as `ExtraCodecs.intervalCodec` reports it.
        let result = codec().parse(&JsonOps::INSTANCE, &json!([4, 2]));
        assert_eq!(
            err(&result),
            "Failed to parse either. First: Not a number: [4,2]; Second: Failed to parse either. First: min_inclusive must be less than or equal to max_inclusive; Second: Not a JSON object: [4,2]"
        );
    }

    #[test]
    fn rejects_out_of_bounds() {
        // Below the allowed minimum.
        let low = codec().parse(&JsonOps::INSTANCE, &json!([0, 2]));
        assert_eq!(err(&low), "Range limit too low, expected at least 1 [0-2]");
        // Above the allowed maximum.
        let high = codec().parse(&JsonOps::INSTANCE, &json!([1, 65]));
        assert_eq!(
            err(&high),
            "Range limit too high, expected at most 64 [1-65]"
        );
    }

    #[test]
    fn rejects_malformed_array_size() {
        let result = codec().parse(&JsonOps::INSTANCE, &json!([1, 2, 3]));
        assert_eq!(err(&result), "Input is not a list of 2 elements");
    }
}
