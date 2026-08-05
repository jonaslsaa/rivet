//! Port of `net.minecraft.nbt.ListTag` — `final class extends AbstractList<Tag>`.

use crate::compound_tag::CompoundTag;
use crate::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use crate::tag::{TAG_COMPOUND, TAG_END, Tag};
use crate::tag_type::TagType;

pub const SELF_SIZE_IN_BYTES: i32 = 36;
const WRAPPER_MARKER: &str = "";

/// `ListTag`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListTag {
    pub list: Vec<Tag>,
}

impl ListTag {
    /// `new ListTag()`.
    pub fn new() -> Self {
        ListTag { list: Vec::new() }
    }

    /// `new ListTag(List<Tag>)`.
    pub fn with_list(list: Vec<Tag>) -> Self {
        ListTag { list }
    }

    /// `ListTag.sizeInBytes()` — `36 + 4 * size + sum(child.sizeInBytes())`.
    pub fn size_in_bytes(&self) -> i32 {
        let mut size = 36;
        size += 4 * self.list.len() as i32;
        for child in &self.list {
            size += child.size_in_bytes();
        }
        size
    }

    /// `ListTag.copy()` — deep copy.
    pub fn copy_tag(&self) -> ListTag {
        let mut copy = Vec::with_capacity(self.list.len());
        for tag in &self.list {
            copy.push(tag.copy_tag());
        }
        ListTag { list: copy }
    }

    /// `ListTag.identifyRawElementType()`.
    pub fn identify_raw_element_type(&self) -> i8 {
        let mut homogenous_type: i8 = TAG_END;
        for element in &self.list {
            let element_type = element.id();
            if homogenous_type == 0 {
                homogenous_type = element_type;
            } else if homogenous_type != element_type {
                return TAG_COMPOUND;
            }
        }
        homogenous_type
    }

    /// `ListTag.addAndUnwrap(Tag)`.
    pub fn add_and_unwrap(&mut self, tag: Tag) {
        if let Tag::Compound(compound) = &tag {
            self.add(try_unwrap(compound));
        } else {
            self.add(tag);
        }
    }

    /// `ListTag.add(int, Tag)` (AbstractList.add).
    pub fn add(&mut self, tag: Tag) {
        self.list.push(tag);
    }

    /// `ListTag.add(int index, Tag)`.
    pub fn add_at(&mut self, index: usize, tag: Tag) {
        self.list.insert(index, tag);
    }

    /// `ListTag.set(int, Tag)`.
    pub fn set(&mut self, index: usize, tag: Tag) -> Tag {
        std::mem::replace(&mut self.list[index], tag)
    }

    /// `ListTag.setTag(int, Tag)` — always true for `ListTag`.
    pub fn set_tag(&mut self, index: usize, tag: Tag) -> bool {
        self.list[index] = tag;
        true
    }

    /// `ListTag.addTag(int, Tag)` — always true for `ListTag`.
    pub fn add_tag(&mut self, index: usize, tag: Tag) -> bool {
        self.list.insert(index, tag);
        true
    }

    /// `ListTag.remove(int)`.
    pub fn remove(&mut self, index: usize) -> Tag {
        self.list.remove(index)
    }

    /// `ListTag.clear()`.
    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// `ListTag.size()`.
    pub fn size(&self) -> usize {
        self.list.len()
    }

    /// `ListTag.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// `ListTag.get(int)`.
    pub fn get(&self, index: usize) -> &Tag {
        &self.list[index]
    }

    /// `ListTag.getCompound(int)`.
    pub fn get_compound(&self, index: usize) -> Option<&CompoundTag> {
        match self.get_nullable(index) {
            Some(Tag::Compound(t)) => Some(t),
            _ => None,
        }
    }

    /// `ListTag.getCompoundOrEmpty(int)`.
    ///
    /// Note: Java returns the live child; this value form clones it (Rust
    /// cannot return a reference from `&self`). Use
    /// [`get_compound_or_empty_mut`](Self::get_compound_or_empty_mut) when the
    /// returned tag will be mutated, so the mutation lands in the list.
    pub fn get_compound_or_empty(&self, index: usize) -> CompoundTag {
        match self.get_compound(index) {
            Some(t) => t.clone(),
            None => CompoundTag::new(),
        }
    }

