//! Rust twins of the reference oracle's ops (`RivetReferenceOracle` registry).
//!
//! Each handler takes the same request object the Java op takes and returns
//! the same canonical outcome: the op's result fields in the op's fixed key
//! order, or an input-rejection verdict. The handlers call the ported Rivet
//! code (`rivet-nbt`, `rivet-text`) — never a re-derivation of the answer —
//! and mirror only the op contract around it (request fields, size limits,
//! trailing-byte checks), which the Java op defines.
//!
//! `nbt.encode` is deliberately absent: its binary field order is the D12
//! accepted `compound_key_order` divergence, which a byte-equality comparator
//! cannot express. It stays on `rivet-parity`'s semantic comparator.

use std::io::Cursor;
use std::sync::Arc;

use base64::Engine as _;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io;
use rivet_serialization::codec::Codec;
use rivet_serialization::json_ops::JsonOps;
use rivet_text::component::Component;
use rivet_util::DataInputStream;
use serde_json::{Map, Value, json};

use super::compare::Outcome;

/// Mirrors `RivetReferenceOracle.MAX_NBT_BYTES`.
const MAX_NBT_BYTES: usize = 16 * 1024 * 1024;
/// Mirrors `RivetReferenceOracle.MAX_BASE64_CHARS`.
const MAX_BASE64_CHARS: usize = MAX_NBT_BYTES.div_ceil(3) * 4;

/// A Rust op handler: request object -> canonical outcome.
pub type Handler = fn(&Map<String, Value>) -> Outcome;

/// The Rust op registry, by op name.
pub fn handler(op: &str) -> Option<Handler> {
    match op {
        "snbt.parse" => Some(snbt_parse),
        "nbt.decode" => Some(nbt_decode),
        "component.json" => Some(component_json),
        "packet.reencode" => Some(super::packet::packet_reencode),
        "codec.reencode" => Some(super::codec::codec_reencode),
        "brigadier.dispatch" => Some(super::brigadier::brigadier_dispatch),
        "effect" => Some(super::gameplay::effect::effect),
        "enchant.select" => Some(super::gameplay::enchant::enchant_select),
        "loot.roll" => Some(super::gameplay::loot::loot_roll),
        "recipe.match" => Some(super::gameplay::recipe::recipe_match),
        "scoreboard.saved_data" => Some(super::gameplay::scoreboard::scoreboard_saved_data),
        "trade.offers" => Some(super::gameplay::trade::trade_offers),
        "damage.calc" => Some(super::entity::damage_calc),
        "damage_source.scaling" => Some(super::entity::damage_source_scaling),
        "food.exhaustion" => Some(super::entity::food_exhaustion),
        "xp.curves" => Some(super::entity::xp_curves),
        _ => None,
    }
}

/// The scoreboard subject of one corpus input: the ported Rust type it
/// exercises (a packet struct, a codec), for per-subject tallies.
pub fn subject(op: &str, request: &Map<String, Value>) -> Option<String> {
    match op {
        "packet.reencode" => super::packet::subject_of(request),
        "codec.reencode" => super::codec::subject_of(request),
        "brigadier.dispatch" => super::brigadier::subject_of(request),
        _ => super::gameplay::subject(op, request),
    }
}

fn required_string<'a>(request: &'a Map<String, Value>, key: &str) -> Result<&'a str, Outcome> {
    request
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Outcome::Unavailable(format!("request property '{key}' must be a string")))
}

fn describe(tag: &rivet_nbt::tag::Tag) -> Map<String, Value> {
    let described = crate::describe_tag(tag);
    let mut result = Map::new();
    result.insert("tag_id".into(), json!(described.id));
    result.insert("tag_type".into(), json!(described.type_name));
    result.insert("snbt".into(), json!(described.canonical));
    result.insert("pretty_snbt".into(), json!(described.pretty));
    result
}

/// `snbt.parse`: `TagParser.create(NbtOps).parseFully(input)`.
pub fn snbt_parse(request: &Map<String, Value>) -> Outcome {
    let input = match required_string(request, "input") {
        Ok(input) => input,
        Err(outcome) => return outcome,
    };
    match crate::rust_parse_snbt(input) {
        Ok(tag) => Outcome::Accepted(describe(&tag)),
        Err(_) => Outcome::Rejected,
    }
}

/// `Base64.getDecoder()`: RFC 4648 basic alphabet, padding optional but
/// validated when present, no whitespace tolerance, trailing bits ignored.
const JAVA_BASE64_DECODER: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    base64::engine::GeneralPurposeConfig::new()
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// `nbt.decode`: base64 -> `NbtIo.read(DataInput, NbtAccounter.create(16 MiB))`
/// with the op's size limits and trailing-byte rejection.
pub fn nbt_decode(request: &Map<String, Value>) -> Outcome {
    let input = match required_string(request, "input_base64") {
        Ok(input) => input,
        Err(outcome) => return outcome,
    };
    // Java compares `String.length()`, the UTF-16 length.
    if input.encode_utf16().count() > MAX_BASE64_CHARS {
        return Outcome::Rejected;
    }
    let Ok(bytes) = JAVA_BASE64_DECODER.decode(input) else {
        return Outcome::Rejected;
    };
    if bytes.len() > MAX_NBT_BYTES {
        return Outcome::Rejected;
    }
    let mut data = DataInputStream::new(Cursor::new(bytes.as_slice()));
    let compound = match nbt_io::read(&mut data, &mut NbtAccounter::create(MAX_NBT_BYTES as i64)) {
        Ok(compound) => compound,
        Err(_) => return Outcome::Rejected,
    };
    if data.into_inner().position() != bytes.len() as u64 {
        return Outcome::Rejected;
    }
    let mut result = describe(&rivet_nbt::tag::Tag::Compound(compound));
    result.insert("bytes".into(), json!(bytes.len()));
    Outcome::Accepted(result)
}

thread_local! {
    static COMPONENT_CODEC: Arc<dyn Codec<Component, JsonOps>> =
        rivet_text::component_serialization::codec();
}

/// `component.json`: `ComponentSerialization.CODEC` decode -> re-encode under
/// non-compressed `JsonOps`; a decline is the `accept:false` verdict.
pub fn component_json(request: &Map<String, Value>) -> Outcome {
    let input = match required_string(request, "input") {
        Ok(input) => input,
        Err(outcome) => return outcome,
    };
    let mut result = Map::new();
    let canonical = COMPONENT_CODEC
        .with(|codec| rivet_text::component_serialization::json_canonical(codec, input));
    match canonical {
        Ok(canonical) => {
            result.insert("accept".into(), json!(true));
            result.insert("canonical".into(), json!(canonical));
        }
        Err(_) => {
            result.insert("accept".into(), json!(false));
        }
    }
    Outcome::Accepted(result)
}

#[cfg(test)]
mod nbt_decode_base64_tests {
    use super::*;

    fn decode(input: &str) -> Outcome {
        let mut request = Map::new();
        request.insert("input_base64".into(), json!(input));
        nbt_decode(&request)
    }

    #[test]
    fn unpadded_input_is_accepted_like_java() {
        // Empty root compound: TAG_Compound, empty name, TAG_End.
        assert!(matches!(decode("CgAAAA"), Outcome::Accepted(_)));
        assert!(matches!(decode("CgAAAA=="), Outcome::Accepted(_)));
    }

    #[test]
    fn whitespace_and_bad_alphabet_are_rejected() {
        assert!(matches!(decode("CgAA AA"), Outcome::Rejected));
        assert!(matches!(decode("CgAA*A"), Outcome::Rejected));
    }
}
