//! Port of `net.minecraft.nbt.visitors.CollectFields` — a `StreamTagVisitor`
//! that walks a stream, keeps only the selected fields, and merges them into a
//! `CompoundTag`.

use std::collections::HashSet;

use crate::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use crate::tag::Tag;
use crate::tag_type::TagType;

use super::collect_to_tag::CollectToTag;
use super::field_selector::FieldSelector;
use super::field_tree::FieldTree;

/// `CollectFields` — a `CollectToTag` that keeps only the wanted fields.
#[derive(Debug)]
pub struct CollectFields {
    collect_to_tag: CollectToTag,
    fields_to_get_count: i32,
    wanted_types: HashSet<TagType>,
    stack: Vec<FieldTree>,
}

impl CollectFields {
    /// `new CollectFields(FieldSelector...)`.
    pub fn new(wanted_fields: &[FieldSelector]) -> Self {
        let fields_to_get_count = wanted_fields.len() as i32;
        let mut wanted_types = HashSet::new();
        let mut root_frame = FieldTree::create_root();

        for wanted_field in wanted_fields {
            root_frame.add_entry(wanted_field);
            wanted_types.insert(wanted_field.ty);
        }

        wanted_types.insert(TagType::Compound);
        CollectFields {
            collect_to_tag: CollectToTag::new(),
            fields_to_get_count,
            wanted_types,
            stack: vec![root_frame],
        }
    }

    /// `CollectFields.getResult()` (inherited from `CollectToTag`).
    pub fn get_result(&self) -> Option<Tag> {
        self.collect_to_tag.get_result()
    }

    /// `CollectFields.getMissingFieldCount()`.
    pub fn get_missing_field_count(&self) -> i32 {
        self.fields_to_get_count
    }
}

impl StreamTagVisitor for CollectFields {
    fn visit_end(&mut self) -> ValueResult {
        self.collect_to_tag.visit_end()
    }

    fn visit_string(&mut self, value: &str) -> ValueResult {
        self.collect_to_tag.visit_string(value)
    }

    fn visit_byte(&mut self, value: i8) -> ValueResult {
        self.collect_to_tag.visit_byte(value)
    }

    fn visit_short(&mut self, value: i16) -> ValueResult {
        self.collect_to_tag.visit_short(value)
    }

    fn visit_int(&mut self, value: i32) -> ValueResult {
        self.collect_to_tag.visit_int(value)
    }

    fn visit_long(&mut self, value: i64) -> ValueResult {
        self.collect_to_tag.visit_long(value)
    }

    fn visit_float(&mut self, value: f32) -> ValueResult {
        self.collect_to_tag.visit_float(value)
    }

    fn visit_double(&mut self, value: f64) -> ValueResult {
        self.collect_to_tag.visit_double(value)
    }

    fn visit_byte_array(&mut self, value: &[i8]) -> ValueResult {
        self.collect_to_tag.visit_byte_array(value)
    }

    fn visit_int_array(&mut self, value: &[i32]) -> ValueResult {
        self.collect_to_tag.visit_int_array(value)
    }

    fn visit_long_array(&mut self, value: &[i64]) -> ValueResult {
        self.collect_to_tag.visit_long_array(value)
    }

    fn visit_list(&mut self, element_type: TagType, size: usize) -> ValueResult {
        self.collect_to_tag.visit_list(element_type, size)
    }

    fn visit_entry(&mut self, ty: TagType) -> EntryResult {
        let current_depth = self.collect_to_tag.depth();
        let frame_depth = self
            .stack
            .last()
            .expect("collect fields: field stack must be non-empty")
            .depth;
        if current_depth > frame_depth {
            self.collect_to_tag.visit_entry(ty)
        } else if self.fields_to_get_count <= 0 {
            EntryResult::Break
        } else if !self.wanted_types.contains(&ty) {
            EntryResult::Skip
        } else {
            self.collect_to_tag.visit_entry(ty)
        }
    }

