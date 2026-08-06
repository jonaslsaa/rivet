//! Fuzz target: SNBT parse → print → re-parse round-trip.
//!
//! `StringTagVisitor` (the SNBT printer) is not a parser but it is the other
//! half of the untrusted-data surface: its output is fed back into the parser
//! in real servers, so a printer bug that emits invalid SNBT (or a parse bug
//! that rejects its own output) is a real failure mode. This target asserts
//! the round-trip identity `parse(print(tag)) == tag` for every successfully
//! parsed input. The second parse must always succeed.
#![no_main]
use libfuzzer_sys::fuzz_target;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::string_tag_visitor::StringTagVisitor;
use rivet_nbt::tag_parser::TagParser;

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let parser = TagParser::create(NbtOps::instance());
    if let Ok(tag) = parser.parse_fully(&input) {
        let printed = StringTagVisitor::to_string(&tag);
        // Re-parse the printer's output — it must parse and be identical.
        let reparsed = parser
            .parse_fully(&printed)
            .expect("printed SNBT must re-parse");
        assert_eq!(reparsed, tag, "round-trip mismatch for {input:?}");
    }
});
