//! Local port of `Codec.optionalFieldOf(String, F default)` — the with-default
//! form of a NON-lenient optional field.
//!
//! Java (DFU 10.0.21): `optionalField(name, codec, false).xmap(o ->
//! o.orElse(default), a -> Objects.equals(a, default) ? Optional.empty() :
//! Optional.of(a))`. The field value defaults on decode when absent, a
//! present-but-malformed value is a decode error (the field is NOT lenient),
//! and the field is OMITTED on encode when value-equal to `default`.
//!
//! This lives here (rather than in `rivet-serialization`, where the
//! `optional_field_of` helper is landing with the config-value-leaves wave)
//! because this unit may not add codecs to another crate: the exact DFU
//! semantics are replicated inline for the template rule tests' `min_chance` /
//! `max_chance` / `min_dist` / `max_dist` / `axis` fields.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use std::sync::Arc;

/// `Codec.optionalFieldOf(String, F default)` over an ops-generic element
/// codec — see the module doc.
pub fn defaulted_optional_field_of<F, Ops: DynamicOps + 'static>(
    name: &str,
    element_codec: Arc<dyn Codec<F, Ops>>,
    default: F,
) -> Arc<dyn MapCodec<F, Ops>>
where
    F: 'static + Clone + PartialEq + Send + Sync,
{
    let inner = codec::optional_field(name.to_string(), element_codec, false);
    let default_for_decode = default.clone();
    let default_for_encode = default;
    map_codec::xmap(
        inner,
        Arc::new(move |o: &Option<F>| o.clone().unwrap_or_else(|| default_for_decode.clone())),
        Arc::new(move |a: &F| {
            if *a == default_for_encode {
                None
            } else {
                Some(a.clone())
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::structure::templatesystem::codec_test_util;
    use serde_json::json;

    #[test]
    fn default_applies_when_absent_and_omitted_when_equal() {
        let map = defaulted_optional_field_of::<f32, rivet_serialization::json_ops::JsonOps>(
            "min_chance",
            codec::float_codec(),
            0.0,
        );
        // Absent decodes to the default; present decodes to the value.
        let codec = codec_test_util::codec(map);
        assert_eq!(codec_test_util::decode(&codec, &json!({})), 0.0);
        assert_eq!(
            codec_test_util::decode(&codec, &json!({"min_chance": 0.5})),
            0.5
        );
        // The default value is omitted on encode.
        assert_eq!(codec_test_util::encode(&codec, &0.0), json!({}));
        assert_eq!(
            codec_test_util::encode(&codec, &0.5),
            json!({"min_chance": 0.5})
        );
    }

    #[test]
    fn present_but_malformed_value_is_a_decode_error() {
        // Non-lenient: a present-but-malformed value errors rather than
        // falling back to the default.
        let map = defaulted_optional_field_of::<f32, rivet_serialization::json_ops::JsonOps>(
            "min_chance",
            codec::float_codec(),
            0.0,
        );
        let codec = codec_test_util::codec(map);
        assert!(codec_test_util::decode_result(&codec, &json!({"min_chance": "oops"})).is_error());
    }
}
