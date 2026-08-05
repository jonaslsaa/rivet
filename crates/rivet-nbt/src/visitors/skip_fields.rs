//! Port of `net.minecraft.nbt.visitors.SkipFields` — a `StreamTagVisitor` that
//! visits all but a chosen field set.

use crate::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use crate::tag::Tag;
use crate::tag_type::TagType;

use super::collect_to_tag::CollectToTag;
use super::field_selector::FieldSelector;
use super::field_tree::FieldTree;

/// `SkipFields` — a `CollectToTag` that skips the wanted fields and keeps all
/// others.
#[derive(Debug)]
pub struct SkipFields {
    collect_to_tag: CollectToTag,
    stack: Vec<FieldTree>,
}

impl SkipFields {
    /// `new SkipFields(FieldSelector...)`.
    pub fn new(wanted_fields: &[FieldSelector]) -> Self {
        let mut root_frame = FieldTree::create_root();

        for wanted_field in wanted_fields {
            root_frame.add_entry(wanted_field);
        }

        SkipFields {
            collect_to_tag: CollectToTag::new(),
            stack: vec![root_frame],
        }
    }

    /// `SkipFields.getResult()` (inherited from `CollectToTag`).
    pub fn get_result(&self) -> Option<Tag> {
        self.collect_to_tag.get_result()
    }
}

impl StreamTagVisitor for SkipFields {
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
        self.collect_to_tag.visit_entry(ty)
    }

    fn visit_entry_named(&mut self, ty: TagType, id: &str) -> EntryResult {
        let frame = self
            .stack
            .last()
            .expect("skip fields: field stack must be non-empty");
        if frame.is_selected(ty, id) {
            return EntryResult::Skip;
        }

        if ty == TagType::Compound {
            let recurse = frame.fields_to_recurse.contains_key(id);
            if recurse {
                // Java pushes the live child node by reference; each child is
                // entered at most once per traversal (compound keys are unique
                // and list elements never push frames), so cloning is
                // behaviorally equivalent.
                let child = frame.fields_to_recurse.get(id).unwrap().clone();
                self.stack.push(child);
            }
        }

        self.collect_to_tag.visit_entry_named(ty, id)
    }

    fn visit_element(&mut self, ty: TagType, index: usize) -> EntryResult {
        self.collect_to_tag.visit_element(ty, index)
    }

    fn visit_container_end(&mut self) -> ValueResult {
        let current_depth = self.collect_to_tag.depth();
        let frame_depth = self
            .stack
            .last()
            .expect("skip fields: field stack must be non-empty")
            .depth;
        if current_depth == frame_depth {
            self.stack.pop();
        }
        self.collect_to_tag.visit_container_end()
    }

    fn visit_root_entry(&mut self, ty: TagType) -> ValueResult {
        self.collect_to_tag.visit_root_entry(ty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compound_tag::CompoundTag;
    use crate::int_tag::IntTag;
    use crate::string_tag::StringTag;

    #[test]
    fn skips_selected_field_keeps_others() {
        let mut a = CompoundTag::new();
        a.put("b".to_string(), Tag::Int(IntTag::value_of(42)));
        a.put("c".to_string(), Tag::Int(IntTag::value_of(99)));
        let mut source = CompoundTag::new();
        source.put("a".to_string(), Tag::Compound(a));
        source.put(
            "x".to_string(),
            Tag::String(StringTag::value_of("keep".to_string())),
        );

        let selector = FieldSelector::with_parent("a".to_string(), TagType::Int, "b".to_string());
        let mut visitor = SkipFields::new(&[selector]);
        Tag::Compound(source).accept_as_root(&mut visitor);

        let result = visitor.get_result().expect("result");
        let compound = result.as_compound().expect("compound result");
        let a = compound.get_compound("a").expect("a");
        assert!(!a.contains("b"));
        assert!(a.contains("c"));
        assert_eq!(a.get_int("c"), Some(99));
        assert!(compound.contains("x"));
    }

    #[test]
    fn skips_selected_compound_subtree() {
        let mut inner = CompoundTag::new();
        inner.put("deep".to_string(), Tag::Int(IntTag::value_of(7)));
        let mut a = CompoundTag::new();
        a.put("inner".to_string(), Tag::Compound(inner));
        let mut source = CompoundTag::new();
        source.put("a".to_string(), Tag::Compound(a));

        let selector = FieldSelector::new(TagType::Compound, "a".to_string());
        let mut visitor = SkipFields::new(&[selector]);
        Tag::Compound(source).accept_as_root(&mut visitor);

        let result = visitor.get_result().expect("result");
        let compound = result.as_compound().expect("compound result");
        assert!(!compound.contains("a"));
    }

    #[test]
    fn skips_nothing_when_no_fields_match() {
        let mut source = CompoundTag::new();
        source.put("x".to_string(), Tag::Int(IntTag::value_of(1)));

        let selector = FieldSelector::with_parent("a".to_string(), TagType::Int, "b".to_string());
        let mut visitor = SkipFields::new(&[selector]);
        Tag::Compound(source).accept_as_root(&mut visitor);

        let result = visitor.get_result().expect("result");
        let compound = result.as_compound().expect("compound result");
        assert!(compound.contains("x"));
        assert_eq!(compound.get_int("x"), Some(1));
    }
}