    fn visit_entry_named(&mut self, ty: TagType, id: &str) -> EntryResult {
        let current_depth = self.collect_to_tag.depth();
        let frame_depth = self
            .stack
            .last()
            .expect("collect fields: field stack must be non-empty")
            .depth;
        if current_depth > frame_depth {
            return self.collect_to_tag.visit_entry_named(ty, id);
        }

        // `selectedFields.remove(id, type)` — remove only when the stored type
        // matches, consuming the field.
        let selected = match self.stack.last() {
            Some(frame) => frame.selected_fields.get(id) == Some(&ty),
            None => false,
        };
        if selected {
            self.stack.last_mut().unwrap().selected_fields.remove(id);
            self.fields_to_get_count -= 1;
            return self.collect_to_tag.visit_entry_named(ty, id);
        }

        if ty == TagType::Compound {
            let recurse = match self.stack.last() {
                Some(frame) => frame.fields_to_recurse.contains_key(id),
                None => false,
            };
            if recurse {
                // Java pushes the live child node by reference; each child is
                // entered at most once per traversal (compound keys are unique
                // and list elements never push frames), so cloning is
                // behaviorally equivalent.
                let child = self
                    .stack
                    .last()
                    .unwrap()
                    .fields_to_recurse
                    .get(id)
                    .unwrap()
                    .clone();
                self.stack.push(child);
                return self.collect_to_tag.visit_entry_named(ty, id);
            }
        }

        EntryResult::Skip
    }

    fn visit_element(&mut self, ty: TagType, index: usize) -> EntryResult {
        self.collect_to_tag.visit_element(ty, index)
    }

    fn visit_container_end(&mut self) -> ValueResult {
        let current_depth = self.collect_to_tag.depth();
        let frame_depth = self
            .stack
            .last()
            .expect("collect fields: field stack must be non-empty")
            .depth;
        if current_depth == frame_depth {
            self.stack.pop();
        }
        self.collect_to_tag.visit_container_end()
    }

    fn visit_root_entry(&mut self, ty: TagType) -> ValueResult {
        if ty != TagType::Compound {
            ValueResult::Halt
        } else {
            self.collect_to_tag.visit_root_entry(ty)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compound_tag::CompoundTag;
    use crate::int_tag::IntTag;
    use crate::string_tag::StringTag;

    #[test]
    fn collects_only_selected_fields() {
        let mut a = CompoundTag::new();
        a.put("b".to_string(), Tag::Int(IntTag::value_of(42)));
        a.put("c".to_string(), Tag::Int(IntTag::value_of(99)));
        let mut source = CompoundTag::new();
        source.put("a".to_string(), Tag::Compound(a));
        source.put("x".to_string(), Tag::Int(IntTag::value_of(1)));

        let selector = FieldSelector::with_parent("a".to_string(), TagType::Int, "b".to_string());
        let mut collector = CollectFields::new(&[selector]);
        Tag::Compound(source).accept_as_root(&mut collector);

        assert_eq!(collector.get_missing_field_count(), 0);
        let result = collector.get_result().expect("result");
        let compound = result.as_compound().expect("compound result");
        let a = compound.get_compound("a").expect("a");
        assert!(a.contains("b"));
        assert_eq!(a.get_int("b"), Some(42));
        assert!(!a.contains("c"));
        assert!(!compound.contains("x"));
    }

    #[test]
    fn missing_field_keeps_count() {
        let a = CompoundTag::new();
        let mut source = CompoundTag::new();
        source.put("a".to_string(), Tag::Compound(a));

        let selector = FieldSelector::with_parent("a".to_string(), TagType::Int, "b".to_string());
        let mut collector = CollectFields::new(&[selector]);
        Tag::Compound(source).accept_as_root(&mut collector);

        assert_eq!(collector.get_missing_field_count(), 1);
    }

    #[test]
    fn selected_compound_keeps_entire_subtree() {
        let mut inner = CompoundTag::new();
        inner.put("deep".to_string(), Tag::Int(IntTag::value_of(7)));
        let mut a = CompoundTag::new();
        a.put("inner".to_string(), Tag::Compound(inner));
        a.put("other".to_string(), Tag::Int(IntTag::value_of(5)));
        let mut source = CompoundTag::new();
        source.put("a".to_string(), Tag::Compound(a));
        source.put(
            "z".to_string(),
            Tag::String(StringTag::value_of("skip".to_string())),
        );

        let selector = FieldSelector::new(TagType::Compound, "a".to_string());
        let mut collector = CollectFields::new(&[selector]);
        Tag::Compound(source).accept_as_root(&mut collector);

        assert_eq!(collector.get_missing_field_count(), 0);
        let result = collector.get_result().expect("result");
        let compound = result.as_compound().expect("compound result");
        let a = compound.get_compound("a").expect("a");
        assert!(a.contains("inner"));
        assert!(a.contains("other"));
        assert_eq!(a.get_compound("inner").unwrap().get_int("deep"), Some(7));
        // All wanted fields were found, so the rest of the root is skipped.
        assert!(!compound.contains("z"));
    }
}