    /// `ListTag.getCompoundOrEmpty(int)` — mutation form. Returns the live
    /// child when the element is a compound, else replaces the element with a
    /// fresh empty `CompoundTag` and returns it. An out-of-range index panics,
    /// matching Java's `IndexOutOfBoundsException` from `List.get`. (Java
    /// returns a detached empty for a non-compound element, silently losing the
    /// mutation; this form surfaces a persistent one.)
    pub fn get_compound_or_empty_mut(&mut self, index: usize) -> &mut CompoundTag {
        if !matches!(self.list.get(index), Some(Tag::Compound(_))) {
            // Replaces any non-compound element (or panics on out-of-range
            // via `&mut self.list[index]` in the assignment).
            self.list[index] = Tag::Compound(CompoundTag::new());
        }
        match &mut self.list[index] {
            Tag::Compound(t) => t,
            _ => unreachable!("compound just replaced or already present"),
        }
    }

    /// `ListTag.getList(int)`.
    pub fn get_list(&self, index: usize) -> Option<&ListTag> {
        match self.get_nullable(index) {
            Some(Tag::List(t)) => Some(t),
            _ => None,
        }
    }

    /// `ListTag.getListOrEmpty(int)`.
    ///
    /// Note: Java returns the live child; this value form clones it (Rust
    /// cannot return a reference from `&self`). Use
    /// [`get_list_or_empty_mut`](Self::get_list_or_empty_mut) when the returned
    /// list will be mutated, so the mutation lands in the list.
    pub fn get_list_or_empty(&self, index: usize) -> ListTag {
        match self.get_list(index) {
            Some(t) => t.clone(),
            None => ListTag::new(),
        }
    }

    /// `ListTag.getListOrEmpty(int)` — mutation form. Returns the live child
    /// when the element is a list, else replaces the element with a fresh empty
    /// `ListTag` and returns it. An out-of-range index panics, matching Java's
    /// `IndexOutOfBoundsException` from `List.get`. (Java returns a detached
    /// empty for a non-list element, silently losing the mutation; this form
    /// surfaces a persistent one.)
    pub fn get_list_or_empty_mut(&mut self, index: usize) -> &mut ListTag {
        if !matches!(self.list.get(index), Some(Tag::List(_))) {
            // Replaces any non-list element (or panics on out-of-range via
            // `&mut self.list[index]` in the assignment).
            self.list[index] = Tag::List(ListTag::new());
        }
        match &mut self.list[index] {
            Tag::List(t) => t,
            _ => unreachable!("list just replaced or already present"),
        }
    }

    /// `ListTag.getShort(int)`.
    pub fn get_short(&self, index: usize) -> Option<i16> {
        self.get_optional(index).and_then(|t| t.as_short())
    }

    /// `ListTag.getShortOr(int, short)`.
    pub fn get_short_or(&self, index: usize, default_value: i16) -> i16 {
        match self.get_nullable(index) {
            Some(t) => match crate::numeric_tag::NumericTag::try_from(t) {
                Ok(n) => n.short_value(),
                Err(_) => default_value,
            },
            None => default_value,
        }
    }

    /// `ListTag.getInt(int)`.
    pub fn get_int(&self, index: usize) -> Option<i32> {
        self.get_optional(index).and_then(|t| t.as_int())
    }

    /// `ListTag.getIntOr(int, int)`.
    pub fn get_int_or(&self, index: usize, default_value: i32) -> i32 {
        match self.get_nullable(index) {
            Some(t) => match crate::numeric_tag::NumericTag::try_from(t) {
                Ok(n) => n.int_value(),
                Err(_) => default_value,
            },
            None => default_value,
        }
    }

    /// `ListTag.getIntArray(int)`.
    pub fn get_int_array(&self, index: usize) -> Option<&Vec<i32>> {
        match self.get_nullable(index) {
            Some(Tag::IntArray(t)) => Some(t.get_as_int_array()),
            _ => None,
        }
    }

    /// `ListTag.getLongArray(int)`.
    pub fn get_long_array(&self, index: usize) -> Option<&Vec<i64>> {
        match self.get_nullable(index) {
            Some(Tag::LongArray(t)) => Some(t.get_as_long_array()),
            _ => None,
        }
    }

    /// `ListTag.getDouble(int)`.
    pub fn get_double(&self, index: usize) -> Option<f64> {
        self.get_optional(index).and_then(|t| t.as_double())
    }

