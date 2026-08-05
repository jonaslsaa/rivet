//! Port of `net.minecraft.nbt.visitors.SkipAll` — a `StreamTagVisitor` that
//! visits nothing: every value visit continues, every entry is skipped, so no
//! content is ever descended into.

use crate::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use crate::tag_type::TagType;

/// `SkipAll` — the `interface SkipAll implements StreamTagVisitor` with all
/// default methods that skip/continue.
#[derive(Debug, Clone, Copy, Default)]
pub struct SkipAll;

/// `SkipAll.INSTANCE`.
pub const INSTANCE: SkipAll = SkipAll;

impl StreamTagVisitor for SkipAll {
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
        EntryResult::Skip
    }

    fn visit_entry_named(&mut self, _ty: TagType, _id: &str) -> EntryResult {
        EntryResult::Skip
    }

    fn visit_element(&mut self, _ty: TagType, _index: usize) -> EntryResult {
        EntryResult::Skip
    }

    fn visit_container_end(&mut self) -> ValueResult {
        ValueResult::Continue
    }

    fn visit_root_entry(&mut self, _ty: TagType) -> ValueResult {
        ValueResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_all_entries() {
        let mut visitor = SkipAll;
        assert_eq!(visitor.visit_end(), ValueResult::Continue);
        assert_eq!(visitor.visit_string("value"), ValueResult::Continue);
        assert_eq!(visitor.visit_byte(1), ValueResult::Continue);
        assert_eq!(visitor.visit_short(1), ValueResult::Continue);
        assert_eq!(visitor.visit_int(1), ValueResult::Continue);
        assert_eq!(visitor.visit_long(1), ValueResult::Continue);
        assert_eq!(visitor.visit_float(1.0), ValueResult::Continue);
        assert_eq!(visitor.visit_double(1.0), ValueResult::Continue);
        assert_eq!(visitor.visit_byte_array(&[1, 2]), ValueResult::Continue);
        assert_eq!(visitor.visit_int_array(&[1, 2]), ValueResult::Continue);
        assert_eq!(visitor.visit_long_array(&[1, 2]), ValueResult::Continue);
        assert_eq!(visitor.visit_list(TagType::Int, 3), ValueResult::Continue);
        assert_eq!(visitor.visit_entry(TagType::Int), EntryResult::Skip);
        assert_eq!(
            visitor.visit_entry_named(TagType::Int, "id"),
            EntryResult::Skip
        );
        assert_eq!(visitor.visit_element(TagType::Int, 0), EntryResult::Skip);
        assert_eq!(visitor.visit_container_end(), ValueResult::Continue);
        assert_eq!(
            visitor.visit_root_entry(TagType::Compound),
            ValueResult::Continue
        );
    }
}
