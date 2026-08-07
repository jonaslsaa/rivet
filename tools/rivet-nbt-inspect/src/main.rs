//! Inspect binary NBT fixture key order (M2 gate-preflight evidence).
//!
//! Prints every compound key and list element in the exact on-disk field order
//! of a committed Paper 26.2 chunk-NBT fixture. Rivet's `CompoundTag` is
//! insertion-ordered (DECISIONS.md D12), so this order is what a Rivet
//! read->write round-trip preserves byte-for-byte.
//!
//! Uses `rivet-nbt`'s well-tested streaming `parse` (a `StreamTagVisitor`) —
//! NOT a hand-rolled byte walker — so it cannot drift from the format. Run:
//!
//! ```sh
//! cargo run -p rivet-nbt-inspect -- tools/rivet-oracle/fixtures/chunk/overworld/0.0/0.0.nbt
//! ```
//!
//! Exit code 0 when the whole file parses to completion; nonzero (with the
//! parse error) if the fixture is malformed.

use std::io::Cursor;

use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io;
use rivet_nbt::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use rivet_nbt::tag_type::TagType;
use rivet_util::DataInputStream;

/// Prints each compound entry / list element as it is visited, indented by
/// container depth, so the printed order is exactly the on-disk field order.
struct OrderPrinter {
    depth: usize,
}

impl OrderPrinter {
    fn pad(&self) -> String {
        "  ".repeat(self.depth)
    }
}

impl StreamTagVisitor for OrderPrinter {
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

    fn visit_list(&mut self, element_type: TagType, size: usize) -> ValueResult {
        println!("{}list[{size}] of {}", self.pad(), element_type.name());
        self.depth += 1;
        ValueResult::Continue
    }

    fn visit_entry(&mut self, _ty: TagType) -> EntryResult {
        EntryResult::Enter
    }

    fn visit_entry_named(&mut self, ty: TagType, id: &str) -> EntryResult {
        println!("{}{}: {}", self.pad(), id, ty.name());
        EntryResult::Enter
    }

    fn visit_element(&mut self, _ty: TagType, index: usize) -> EntryResult {
        println!("{}[{index}]", self.pad());
        EntryResult::Enter
    }

    fn visit_container_end(&mut self) -> ValueResult {
        if self.depth > 0 {
            self.depth -= 1;
        }
        println!("{}<end>", self.pad());
        ValueResult::Continue
    }

    fn visit_root_entry(&mut self, ty: TagType) -> ValueResult {
        println!("root {}", ty.name());
        ValueResult::Continue
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: rivet-nbt-inspect <fixture.nbt>");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    println!("=== {path} === ({} bytes)", bytes.len());

    let mut input = DataInputStream::new(Cursor::new(bytes.as_slice()));
    let mut visitor = OrderPrinter { depth: 0 };
    nbt_io::parse(
        &mut input,
        &mut visitor,
        &mut NbtAccounter::unlimited_heap(),
    )
    .unwrap_or_else(|e| panic!("malformed NBT: {e}"));
    println!("=== parsed to completion ===");
}
