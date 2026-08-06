//! Fuzz target: the SNBT parser (`TagParser`), the reader for untrusted
//! server-side `net.minecraft.nbt` input.
//!
//! The parser takes UTF-8 strings; a raw byte slice is interpreted as UTF-8
//! with lossy replacement so the fuzzer explores the full grammar surface
//! (numbers, quoted/unquoted strings, escapes, maps, lists, typed arrays,
//! builtins) without being blocked on invalid UTF-8. Error paths
//! (`NbtFormatException`) are expected and must not panic.
#![no_main]
use libfuzzer_sys::fuzz_target;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag_parser::TagParser;

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let parser = TagParser::create(NbtOps::instance());
    // `parse_fully` (reject trailing data) and `parse_as_argument` (leave
    // trailing input unconsumed) are the two Java entry points.
    let _ = parser.parse_fully(&input);
    let _ = parser.parse_as_argument(&input);
    let _ = rivet_nbt::tag_parser::parse_compound_fully(&input);
    let _ = rivet_nbt::tag_parser::parse_compound_as_argument(&input);
});
