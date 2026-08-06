//! Fuzz target: the streaming binary NBT reader (`NbtIo.parse` /
//! `parse_compressed`) with a visitor that always returns `Continue`, forcing
//! full traversal of the tag tree.
//!
//! This exercises `skip_string`, `skip`, `parse_tag`, `parse_list`,
//! `parse_compound` and the nested-tag parse paths — a distinct dispatch path
//! (`TagType.parse`) from the `read` family, where a buggy visitor or a
//! traversal bug could otherwise hide.
//!
//! As in `nbt_binary`, the read path panics on inputs that crash Java's parse
//! (missing list element type, negative list length, oversized array,
//! `NbtAccounter` quota/depth) — all faithful expected outcomes, swallowed by
//! `catch_unwind`. The accounter is bounded to the server's 2 MiB default quota.
#![no_main]
use libfuzzer_sys::fuzz_target;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io::{parse, parse_compressed};
use rivet_nbt::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use rivet_nbt::tag_type::TagType;
use rivet_util::data_io::DataInputStream;
use std::io::Cursor;

mod common;
use common::guarded;

/// A visitor that always accepts, so `parse` walks the whole tree.
#[derive(Debug, Default)]
struct AcceptAll;

impl StreamTagVisitor for AcceptAll {
    fn visit_end(&mut self) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_string(&mut self, _value: &str) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_byte(&mut self, _value: i8) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_short(&mut self, _value: i16) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_int(&mut self, _value: i32) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_long(&mut self, _value: i64) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_float(&mut self, _value: f32) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_double(&mut self, _value: f64) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_byte_array(&mut self, _value: &[i8]) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_int_array(&mut self, _value: &[i32]) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_long_array(&mut self, _value: &[i64]) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_list(&mut self, _element_type: TagType, _size: usize) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_entry(&mut self, _ty: TagType) -> EntryResult {
        EntryResult::Enter
    }
    fn visit_entry_named(&mut self, _ty: TagType, _id: &str) -> EntryResult {
        EntryResult::Enter
    }
    fn visit_element(&mut self, _ty: TagType, _index: usize) -> EntryResult {
        EntryResult::Enter
    }
    fn visit_container_end(&mut self) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_root_entry(&mut self, _ty: TagType) -> ValueResult {
        ValueResult::Continue
    }
}

fuzz_target!(|data: &[u8]| {
    // Raw binary path.
    guarded(|| {
        let mut dis = DataInputStream::new(Cursor::new(data));
        let mut visitor = AcceptAll;
        let mut acc = NbtAccounter::default_quota();
        let _ = parse(&mut dis, &mut visitor, &mut acc);
    });
    // Gzip-compressed path.
    guarded(|| {
        let mut visitor = AcceptAll;
        let mut acc = NbtAccounter::default_quota();
        let _ = parse_compressed(Cursor::new(data), &mut visitor, &mut acc);
    });
});
