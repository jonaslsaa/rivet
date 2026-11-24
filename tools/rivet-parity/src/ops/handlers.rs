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
        _ => None,
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

/// `nbt.decode`: base64 -> `NbtIo.read(DataInput, NbtAccounter.create(16 MiB))`
/// with the op's size limits and trailing-byte rejection.
pub fn nbt_decode(request: &Map<String, Value>) -> Outcome {
    let input = match required_string(request, "input_base64") {
        Ok(input) => input,
        Err(outcome) => return outcome,
    };
    if input.len() > MAX_BASE64_CHARS {
        return Outcome::Rejected;
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(input) else {
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
