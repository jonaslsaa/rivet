//! Fuzz target: the DFU codec combinators (`rivet-serialization`) decoding
//! untrusted `Tag` values over `NbtOps`.
//!
//! Input bytes are parsed as SNBT (when valid) to obtain a `Tag`, which is then
//! fed through a battery of codec combinators: primitives, list, pair, either,
//! unbounded/simple map, compound list, and a `RecordCodecBuilder` over a
//! mixed-type record. Decoding must never panic — error paths surface as
//! `DataResult::error`, and the `NbtOps` primitives panic on structurally
//! invalid input the same way the Java server does, so a panic here is a real
//! bug. Results (and any errors) are consumed; only panics are treated as
//! failures.
#![no_main]
use libfuzzer_sys::fuzz_target;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag_parser::TagParser;
use rivet_serialization::codec;
use rivet_serialization::record_builder::RecordCodecBuilder;
use rivet_serialization::{Codec, DynamicOps};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
struct Record {
    id: i32,
    name: String,
    flag: Option<bool>,
}

fn decode_battery(ops: &NbtOps, tag: &rivet_nbt::tag::Tag) {
    let int_codec: Arc<dyn Codec<i32, NbtOps>> = codec::int_codec();
    let _ = int_codec.decode(ops, tag);
    let str_codec: Arc<dyn Codec<String, NbtOps>> = codec::string_codec();
    let _ = str_codec.decode(ops, tag);
    let bool_codec: Arc<dyn Codec<bool, NbtOps>> = codec::bool_codec();
    let _ = bool_codec.decode(ops, tag);
    let byte_codec: Arc<dyn Codec<i8, NbtOps>> = codec::byte_codec();
    let _ = byte_codec.decode(ops, tag);
    let long_codec: Arc<dyn Codec<i64, NbtOps>> = codec::long_codec();
    let _ = long_codec.decode(ops, tag);
    let double_codec: Arc<dyn Codec<f64, NbtOps>> = codec::double_codec();
    let _ = double_codec.decode(ops, tag);

    let list_codec = codec::list(int_codec.clone());
    let _ = list_codec.decode(ops, tag);
    let pair_codec = codec::pair(int_codec.clone(), str_codec.clone());
    let _ = pair_codec.decode(ops, tag);
    let either_codec = codec::either(int_codec.clone(), str_codec.clone());
    let _ = either_codec.decode(ops, tag);
    let map_codec = codec::unbounded_map(str_codec.clone(), int_codec.clone());
    let _ = map_codec.decode(ops, tag);
    let compound_list = codec::compound_list(str_codec.clone(), int_codec.clone());
    let _ = compound_list.decode(ops, tag);

    let id_field = RecordCodecBuilder::of(
        Arc::new(|r: &Record| r.id),
        codec::field_of(int_codec.clone(), "id".to_string()),
    );
    let name_field = RecordCodecBuilder::of(
        Arc::new(|r: &Record| r.name.clone()),
        codec::field_of(str_codec.clone(), "name".to_string()),
    );
    let flag_field = RecordCodecBuilder::of(
        Arc::new(|r: &Record| r.flag),
        codec::optional_field("flag".to_string(), bool_codec.clone(), false),
    );
    let record_codec =
        rivet_serialization::record_builder::create::<Record, NbtOps>(move |instance| {
            instance
                .group(id_field)
                .and(name_field)
                .and(flag_field)
                .apply(
                    instance,
                    Arc::new(|id, name, flag| Record { id, name, flag }),
                )
        });
    let _ = record_codec.decode(ops, tag);

    let passthrough: Arc<dyn Codec<rivet_serialization::Dynamic<rivet_nbt::tag::Tag>, NbtOps>> =
        codec::passthrough();
    let _ = passthrough.decode(ops, tag);
}

fuzz_target!(|data: &[u8]| {
    let ops = NbtOps::instance();
    let input = String::from_utf8_lossy(data);
    let tag = match TagParser::create(ops).parse_fully(&input) {
        Ok(t) => t,
        Err(_) => ops.empty(),
    };
    decode_battery(&ops, &tag);
});
