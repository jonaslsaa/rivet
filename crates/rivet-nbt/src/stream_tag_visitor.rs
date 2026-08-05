//! Port of `net.minecraft.nbt.StreamTagVisitor`.

use crate::tag_type::TagType;

/// `StreamTagVisitor.EntryResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryResult {
    Enter,
    Skip,
    Break,
    Halt,
}

/// `StreamTagVisitor.ValueResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueResult {
    Continue,
    Break,
    Halt,
}

/// Port of the `StreamTagVisitor` interface.
pub trait StreamTagVisitor {
    fn visit_end(&mut self) -> ValueResult;
    fn visit_string(&mut self, value: &str) -> ValueResult;
    fn visit_byte(&mut self, value: i8) -> ValueResult;
    fn visit_short(&mut self, value: i16) -> ValueResult;
    fn visit_int(&mut self, value: i32) -> ValueResult;
    fn visit_long(&mut self, value: i64) -> ValueResult;
    fn visit_float(&mut self, value: f32) -> ValueResult;
    fn visit_double(&mut self, value: f64) -> ValueResult;
    fn visit_byte_array(&mut self, value: &[i8]) -> ValueResult;
    fn visit_int_array(&mut self, value: &[i32]) -> ValueResult;
    fn visit_long_array(&mut self, value: &[i64]) -> ValueResult;
    fn visit_list(&mut self, element_type: TagType, size: usize) -> ValueResult;
    fn visit_entry(&mut self, ty: TagType) -> EntryResult;
    fn visit_entry_named(&mut self, ty: TagType, id: &str) -> EntryResult;
    fn visit_element(&mut self, ty: TagType, index: usize) -> EntryResult;
    fn visit_container_end(&mut self) -> ValueResult;
    fn visit_root_entry(&mut self, ty: TagType) -> ValueResult;
}
