//! Port of `net.minecraft.util.ExtraCodecs` — the shared validation codecs
//! used by worldgen/config and chat serialization.
//!
//! PROVENANCE: `ExtraCodecs.POSITIVE_INT` lives here so the worldgen
//! configuration leaves (`TwistingVinesConfig`, `mc.world.level.levelgen.feature
//! .configurations`) can use it without a `rivet-text` dependency. The
//! `rivet-text` slice of `ExtraCodecs` (`chat_string`, `untrusted_uri`, …) keeps
//! its own module in `rivet-text/src/extra_codecs.rs` and delegates here for the
//! codecs it shares with `rivet-util`.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use std::sync::Arc;

/// `ExtraCodecs.POSITIVE_INT` — `Codec.INT` validated to `[1, MAX]` with the
/// Java-exact message `"Value must be positive: {n}"` (Java's
/// `intRangeWithMessage(1, Integer.MAX_VALUE, ...)`).
pub fn positive_int<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i32, Ops>> {
    codec::validate(
        codec::int_codec(),
        Arc::new(|value: &i32| {
            if *value >= 1 {
                DataResult::success(*value)
            } else {
                DataResult::error(format!("Value must be positive: {}", value))
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;

    /// The exact error message of a failed decode (Java's `error().message()`),
    /// or a panic on success.
    fn error_message<T: std::fmt::Debug>(result: &DataResult<T>) -> String {
        result
            .error_ref()
            .unwrap_or_else(|| panic!("expected an error, got {:?}", result.result()))
            .message()
            .to_string()
    }

    #[test]
    fn positive_int_round_trips_positive_values() {
        let codec = positive_int::<JsonOps>();
        let input = JsonOps::INSTANCE.create_int(7);
        let result = codec.decode(&JsonOps::INSTANCE, &input);
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(decoded.0, 7);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &7)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn positive_int_rejects_zero_and_negative() {
        let codec = positive_int::<JsonOps>();
        let zero = codec.decode(&JsonOps::INSTANCE, &JsonOps::INSTANCE.create_int(0));
        assert_eq!(error_message(&zero), "Value must be positive: 0");
        let negative = codec.decode(&JsonOps::INSTANCE, &JsonOps::INSTANCE.create_int(-5));
        assert_eq!(error_message(&negative), "Value must be positive: -5");
    }

    #[test]
    fn positive_int_rejects_on_encode() {
        // Java's `intRangeWithMessage` validates on both decode and encode
        // (flatXMap); a non-positive value fails encode with the same message.
        let codec = positive_int::<JsonOps>();
        let result = codec.encode_start(&JsonOps::INSTANCE, &0);
        assert_eq!(error_message(&result), "Value must be positive: 0");
    }
}