    /// `ListTag.getDoubleOr(int, double)`.
    pub fn get_double_or(&self, index: usize, default_value: f64) -> f64 {
        match self.get_nullable(index) {
            Some(t) => match crate::numeric_tag::NumericTag::try_from(t) {
                Ok(n) => n.double_value(),
                Err(_) => default_value,
            },
            None => default_value,
        }
    }

    /// `ListTag.getFloat(int)`.
    pub fn get_float(&self, index: usize) -> Option<f32> {
        self.get_optional(index).and_then(|t| t.as_float())
    }

    /// `ListTag.getFloatOr(int, float)`.
    pub fn get_float_or(&self, index: usize, default_value: f32) -> f32 {
        match self.get_nullable(index) {
            Some(t) => match crate::numeric_tag::NumericTag::try_from(t) {
                Ok(n) => n.float_value(),
                Err(_) => default_value,
            },
            None => default_value,
        }
    }

    /// `ListTag.getString(int)`.
    pub fn get_string(&self, index: usize) -> Option<&String> {
        self.get_optional(index).and_then(|t| t.as_string())
    }

    /// `ListTag.getStringOr(int, String)`.
    pub fn get_string_or<'a>(&'a self, index: usize, default_value: &'a str) -> &'a str {
        match self.get_nullable(index) {
            Some(Tag::String(t)) => &t.value,
            _ => default_value,
        }
    }

    /// `ListTag.getNullable(int)` — `null` for out-of-range.
    pub fn get_nullable(&self, index: usize) -> Option<&Tag> {
        if index < self.list.len() {
            Some(&self.list[index])
        } else {
            None
        }
    }

    /// `ListTag.getOptional(int)`.
    pub fn get_optional(&self, index: usize) -> Option<&Tag> {
        self.get_nullable(index)
    }

    /// `ListTag.stream()` — iterator over the elements.
    pub fn iter(&self) -> std::slice::Iter<'_, Tag> {
        self.list.iter()
    }

    /// `ListTag.compoundStream()` — elements that are compounds.
    pub fn compound_stream(&self) -> impl Iterator<Item = &CompoundTag> {
        self.list.iter().filter_map(|t| match t {
            Tag::Compound(c) => Some(c),
            _ => None,
        })
    }

    /// `ListTag.accept(StreamTagVisitor)`.
    pub fn accept_stream(&self, visitor: &mut dyn StreamTagVisitor) -> ValueResult {
        let element_type = self.identify_raw_element_type();
        match visitor.visit_list(TagType::from_id(element_type), self.list.len()) {
            ValueResult::Halt => ValueResult::Halt,
            ValueResult::Break => visitor.visit_container_end(),
            ValueResult::Continue => {
                let mut i = 0;
                while i < self.list.len() {
                    let tag = wrap_if_needed(element_type, &self.list[i]);
                    match visitor.visit_element(tag.get_type(), i) {
                        EntryResult::Halt => return ValueResult::Halt,
                        EntryResult::Break => return visitor.visit_container_end(),
                        EntryResult::Enter => {
                            match tag.accept_stream(visitor) {
                                ValueResult::Halt => return ValueResult::Halt,
                                ValueResult::Break => return visitor.visit_container_end(),
                                ValueResult::Continue => {}
                            }
                            i += 1;
                        }
                        EntryResult::Skip => i += 1,
                    }
                }
                visitor.visit_container_end()
            }
        }
    }
}

/// `ListTag.tryUnwrap(CompoundTag)`.
fn try_unwrap(tag: &CompoundTag) -> Tag {
    if tag.size() == 1
        && let Some(value) = tag.get("")
    {
        return value.clone();
    }
    Tag::Compound(tag.clone())
}

/// `ListTag.isWrapper(CompoundTag)`.
fn is_wrapper(tag: &CompoundTag) -> bool {
    tag.size() == 1 && tag.contains("")
}

/// `ListTag.wrapIfNeeded(byte, Tag)`.
fn wrap_if_needed(element_type: i8, tag: &Tag) -> Tag {
    if element_type != TAG_COMPOUND {
        return tag.clone();
    }
    match tag {
        Tag::Compound(c) if !is_wrapper(c) => tag.clone(),
        other => wrap_element(other),
    }
}

/// `ListTag.wrapElement(Tag)`.
fn wrap_element(tag: &Tag) -> Tag {
    let mut c = CompoundTag::new();
    c.put(WRAPPER_MARKER.to_string(), tag.clone());
    Tag::Compound(c)
}
